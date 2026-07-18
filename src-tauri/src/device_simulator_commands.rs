use crate::{config, AppState};
use app_lib::device_simulator::api::{
    command_not_ready, list_first_release_profiles, preview_devices, AlarmJobRequest,
    AlarmTriggerResult, AssetPackStatus, AssetStatus, DevicePreview, DeviceProfileSummary,
    PreflightReport, RecoveryResult, SimulatorStartRequest, SimulatorStatusSnapshot,
    DEVICE_SIMULATOR_EVENT_STATUS,
};
use app_lib::device_simulator::errors::{SimulatorError, SimulatorErrorBody, SimulatorResult};
use app_lib::device_simulator::manager::SimulatorManager;
use app_lib::device_simulator::models::{AssetState, SessionState, SimulatorStatus};
use app_lib::device_simulator::preflight::{run_preflight, PreflightEnvironment};
use app_lib::device_simulator::session_journal::{SessionJournalStore, SessionResourceCleaner};
use app_lib::device_simulator::windows::firewall::{FirewallBackend, SystemFirewallBackend};
use app_lib::device_simulator::windows::interfaces::{
    list_system_interfaces, NetworkInterfaceInfo,
};
use app_lib::device_simulator::windows::ip_alias::{IpAliasBackend, SystemIpAliasBackend};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, TcpListener};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
pub struct DeviceSimulatorCommandState {
    manager: SimulatorManager,
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
pub fn device_simulator_list_profiles() -> Vec<DeviceProfileSummary> {
    list_first_release_profiles()
}

#[tauri::command]
pub fn device_simulator_get_asset_status(
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
    Ok(AssetStatus {
        state: AssetState::Missing,
        packs: profile_ids
            .iter()
            .map(|profile_id| AssetPackStatus {
                id: profile_id.clone(),
                required_version: "unpublished".into(),
                installed_version: None,
                size: 0,
                state: AssetState::Missing,
                error_code: Some("device_simulator.assets.catalog_unpublished".into()),
            })
            .collect(),
        profile_ids,
        update_available: false,
        error_code: Some("device_simulator.assets.catalog_unpublished".into()),
    })
}

#[tauri::command]
pub fn device_simulator_prepare_assets(
    profile_ids: Vec<String>,
) -> Result<String, SimulatorErrorBody> {
    validate_profile_ids(&profile_ids)?;
    Err(runtime_error(
        "device_simulator.assets.catalog_unpublished",
        "deviceSimulator.errors.assetCatalogUnpublished",
        "no approved signed production catalog/public-key set is published in the current repository; asset preparation remains externally gated",
    ))
}

#[tauri::command]
pub fn device_simulator_cancel_asset_download(job_id: String) -> Result<(), SimulatorErrorBody> {
    if job_id.trim().is_empty() {
        return Err(runtime_error(
            "device_simulator.assets.job_id_invalid",
            "deviceSimulator.errors.assetJobInvalid",
            "asset download job id is empty",
        ));
    }
    Err(command_not_ready("cancel_asset_download"))
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

    // This branch intentionally remains closed until the evidence-backed
    // profile packs and Worker service orchestration are both approved.
    Err(command_not_ready("start"))
}

#[tauri::command]
pub async fn device_simulator_stop(
    app_handle: AppHandle,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
) -> Result<(), SimulatorErrorBody> {
    let status = status_with_recovery(&app_handle, simulator_state.inner()).await?;
    match status.state {
        SessionState::Idle | SessionState::Stopped | SessionState::Failed => Ok(()),
        SessionState::RecoveryRequired | SessionState::Recovering => Err(runtime_error(
            "device_simulator.recovery.required",
            "deviceSimulator.errors.recoveryRequired",
            "recover the journaled session instead of treating it as a normal stop",
        )),
        _ => Err(command_not_ready("stop")),
    }
}

#[tauri::command]
pub async fn device_simulator_get_status(
    app_handle: AppHandle,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
) -> Result<SimulatorStatusSnapshot, SimulatorErrorBody> {
    status_with_recovery(&app_handle, simulator_state.inner()).await
}

#[tauri::command]
pub fn device_simulator_start_alarm(
    _request: AlarmJobRequest,
) -> Result<String, SimulatorErrorBody> {
    Err(command_not_ready("start_alarm"))
}

#[tauri::command]
pub fn device_simulator_trigger_alarm_once(
    _request: AlarmJobRequest,
) -> Result<AlarmTriggerResult, SimulatorErrorBody> {
    Err(command_not_ready("trigger_alarm_once"))
}

#[tauri::command]
pub fn device_simulator_stop_alarm(job_id: String) -> Result<(), SimulatorErrorBody> {
    if job_id.trim().is_empty() {
        return Err(runtime_error(
            "device_simulator.alarm.job_id_invalid",
            "deviceSimulator.errors.alarmJobInvalid",
            "alarm job id is empty",
        ));
    }
    Err(command_not_ready("stop_alarm"))
}

#[tauri::command]
pub async fn device_simulator_recover(
    app_handle: AppHandle,
    simulator_state: State<'_, DeviceSimulatorCommandState>,
    session_id: String,
) -> Result<RecoveryResult, SimulatorErrorBody> {
    let app_data_dir = app_data_dir(&app_handle)?;
    let store = SessionJournalStore::from_app_data_dir(&app_data_dir);
    let session_for_load = session_id.clone();
    let journal = tokio::task::spawn_blocking(move || store.load(&session_for_load))
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.recovery.task_failed",
                "deviceSimulator.errors.recoveryFailed",
                source.to_string(),
            )
        })?
        .map_err(|source| source.into_body())?;

    // A recorded Worker identity must be reconciled in an isolated Windows VM
    // before resource mutation. This avoids deleting aliases while a possibly
    // live Worker still owns listeners.
    if journal.worker_process.is_some() {
        return Err(runtime_error(
            "device_simulator.recovery.worker_presence_unverified",
            "deviceSimulator.errors.recoveryWorkerUnverified",
            "the journal records a Worker process; process identity/liveness must be verified in the isolated Windows acceptance environment",
        ));
    }

    let store = SessionJournalStore::from_app_data_dir(&app_data_dir);
    let session_for_recovery = session_id.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut cleaner = SystemSessionCleaner::default();
        store.recover_session(journal, &mut cleaner, now_ms())
    })
    .await
    .map_err(|source| {
        runtime_error(
            "device_simulator.recovery.task_failed",
            "deviceSimulator.errors.recoveryFailed",
            source.to_string(),
        )
    })?
    .map_err(|source| source.into_body())?;

    let status = SimulatorStatusSnapshot::from(SimulatorStatus {
        session_id: Some(session_for_recovery.clone()),
        state: outcome.journal.state,
        updated_at_ms: outcome.journal.updated_at_ms,
        error: outcome.error.clone(),
    });
    let _ = app_handle.emit(DEVICE_SIMULATOR_EVENT_STATUS, &status);
    let _ = simulator_state;
    Ok(RecoveryResult {
        session_id: session_for_recovery,
        recovered: outcome.recovered,
        remaining_resources: outcome.remaining_resources,
        error: outcome.error,
    })
}

pub async fn shutdown_for_exit(
    app_handle: &AppHandle,
    state: &DeviceSimulatorCommandState,
) -> Result<(), SimulatorErrorBody> {
    let status = status_with_recovery(app_handle, state).await?;
    match status.state {
        SessionState::Idle | SessionState::Stopped | SessionState::Failed => Ok(()),
        SessionState::RecoveryRequired | SessionState::Recovering => Err(runtime_error(
            "device_simulator.exit.recovery_required",
            "deviceSimulator.errors.exitRecoveryRequired",
            "application exit cannot claim simulator cleanup while a recovery journal remains",
        )),
        _ => Err(command_not_ready("shutdown_for_exit")),
    }
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
        .collect();
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
        .map(|group| group.profile_id.as_str())
        .collect::<BTreeSet<_>>();
    let summaries = list_first_release_profiles();
    let profiles_platform_verified = profile_ids.iter().all(|profile_id| {
        summaries.iter().any(|summary| {
            summary.id == *profile_id && summary.verified_platforms.contains(&request.platform.kind)
        })
    });
    let environment = PreflightEnvironment {
        interfaces,
        local_addresses,
        conflict_assessments: Vec::new(),
        unavailable_tcp_ports,
        assets_ready: false,
        asset_details: Some(
            "approved signed production catalog and profile packs are not published".into(),
        ),
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

#[derive(Default)]
struct SystemSessionCleaner {
    firewall: SystemFirewallBackend,
    ip_alias: SystemIpAliasBackend,
}

impl SessionResourceCleaner for SystemSessionCleaner {
    fn stop_alarm_jobs(&mut self, _session_id: &str) -> SimulatorResult<()> {
        Ok(())
    }

    fn stop_services(&mut self, _session_id: &str) -> SimulatorResult<()> {
        Ok(())
    }

    fn firewall_rule_exists(&mut self, rule_name: &str) -> SimulatorResult<bool> {
        self.firewall
            .list_managed_rules()
            .map(|rules| rules.iter().any(|rule| rule.name == rule_name))
            .map_err(|source| {
                cleanup_error(
                    "device_simulator.recovery.firewall_query_failed",
                    source.to_string(),
                )
            })
    }

    fn remove_firewall_rule(&mut self, rule_name: &str) -> SimulatorResult<()> {
        self.firewall.delete_rule(rule_name).map_err(|source| {
            cleanup_error(
                "device_simulator.recovery.firewall_remove_failed",
                source.to_string(),
            )
        })
    }

    fn ip_address_exists(
        &mut self,
        interface_id: &str,
        address: Ipv4Addr,
    ) -> SimulatorResult<bool> {
        let interfaces = list_system_interfaces().map_err(|source| {
            cleanup_error(
                "device_simulator.recovery.interface_query_failed",
                source.to_string(),
            )
        })?;
        Ok(interfaces.iter().any(|interface| {
            interface.id.as_str() == interface_id
                && interface
                    .ipv4_addresses
                    .iter()
                    .any(|item| item.address == address)
        }))
    }

    fn remove_ip_address(
        &mut self,
        interface_id: &str,
        address: Ipv4Addr,
        prefix_len: u8,
    ) -> SimulatorResult<()> {
        self.ip_alias
            .remove_alias(interface_id, address, prefix_len)
            .map_err(|source| {
                cleanup_error(
                    "device_simulator.recovery.ip_remove_failed",
                    source.to_string(),
                )
            })
    }

    fn pack_pin_exists(&mut self, _pack_id: &str, _version: &str) -> SimulatorResult<bool> {
        // Session pins are in-memory guards. If no Worker process is recorded,
        // there is no live pin object to release.
        Ok(false)
    }

    fn release_pack_pin(&mut self, _pack_id: &str, _version: &str) -> SimulatorResult<()> {
        Ok(())
    }
}

fn cleanup_error(code: &'static str, details: String) -> SimulatorError {
    SimulatorError::new(code, "deviceSimulator.errors.recoveryFailed").with_public_details(details)
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
    fn asset_status_never_claims_unpublished_profile_packs_are_ready() {
        let status = device_simulator_get_asset_status(vec!["ipc-custom".into()]).unwrap();
        assert_eq!(status.state, AssetState::Missing);
        assert_eq!(
            status.error_code.as_deref(),
            Some("device_simulator.assets.catalog_unpublished")
        );
    }
}
