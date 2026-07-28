use crate::{config, AppState};
use app_lib::device_simulator::alarms::{AlarmHandlerId, AlarmTypeId};
use app_lib::device_simulator::api::{
    list_first_release_profiles, preview_devices, AlarmJobRequest, AlarmTriggerResult,
    AlarmTypeSummary, AssetPackStatus, AssetProgressSnapshot, AssetStatus, DevicePreview,
    DeviceProfileAvailability, DeviceProfileSummary, ImportedAlarmImage, MediaThemeSummary,
    PreflightReport, ProfileAlarmTypes, RecoveryResult, RuntimeTelemetrySnapshot,
    SimulatorStartRequest, SimulatorStatusSnapshot, DEVICE_SIMULATOR_EVENT_ALARM_STATS,
    DEVICE_SIMULATOR_EVENT_ALARM_SUBSCRIPTION, DEVICE_SIMULATOR_EVENT_ASSET_PROGRESS,
    DEVICE_SIMULATOR_EVENT_CLEANUP_PROGRESS, DEVICE_SIMULATOR_EVENT_DEVICE_STATUS,
    DEVICE_SIMULATOR_EVENT_LOG, DEVICE_SIMULATOR_EVENT_RTSP_STATS, DEVICE_SIMULATOR_EVENT_STATUS,
};
use app_lib::device_simulator::assets::cache::{
    validate_installed_pack, AssetStore, AssetStorePaths,
};
use app_lib::device_simulator::assets::catalog::CatalogV1;
use app_lib::device_simulator::assets::catalog_cache::{
    fetch_and_cache_signed_catalog, load_cached_signed_catalog, CachedCatalog,
};
use app_lib::device_simulator::assets::download::build_asset_http_client;
use app_lib::device_simulator::assets::resolver::resolve_profile_dependencies;
use app_lib::device_simulator::assets::signature::trusted_catalog_keys;
use app_lib::device_simulator::assets::store::{AssetPreparationPhase, AssetPreparationService};
use app_lib::device_simulator::errors::SimulatorErrorBody;
use app_lib::device_simulator::events::WorkerEventPayload;
use app_lib::device_simulator::manager::{ManagerNotification, SimulatorManager};
use app_lib::device_simulator::models::{AssetState, SessionState, SimulatorStatus};
use app_lib::device_simulator::preflight::{run_preflight, PreflightEnvironment};
use app_lib::device_simulator::profiles::loader::load_profile_from_pack;
use app_lib::device_simulator::profiles::schema::EvidenceStatus;
use app_lib::device_simulator::runtime_assets::{list_media_themes, PinnedPackDirectory};
use app_lib::device_simulator::session_journal::SessionJournalStore;
use app_lib::device_simulator::windows::interfaces::{
    list_system_interfaces, NetworkInterfaceInfo,
};
use app_lib::device_simulator::windows::ip_alias::{
    assess_system_address_conflicts, unknown_address_conflict_assessments, ConflictEvidenceKind,
};
use app_lib::device_simulator::windows::named_pipe::PipeIdentity;
use app_lib::device_simulator::worker_protocol::{
    AlarmJobCommandPayload, InitializeSessionPayload, RecoverSessionPayload, StopAlarmJobPayload,
    WorkerCommandName,
};
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, watch, Mutex as AsyncMutex};

const MAX_IMPORTED_ALARM_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ALARM_TYPES_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;

pub struct DeviceSimulatorCommandState {
    manager: Arc<SimulatorManager>,
    asset_job: Arc<AsyncMutex<Option<AssetJobControl>>>,
}

impl Default for DeviceSimulatorCommandState {
    fn default() -> Self {
        Self {
            manager: Arc::new(SimulatorManager::default()),
            asset_job: Arc::new(AsyncMutex::new(None)),
        }
    }
}

struct AssetJobControl {
    id: String,
    cancel: watch::Sender<bool>,
}

struct CatalogContext {
    cached: CachedCatalog,
    paths: AssetStorePaths,
    base_url: reqwest::Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkInterfaceDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_enabled: bool,
    pub is_up: bool,
    pub ipv4_addresses: Vec<String>,
}

impl From<NetworkInterfaceInfo> for NetworkInterfaceDto {
    fn from(interface: NetworkInterfaceInfo) -> Self {
        Self {
            id: interface.id.as_str().to_owned(),
            name: interface.name,
            description: interface.description,
            is_enabled: interface.is_enabled,
            is_up: interface.is_up,
            ipv4_addresses: interface
                .ipv4_addresses
                .into_iter()
                .map(|address| format!("{}/{}", address.address, address.prefix_len))
                .collect(),
        }
    }
}

#[tauri::command]
pub fn device_simulator_get_settings(
    state: State<'_, AppState>,
) -> config::DeviceSimulatorSettings {
    state.config.lock().unwrap().device_simulator.clone()
}

#[tauri::command]
pub async fn device_simulator_save_settings(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    settings: config::DeviceSimulatorSettings,
) -> Result<config::DeviceSimulatorSettings, SimulatorErrorBody> {
    let settings = config::normalize_device_simulator_settings(settings);
    config::validate_device_simulator_settings(&settings)
        .map_err(|details| settings_error("device_simulator.settings.invalid", details))?;
    let mut next = state.config.lock().unwrap().clone();
    next.device_simulator = settings.clone();
    config::validate_config(&next)
        .map_err(|details| settings_error("device_simulator.settings.invalid", details))?;
    config::save_config(&app_handle, &next)
        .map_err(|details| settings_error("device_simulator.settings.save_failed", details))?;
    *state.config.lock().unwrap() = next;
    Ok(settings)
}

#[tauri::command]
pub async fn device_simulator_list_interfaces(
) -> Result<Vec<NetworkInterfaceDto>, SimulatorErrorBody> {
    tokio::task::spawn_blocking(list_system_interfaces)
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.interface.task_failed",
                "deviceSimulator.errors.interfaceEnumerationFailed",
                source.to_string(),
            )
        })?
        .map(|interfaces| interfaces.into_iter().map(Into::into).collect())
        .map_err(|source| {
            runtime_error(
                "device_simulator.interface.enumeration_failed",
                "deviceSimulator.errors.interfaceEnumerationFailed",
                source.to_string(),
            )
        })
}

#[tauri::command]
pub async fn device_simulator_list_profiles(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<Vec<DeviceProfileSummary>, SimulatorErrorBody> {
    let mut summaries = list_first_release_profiles();
    let Ok(context) = load_catalog_context(&app_handle, app_state.inner(), false).await else {
        return Ok(summaries);
    };
    let paths = context.paths;
    let catalog = context.cached.catalog;
    let fallback = summaries.clone();
    match tokio::task::spawn_blocking(move || {
        apply_profile_availability(&mut summaries, &paths, &catalog);
        summaries
    })
    .await
    {
        Ok(updated) => Ok(updated),
        Err(_) => Ok(fallback),
    }
}

#[tauri::command]
pub async fn device_simulator_list_alarm_types(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<Vec<ProfileAlarmTypes>, SimulatorErrorBody> {
    let context = load_catalog_context(&app_handle, app_state.inner(), false).await?;
    tokio::task::spawn_blocking(move || {
        list_active_alarm_types(&context.paths, &context.cached.catalog)
    })
    .await
    .map_err(|source| {
        runtime_error(
            "device_simulator.alarm.type_list_task_failed",
            "deviceSimulator.errors.assetPreparationFailed",
            source.to_string(),
        )
    })?
}

#[tauri::command]
pub async fn device_simulator_list_media_themes(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
) -> Result<Vec<MediaThemeSummary>, SimulatorErrorBody> {
    let context = load_catalog_context(&app_handle, app_state.inner(), false).await?;
    tokio::task::spawn_blocking(move || {
        let pin = AssetStore::new(context.paths)
            .pin_active(&context.cached.catalog)
            .map_err(|source| {
                runtime_error(
                    source.code,
                    "deviceSimulator.errors.assetPreparationFailed",
                    source.message,
                )
            })?;
        let media_directory = pin
            .selection
            .packs
            .iter()
            .zip(pin.pack_directories)
            .find_map(|(pack, directory)| (pack.id == "media-h264-live").then_some(directory))
            .ok_or_else(|| {
                runtime_error(
                    "device_simulator.assets.media_pack_missing",
                    "deviceSimulator.errors.assetPreparationFailed",
                    "active media-h264-live pack is missing",
                )
            })?;
        list_media_themes(&media_directory).map_err(|source| {
            runtime_error(
                source.code,
                "deviceSimulator.errors.assetPreparationFailed",
                source.message,
            )
        })
    })
    .await
    .map_err(|source| {
        runtime_error(
            "device_simulator.assets.media_theme_task_failed",
            "deviceSimulator.errors.assetPreparationFailed",
            source.to_string(),
        )
    })?
}

#[tauri::command]
pub async fn device_simulator_get_asset_status(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
    profile_ids: Vec<String>,
) -> Result<AssetStatus, SimulatorErrorBody> {
    validate_profile_ids(&profile_ids)?;
    if profile_ids.is_empty() {
        return Ok(AssetStatus {
            state: AssetState::Unknown,
            profile_ids,
            packs: Vec::new(),
            update_available: false,
            error_code: None,
        });
    }
    match load_catalog_context(&app_handle, app_state.inner(), true).await {
        Ok(context) => {
            let paths = context.paths;
            let catalog = context.cached.catalog;
            tokio::task::spawn_blocking(move || {
                asset_status_from_catalog(&paths, &catalog, profile_ids)
            })
            .await
            .map_err(|source| {
                runtime_error(
                    "device_simulator.assets.status_task_failed",
                    "deviceSimulator.errors.assetPreparationFailed",
                    source.to_string(),
                )
            })?
        }
        Err(error) => Ok(AssetStatus {
            state: AssetState::Missing,
            packs: profile_ids
                .iter()
                .map(|profile_id| AssetPackStatus {
                    id: profile_id.clone(),
                    required_version: "unavailable".into(),
                    installed_version: None,
                    size: 0,
                    state: AssetState::Missing,
                    error_code: Some(error.code.clone()),
                })
                .collect(),
            profile_ids,
            update_available: false,
            error_code: Some(error.code),
        }),
    }
}

#[tauri::command]
pub async fn device_simulator_prepare_assets(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    profile_ids: Vec<String>,
) -> Result<String, SimulatorErrorBody> {
    validate_profile_ids(&profile_ids)?;
    if profile_ids.is_empty() {
        return Err(runtime_error(
            "device_simulator.assets.profile_selection_empty",
            "deviceSimulator.errors.assetSelectionEmpty",
            "select at least one profile before preparing assets",
        ));
    }
    let context = load_catalog_context(&app_handle, app_state.inner(), true).await?;
    let client = build_asset_http_client().map_err(|source| {
        runtime_error(
            source.code,
            "deviceSimulator.errors.assetPreparationFailed",
            source.message,
        )
    })?;
    let service = AssetPreparationService::new(context.paths, client, context.base_url);
    let job_id = uuid::Uuid::new_v4().simple().to_string();
    let (cancel, cancel_rx) = watch::channel(false);
    {
        let mut active = simulator_state.asset_job.lock().await;
        if active.is_some() {
            return Err(runtime_error(
                "device_simulator.assets.job_already_running",
                "deviceSimulator.errors.assetJobRunning",
                "another asset preparation job is already running",
            ));
        }
        *active = Some(AssetJobControl {
            id: job_id.clone(),
            cancel,
        });
    }
    let jobs = Arc::clone(&simulator_state.asset_job);
    let catalog = context.cached.catalog;
    let event_app = app_handle.clone();
    let event_job_id = job_id.clone();
    tokio::spawn(async move {
        let started = Instant::now();
        let result = service
            .prepare_profiles(&catalog, &profile_ids, cancel_rx, |progress| {
                let state = match progress.phase {
                    AssetPreparationPhase::Resolving | AssetPreparationPhase::CheckingDisk => {
                        AssetState::Checking
                    }
                    AssetPreparationPhase::Downloading => AssetState::Downloading,
                    AssetPreparationPhase::Installing => AssetState::Installing,
                    AssetPreparationPhase::Activating => AssetState::Verifying,
                };
                let elapsed = started.elapsed().as_secs_f64();
                let speed_bps = if elapsed > 0.0 {
                    (progress.downloaded_bytes as f64 / elapsed) as u64
                } else {
                    0
                };
                let snapshot = AssetProgressSnapshot {
                    job_id: event_job_id.clone(),
                    state,
                    current_pack_id: progress.current_pack.map(|pack| pack.id),
                    downloaded: progress.downloaded_bytes,
                    total: Some(progress.total_download_bytes),
                    speed_bps,
                    error: None,
                };
                let _ = event_app.emit(DEVICE_SIMULATOR_EVENT_ASSET_PROGRESS, snapshot);
            })
            .await;
        let final_progress = match result {
            Ok(_) => AssetProgressSnapshot {
                job_id: event_job_id.clone(),
                state: AssetState::Ready,
                current_pack_id: None,
                downloaded: 0,
                total: None,
                speed_bps: 0,
                error: None,
            },
            Err(error) => AssetProgressSnapshot {
                job_id: event_job_id.clone(),
                state: AssetState::Failed,
                current_pack_id: None,
                downloaded: 0,
                total: None,
                speed_bps: 0,
                error: Some(runtime_error(
                    error.code,
                    "deviceSimulator.errors.assetPreparationFailed",
                    error.message,
                )),
            },
        };
        let _ = event_app.emit(DEVICE_SIMULATOR_EVENT_ASSET_PROGRESS, final_progress);
        let mut active = jobs.lock().await;
        if active.as_ref().is_some_and(|job| job.id == event_job_id) {
            *active = None;
        }
    });
    Ok(job_id)
}

#[tauri::command]
pub async fn device_simulator_cancel_asset_download(
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    job_id: String,
) -> Result<(), SimulatorErrorBody> {
    if job_id.trim().is_empty() {
        return Err(runtime_error(
            "device_simulator.assets.job_id_invalid",
            "deviceSimulator.errors.assetJobInvalid",
            "asset download job id is empty",
        ));
    }
    let active = simulator_state.asset_job.lock().await;
    let Some(job) = active.as_ref() else {
        return Err(runtime_error(
            "device_simulator.assets.job_not_found",
            "deviceSimulator.errors.assetJobInvalid",
            "there is no active asset preparation job",
        ));
    };
    if job.id != job_id {
        return Err(runtime_error(
            "device_simulator.assets.job_id_mismatch",
            "deviceSimulator.errors.assetJobInvalid",
            "the requested asset job is not active",
        ));
    }
    job.cancel.send(true).map_err(|source| {
        runtime_error(
            "device_simulator.assets.cancel_failed",
            "deviceSimulator.errors.assetPreparationFailed",
            source.to_string(),
        )
    })
}

#[tauri::command]
pub fn device_simulator_preview_devices(
    request: SimulatorStartRequest,
) -> Result<DevicePreview, SimulatorErrorBody> {
    preview_devices(&request)
}

#[tauri::command]
pub async fn device_simulator_preflight(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    request: SimulatorStartRequest,
) -> Result<PreflightReport, SimulatorErrorBody> {
    build_preflight(
        &app_handle,
        app_state.inner(),
        simulator_state.inner(),
        &request,
    )
    .await
}

#[tauri::command]
pub async fn device_simulator_start(
    app_handle: AppHandle,
    app_state: State<'_, AppState>,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    request: SimulatorStartRequest,
) -> Result<SimulatorStatusSnapshot, SimulatorErrorBody> {
    let report = build_preflight(
        &app_handle,
        app_state.inner(),
        simulator_state.inner(),
        &request,
    )
    .await?;
    if !report.ok {
        let failed = report
            .checks
            .iter()
            .filter(|check| {
                check.status == app_lib::device_simulator::api::PreflightCheckStatus::Failed
            })
            .map(|check| check.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(runtime_error(
            "device_simulator.preflight.blocked",
            "deviceSimulator.errors.preflightBlocked",
            format!("blocking preflight checks: {failed}"),
        ));
    }

    if simulator_state.manager.has_worker().await {
        return Err(runtime_error(
            "device_simulator.session.already_active",
            "deviceSimulator.errors.sessionAlreadyActive",
            "an elevated simulator Worker is already connected",
        ));
    }
    let profile_ids = request
        .groups
        .iter()
        .map(|group| group.profile_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let pinned_packs = pin_runtime_assets(&app_handle, app_state.inner(), &profile_ids).await?;
    let runtime_app_data_dir = app_data_dir(&app_handle)?;
    let started = simulator_state
        .manager
        .begin_random_session()
        .map_err(manager_error)?;
    let session_id = started.identity.session_id.clone();
    emit_manager_status(&app_handle, started.status.clone());
    simulator_state
        .manager
        .transition(&session_id, SessionState::Preflighting)
        .map(|status| emit_manager_status(&app_handle, status))
        .map_err(manager_error)?;
    simulator_state
        .manager
        .transition(&session_id, SessionState::StartingWorker)
        .map(|status| emit_manager_status(&app_handle, status))
        .map_err(manager_error)?;

    let (notifications, receiver) = mpsc::unbounded_channel();
    spawn_manager_notification_forwarder(
        app_handle.clone(),
        Arc::clone(&simulator_state.manager),
        receiver,
    );
    if let Err(source) = simulator_state
        .manager
        .launch_worker(&started.identity, notifications)
        .await
    {
        let body = source.into_body();
        fail_manager_session(
            &app_handle,
            &simulator_state.manager,
            &session_id,
            body.clone(),
        );
        return Err(body);
    }

    let initialize = InitializeSessionPayload {
        app_data_dir: runtime_app_data_dir,
        request: request.clone(),
        preview: report.device_preview.clone(),
        pinned_packs,
        manage_firewall: app_state
            .config
            .lock()
            .unwrap()
            .device_simulator
            .manage_firewall,
    };
    if let Err(source) = worker_request::<SimulatorStatusSnapshot, _>(
        &simulator_state.manager,
        WorkerCommandName::InitializeSession,
        Some(&initialize),
    )
    .await
    {
        return Err(abort_start_after_worker_error(
            &app_handle,
            &simulator_state.manager,
            &session_id,
            source,
        )
        .await);
    }
    let worker_preflight = match worker_request::<
        app_lib::device_simulator::worker_runtime::WorkerPreflightResult,
        (),
    >(
        &simulator_state.manager,
        WorkerCommandName::RunPreflight,
        None,
    )
    .await
    {
        Ok(report) => report,
        Err(source) => {
            return Err(abort_start_after_worker_error(
                &app_handle,
                &simulator_state.manager,
                &session_id,
                source,
            )
            .await)
        }
    };
    if !worker_preflight.ok {
        let error = runtime_error(
            "device_simulator.worker.preflight_blocked",
            "deviceSimulator.errors.preflightBlocked",
            worker_preflight.blocking_codes.join(", "),
        );
        return Err(abort_start_after_worker_error(
            &app_handle,
            &simulator_state.manager,
            &session_id,
            error,
        )
        .await);
    }

    simulator_state
        .manager
        .transition(&session_id, SessionState::AddingIps)
        .map(|status| emit_manager_status(&app_handle, status))
        .map_err(manager_error)?;
    let worker_status = match worker_request::<SimulatorStatusSnapshot, ()>(
        &simulator_state.manager,
        WorkerCommandName::StartServices,
        None,
    )
    .await
    {
        Ok(status) => status,
        Err(source) => {
            return Err(abort_start_after_worker_error(
                &app_handle,
                &simulator_state.manager,
                &session_id,
                source,
            )
            .await)
        }
    };
    simulator_state
        .manager
        .transition(&session_id, SessionState::StartingServices)
        .map_err(manager_error)?;
    simulator_state
        .manager
        .transition(&session_id, SessionState::Running)
        .map_err(manager_error)?;
    let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_STATUS, &worker_status);
    spawn_runtime_telemetry_forwarder(
        app_handle.clone(),
        Arc::clone(&simulator_state.manager),
        session_id,
    );
    Ok(worker_status)
}

#[tauri::command]
pub async fn device_simulator_stop(
    app_handle: AppHandle,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
) -> Result<(), SimulatorErrorBody> {
    let status = status_with_recovery(&app_handle, simulator_state.inner()).await?;
    match status.state {
        SessionState::Idle | SessionState::Stopped | SessionState::Failed => {
            simulator_state
                .manager
                .shutdown_worker()
                .await
                .map_err(manager_error)?;
            Ok(())
        }
        SessionState::RecoveryRequired | SessionState::Recovering => Err(runtime_error(
            "device_simulator.recovery.required",
            "deviceSimulator.errors.recoveryRequired",
            "recover the journaled session instead of treating it as a normal stop",
        )),
        _ => stop_active_session(&app_handle, &simulator_state.manager).await,
    }
}

#[tauri::command]
pub async fn device_simulator_get_status(
    app_handle: AppHandle,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
) -> Result<SimulatorStatusSnapshot, SimulatorErrorBody> {
    if simulator_state.manager.has_worker().await {
        if let Ok(status) = worker_request::<SimulatorStatusSnapshot, ()>(
            &simulator_state.manager,
            WorkerCommandName::GetStatus,
            None,
        )
        .await
        {
            return Ok(status);
        }
    }
    status_with_recovery(&app_handle, simulator_state.inner()).await
}

#[tauri::command]
pub async fn device_simulator_import_alarm_image(
    app_handle: AppHandle,
) -> Result<Option<ImportedAlarmImage>, SimulatorErrorBody> {
    let selected = rfd::AsyncFileDialog::new()
        .add_filter("Alarm image", &["jpg", "jpeg", "png"])
        .pick_file()
        .await;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let source = selected.path().to_path_buf();
    let user_asset_root = app_data_dir(&app_handle)?
        .join("device-simulator")
        .join("user-alarm-images");
    tokio::task::spawn_blocking(move || import_alarm_image_file(&source, &user_asset_root))
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.alarm.image_import_task_failed",
                "deviceSimulator.errors.alarmImageImportFailed",
                source.to_string(),
            )
        })?
        .map(Some)
}

#[tauri::command]
pub async fn device_simulator_start_alarm(
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    request: AlarmJobRequest,
) -> Result<String, SimulatorErrorBody> {
    worker_request(
        &simulator_state.manager,
        WorkerCommandName::StartAlarmJob,
        Some(&AlarmJobCommandPayload { request }),
    )
    .await
}

#[tauri::command]
pub async fn device_simulator_trigger_alarm_once(
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    request: AlarmJobRequest,
) -> Result<AlarmTriggerResult, SimulatorErrorBody> {
    worker_request(
        &simulator_state.manager,
        WorkerCommandName::TriggerAlarmOnce,
        Some(&AlarmJobCommandPayload { request }),
    )
    .await
}

#[tauri::command]
pub async fn device_simulator_stop_alarm(
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    job_id: String,
) -> Result<(), SimulatorErrorBody> {
    if job_id.trim().is_empty() {
        return Err(runtime_error(
            "device_simulator.alarm.job_id_invalid",
            "deviceSimulator.errors.alarmJobInvalid",
            "alarm job id is empty",
        ));
    }
    let _: Option<serde_json::Value> = simulator_state
        .manager
        .request_worker(
            WorkerCommandName::StopAlarmJob,
            Some(&StopAlarmJobPayload { job_id }),
        )
        .await
        .map_err(manager_error)?;
    Ok(())
}

#[tauri::command]
pub async fn device_simulator_recover(
    app_handle: AppHandle,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    session_id: String,
) -> Result<RecoveryResult, SimulatorErrorBody> {
    let app_data_dir = app_data_dir(&app_handle)?;
    // A recovery Worker is always launched elevated. It owns process identity
    // verification and all resource mutations, so cleanup still works after a
    // desktop restart when the original Worker is no longer connected here.
    let _ = simulator_state.manager.shutdown_worker().await;
    let recovery_manager = Arc::new(SimulatorManager::default());
    recovery_manager
        .begin_session(session_id.clone())
        .map_err(manager_error)?;
    recovery_manager
        .transition(&session_id, SessionState::Preflighting)
        .map_err(manager_error)?;
    recovery_manager
        .transition(&session_id, SessionState::StartingWorker)
        .map_err(manager_error)?;
    let generated = PipeIdentity::generate();
    let identity = PipeIdentity {
        session_id: session_id.clone(),
        pipe_name: generated.pipe_name,
    };
    let (notifications, _notification_rx) = mpsc::unbounded_channel();
    recovery_manager
        .launch_worker(&identity, notifications)
        .await
        .map_err(manager_error)?;
    recovery_manager
        .transition(&session_id, SessionState::RecoveryRequired)
        .map_err(manager_error)?;
    recovery_manager
        .transition(&session_id, SessionState::Recovering)
        .map_err(manager_error)?;

    let payload = RecoverSessionPayload {
        app_data_dir,
        session_id: session_id.clone(),
    };
    let outcome = worker_request_with_timeout::<RecoveryResult, _>(
        &recovery_manager,
        WorkerCommandName::RecoverSession,
        Some(&payload),
        Duration::from_secs(120),
    )
    .await;
    let _ = recovery_manager.shutdown_worker().await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let status = simulator_state
                .manager
                .record_recovery_outcome(&session_id, false, Some(error.clone()))
                .map_err(manager_error)?;
            emit_manager_status(&app_handle, status);
            return Err(error);
        }
    };
    let status = simulator_state
        .manager
        .record_recovery_outcome(&session_id, outcome.recovered, outcome.error.clone())
        .map_err(manager_error)?;
    emit_manager_status(&app_handle, status);
    Ok(outcome)
}

pub async fn shutdown_for_exit(
    app_handle: &AppHandle,
    state: &DeviceSimulatorCommandState,
) -> Result<(), SimulatorErrorBody> {
    let status = status_with_recovery(app_handle, state).await?;
    match status.state {
        SessionState::Idle | SessionState::Stopped | SessionState::Failed => {
            state.manager.shutdown_worker().await.map_err(manager_error)
        }
        SessionState::RecoveryRequired | SessionState::Recovering => {
            // A residual/recovery journal must not trap the user inside the app.
            // The journal is durable on disk and is reconciled idempotently on
            // the next launch (foreign or stale resources are simply released),
            // so exiting now leaks nothing. Blocking exit here previously left
            // "cleanup failed" as the only outcome and forced users to kill the
            // process, which orphaned a hung instance holding the single-instance
            // guard and made the app impossible to relaunch. Let exit proceed and
            // shut down any connected worker best-effort.
            let _ = state.manager.shutdown_worker().await;
            Ok(())
        }
        _ => stop_active_session(app_handle, &state.manager).await,
    }
}

async fn pin_runtime_assets(
    app_handle: &AppHandle,
    app_state: &AppState,
    profile_ids: &[String],
) -> Result<Vec<PinnedPackDirectory>, SimulatorErrorBody> {
    let context = load_catalog_context(app_handle, app_state, false).await?;
    let paths = context.paths;
    let catalog = context.cached.catalog;
    let requested = {
        let mut requested = profile_ids.to_vec();
        requested.sort();
        requested.dedup();
        requested
    };
    tokio::task::spawn_blocking(move || {
        let pin = AssetStore::new(paths)
            .pin_active(&catalog)
            .map_err(|source| {
                runtime_error(
                    source.code,
                    "deviceSimulator.errors.assetPreparationFailed",
                    source.message,
                )
            })?;
        if pin.selection.profiles != requested
            || pin.selection.packs.len() != pin.pack_directories.len()
        {
            return Err(runtime_error(
                "device_simulator.assets.active_selection_mismatch",
                "deviceSimulator.errors.assetPreparationFailed",
                "active asset selection does not exactly match the requested profiles",
            ));
        }
        Ok(pin
            .selection
            .packs
            .into_iter()
            .zip(pin.pack_directories)
            .map(|(pack, directory)| PinnedPackDirectory {
                id: pack.id,
                version: pack.version.to_string(),
                directory,
            })
            .collect())
    })
    .await
    .map_err(|source| {
        runtime_error(
            "device_simulator.assets.pin_task_failed",
            "deviceSimulator.errors.assetPreparationFailed",
            source.to_string(),
        )
    })?
}

async fn worker_request<R, P>(
    manager: &SimulatorManager,
    command: WorkerCommandName,
    payload: Option<&P>,
) -> Result<R, SimulatorErrorBody>
where
    R: DeserializeOwned,
    P: Serialize,
{
    let value = manager
        .request_worker(command, payload)
        .await
        .map_err(manager_error)?
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.worker.response_payload_missing",
                "deviceSimulator.errors.workerCommandFailed",
                format!("Worker command {command:?} returned no payload"),
            )
        })?;
    serde_json::from_value(value).map_err(|source| {
        runtime_error(
            "device_simulator.worker.response_payload_invalid",
            "deviceSimulator.errors.workerCommandFailed",
            source.to_string(),
        )
    })
}

async fn worker_request_with_timeout<R, P>(
    manager: &SimulatorManager,
    command: WorkerCommandName,
    payload: Option<&P>,
    timeout: Duration,
) -> Result<R, SimulatorErrorBody>
where
    R: DeserializeOwned,
    P: Serialize,
{
    let value = manager
        .request_worker_with_timeout(command, payload, timeout)
        .await
        .map_err(manager_error)?
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.worker.response_payload_missing",
                "deviceSimulator.errors.workerCommandFailed",
                format!("Worker command {command:?} returned no payload"),
            )
        })?;
    serde_json::from_value(value).map_err(|source| {
        runtime_error(
            "device_simulator.worker.response_payload_invalid",
            "deviceSimulator.errors.workerCommandFailed",
            source.to_string(),
        )
    })
}

fn spawn_manager_notification_forwarder(
    app_handle: AppHandle,
    manager: Arc<SimulatorManager>,
    mut receiver: mpsc::UnboundedReceiver<ManagerNotification>,
) {
    tokio::spawn(async move {
        while let Some(notification) = receiver.recv().await {
            match notification {
                ManagerNotification::Heartbeat {
                    process_id,
                    heartbeat,
                } => {
                    let _ = manager.record_heartbeat(
                        &heartbeat.session_id,
                        process_id,
                        heartbeat.sent_at_ms,
                    );
                }
                ManagerNotification::Event(event) => match event.payload {
                    WorkerEventPayload::StatusChanged { current, .. } => {
                        let status = SimulatorStatusSnapshot::from(SimulatorStatus {
                            session_id: Some(event.session_id),
                            state: current,
                            updated_at_ms: event.emitted_at_ms,
                            error: None,
                        });
                        let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_STATUS, status);
                    }
                    WorkerEventPayload::Log {
                        level,
                        component,
                        message,
                        error_code,
                    } => {
                        let level = match level {
                            app_lib::device_simulator::events::WorkerLogLevel::Trace => "trace",
                            app_lib::device_simulator::events::WorkerLogLevel::Debug => "debug",
                            app_lib::device_simulator::events::WorkerLogLevel::Info => "info",
                            app_lib::device_simulator::events::WorkerLogLevel::Warn => "warning",
                            app_lib::device_simulator::events::WorkerLogLevel::Error => "error",
                        };
                        let payload = serde_json::json!({
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                            "level": level,
                            "session_id": event.session_id,
                            "component": component,
                            "profile_id": null,
                            "device_id": null,
                            "device_ip": null,
                            "channel_id": null,
                            "alarm_job_id": null,
                            "rtsp_session_id": null,
                            "error_code": error_code,
                            "message": message,
                        });
                        let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_LOG, payload);
                    }
                    WorkerEventPayload::AlarmStats { stats } => {
                        let payload = app_lib::device_simulator::api::AlarmJobStatsSnapshot {
                            job_id: stats.alarm_job_id,
                            state: stats.state,
                            attempted: stats.total,
                            succeeded: stats.succeeded,
                            failed: stats.failed,
                            unverified: 0,
                            in_flight: stats.in_flight,
                            average_duration_ms: 0.0,
                            last_http_status: None,
                            last_error: None,
                        };
                        let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_ALARM_STATS, payload);
                    }
                    WorkerEventPayload::FatalError { error } => {
                        if let Ok(status) = manager.fail(&event.session_id, error) {
                            emit_manager_status(&app_handle, status);
                        }
                    }
                    _ => {}
                },
                ManagerNotification::WorkerLost {
                    session_id,
                    process_id: _,
                    code,
                    details,
                } => {
                    if let Ok(mut status) = manager.record_worker_loss(&session_id, code) {
                        if let Some(error) = status.error.as_mut() {
                            error.details = Some(details);
                        }
                        emit_manager_status(&app_handle, status);
                    }
                }
            }
        }
    });
}

fn spawn_runtime_telemetry_forwarder(
    app_handle: AppHandle,
    manager: Arc<SimulatorManager>,
    session_id: String,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            let manager_status = manager.status();
            if manager_status.session_id.as_deref() != Some(session_id.as_str())
                || manager_status.state != SessionState::Running
            {
                break;
            }
            let snapshot = match worker_request::<RuntimeTelemetrySnapshot, ()>(
                &manager,
                WorkerCommandName::GetRuntimeTelemetry,
                None,
            )
            .await
            {
                Ok(snapshot) => snapshot,
                Err(_) => break,
            };
            let RuntimeTelemetrySnapshot { status, events } = snapshot;
            let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_STATUS, status);
            if let Some(device_status) = events.device_status {
                let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_DEVICE_STATUS, device_status);
            }
            if let Some(rtsp_stats) = events.rtsp_stats {
                let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_RTSP_STATS, rtsp_stats);
            }
            for alarm_stats in events.alarm_stats {
                let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_ALARM_STATS, alarm_stats);
            }
            if let Some(subscription) = events.alarm_subscription {
                let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_ALARM_SUBSCRIPTION, subscription);
            }
        }
    });
}

async fn stop_active_session(
    app_handle: &AppHandle,
    manager: &SimulatorManager,
) -> Result<(), SimulatorErrorBody> {
    let status = manager.status();
    let session_id = status.session_id.clone().ok_or_else(|| {
        runtime_error(
            "device_simulator.session.missing",
            "deviceSimulator.errors.workerCommandFailed",
            "active simulator status has no session ID",
        )
    })?;
    if status.state == SessionState::Running {
        manager
            .transition(&session_id, SessionState::StoppingAlarms)
            .map(|status| emit_manager_status(app_handle, status))
            .map_err(manager_error)?;
    }
    emit_cleanup_progress(
        app_handle,
        &session_id,
        "stopping_alarms",
        0,
        4,
        "deviceSimulator.cleanup.stoppingAlarms",
    );
    let worker_status = worker_request::<SimulatorStatusSnapshot, ()>(
        manager,
        WorkerCommandName::StopServices,
        None,
    )
    .await;
    let worker_status = match worker_status {
        Ok(status) => status,
        Err(source) => {
            let status = manager
                .record_stop_timeout(&session_id)
                .map_err(manager_error)?;
            emit_manager_status(app_handle, status);
            return Err(source);
        }
    };
    advance_manager_cleanup_to_stopped(app_handle, manager, &session_id)?;
    manager
        .mark_resources_released(&session_id)
        .map_err(manager_error)?;
    if let Err(source) = manager.shutdown_worker().await {
        let status = manager
            .record_stop_timeout(&session_id)
            .map_err(manager_error)?;
        emit_manager_status(app_handle, status);
        return Err(manager_error(source));
    }
    let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_STATUS, worker_status);
    Ok(())
}

fn advance_manager_cleanup_to_stopped(
    app_handle: &AppHandle,
    manager: &SimulatorManager,
    session_id: &str,
) -> Result<(), SimulatorErrorBody> {
    let current = manager.status().state;
    let path: &[SessionState] = match current {
        SessionState::StoppingAlarms => &[
            SessionState::StoppingServices,
            SessionState::RemovingFirewall,
            SessionState::RemovingIps,
            SessionState::Stopped,
        ],
        SessionState::StartingWorker => &[
            SessionState::StoppingServices,
            SessionState::RemovingFirewall,
            SessionState::RemovingIps,
            SessionState::Stopped,
        ],
        SessionState::AddingIps => &[SessionState::RemovingIps, SessionState::Stopped],
        SessionState::StartingServices => &[
            SessionState::StoppingServices,
            SessionState::RemovingFirewall,
            SessionState::RemovingIps,
            SessionState::Stopped,
        ],
        SessionState::RemovingFirewall => &[SessionState::RemovingIps, SessionState::Stopped],
        SessionState::RemovingIps => &[SessionState::Stopped],
        SessionState::Stopped => &[],
        _ => {
            return Err(runtime_error(
                "device_simulator.session.stop_state_invalid",
                "deviceSimulator.errors.workerCommandFailed",
                format!("cannot finish cleanup from {current:?}"),
            ))
        }
    };
    for state in path {
        manager
            .transition(session_id, *state)
            .map_err(manager_error)?;
        let (stage, completed, message_key) = match state {
            SessionState::StoppingServices => (
                "stopping_services",
                1,
                "deviceSimulator.cleanup.stoppingServices",
            ),
            SessionState::RemovingFirewall => (
                "removing_firewall",
                2,
                "deviceSimulator.cleanup.removingFirewall",
            ),
            SessionState::RemovingIps => ("removing_ips", 3, "deviceSimulator.cleanup.removingIps"),
            SessionState::Stopped => ("complete", 4, "deviceSimulator.cleanup.complete"),
            _ => continue,
        };
        emit_cleanup_progress(app_handle, session_id, stage, completed, 4, message_key);
    }
    Ok(())
}

fn emit_cleanup_progress(
    app_handle: &AppHandle,
    session_id: &str,
    stage: &str,
    completed: u32,
    total: u32,
    message_key: &str,
) {
    let payload = serde_json::json!({
        "session_id": session_id,
        "stage": stage,
        "completed": completed,
        "total": total,
        "message_key": message_key,
        "error": null,
    });
    let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_CLEANUP_PROGRESS, payload);
}

async fn abort_start_after_worker_error(
    app_handle: &AppHandle,
    manager: &SimulatorManager,
    session_id: &str,
    error: SimulatorErrorBody,
) -> SimulatorErrorBody {
    let worker_status =
        worker_request::<SimulatorStatusSnapshot, ()>(manager, WorkerCommandName::GetStatus, None)
            .await
            .ok();
    if worker_status
        .as_ref()
        .is_some_and(|status| matches!(status.state, SessionState::Stopped | SessionState::Failed))
    {
        let _ = manager.mark_resources_released(session_id);
    } else {
        let _ = worker_request::<SimulatorStatusSnapshot, ()>(
            manager,
            WorkerCommandName::StopServices,
            None,
        )
        .await;
        if worker_request::<SimulatorStatusSnapshot, ()>(
            manager,
            WorkerCommandName::GetStatus,
            None,
        )
        .await
        .is_ok_and(|status| matches!(status.state, SessionState::Stopped | SessionState::Failed))
        {
            let _ = manager.mark_resources_released(session_id);
        }
    }
    let _ = manager.shutdown_worker().await;
    fail_manager_session(app_handle, manager, session_id, error.clone());
    error
}

fn fail_manager_session(
    app_handle: &AppHandle,
    manager: &SimulatorManager,
    session_id: &str,
    error: SimulatorErrorBody,
) {
    if let Ok(status) = manager.fail(session_id, error) {
        emit_manager_status(app_handle, status);
    }
}

fn emit_manager_status(app_handle: &AppHandle, status: SimulatorStatus) {
    let snapshot = SimulatorStatusSnapshot::from(status);
    let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_STATUS, snapshot);
}

fn manager_error(source: app_lib::device_simulator::manager::ManagerError) -> SimulatorErrorBody {
    source.into_body()
}

async fn build_preflight(
    app_handle: &AppHandle,
    app_state: &AppState,
    simulator_state: &DeviceSimulatorCommandState,
    request: &SimulatorStartRequest,
) -> Result<PreflightReport, SimulatorErrorBody> {
    let interfaces = tokio::task::spawn_blocking(list_system_interfaces)
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.interface.task_failed",
                "deviceSimulator.errors.interfaceEnumerationFailed",
                source.to_string(),
            )
        })?
        .map_err(|source| {
            runtime_error(
                "device_simulator.interface.enumeration_failed",
                "deviceSimulator.errors.interfaceEnumerationFailed",
                source.to_string(),
            )
        })?;
    let local_addresses = interfaces
        .iter()
        .flat_map(|interface| interface.ipv4_addresses.iter())
        .map(|address| address.address)
        .collect::<HashSet<_>>();
    let planned_addresses = preview_devices(request)
        .map(|preview| {
            preview
                .devices
                .into_iter()
                .map(|device| device.ip)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut conflict_assessments = if planned_addresses.is_empty() {
        Vec::new()
    } else {
        let interface_id = request.interface_id.clone();
        let addresses_for_probe = planned_addresses.clone();
        let local_for_probe = local_addresses.clone();
        match tokio::task::spawn_blocking(move || {
            assess_system_address_conflicts(&interface_id, &addresses_for_probe, &local_for_probe)
        })
        .await
        {
            Ok(Ok(assessments)) => assessments,
            Ok(Err(source)) => unknown_address_conflict_assessments(
                &planned_addresses,
                &local_addresses,
                format!("Windows neighbor-table inspection failed: {source}"),
            ),
            Err(source) => unknown_address_conflict_assessments(
                &planned_addresses,
                &local_addresses,
                format!("neighbor-table inspection task failed: {source}"),
            ),
        }
    };
    attach_local_address_owners(&mut conflict_assessments, &interfaces);
    let requested_ports = [
        request.device_http_port,
        request.rtsp_ports.main,
        request.rtsp_ports.sub,
        request.rtsp_ports.third,
    ];
    let unavailable_tcp_ports =
        tokio::task::spawn_blocking(move || probe_tcp_ports(requested_ports))
            .await
            .map_err(|source| {
                runtime_error(
                    "device_simulator.preflight.port_probe_failed",
                    "deviceSimulator.errors.preflightFailed",
                    source.to_string(),
                )
            })?;
    let mut platform_connectivity = BTreeMap::new();
    for server in &request.platform.servers {
        let address = format!("{}:{}", server.host.trim(), server.port);
        let reachable = tokio::time::timeout(
            Duration::from_millis(750),
            tokio::net::TcpStream::connect(address),
        )
        .await
        .ok()
        .map(|result| result.is_ok());
        platform_connectivity.insert(server.id.clone(), reachable);
    }
    let residual_session_id = first_recovery_session(app_handle).await?;
    let settings = app_state.config.lock().unwrap().device_simulator.clone();
    let profile_ids = request
        .groups
        .iter()
        .map(|group| group.profile_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let (assets_ready, asset_details, profiles_static_reviewed, profiles_platform_verified) =
        match load_catalog_context(app_handle, app_state, false).await {
            Ok(context) => {
                let paths = context.paths;
                let catalog = context.cached.catalog;
                let selected = profile_ids.clone();
                let platform = request.platform.kind;
                tokio::task::spawn_blocking(move || {
                    let status = asset_status_from_catalog(&paths, &catalog, selected.clone())?;
                    let assets_ready = status.state == AssetState::Ready;
                    let mut static_reviewed = true;
                    let mut platform_verified = true;
                    if assets_ready {
                        for profile_id in &selected {
                            let profile_pack = catalog
                                .profiles
                                .iter()
                                .find(|profile| profile.id == *profile_id)
                                .and_then(|profile| {
                                    profile
                                        .required_packs
                                        .iter()
                                        .find(|pack| pack.id == *profile_id)
                                })
                                .ok_or_else(|| {
                                    runtime_error(
                                        "device_simulator.assets.profile_pack_missing",
                                        "deviceSimulator.errors.assetPreparationFailed",
                                        format!("profile pack for '{profile_id}' is missing"),
                                    )
                                })?;
                            let directory = paths.pack_dir(profile_pack).map_err(|source| {
                                runtime_error(
                                    source.code,
                                    "deviceSimulator.errors.assetPreparationFailed",
                                    source.message,
                                )
                            })?;
                            let profile = load_profile_from_pack(&directory, profile_id).map_err(
                                |source| {
                                    runtime_error(
                                        source.code,
                                        "deviceSimulator.errors.assetPreparationFailed",
                                        source.message,
                                    )
                                },
                            )?;
                            static_reviewed &= profile.evidence.iter().all(|evidence| {
                                matches!(
                                    evidence.status,
                                    EvidenceStatus::ReviewedStatic
                                        | EvidenceStatus::PlatformVerified
                                )
                            });
                            platform_verified &= profile.evidence.iter().all(|evidence| {
                                evidence.status == EvidenceStatus::PlatformVerified
                                    && evidence.verified_platforms.contains(&platform)
                            });
                        }
                    } else {
                        static_reviewed = false;
                        platform_verified = false;
                    }
                    Ok::<_, SimulatorErrorBody>((
                        assets_ready,
                        Some(format!(
                            "{} signed pack(s); catalog {}",
                            status.packs.len(),
                            catalog.generated_at
                        )),
                        static_reviewed,
                        platform_verified,
                    ))
                })
                .await
                .map_err(|source| {
                    runtime_error(
                        "device_simulator.preflight.asset_task_failed",
                        "deviceSimulator.errors.preflightFailed",
                        source.to_string(),
                    )
                })??
            }
            Err(error) => (
                false,
                Some(error.details.unwrap_or(error.code)),
                false,
                false,
            ),
        };
    let environment = PreflightEnvironment {
        interfaces,
        local_addresses,
        conflict_assessments,
        unavailable_tcp_ports,
        assets_ready,
        asset_details,
        profiles_static_reviewed,
        profiles_platform_verified,
        worker_available: cfg!(target_os = "windows")
            && std::env::current_exe().is_ok_and(|path| path.is_absolute()),
        firewall_required: settings.manage_firewall,
        firewall_available: cfg!(target_os = "windows"),
        residual_session_id,
        platform_connectivity,
    };
    let _ = simulator_state;
    Ok(run_preflight(request, &environment))
}

fn attach_local_address_owners(
    assessments: &mut [app_lib::device_simulator::windows::ip_alias::AddressConflictAssessment],
    interfaces: &[NetworkInterfaceInfo],
) {
    for assessment in assessments {
        for evidence in &mut assessment.evidence {
            if evidence.kind != ConflictEvidenceKind::Local {
                continue;
            }
            let owners = interfaces
                .iter()
                .filter(|interface| {
                    interface
                        .ipv4_addresses
                        .iter()
                        .any(|address| address.address == assessment.address)
                })
                .map(|interface| format!("{} ({})", interface.name, interface.description))
                .collect::<Vec<_>>();
            evidence.details = Some(if owners.is_empty() {
                "local network interface".into()
            } else {
                owners.join(", ")
            });
        }
    }
}

async fn status_with_recovery(
    app_handle: &AppHandle,
    state: &DeviceSimulatorCommandState,
) -> Result<SimulatorStatusSnapshot, SimulatorErrorBody> {
    let status = state.manager.status();
    if !matches!(
        status.state,
        SessionState::Idle | SessionState::Stopped | SessionState::Failed
    ) {
        return Ok(status.into());
    }
    if let Some(session_id) = first_recovery_session(app_handle).await? {
        return Ok(SimulatorStatusSnapshot::from(SimulatorStatus {
            session_id: Some(session_id),
            state: SessionState::RecoveryRequired,
            updated_at_ms: now_ms(),
            error: None,
        }));
    }
    Ok(status.into())
}

async fn first_recovery_session(
    app_handle: &AppHandle,
) -> Result<Option<String>, SimulatorErrorBody> {
    let app_data_dir = app_data_dir(app_handle)?;
    tokio::task::spawn_blocking(move || {
        SessionJournalStore::from_app_data_dir(app_data_dir)
            .list_non_terminal()
            .map(|journals| {
                journals
                    .into_iter()
                    .map(|journal| journal.session_id)
                    .next()
            })
    })
    .await
    .map_err(|source| {
        runtime_error(
            "device_simulator.recovery.scan_task_failed",
            "deviceSimulator.errors.recoveryScanFailed",
            source.to_string(),
        )
    })?
    .map_err(|source| source.into_body())
}

async fn load_catalog_context(
    app_handle: &AppHandle,
    app_state: &AppState,
    refresh_network: bool,
) -> Result<CatalogContext, SimulatorErrorBody> {
    let app_data_dir = app_data_dir(app_handle)?;
    let paths = AssetStorePaths::from_app_data_dir(&app_data_dir);
    let base_url = asset_base_url(app_state)?;
    let catalog_url = base_url.join("catalog-v1.json").map_err(|source| {
        runtime_error(
            "device_simulator.assets.catalog_url_invalid",
            "deviceSimulator.errors.assetCatalogUnpublished",
            source.to_string(),
        )
    })?;
    let current_version = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|source| {
        runtime_error(
            "device_simulator.assets.app_version_invalid",
            "deviceSimulator.errors.assetCatalogUnpublished",
            source.to_string(),
        )
    })?;
    let keys = trusted_catalog_keys();

    if refresh_network {
        let client = build_asset_http_client().map_err(|source| {
            runtime_error(
                source.code,
                "deviceSimulator.errors.assetCatalogUnpublished",
                source.message,
            )
        })?;
        match fetch_and_cache_signed_catalog(&client, &catalog_url, &paths, &keys, &current_version)
            .await
        {
            Ok(cached) => {
                return Ok(CatalogContext {
                    cached,
                    paths,
                    base_url,
                })
            }
            Err(network_error) => {
                let cached =
                    load_cached_catalog(paths.clone(), keys.clone(), current_version.clone())
                        .await?;
                if let Some(cached) = cached {
                    return Ok(CatalogContext {
                        cached,
                        paths,
                        base_url,
                    });
                }
                return Err(runtime_error(
                    network_error.code,
                    "deviceSimulator.errors.assetCatalogUnpublished",
                    network_error.message,
                ));
            }
        }
    }

    let cached = load_cached_catalog(paths.clone(), keys, current_version)
        .await?
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.assets.catalog_cache_missing",
                "deviceSimulator.errors.assetCatalogUnpublished",
                "no verified asset catalog is cached",
            )
        })?;
    Ok(CatalogContext {
        cached,
        paths,
        base_url,
    })
}

async fn load_cached_catalog(
    paths: AssetStorePaths,
    keys: Vec<app_lib::device_simulator::assets::signature::TrustedCatalogKey>,
    current_version: Version,
) -> Result<Option<CachedCatalog>, SimulatorErrorBody> {
    tokio::task::spawn_blocking(move || load_cached_signed_catalog(&paths, &keys, &current_version))
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.assets.catalog_cache_task_failed",
                "deviceSimulator.errors.assetCatalogUnpublished",
                source.to_string(),
            )
        })?
        .map_err(|source| {
            runtime_error(
                source.code,
                "deviceSimulator.errors.assetCatalogUnpublished",
                source.message,
            )
        })
}

fn asset_base_url(app_state: &AppState) -> Result<reqwest::Url, SimulatorErrorBody> {
    let config = app_state.config.lock().unwrap();
    let value = config
        .device_simulator
        .asset_server_url_override
        .clone()
        .unwrap_or_else(|| {
            format!(
                "{}/virtual-device-assets",
                config.update_server_url.trim_end_matches('/')
            )
        });
    let normalized = format!("{}/", value.trim().trim_end_matches('/'));
    let url = reqwest::Url::parse(&normalized).map_err(|source| {
        runtime_error(
            "device_simulator.assets.server_url_invalid",
            "deviceSimulator.errors.assetCatalogUnpublished",
            source.to_string(),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(runtime_error(
            "device_simulator.assets.server_url_invalid",
            "deviceSimulator.errors.assetCatalogUnpublished",
            "asset server URL must be absolute HTTP(S)",
        ));
    }
    Ok(url)
}

fn asset_status_from_catalog(
    paths: &AssetStorePaths,
    catalog: &CatalogV1,
    profile_ids: Vec<String>,
) -> Result<AssetStatus, SimulatorErrorBody> {
    let resolved = resolve_profile_dependencies(catalog, &profile_ids).map_err(|source| {
        runtime_error(
            source.code,
            "deviceSimulator.errors.assetPreparationFailed",
            source.message,
        )
    })?;
    let active = AssetStore::new(paths.clone())
        .load_active()
        .map_err(|source| {
            runtime_error(
                source.code,
                "deviceSimulator.errors.assetPreparationFailed",
                source.message,
            )
        })?;
    let mut packs = Vec::with_capacity(resolved.len());
    let mut has_missing = false;
    let mut has_failed = false;
    for pack_ref in &resolved {
        let expected = catalog
            .packs
            .iter()
            .find(|pack| pack.id == pack_ref.id && pack.version == pack_ref.version)
            .ok_or_else(|| {
                runtime_error(
                    "device_simulator.assets.catalog_pack_missing",
                    "deviceSimulator.errors.assetPreparationFailed",
                    format!("catalog is missing resolved pack {pack_ref}"),
                )
            })?;
        let directory = paths.pack_dir(pack_ref).map_err(|source| {
            runtime_error(
                source.code,
                "deviceSimulator.errors.assetPreparationFailed",
                source.message,
            )
        })?;
        let (state, installed_version, error_code) = if !directory.exists() {
            has_missing = true;
            (AssetState::Missing, None, None)
        } else {
            match validate_installed_pack(&directory, expected) {
                Ok(_) => (AssetState::Ready, Some(pack_ref.version.to_string()), None),
                Err(error) => {
                    has_failed = true;
                    (AssetState::Failed, None, Some(error.code.to_owned()))
                }
            }
        };
        packs.push(AssetPackStatus {
            id: pack_ref.id.clone(),
            required_version: pack_ref.version.to_string(),
            installed_version,
            size: expected.size,
            state,
            error_code,
        });
    }
    let mut requested_profiles = profile_ids.clone();
    requested_profiles.sort();
    requested_profiles.dedup();
    let active_matches = active.as_ref().is_some_and(|state| {
        state.active.profiles == requested_profiles && state.active.packs == resolved
    });
    let update_available = !has_missing && !has_failed && !active_matches;
    let state = if has_failed {
        AssetState::Failed
    } else if has_missing {
        AssetState::Missing
    } else if update_available {
        AssetState::UpdateAvailable
    } else {
        AssetState::Ready
    };
    Ok(AssetStatus {
        state,
        profile_ids,
        packs,
        update_available,
        error_code: has_failed.then(|| "device_simulator.assets.installed_pack_invalid".into()),
    })
}

fn apply_profile_availability(
    summaries: &mut [DeviceProfileSummary],
    paths: &AssetStorePaths,
    catalog: &CatalogV1,
) {
    for summary in summaries {
        let Some(profile) = catalog
            .profiles
            .iter()
            .find(|profile| profile.id == summary.id)
        else {
            continue;
        };
        let Ok(resolved) = resolve_profile_dependencies(catalog, &[summary.id.clone()]) else {
            continue;
        };
        let local = resolved.iter().all(|pack_ref| {
            let Some(expected) = catalog
                .packs
                .iter()
                .find(|pack| pack.id == pack_ref.id && pack.version == pack_ref.version)
            else {
                return false;
            };
            paths
                .pack_dir(pack_ref)
                .is_ok_and(|directory| validate_installed_pack(&directory, expected).is_ok())
        });
        let profile_pack = profile
            .required_packs
            .iter()
            .find(|pack| pack.id == summary.id);
        summary.available_version = profile_pack.map(|pack| pack.version.to_string());
        summary.installed_version = local
            .then(|| profile_pack.map(|pack| pack.version.to_string()))
            .flatten();
        summary.availability = if local {
            DeviceProfileAvailability::Local
        } else {
            DeviceProfileAvailability::Remote
        };
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AlarmTypesManifestSummary {
    schema_version: u32,
    profile_id: String,
    handler_id: String,
    definitions: Vec<AlarmTypeDefinitionSummary>,
}

#[derive(Debug, Deserialize)]
struct AlarmTypeDefinitionSummary {
    id: String,
    display_name: String,
    platforms: Vec<app_lib::device_simulator::profiles::scope::TargetPlatform>,
    supports_pictures: bool,
}

fn list_active_alarm_types(
    paths: &AssetStorePaths,
    catalog: &CatalogV1,
) -> Result<Vec<ProfileAlarmTypes>, SimulatorErrorBody> {
    let pin = AssetStore::new(paths.clone())
        .pin_active(catalog)
        .map_err(|source| {
            runtime_error(
                source.code,
                "deviceSimulator.errors.assetPreparationFailed",
                source.message,
            )
        })?;
    let active_profiles = pin
        .selection
        .profiles
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let pack_directories = pin
        .selection
        .packs
        .iter()
        .zip(pin.pack_directories)
        .map(|(pack, directory)| (pack.id.clone(), directory))
        .collect::<BTreeMap<_, _>>();

    list_first_release_profiles()
        .into_iter()
        .filter(|profile| active_profiles.contains(&profile.id))
        .map(|profile| {
            let directory = pack_directories.get(&profile.id).ok_or_else(|| {
                runtime_error(
                    "device_simulator.assets.profile_pack_missing",
                    "deviceSimulator.errors.assetPreparationFailed",
                    format!("active profile pack '{}' is missing", profile.id),
                )
            })?;
            let bytes = read_alarm_types_manifest(directory, &profile.id)?;
            parse_alarm_types_manifest(&profile.id, &bytes)
        })
        .collect()
}

fn read_alarm_types_manifest(
    pack_directory: &Path,
    profile_id: &str,
) -> Result<Vec<u8>, SimulatorErrorBody> {
    let path = pack_directory.join("runtime").join("alarm-types.json");
    let metadata = fs::symlink_metadata(&path).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.manifest_read_failed",
            "deviceSimulator.errors.assetPreparationFailed",
            format!("failed to inspect alarm manifest for '{profile_id}': {source}"),
        )
    })?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_ALARM_TYPES_MANIFEST_BYTES
    {
        return Err(runtime_error(
            "device_simulator.alarm.manifest_file_invalid",
            "deviceSimulator.errors.assetPreparationFailed",
            format!("alarm manifest for '{profile_id}' is not a bounded regular file"),
        ));
    }
    fs::read(path).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.manifest_read_failed",
            "deviceSimulator.errors.assetPreparationFailed",
            format!("failed to read alarm manifest for '{profile_id}': {source}"),
        )
    })
}

fn parse_alarm_types_manifest(
    profile_id: &str,
    bytes: &[u8],
) -> Result<ProfileAlarmTypes, SimulatorErrorBody> {
    use app_lib::device_simulator::profiles::scope::TargetPlatform;

    let manifest: AlarmTypesManifestSummary = serde_json::from_slice(bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.manifest_invalid",
            "deviceSimulator.errors.assetPreparationFailed",
            format!("alarm manifest for '{profile_id}' is invalid: {source}"),
        )
    })?;
    let handler_id = AlarmHandlerId::from_str(&manifest.handler_id).map_err(|source| {
        runtime_error(
            source.code,
            "deviceSimulator.errors.assetPreparationFailed",
            source.message,
        )
    })?;
    if manifest.schema_version != 1
        || manifest.profile_id != profile_id
        || handler_id.profile_id().as_str() != profile_id
    {
        return Err(runtime_error(
            "device_simulator.alarm.manifest_identity_mismatch",
            "deviceSimulator.errors.assetPreparationFailed",
            format!("alarm manifest for '{profile_id}' has the wrong identity"),
        ));
    }
    let mut seen = BTreeSet::new();
    let alarm_types = manifest
        .definitions
        .into_iter()
        .filter(|definition| definition.platforms.contains(&TargetPlatform::Ums))
        .map(|definition| {
            let alarm_type_id = AlarmTypeId::new(definition.id).map_err(|source| {
                runtime_error(
                    source.code,
                    "deviceSimulator.errors.assetPreparationFailed",
                    source.message,
                )
            })?;
            if definition.display_name.trim().is_empty()
                || !seen.insert(alarm_type_id.as_str().to_owned())
            {
                return Err(runtime_error(
                    "device_simulator.alarm.manifest_invalid",
                    "deviceSimulator.errors.assetPreparationFailed",
                    format!("alarm manifest for '{profile_id}' has an empty name or duplicate ID"),
                ));
            }
            Ok(AlarmTypeSummary {
                id: alarm_type_id.as_str().to_owned(),
                display_name: definition.display_name,
                supports_pictures: definition.supports_pictures,
            })
        })
        .collect::<Result<Vec<_>, SimulatorErrorBody>>()?;
    Ok(ProfileAlarmTypes {
        profile_id: profile_id.to_owned(),
        alarm_types,
    })
}

fn import_alarm_image_file(
    source: &Path,
    user_asset_root: &Path,
) -> Result<ImportedAlarmImage, SimulatorErrorBody> {
    let source_metadata = fs::symlink_metadata(source).map_err(|source_error| {
        alarm_image_import_error(
            "device_simulator.alarm.image_source_unavailable",
            format!("failed to inspect selected alarm image: {source_error}"),
        )
    })?;
    if source_metadata.file_type().is_symlink() {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_source_symlink_forbidden",
            "symbolic links cannot be imported as alarm images",
        ));
    }
    if !source_metadata.is_file() {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_source_not_file",
            "the selected alarm image is not a regular file",
        ));
    }
    if source_metadata.len() == 0 || source_metadata.len() > MAX_IMPORTED_ALARM_IMAGE_BYTES {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_source_size_invalid",
            "alarm images must be non-empty and no larger than 16 MiB",
        ));
    }

    let source_extension = source
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "jpg" | "jpeg" | "png"))
        .ok_or_else(|| {
            alarm_image_import_error(
                "device_simulator.alarm.image_source_type_unsupported",
                "alarm images must use a .jpg, .jpeg, or .png extension",
            )
        })?;
    let extension = if source_extension == "jpeg" {
        "jpg".to_owned()
    } else {
        source_extension
    };
    let file = fs::File::open(source).map_err(|source_error| {
        alarm_image_import_error(
            "device_simulator.alarm.image_source_read_failed",
            format!("failed to open selected alarm image: {source_error}"),
        )
    })?;
    let mut bytes = Vec::with_capacity(source_metadata.len() as usize);
    file.take(MAX_IMPORTED_ALARM_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source_error| {
            alarm_image_import_error(
                "device_simulator.alarm.image_source_read_failed",
                format!("failed to read selected alarm image: {source_error}"),
            )
        })?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMPORTED_ALARM_IMAGE_BYTES {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_source_size_invalid",
            "alarm images must be non-empty and no larger than 16 MiB",
        ));
    }
    let signature_matches = match extension.as_str() {
        "png" => bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "jpg" | "jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        _ => false,
    };
    if !signature_matches {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_source_signature_invalid",
            "the selected file contents do not match its JPEG or PNG extension",
        ));
    }

    let image_id = format!("{:x}", Sha256::digest(&bytes));
    fs::create_dir_all(user_asset_root).map_err(|source_error| {
        alarm_image_import_error(
            "device_simulator.alarm.image_store_create_failed",
            format!("failed to create user alarm image storage: {source_error}"),
        )
    })?;
    let root_metadata = fs::symlink_metadata(user_asset_root).map_err(|source_error| {
        alarm_image_import_error(
            "device_simulator.alarm.image_store_unavailable",
            format!("failed to inspect user alarm image storage: {source_error}"),
        )
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_store_invalid",
            "user alarm image storage must be a regular directory, not a symbolic link",
        ));
    }

    let destination = user_asset_root.join(format!("{image_id}.{extension}"));
    match fs::symlink_metadata(&destination) {
        Ok(_) => verify_imported_alarm_image(&destination, bytes.len() as u64, &image_id)?,
        Err(source_error) if source_error.kind() == std::io::ErrorKind::NotFound => {
            atomic_store_alarm_image(user_asset_root, &destination, &image_id, &bytes)?;
        }
        Err(source_error) => {
            return Err(alarm_image_import_error(
                "device_simulator.alarm.image_store_unavailable",
                format!("failed to inspect imported alarm image: {source_error}"),
            ));
        }
    }

    Ok(ImportedAlarmImage {
        image_id,
        file_name: source
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("alarm-image.{extension}")),
        extension,
        size: bytes.len() as u64,
    })
}

fn atomic_store_alarm_image(
    root: &Path,
    destination: &Path,
    image_id: &str,
    bytes: &[u8],
) -> Result<(), SimulatorErrorBody> {
    let temporary = root.join(format!(".{image_id}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let write_result = (|| -> Result<(), std::io::Error> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, destination)
    })();
    if let Err(source_error) = write_result {
        let destination_exists = fs::symlink_metadata(destination).is_ok();
        let _ = fs::remove_file(&temporary);
        if destination_exists {
            return verify_imported_alarm_image(destination, bytes.len() as u64, image_id);
        }
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_store_write_failed",
            format!("failed to atomically store imported alarm image: {source_error}"),
        ));
    }
    verify_imported_alarm_image(destination, bytes.len() as u64, image_id)
}

fn verify_imported_alarm_image(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), SimulatorErrorBody> {
    let metadata = fs::symlink_metadata(path).map_err(|source_error| {
        alarm_image_import_error(
            "device_simulator.alarm.image_store_unavailable",
            format!("failed to inspect stored alarm image: {source_error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected_size {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_store_conflict",
            "an existing content-addressed alarm image does not match the imported content",
        ));
    }
    let stored = fs::read(path).map_err(|source_error| {
        alarm_image_import_error(
            "device_simulator.alarm.image_store_read_failed",
            format!("failed to verify stored alarm image: {source_error}"),
        )
    })?;
    if format!("{:x}", Sha256::digest(&stored)) != expected_sha256 {
        return Err(alarm_image_import_error(
            "device_simulator.alarm.image_store_conflict",
            "an existing content-addressed alarm image does not match the imported content",
        ));
    }
    Ok(())
}

fn alarm_image_import_error(code: &'static str, details: impl Into<String>) -> SimulatorErrorBody {
    runtime_error(
        code,
        "deviceSimulator.errors.alarmImageImportFailed",
        details,
    )
}

fn app_data_dir(app_handle: &AppHandle) -> Result<std::path::PathBuf, SimulatorErrorBody> {
    app_handle.path().app_data_dir().map_err(|source| {
        runtime_error(
            "device_simulator.path.app_data_unavailable",
            "deviceSimulator.errors.appDataUnavailable",
            source.to_string(),
        )
    })
}

fn probe_tcp_ports(ports: [u16; 4]) -> BTreeSet<u16> {
    ports
        .into_iter()
        .filter(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).is_err())
        .collect()
}

fn validate_profile_ids(profile_ids: &[String]) -> Result<(), SimulatorErrorBody> {
    let allowed = list_first_release_profiles()
        .into_iter()
        .map(|profile| profile.id)
        .collect::<BTreeSet<_>>();
    if let Some(profile_id) = profile_ids.iter().find(|id| !allowed.contains(id.as_str())) {
        return Err(runtime_error(
            "device_simulator.validation.profile_unknown",
            "deviceSimulator.errors.validationFailed",
            format!("unknown first-release profile '{profile_id}'"),
        ));
    }
    Ok(())
}

fn settings_error(code: &'static str, details: String) -> SimulatorErrorBody {
    runtime_error(code, "deviceSimulator.errors.settingsInvalid", details)
}

fn runtime_error(
    code: impl Into<String>,
    message_key: impl Into<String>,
    details: impl Into<String>,
) -> SimulatorErrorBody {
    SimulatorErrorBody::new(code, message_key).with_public_details(details)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_lib::device_simulator::assets::catalog::{
        CatalogPack, CatalogProfile, DeviceKind, PackKind, PackRef,
    };
    use tempfile::TempDir;

    #[test]
    fn port_probe_is_bounded_to_requested_ports_and_releases_successful_binds() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let occupied = listener.local_addr().unwrap().port();
        let free_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let free = free_listener.local_addr().unwrap().port();
        drop(free_listener);
        let result = probe_tcp_ports([occupied, free, free, free]);
        assert!(result.contains(&occupied));
        assert!(!result.contains(&free));
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, free)).is_ok());
    }

    #[test]
    fn asset_status_never_claims_absent_profile_packs_are_ready() {
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        let pack = PackRef {
            id: "ipc-custom".into(),
            version: Version::new(1, 0, 0),
        };
        let catalog = CatalogV1 {
            schema_version: 1,
            generated_at: "2026-07-19T03:30:00+08:00".into(),
            engine_api: 1,
            packs: vec![CatalogPack {
                id: pack.id.clone(),
                version: pack.version.clone(),
                kind: PackKind::DeviceProfile,
                url: "packs/ipc-custom/1.0.0/ipc-custom-1.0.0.zip".into(),
                sha256: "0".repeat(64),
                size: 10,
                unpacked_size: 10,
                dependencies: vec![],
                min_app_version: Version::new(1, 2, 1),
            }],
            profiles: vec![CatalogProfile {
                id: "ipc-custom".into(),
                device_kind: DeviceKind::Ipc,
                required_packs: vec![pack],
            }],
        };
        let status =
            asset_status_from_catalog(&paths, &catalog, vec!["ipc-custom".into()]).unwrap();
        assert_eq!(status.state, AssetState::Missing);
        assert_eq!(status.error_code, None);
        assert_eq!(status.packs[0].required_version, "1.0.0");
    }

    #[test]
    fn alarm_image_import_is_content_addressed_and_idempotent() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("capture.PNG");
        let bytes = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x01, 0x02,
        ];
        fs::write(&source, bytes).unwrap();
        let store = root.path().join("user-alarm-images");

        let imported = import_alarm_image_file(&source, &store).unwrap();
        let expected_id = format!("{:x}", Sha256::digest(bytes));
        assert_eq!(imported.image_id, expected_id);
        assert_eq!(imported.extension, "png");
        assert_eq!(imported.file_name, "capture.PNG");
        assert_eq!(imported.size, bytes.len() as u64);
        assert_eq!(
            fs::read(store.join(format!("{expected_id}.png"))).unwrap(),
            bytes
        );

        let second = import_alarm_image_file(&source, &store).unwrap();
        assert_eq!(second, imported);
        assert_eq!(fs::read_dir(&store).unwrap().count(), 1);
    }

    #[test]
    fn alarm_image_import_rejects_extension_spoofing() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("not-an-image.jpg");
        fs::write(&source, b"plain text").unwrap();

        let error = import_alarm_image_file(&source, &root.path().join("store")).unwrap_err();
        assert_eq!(
            error.code,
            "device_simulator.alarm.image_source_signature_invalid"
        );
    }

    #[test]
    fn alarm_type_manifest_projection_keeps_only_ums_fields() {
        let manifest = serde_json::json!({
            "schema_version": 1,
            "profile_id": "ipc-smart",
            "handler_id": "alarm.smart.v1",
            "definitions": [
                {
                    "id": "motion",
                    "display_name": "Motion",
                    "platforms": ["ums"],
                    "supports_pictures": true,
                    "protocol": "v1_0"
                },
                {
                    "id": "not-for-ums",
                    "display_name": "Not for UMS",
                    "platforms": [],
                    "supports_pictures": false
                }
            ]
        });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let result = parse_alarm_types_manifest("ipc-smart", &bytes).unwrap();
        assert_eq!(result.profile_id, "ipc-smart");
        assert_eq!(
            result.alarm_types,
            vec![AlarmTypeSummary {
                id: "motion".into(),
                display_name: "Motion".into(),
                supports_pictures: true,
            }]
        );
    }
}
