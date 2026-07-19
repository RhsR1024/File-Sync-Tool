use crate::device_simulator::alarm_runtime::{AlarmRuntime, AlarmRuntimeConfig, AlarmRuntimeError};
use crate::device_simulator::api::{
    preview_devices, AlarmJobRequest, AlarmJobStatsSnapshot, AlarmTriggerResult, DevicePreview,
    DeviceRuntimeStatusSnapshot, RtspStatsSnapshot, RuntimeEventBatcher, RuntimeTelemetrySnapshot,
    SimulatorMetricsSnapshot, SimulatorStatusSnapshot, TargetPlatformServer,
};
use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::{AlarmJobState, SessionState};
use crate::device_simulator::protocol_runtime::{
    ProtocolRuntime, ProtocolRuntimeConfig, ProtocolRuntimeError, ProtocolRuntimeStats,
    ProtocolRuntimeSummary,
};
use crate::device_simulator::runtime_assets::RuntimeAssetLayout;
use crate::device_simulator::session_journal::{
    CleanupProgress, DeviceRequestSummary, JournalCleanupStage, OwnedFirewallRule, OwnedIpAddress,
    OwnedPack, OwnedResources, ResourceOwnershipState, SessionJournalStore, SessionJournalV1,
    WorkerProcessIdentity, SESSION_JOURNAL_SCHEMA_VERSION,
};
use crate::device_simulator::windows::firewall::{
    plan_firewall_rules, FirewallBackend, FirewallProtocol, FirewallRemoteScope, FirewallRuleSpec,
    FirewallServiceIntent, SystemFirewallBackend,
};
use crate::device_simulator::windows::ip_alias::{
    IpAliasBackend, Ipv4Subnet, SystemIpAliasBackend,
};
use crate::device_simulator::worker_protocol::InitializeSessionPayload;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::future::Future;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ALIAS_READY_ATTEMPTS: usize = 20;
const ALIAS_READY_INTERVAL: Duration = Duration::from_millis(100);

pub type WorkerServiceFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WorkerRuntimeError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRuntimeError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for WorkerRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WorkerRuntimeError {}

impl WorkerRuntimeError {
    pub fn into_body(self) -> SimulatorErrorBody {
        SimulatorErrorBody::new(self.code, "deviceSimulator.errors.workerCommandFailed")
            .with_public_details(self.message)
            .retryable(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerPreflightResult {
    pub ok: bool,
    pub blocking_codes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerServiceStartConfig {
    pub request: crate::device_simulator::api::SimulatorStartRequest,
    pub preview: DevicePreview,
    pub assets: Option<Arc<RuntimeAssetLayout>>,
    pub app_data_dir: PathBuf,
}

pub trait WorkerServiceRuntime: Send {
    fn start<'a>(
        &'a mut self,
        config: WorkerServiceStartConfig,
    ) -> WorkerServiceFuture<'a, ProtocolRuntimeSummary>;
    fn stop_alarms<'a>(&'a mut self) -> WorkerServiceFuture<'a, ()>;
    fn stop_protocol<'a>(&'a mut self) -> WorkerServiceFuture<'a, ()>;
    fn start_alarm_job<'a>(
        &'a mut self,
        request: AlarmJobRequest,
    ) -> WorkerServiceFuture<'a, String>;
    fn trigger_alarm_once<'a>(
        &'a mut self,
        request: AlarmJobRequest,
    ) -> WorkerServiceFuture<'a, AlarmTriggerResult>;
    fn stop_alarm_job<'a>(&'a mut self, job_id: String) -> WorkerServiceFuture<'a, ()>;
    fn alarm_stats<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Vec<AlarmJobStatsSnapshot>> + Send + 'a>>;
    fn protocol_stats(&self) -> ProtocolRuntimeStats;
}

#[derive(Default)]
pub struct SystemWorkerServices {
    protocol: Option<ProtocolRuntime>,
    alarms: Option<AlarmRuntime>,
}

impl WorkerServiceRuntime for SystemWorkerServices {
    fn start<'a>(
        &'a mut self,
        config: WorkerServiceStartConfig,
    ) -> WorkerServiceFuture<'a, ProtocolRuntimeSummary> {
        Box::pin(async move {
            if self.protocol.is_some() || self.alarms.is_some() {
                return Err(runtime_error(
                    "device_simulator.worker.services_already_started",
                    "Worker services are already active",
                ));
            }
            let assets = config.assets.ok_or_else(|| {
                runtime_error(
                    "device_simulator.worker.assets_missing",
                    "Worker service start requires a pinned runtime asset layout",
                )
            })?;
            let alarm_config = AlarmRuntimeConfig {
                platform: config.request.platform.kind,
                target: config.request.platform.clone(),
                preview: config.preview.clone(),
                device_http_port: config.request.device_http_port,
                assets: Arc::clone(&assets),
                app_data_dir: config.app_data_dir,
            };
            let alarms = tokio::task::spawn_blocking(move || AlarmRuntime::new(alarm_config))
                .await
                .map_err(|source| {
                    runtime_error(
                        "device_simulator.alarm.runtime_task_failed",
                        format!("alarm runtime initialization task failed: {source}"),
                    )
                })?
                .map_err(alarm_runtime_error)?;
            let picture_cache = alarms.image_cache();
            let protocol = ProtocolRuntime::start(ProtocolRuntimeConfig {
                request: config.request,
                preview: config.preview,
                assets,
                picture_cache,
                enable_discovery: true,
            })
            .await
            .map_err(protocol_runtime_error)?;
            let summary = protocol.summary().clone();
            self.alarms = Some(alarms);
            self.protocol = Some(protocol);
            Ok(summary)
        })
    }

    fn stop_alarms<'a>(&'a mut self) -> WorkerServiceFuture<'a, ()> {
        Box::pin(async move {
            if let Some(alarms) = self.alarms.as_ref() {
                alarms.stop_all().await.map_err(alarm_runtime_error)?;
            }
            self.alarms = None;
            Ok(())
        })
    }

    fn stop_protocol<'a>(&'a mut self) -> WorkerServiceFuture<'a, ()> {
        Box::pin(async move {
            if let Some(protocol) = self.protocol.take() {
                protocol.stop().await.map_err(protocol_runtime_error)?;
            }
            Ok(())
        })
    }

    fn start_alarm_job<'a>(
        &'a mut self,
        request: AlarmJobRequest,
    ) -> WorkerServiceFuture<'a, String> {
        Box::pin(async move {
            self.alarms
                .as_ref()
                .ok_or_else(|| {
                    runtime_error(
                        "device_simulator.alarm.runtime_not_started",
                        "alarm runtime is not active",
                    )
                })?
                .start_job(request)
                .await
                .map_err(alarm_runtime_error)
        })
    }

    fn trigger_alarm_once<'a>(
        &'a mut self,
        request: AlarmJobRequest,
    ) -> WorkerServiceFuture<'a, AlarmTriggerResult> {
        Box::pin(async move {
            self.alarms
                .as_ref()
                .ok_or_else(|| {
                    runtime_error(
                        "device_simulator.alarm.runtime_not_started",
                        "alarm runtime is not active",
                    )
                })?
                .trigger_once(request)
                .await
                .map_err(alarm_runtime_error)
        })
    }

    fn stop_alarm_job<'a>(&'a mut self, job_id: String) -> WorkerServiceFuture<'a, ()> {
        Box::pin(async move {
            self.alarms
                .as_ref()
                .ok_or_else(|| {
                    runtime_error(
                        "device_simulator.alarm.runtime_not_started",
                        "alarm runtime is not active",
                    )
                })?
                .stop_job(&job_id)
                .await
                .map_err(alarm_runtime_error)
        })
    }

    fn alarm_stats<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Vec<AlarmJobStatsSnapshot>> + Send + 'a>> {
        Box::pin(async move {
            match self.alarms.as_ref() {
                Some(alarms) => alarms.stats().await,
                None => Vec::new(),
            }
        })
    }

    fn protocol_stats(&self) -> ProtocolRuntimeStats {
        self.protocol
            .as_ref()
            .map(ProtocolRuntime::stats)
            .unwrap_or_default()
    }
}

struct InitializedSession {
    payload: InitializeSessionPayload,
    assets: Option<Arc<RuntimeAssetLayout>>,
    firewall_rules: Vec<FirewallRuleSpec>,
}

pub struct WorkerRuntime {
    session_id: String,
    state: SessionState,
    last_error: Option<SimulatorErrorBody>,
    started_at_ms: Option<u64>,
    preflight_ok: bool,
    initialized: Option<InitializedSession>,
    journal_store: Option<SessionJournalStore>,
    journal: Option<SessionJournalV1>,
    ip_alias: Arc<dyn IpAliasBackend>,
    firewall: Arc<dyn FirewallBackend>,
    services: Box<dyn WorkerServiceRuntime>,
    worker_process: Option<WorkerProcessIdentity>,
    event_batcher: RuntimeEventBatcher,
}

impl WorkerRuntime {
    pub fn system(
        session_id: impl Into<String>,
        worker_process: Option<WorkerProcessIdentity>,
    ) -> Self {
        Self::new(
            session_id,
            worker_process,
            Arc::new(SystemIpAliasBackend),
            Arc::new(SystemFirewallBackend),
            Box::<SystemWorkerServices>::default(),
        )
    }

    pub fn new(
        session_id: impl Into<String>,
        worker_process: Option<WorkerProcessIdentity>,
        ip_alias: Arc<dyn IpAliasBackend>,
        firewall: Arc<dyn FirewallBackend>,
        services: Box<dyn WorkerServiceRuntime>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            state: SessionState::StartingWorker,
            last_error: None,
            started_at_ms: None,
            preflight_ok: false,
            initialized: None,
            journal_store: None,
            journal: None,
            ip_alias,
            firewall,
            services,
            worker_process,
            event_batcher: RuntimeEventBatcher::default(),
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub async fn initialize_session(
        &mut self,
        payload: InitializeSessionPayload,
    ) -> Result<SimulatorStatusSnapshot, WorkerRuntimeError> {
        if self.initialized.is_some() || self.state != SessionState::StartingWorker {
            return Err(runtime_error(
                "device_simulator.worker.initialize_state_invalid",
                "Worker session can only be initialized once after handshake",
            ));
        }
        if !payload.app_data_dir.is_absolute() {
            return Err(runtime_error(
                "device_simulator.worker.app_data_path_invalid",
                "Worker app data directory must be absolute",
            ));
        }
        let expected_preview = preview_devices(&payload.request).map_err(|source| {
            runtime_error(
                "device_simulator.worker.request_invalid",
                source.details.unwrap_or(source.code),
            )
        })?;
        if expected_preview != payload.preview {
            return Err(runtime_error(
                "device_simulator.worker.preview_mismatch",
                "Worker preview does not match the deterministic start request",
            ));
        }
        let selected_profiles = payload
            .request
            .groups
            .iter()
            .map(|group| group.profile_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let pins = payload.pinned_packs.clone();
        let selected_for_load = selected_profiles.clone();
        let assets = tokio::task::spawn_blocking(move || {
            RuntimeAssetLayout::load(&pins, &selected_for_load)
        })
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.worker.asset_task_failed",
                format!("runtime asset load task failed: {source}"),
            )
        })?
        .map_err(|source| runtime_error(source.code, source.message))?;
        let remote_scope = if payload.manage_firewall {
            let servers = payload.request.platform.servers.clone();
            let remote_addresses =
                tokio::task::spawn_blocking(move || resolve_platform_ipv4_addresses(&servers))
                    .await
                    .map_err(|source| {
                        runtime_error(
                            "device_simulator.worker.firewall_scope_task_failed",
                            format!("platform address resolution task failed: {source}"),
                        )
                    })??;
            FirewallRemoteScope::Addresses(remote_addresses)
        } else {
            let subnet =
                Ipv4Subnet::from_address(payload.request.start_ip, payload.request.subnet_prefix)
                    .map_err(|source| {
                    runtime_error("device_simulator.worker.subnet_invalid", source.to_string())
                })?;
            FirewallRemoteScope::SelectedSubnet(subnet)
        };
        let firewall_rules = plan_session_firewall(&self.session_id, &payload, remote_scope)?;
        let now = now_ms();
        let total_nvr_channels = payload
            .preview
            .devices
            .iter()
            .filter_map(|device| device.channel_count)
            .map(u32::from)
            .sum();
        let journal = SessionJournalV1 {
            schema_version: SESSION_JOURNAL_SCHEMA_VERSION,
            session_id: self.session_id.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            app_version: env!("CARGO_PKG_VERSION").into(),
            worker_version: env!("CARGO_PKG_VERSION").into(),
            interface_id: payload.request.interface_id.clone(),
            device_summary: DeviceRequestSummary {
                profile_ids: selected_profiles,
                total_devices: payload.preview.total_devices,
                total_nvr_channels,
            },
            worker_process: self.worker_process.clone(),
            resources: OwnedResources {
                ip_addresses: payload
                    .preview
                    .devices
                    .iter()
                    .map(|device| OwnedIpAddress {
                        interface_id: payload.request.interface_id.clone(),
                        address: device.ip,
                        prefix_len: payload.request.subnet_prefix,
                        state: ResourceOwnershipState::Planned,
                    })
                    .collect(),
                firewall_rules: firewall_rules
                    .iter()
                    .map(|rule| OwnedFirewallRule {
                        // The backend mutation key is the stable rule ID. The
                        // display name remains recoverable from Firewall metadata.
                        rule_name: rule.rule_id.clone(),
                        state: ResourceOwnershipState::Planned,
                    })
                    .collect(),
                packs: payload
                    .pinned_packs
                    .iter()
                    .map(|pack| OwnedPack {
                        id: pack.id.clone(),
                        version: pack.version.clone(),
                        state: ResourceOwnershipState::Owned,
                    })
                    .collect(),
            },
            cleanup: CleanupProgress::default(),
            state: SessionState::Preflighting,
            last_error: None,
        };
        self.journal_store = Some(SessionJournalStore::from_app_data_dir(
            &payload.app_data_dir,
        ));
        self.journal = Some(journal);
        self.initialized = Some(InitializedSession {
            payload,
            assets: Some(Arc::new(assets)),
            firewall_rules,
        });
        self.state = SessionState::Preflighting;
        self.save_journal().await?;
        Ok(self.status_snapshot().await)
    }

    pub async fn run_preflight(&mut self) -> Result<WorkerPreflightResult, WorkerRuntimeError> {
        if self.state != SessionState::Preflighting {
            return Err(runtime_error(
                "device_simulator.worker.preflight_state_invalid",
                "Worker preflight is only valid after initialization",
            ));
        }
        let initialized = self.initialized.as_ref().ok_or_else(|| {
            runtime_error(
                "device_simulator.worker.session_not_initialized",
                "Worker session is not initialized",
            )
        })?;
        let requested = initialized
            .payload
            .preview
            .devices
            .iter()
            .map(|device| device.ip)
            .collect::<BTreeSet<_>>();
        let backend = Arc::clone(&self.ip_alias);
        let local = tokio::task::spawn_blocking(move || backend.list_local_addresses())
            .await
            .map_err(|source| {
                runtime_error(
                    "device_simulator.worker.preflight_task_failed",
                    format!("IP preflight task failed: {source}"),
                )
            })?
            .map_err(|source| {
                runtime_error(
                    "device_simulator.worker.ip_query_failed",
                    source.to_string(),
                )
            })?;
        let conflicts = requested
            .into_iter()
            .filter(|address| local.contains(address))
            .collect::<Vec<_>>();
        let result = WorkerPreflightResult {
            ok: conflicts.is_empty(),
            blocking_codes: conflicts
                .iter()
                .map(|address| format!("device_simulator.ip.conflict:{address}"))
                .collect(),
            warnings: vec![
                "protocol fixtures are reviewed_static; real UMS compatibility remains unverified"
                    .into(),
            ],
        };
        self.preflight_ok = result.ok;
        Ok(result)
    }

    pub async fn start_services(&mut self) -> Result<SimulatorStatusSnapshot, WorkerRuntimeError> {
        if self.state != SessionState::Preflighting || !self.preflight_ok {
            return Err(runtime_error(
                "device_simulator.worker.start_state_invalid",
                "Worker services require a successful Worker preflight",
            ));
        }
        self.set_state(SessionState::AddingIps).await?;
        let ip_resources = self
            .journal
            .as_ref()
            .expect("initialized journal")
            .resources
            .ip_addresses
            .clone();
        for (index, resource) in ip_resources.into_iter().enumerate() {
            let backend = Arc::clone(&self.ip_alias);
            let interface_id = resource.interface_id.clone();
            let added = tokio::task::spawn_blocking(move || {
                backend.add_alias(&interface_id, resource.address, resource.prefix_len)
            })
            .await
            .map_err(|source| {
                runtime_error(
                    "device_simulator.worker.ip_add_task_failed",
                    format!("IP add task failed: {source}"),
                )
            })?
            .map_err(|source| {
                runtime_error("device_simulator.worker.ip_add_failed", source.to_string())
            });
            if let Err(source) = added {
                return self.fail_start_and_cleanup(source).await;
            }
            self.journal
                .as_mut()
                .expect("initialized journal")
                .resources
                .ip_addresses[index]
                .state = ResourceOwnershipState::Owned;
            self.save_journal().await?;
            if let Err(source) = self.wait_for_alias(resource.address).await {
                return self.fail_start_and_cleanup(source).await;
            }
        }

        self.set_state(SessionState::StartingServices).await?;
        let initialized = self.initialized.as_ref().expect("initialized session");
        if initialized.payload.manage_firewall {
            let rules = initialized.firewall_rules.clone();
            for (index, rule) in rules.into_iter().enumerate() {
                let backend = Arc::clone(&self.firewall);
                let rule_for_create = rule.clone();
                if let Err(source) =
                    tokio::task::spawn_blocking(move || backend.create_rule(&rule_for_create))
                        .await
                        .map_err(|source| {
                            runtime_error(
                                "device_simulator.worker.firewall_task_failed",
                                format!("firewall create task failed: {source}"),
                            )
                        })?
                        .map_err(|source| {
                            runtime_error(
                                "device_simulator.worker.firewall_create_failed",
                                source.to_string(),
                            )
                        })
                {
                    return self.fail_start_and_cleanup(source).await;
                }
                self.journal
                    .as_mut()
                    .expect("initialized journal")
                    .resources
                    .firewall_rules[index]
                    .state = ResourceOwnershipState::Owned;
                self.save_journal().await?;
            }
        } else {
            for resource in &mut self
                .journal
                .as_mut()
                .expect("initialized journal")
                .resources
                .firewall_rules
            {
                resource.state = ResourceOwnershipState::Released;
            }
            self.save_journal().await?;
        }

        let service_config = WorkerServiceStartConfig {
            request: initialized.payload.request.clone(),
            preview: initialized.payload.preview.clone(),
            assets: initialized.assets.clone(),
            app_data_dir: initialized.payload.app_data_dir.clone(),
        };
        if let Err(source) = self.services.start(service_config).await {
            return self.fail_start_and_cleanup(source).await;
        }
        self.started_at_ms = Some(now_ms());
        self.set_state(SessionState::Running).await?;
        Ok(self.status_snapshot().await)
    }

    pub async fn stop_services(&mut self) -> Result<SimulatorStatusSnapshot, WorkerRuntimeError> {
        if matches!(self.state, SessionState::Stopped | SessionState::Failed) {
            return Ok(self.status_snapshot().await);
        }
        self.cleanup_resources(SessionState::Stopped).await?;
        Ok(self.status_snapshot().await)
    }

    pub async fn start_alarm_job(
        &mut self,
        request: AlarmJobRequest,
    ) -> Result<String, WorkerRuntimeError> {
        self.ensure_running()?;
        self.services.start_alarm_job(request).await
    }

    pub async fn trigger_alarm_once(
        &mut self,
        request: AlarmJobRequest,
    ) -> Result<AlarmTriggerResult, WorkerRuntimeError> {
        self.ensure_running()?;
        self.services.trigger_alarm_once(request).await
    }

    pub async fn stop_alarm_job(&mut self, job_id: String) -> Result<(), WorkerRuntimeError> {
        self.ensure_running()?;
        self.services.stop_alarm_job(job_id).await
    }

    pub async fn status_snapshot(&self) -> SimulatorStatusSnapshot {
        let protocol_stats = self.services.protocol_stats();
        let alarm_stats = self.services.alarm_stats().await;
        self.build_status_snapshot(&protocol_stats, &alarm_stats)
    }

    pub async fn telemetry_snapshot(&mut self) -> RuntimeTelemetrySnapshot {
        let protocol_stats = self.services.protocol_stats();
        let alarm_stats = self.services.alarm_stats().await;
        let status = self.build_status_snapshot(&protocol_stats, &alarm_stats);
        if let Some(initialized) = self.initialized.as_ref() {
            for device in &initialized.payload.preview.devices {
                self.event_batcher
                    .update_device(DeviceRuntimeStatusSnapshot {
                        device_id: device.device_id.clone(),
                        online: self.state == SessionState::Running,
                        active_http_connections: protocol_stats
                            .active_http_connections_by_device
                            .get(&device.device_id)
                            .copied()
                            .unwrap_or(0),
                        active_rtsp_clients: protocol_stats
                            .active_rtsp_clients_by_device
                            .get(&device.device_id)
                            .copied()
                            .unwrap_or(0),
                        last_error_code: None,
                    });
            }
        }
        self.event_batcher.update_rtsp(RtspStatsSnapshot {
            session_id: self.session_id.clone(),
            active_clients: protocol_stats.active_rtsp_clients,
            bitrate_kbps: protocol_stats.outbound_bitrate_kbps,
            bytes_sent: protocol_stats.bytes_sent,
            disconnected_clients: protocol_stats.disconnected_clients,
        });
        for stats in alarm_stats {
            self.event_batcher.update_alarm(stats);
        }
        RuntimeTelemetrySnapshot {
            status,
            events: self.event_batcher.drain(&self.session_id, usize::MAX),
        }
    }

    fn build_status_snapshot(
        &self,
        protocol_stats: &ProtocolRuntimeStats,
        alarm_stats: &[AlarmJobStatsSnapshot],
    ) -> SimulatorStatusSnapshot {
        let total_devices = self
            .initialized
            .as_ref()
            .map(|session| session.payload.preview.total_devices)
            .unwrap_or(0);
        let total_channels = self
            .initialized
            .as_ref()
            .map(|session| session.payload.preview.total_channels)
            .unwrap_or(0);
        SimulatorStatusSnapshot {
            state: self.state,
            session_id: Some(self.session_id.clone()),
            started_at: self.started_at_ms.map(format_timestamp_ms),
            phase_progress: phase_progress(self.state),
            metrics: SimulatorMetricsSnapshot {
                total_devices,
                online_devices: if self.state == SessionState::Running {
                    total_devices
                } else {
                    0
                },
                total_channels,
                active_rtsp_clients: protocol_stats.active_rtsp_clients,
                outbound_bitrate_kbps: protocol_stats.outbound_bitrate_kbps,
                active_alarm_jobs: alarm_stats
                    .iter()
                    .filter(|job| {
                        matches!(job.state, AlarmJobState::Starting | AlarmJobState::Running)
                    })
                    .count()
                    .try_into()
                    .unwrap_or(u32::MAX),
            },
            cleanup_stage: self
                .journal
                .as_ref()
                .map(|journal| format!("{:?}", journal.cleanup.stage).to_ascii_lowercase()),
            recovery_session_id: self
                .state
                .requires_recovery()
                .then(|| self.session_id.clone()),
            last_error: self.last_error.clone(),
        }
    }

    pub async fn alarm_stats(&self) -> Vec<AlarmJobStatsSnapshot> {
        self.services.alarm_stats().await
    }

    async fn fail_start_and_cleanup<T>(
        &mut self,
        source: WorkerRuntimeError,
    ) -> Result<T, WorkerRuntimeError> {
        self.last_error = Some(source.clone().into_body());
        match self.cleanup_resources(SessionState::Failed).await {
            Ok(()) => Err(source),
            Err(cleanup) => {
                self.state = SessionState::RecoveryRequired;
                self.last_error = Some(cleanup.clone().into_body());
                let _ = self.save_journal().await;
                Err(cleanup)
            }
        }
    }

    async fn cleanup_resources(
        &mut self,
        terminal_state: SessionState,
    ) -> Result<(), WorkerRuntimeError> {
        if self.journal.is_none() {
            self.state = terminal_state;
            return Ok(());
        }
        let mut failures = Vec::new();
        self.update_cleanup_stage(
            SessionState::StoppingAlarms,
            JournalCleanupStage::StoppingAlarms,
        )
        .await?;
        if let Err(source) = self.services.stop_alarms().await {
            failures.push(source);
        }
        self.update_cleanup_stage(
            SessionState::StoppingServices,
            JournalCleanupStage::StoppingServices,
        )
        .await?;
        if let Err(source) = self.services.stop_protocol().await {
            failures.push(source);
        }

        self.update_cleanup_stage(
            SessionState::RemovingFirewall,
            JournalCleanupStage::RemovingFirewall,
        )
        .await?;
        let firewall_resources = self
            .journal
            .as_ref()
            .expect("journal checked")
            .resources
            .firewall_rules
            .clone();
        for (index, resource) in firewall_resources.into_iter().enumerate() {
            if resource.state == ResourceOwnershipState::Owned {
                let backend = Arc::clone(&self.firewall);
                let rule_id = resource.rule_name.clone();
                match tokio::task::spawn_blocking(move || backend.delete_rule(&rule_id)).await {
                    Ok(Ok(())) => {
                        self.journal
                            .as_mut()
                            .expect("journal checked")
                            .resources
                            .firewall_rules[index]
                            .state = ResourceOwnershipState::Released;
                        self.save_journal().await?;
                    }
                    Ok(Err(source)) => failures.push(runtime_error(
                        "device_simulator.worker.firewall_remove_failed",
                        source.to_string(),
                    )),
                    Err(source) => failures.push(runtime_error(
                        "device_simulator.worker.firewall_task_failed",
                        format!("firewall delete task failed: {source}"),
                    )),
                }
            } else if resource.state == ResourceOwnershipState::Planned {
                self.journal
                    .as_mut()
                    .expect("journal checked")
                    .resources
                    .firewall_rules[index]
                    .state = ResourceOwnershipState::Released;
            }
        }
        self.save_journal().await?;

        self.update_cleanup_stage(SessionState::RemovingIps, JournalCleanupStage::RemovingIps)
            .await?;
        let ip_resources = self
            .journal
            .as_ref()
            .expect("journal checked")
            .resources
            .ip_addresses
            .clone();
        for (index, resource) in ip_resources.into_iter().enumerate().rev() {
            if resource.state == ResourceOwnershipState::Owned {
                let backend = Arc::clone(&self.ip_alias);
                let interface_id = resource.interface_id.clone();
                match tokio::task::spawn_blocking(move || {
                    backend.remove_alias(&interface_id, resource.address, resource.prefix_len)
                })
                .await
                {
                    Ok(Ok(())) => {
                        self.journal
                            .as_mut()
                            .expect("journal checked")
                            .resources
                            .ip_addresses[index]
                            .state = ResourceOwnershipState::Released;
                        self.save_journal().await?;
                    }
                    Ok(Err(source)) => failures.push(runtime_error(
                        "device_simulator.worker.ip_remove_failed",
                        source.to_string(),
                    )),
                    Err(source) => failures.push(runtime_error(
                        "device_simulator.worker.ip_remove_task_failed",
                        format!("IP remove task failed: {source}"),
                    )),
                }
            } else if resource.state == ResourceOwnershipState::Planned {
                self.journal
                    .as_mut()
                    .expect("journal checked")
                    .resources
                    .ip_addresses[index]
                    .state = ResourceOwnershipState::Released;
            }
        }
        self.save_journal().await?;

        {
            let journal = self.journal.as_mut().expect("journal checked");
            journal.cleanup.stage = JournalCleanupStage::ReleasingPacks;
            for pack in &mut journal.resources.packs {
                pack.state = ResourceOwnershipState::Released;
            }
        }
        self.save_journal().await?;

        if failures.is_empty() {
            let now = now_ms();
            let journal = self.journal.as_mut().expect("journal checked");
            journal.cleanup.stage = JournalCleanupStage::Complete;
            journal.cleanup.completed_at_ms = Some(now);
            journal.worker_process = None;
            journal.state = terminal_state;
            journal.updated_at_ms = now;
            journal.last_error = if terminal_state == SessionState::Failed {
                self.last_error.clone()
            } else {
                None
            };
            self.state = terminal_state;
            if terminal_state == SessionState::Stopped {
                self.last_error = None;
            }
            self.save_journal().await?;
            Ok(())
        } else {
            let details = failures
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ");
            let source = runtime_error(
                "device_simulator.worker.cleanup_incomplete",
                format!("Worker cleanup left owned resources: {details}"),
            );
            self.state = SessionState::RecoveryRequired;
            self.last_error = Some(source.clone().into_body());
            let journal = self.journal.as_mut().expect("journal checked");
            journal.state = SessionState::RecoveryRequired;
            journal.updated_at_ms = now_ms();
            journal.last_error = self.last_error.clone();
            self.save_journal().await?;
            Err(source)
        }
    }

    async fn update_cleanup_stage(
        &mut self,
        state: SessionState,
        stage: JournalCleanupStage,
    ) -> Result<(), WorkerRuntimeError> {
        let now = now_ms();
        let journal = self.journal.as_mut().expect("initialized journal");
        journal.state = state;
        journal.updated_at_ms = now;
        journal.cleanup.stage = stage;
        journal.cleanup.attempts = journal.cleanup.attempts.saturating_add(1);
        journal.cleanup.last_started_at_ms = Some(now);
        self.state = state;
        self.save_journal().await
    }

    async fn set_state(&mut self, state: SessionState) -> Result<(), WorkerRuntimeError> {
        self.state = state;
        if let Some(journal) = self.journal.as_mut() {
            journal.state = state;
            journal.updated_at_ms = now_ms();
            journal.last_error = self.last_error.clone();
            self.save_journal().await?;
        }
        Ok(())
    }

    async fn wait_for_alias(&self, address: Ipv4Addr) -> Result<(), WorkerRuntimeError> {
        for _ in 0..ALIAS_READY_ATTEMPTS {
            let backend = Arc::clone(&self.ip_alias);
            let addresses = tokio::task::spawn_blocking(move || backend.list_local_addresses())
                .await
                .map_err(|source| {
                    runtime_error(
                        "device_simulator.worker.ip_query_task_failed",
                        format!("IP query task failed: {source}"),
                    )
                })?
                .map_err(|source| {
                    runtime_error(
                        "device_simulator.worker.ip_query_failed",
                        source.to_string(),
                    )
                })?;
            if addresses.contains(&address) {
                return Ok(());
            }
            tokio::time::sleep(ALIAS_READY_INTERVAL).await;
        }
        Err(runtime_error(
            "device_simulator.worker.ip_not_ready",
            format!("added IP address {address} did not become observable in time"),
        ))
    }

    async fn save_journal(&self) -> Result<(), WorkerRuntimeError> {
        let store = self.journal_store.clone().ok_or_else(|| {
            runtime_error(
                "device_simulator.worker.journal_not_initialized",
                "Worker journal store is not initialized",
            )
        })?;
        let journal = self.journal.clone().ok_or_else(|| {
            runtime_error(
                "device_simulator.worker.journal_not_initialized",
                "Worker journal is not initialized",
            )
        })?;
        tokio::task::spawn_blocking(move || store.save(&journal))
            .await
            .map_err(|source| {
                runtime_error(
                    "device_simulator.worker.journal_task_failed",
                    format!("journal persistence task failed: {source}"),
                )
            })?
            .map_err(|source| {
                runtime_error(
                    "device_simulator.worker.journal_save_failed",
                    source.to_string(),
                )
            })
    }

    fn ensure_running(&self) -> Result<(), WorkerRuntimeError> {
        if self.state != SessionState::Running {
            return Err(runtime_error(
                "device_simulator.worker.session_not_running",
                "alarm commands require a running simulator session",
            ));
        }
        Ok(())
    }
}

fn plan_session_firewall(
    session_id: &str,
    payload: &InitializeSessionPayload,
    remote_scope: FirewallRemoteScope,
) -> Result<Vec<FirewallRuleSpec>, WorkerRuntimeError> {
    let addresses = payload
        .preview
        .devices
        .iter()
        .map(|device| device.ip)
        .collect::<Vec<_>>();
    let program = std::env::current_exe().map_err(|source| {
        runtime_error(
            "device_simulator.worker.executable_unavailable",
            format!("Worker executable path is unavailable: {source}"),
        )
    })?;
    let intents = vec![
        FirewallServiceIntent {
            service_id: "http".into(),
            protocol: FirewallProtocol::Tcp,
            local_ports: vec![payload.request.device_http_port],
            local_addresses: addresses.clone(),
            remote_scope: remote_scope.clone(),
        },
        FirewallServiceIntent {
            service_id: "rtsp".into(),
            protocol: FirewallProtocol::Tcp,
            local_ports: vec![
                payload.request.rtsp_ports.main,
                payload.request.rtsp_ports.sub,
                payload.request.rtsp_ports.third,
            ],
            local_addresses: addresses.clone(),
            remote_scope: remote_scope.clone(),
        },
        FirewallServiceIntent {
            service_id: "discovery".into(),
            protocol: FirewallProtocol::Udp,
            local_ports: vec![crate::device_simulator::discovery::DISCOVERY_LISTEN_PORT],
            local_addresses: addresses,
            remote_scope,
        },
    ];
    plan_firewall_rules(session_id, &program, intents)
        .map(|plan| plan.rules)
        .map_err(|source| {
            runtime_error(
                "device_simulator.worker.firewall_plan_invalid",
                source.to_string(),
            )
        })
}

fn resolve_platform_ipv4_addresses(
    servers: &[TargetPlatformServer],
) -> Result<Vec<Ipv4Addr>, WorkerRuntimeError> {
    let mut addresses = BTreeSet::new();
    for server in servers {
        let host = server.host.trim();
        if host.is_empty() || server.port == 0 {
            return Err(runtime_error(
                "device_simulator.worker.firewall_remote_invalid",
                format!("platform server '{}' has no resolvable endpoint", server.id),
            ));
        }
        if let Ok(address) = host.parse::<Ipv4Addr>() {
            addresses.insert(address);
            continue;
        }
        let resolved = (host, server.port).to_socket_addrs().map_err(|source| {
            runtime_error(
                "device_simulator.worker.firewall_remote_resolution_failed",
                format!(
                    "failed to resolve platform server '{}': {source}",
                    server.id
                ),
            )
        })?;
        let resolved = resolved
            .filter_map(|address| match address.ip() {
                std::net::IpAddr::V4(address) => Some(address),
                std::net::IpAddr::V6(_) => None,
            })
            .collect::<BTreeSet<_>>();
        if resolved.is_empty() {
            return Err(runtime_error(
                "device_simulator.worker.firewall_remote_resolution_failed",
                format!(
                    "platform server '{}' resolved to no IPv4 address",
                    server.id
                ),
            ));
        }
        addresses.extend(resolved);
    }
    if addresses.is_empty() {
        return Err(runtime_error(
            "device_simulator.worker.firewall_remote_missing",
            "at least one target platform IPv4 address is required for managed firewall rules",
        ));
    }
    Ok(addresses.into_iter().collect())
}

fn phase_progress(state: SessionState) -> Option<f32> {
    Some(match state {
        SessionState::Validating => 0.05,
        SessionState::Preflighting => 0.15,
        SessionState::StartingWorker => 0.25,
        SessionState::AddingIps => 0.4,
        SessionState::StartingServices => 0.7,
        SessionState::Running => 1.0,
        SessionState::StoppingAlarms => 0.15,
        SessionState::StoppingServices => 0.35,
        SessionState::RemovingFirewall => 0.65,
        SessionState::RemovingIps => 0.85,
        SessionState::Stopped | SessionState::Failed => 1.0,
        SessionState::RecoveryRequired | SessionState::Recovering => 0.0,
        SessionState::Idle | SessionState::AssetsRequired | SessionState::DownloadingAssets => {
            return None
        }
    })
}

fn format_timestamp_ms(value: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(value as i64)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn alarm_runtime_error(source: AlarmRuntimeError) -> WorkerRuntimeError {
    runtime_error(source.code, source.message)
}

fn protocol_runtime_error(source: ProtocolRuntimeError) -> WorkerRuntimeError {
    runtime_error(source.code, source.message)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn runtime_error(code: &'static str, message: impl Into<String>) -> WorkerRuntimeError {
    WorkerRuntimeError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::api::{
        DeviceGroupDraft, DeviceSimulatorStreamKind, RtspPorts, StreamRuntimeConfig,
        StreamTransport, TargetPlatformConfig,
    };
    use crate::device_simulator::profiles::scope::TargetPlatform;
    use crate::device_simulator::windows::firewall::{FirewallBackendError, FirewallRuleSpec};
    use crate::device_simulator::windows::ip_alias::IpAliasBackendError;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex as TestMutex;
    use tempfile::TempDir;

    #[derive(Default)]
    struct FakeIpBackend {
        addresses: TestMutex<HashSet<Ipv4Addr>>,
        fail_add: TestMutex<Option<Ipv4Addr>>,
    }

    impl IpAliasBackend for FakeIpBackend {
        fn list_local_addresses(&self) -> Result<HashSet<Ipv4Addr>, IpAliasBackendError> {
            Ok(self.addresses.lock().unwrap().clone())
        }

        fn add_alias(
            &self,
            _interface_id: &str,
            address: Ipv4Addr,
            _prefix_len: u8,
        ) -> Result<(), IpAliasBackendError> {
            if *self.fail_add.lock().unwrap() == Some(address) {
                return Err(IpAliasBackendError::Native("injected add failure".into()));
            }
            self.addresses.lock().unwrap().insert(address);
            Ok(())
        }

        fn remove_alias(
            &self,
            _interface_id: &str,
            address: Ipv4Addr,
            _prefix_len: u8,
        ) -> Result<(), IpAliasBackendError> {
            self.addresses.lock().unwrap().remove(&address);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeFirewallBackend {
        rules: TestMutex<HashMap<String, FirewallRuleSpec>>,
    }

    impl FirewallBackend for FakeFirewallBackend {
        fn list_managed_rules(&self) -> Result<Vec<FirewallRuleSpec>, FirewallBackendError> {
            Ok(self.rules.lock().unwrap().values().cloned().collect())
        }

        fn create_rule(&self, rule: &FirewallRuleSpec) -> Result<(), FirewallBackendError> {
            self.rules
                .lock()
                .unwrap()
                .insert(rule.rule_id.clone(), rule.clone());
            Ok(())
        }

        fn delete_rule(&self, rule_id: &str) -> Result<(), FirewallBackendError> {
            self.rules.lock().unwrap().remove(rule_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeServices {
        started: bool,
    }

    impl WorkerServiceRuntime for FakeServices {
        fn start<'a>(
            &'a mut self,
            config: WorkerServiceStartConfig,
        ) -> WorkerServiceFuture<'a, ProtocolRuntimeSummary> {
            Box::pin(async move {
                self.started = true;
                Ok(ProtocolRuntimeSummary {
                    total_devices: config.preview.total_devices,
                    discovery_listeners: config.preview.devices.len(),
                    http_listeners: config.preview.devices.len(),
                    rtsp_listeners: config.preview.devices.len() * 3,
                    bind_addresses: vec![],
                })
            })
        }

        fn stop_alarms<'a>(&'a mut self) -> WorkerServiceFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn stop_protocol<'a>(&'a mut self) -> WorkerServiceFuture<'a, ()> {
            Box::pin(async move {
                self.started = false;
                Ok(())
            })
        }

        fn start_alarm_job<'a>(
            &'a mut self,
            _request: AlarmJobRequest,
        ) -> WorkerServiceFuture<'a, String> {
            Box::pin(async { Ok("fake-alarm".into()) })
        }

        fn trigger_alarm_once<'a>(
            &'a mut self,
            _request: AlarmJobRequest,
        ) -> WorkerServiceFuture<'a, AlarmTriggerResult> {
            Box::pin(async {
                Ok(AlarmTriggerResult {
                    attempted: 1,
                    succeeded: 0,
                    failed: 0,
                    unverified: 1,
                    duration_ms: 1,
                    errors: vec![],
                })
            })
        }

        fn stop_alarm_job<'a>(&'a mut self, _job_id: String) -> WorkerServiceFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn alarm_stats<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Vec<AlarmJobStatsSnapshot>> + Send + 'a>> {
            Box::pin(async { Vec::new() })
        }

        fn protocol_stats(&self) -> ProtocolRuntimeStats {
            ProtocolRuntimeStats::default()
        }
    }

    fn request() -> crate::device_simulator::api::SimulatorStartRequest {
        crate::device_simulator::api::SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Ums,
                servers: vec![TargetPlatformServer {
                    id: "ums".into(),
                    host: "127.0.0.1".into(),
                    port: 18080,
                }],
                alarm_receiver_url: None,
            },
            interface_id: "test-interface".into(),
            start_ip: "127.20.0.10".parse().unwrap(),
            subnet_prefix: 24,
            device_http_port: 18081,
            rtsp_ports: RtspPorts {
                main: 18554,
                sub: 18555,
                third: 18556,
            },
            groups: vec![DeviceGroupDraft {
                id: "group".into(),
                profile_id: "ipc-smart".into(),
                count: 2,
                nvr_channel_count: None,
            }],
            stream: StreamRuntimeConfig {
                transport: StreamTransport::TcpInterleaved,
                enabled_streams: vec![
                    DeviceSimulatorStreamKind::Main,
                    DeviceSimulatorStreamKind::Sub,
                    DeviceSimulatorStreamKind::Third,
                ],
                audio_enabled: false,
            },
        }
    }

    fn seed_runtime(
        root: &TempDir,
        ip: Arc<FakeIpBackend>,
        firewall: Arc<FakeFirewallBackend>,
    ) -> WorkerRuntime {
        let request = request();
        let preview = preview_devices(&request).unwrap();
        let payload = InitializeSessionPayload {
            app_data_dir: root.path().to_path_buf(),
            request: request.clone(),
            preview: preview.clone(),
            pinned_packs: vec![],
            manage_firewall: true,
        };
        let firewall_rules = plan_session_firewall(
            "session-test",
            &payload,
            FirewallRemoteScope::Addresses(vec![Ipv4Addr::LOCALHOST]),
        )
        .unwrap();
        let now = now_ms();
        let journal = SessionJournalV1 {
            schema_version: SESSION_JOURNAL_SCHEMA_VERSION,
            session_id: "session-test".into(),
            created_at_ms: now,
            updated_at_ms: now,
            app_version: "test".into(),
            worker_version: "test".into(),
            interface_id: request.interface_id.clone(),
            device_summary: DeviceRequestSummary {
                profile_ids: vec!["ipc-smart".into()],
                total_devices: preview.total_devices,
                total_nvr_channels: 0,
            },
            worker_process: None,
            resources: OwnedResources {
                ip_addresses: preview
                    .devices
                    .iter()
                    .map(|device| OwnedIpAddress {
                        interface_id: request.interface_id.clone(),
                        address: device.ip,
                        prefix_len: request.subnet_prefix,
                        state: ResourceOwnershipState::Planned,
                    })
                    .collect(),
                firewall_rules: firewall_rules
                    .iter()
                    .map(|rule| OwnedFirewallRule {
                        rule_name: rule.rule_id.clone(),
                        state: ResourceOwnershipState::Planned,
                    })
                    .collect(),
                packs: vec![],
            },
            cleanup: CleanupProgress::default(),
            state: SessionState::Preflighting,
            last_error: None,
        };
        let mut runtime = WorkerRuntime::new(
            "session-test",
            None,
            ip,
            firewall,
            Box::<FakeServices>::default(),
        );
        runtime.state = SessionState::Preflighting;
        runtime.preflight_ok = true;
        runtime.journal_store = Some(SessionJournalStore::from_app_data_dir(root.path()));
        runtime.journal = Some(journal);
        runtime.initialized = Some(InitializedSession {
            payload,
            assets: None,
            firewall_rules,
        });
        runtime
    }

    #[test]
    fn managed_firewall_scope_uses_exact_resolved_platform_addresses() {
        let servers = vec![
            TargetPlatformServer {
                id: "primary".into(),
                host: "192.0.2.10".into(),
                port: 80,
            },
            TargetPlatformServer {
                id: "secondary".into(),
                host: "198.51.100.20".into(),
                port: 80,
            },
        ];
        assert_eq!(
            resolve_platform_ipv4_addresses(&servers).unwrap(),
            vec![
                "192.0.2.10".parse::<Ipv4Addr>().unwrap(),
                "198.51.100.20".parse::<Ipv4Addr>().unwrap(),
            ]
        );
    }

    #[tokio::test]
    async fn fake_backends_prove_owned_resources_are_added_then_precisely_removed() {
        let root = TempDir::new().unwrap();
        let ip = Arc::new(FakeIpBackend::default());
        let firewall = Arc::new(FakeFirewallBackend::default());
        let mut runtime = seed_runtime(&root, Arc::clone(&ip), Arc::clone(&firewall));
        runtime.save_journal().await.unwrap();
        let status = runtime.start_services().await.unwrap();
        assert_eq!(status.state, SessionState::Running);
        assert_eq!(ip.addresses.lock().unwrap().len(), 2);
        assert_eq!(firewall.rules.lock().unwrap().len(), 3);

        let status = runtime.stop_services().await.unwrap();
        assert_eq!(status.state, SessionState::Stopped);
        assert!(ip.addresses.lock().unwrap().is_empty());
        assert!(firewall.rules.lock().unwrap().is_empty());
        let journal = SessionJournalStore::from_app_data_dir(root.path())
            .load("session-test")
            .unwrap();
        assert!(journal.is_terminal());
    }

    #[tokio::test]
    async fn partial_ip_failure_rolls_back_only_resources_owned_by_the_session() {
        let root = TempDir::new().unwrap();
        let ip = Arc::new(FakeIpBackend::default());
        *ip.fail_add.lock().unwrap() = Some("127.20.0.11".parse().unwrap());
        let firewall = Arc::new(FakeFirewallBackend::default());
        let mut runtime = seed_runtime(&root, Arc::clone(&ip), Arc::clone(&firewall));
        runtime.save_journal().await.unwrap();
        let error = runtime.start_services().await.unwrap_err();
        assert_eq!(error.code, "device_simulator.worker.ip_add_failed");
        assert!(ip.addresses.lock().unwrap().is_empty());
        assert!(firewall.rules.lock().unwrap().is_empty());
        assert_eq!(runtime.state(), SessionState::Failed);
    }
}
