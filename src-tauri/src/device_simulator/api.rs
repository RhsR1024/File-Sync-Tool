use crate::device_simulator::assets::catalog::DeviceKind;
use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::{SessionState, SimulatorStatus};
use crate::device_simulator::profiles::identity::{
    generate_device_previews, IdentityPlan, MAX_PREVIEW_DEVICES,
};
use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use crate::device_simulator::windows::ip_alias::AddressConflictAssessment;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::net::Ipv4Addr;

/// Fallback alarm receiver port used until the platform advertises its own via
/// `Event/Subscription`. Observed on real UMS deployments; a learned port always
/// replaces it.
pub const DEFAULT_ALARM_RECEIVER_PORT: u16 = 22_815;
pub const DEFAULT_MEDIA_THEME_ID: &str = "classic";

pub const DEVICE_SIMULATOR_EVENT_STATUS: &str = "device-simulator-status";
pub const DEVICE_SIMULATOR_EVENT_LOG: &str = "device-simulator-log";
pub const DEVICE_SIMULATOR_EVENT_ASSET_PROGRESS: &str = "device-simulator-asset-progress";
pub const DEVICE_SIMULATOR_EVENT_DEVICE_STATUS: &str = "device-simulator-device-status";
pub const DEVICE_SIMULATOR_EVENT_RTSP_STATS: &str = "device-simulator-rtsp-stats";
pub const DEVICE_SIMULATOR_EVENT_ALARM_STATS: &str = "device-simulator-alarm-stats";
pub const DEVICE_SIMULATOR_EVENT_ALARM_SUBSCRIPTION: &str = "device-simulator-alarm-subscription";
pub const DEVICE_SIMULATOR_EVENT_CLEANUP_PROGRESS: &str = "device-simulator-cleanup-progress";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RtspPorts {
    pub main: u16,
    pub sub: u16,
    pub third: u16,
}

impl Default for RtspPorts {
    fn default() -> Self {
        Self {
            main: 554,
            sub: 555,
            third: 556,
        }
    }
}

/// Unlike its sibling request types this one does not set `deny_unknown_fields`.
/// Configs saved before the NVR profiles were removed still carry an
/// `nvr_channel_count` key, and `load_config` discards the entire config file on
/// any parse error -- rejecting the key would silently reset every unrelated
/// setting. The group is fully validated after deserialization instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceGroupDraft {
    pub id: String,
    pub profile_id: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlatformServer {
    pub id: String,
    pub host: String,
    pub port: u16,
}

/// Which callers a running session answers.
///
/// The virtual devices are ordinary listeners with no protocol-level
/// credentials, so by default any platform that can route to them may discover
/// and add them. `ConfiguredServersOnly` narrows that to the addresses behind
/// [`TargetPlatformConfig::servers`], enforced in-process rather than relying on
/// Windows Firewall scoping alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformAccessMode {
    /// Legacy behaviour: answer every reachable caller.
    #[default]
    Open,
    /// Answer only the configured platform servers (and loopback).
    ConfiguredServersOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlatformConfig {
    pub kind: TargetPlatform,
    pub servers: Vec<TargetPlatformServer>,
    /// Defaulted so journals and settings written before admission control
    /// existed keep deserializing as `Open`.
    #[serde(default)]
    pub access_mode: PlatformAccessMode,
    pub alarm_receiver_url: Option<String>,
    #[serde(default)]
    pub alarm_receiver_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceSimulatorStreamKind {
    Main,
    Sub,
    Third,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamTransport {
    TcpInterleaved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamRuntimeConfig {
    pub transport: StreamTransport,
    pub enabled_streams: Vec<DeviceSimulatorStreamKind>,
    pub audio_enabled: bool,
    #[serde(default = "time_watermark_enabled_by_default")]
    pub time_watermark_enabled: bool,
}

fn time_watermark_enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulatorStartRequest {
    pub platform: TargetPlatformConfig,
    pub interface_id: String,
    pub start_ip: Ipv4Addr,
    #[serde(default)]
    pub device_ips: Vec<Ipv4Addr>,
    pub subnet_prefix: u8,
    pub device_http_port: u16,
    pub rtsp_ports: RtspPorts,
    /// Accepted only so journals created by older versions remain recoverable.
    /// Local RTSP player access is now always enabled, regardless of this value.
    #[serde(
        rename = "allow_local_player_access",
        default = "local_player_access_enabled",
        skip_serializing
    )]
    pub _legacy_allow_local_player_access: bool,
    #[serde(default = "default_media_theme_id")]
    pub media_theme_id: String,
    pub groups: Vec<DeviceGroupDraft>,
    pub stream: StreamRuntimeConfig,
}

fn default_media_theme_id() -> String {
    DEFAULT_MEDIA_THEME_ID.to_owned()
}

fn local_player_access_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaThemeSummary {
    pub id: String,
    pub display_name_key: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceProfileAvailability {
    Local,
    Remote,
    UpdateAvailable,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceProfileSummary {
    pub id: String,
    pub display_name_key: String,
    pub device_kind: DeviceKind,
    pub supported_platforms: Vec<TargetPlatform>,
    pub availability: DeviceProfileAvailability,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub verified_platforms: Vec<TargetPlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlarmTypeSummary {
    pub id: String,
    pub display_name: String,
    pub supports_pictures: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileAlarmTypes {
    pub profile_id: String,
    pub alarm_types: Vec<AlarmTypeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetPackStatus {
    pub id: String,
    pub required_version: String,
    pub installed_version: Option<String>,
    pub size: u64,
    pub state: crate::device_simulator::models::AssetState,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetStatus {
    pub state: crate::device_simulator::models::AssetState,
    pub profile_ids: Vec<String>,
    pub packs: Vec<AssetPackStatus>,
    pub update_available: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetProgressSnapshot {
    pub job_id: String,
    pub state: crate::device_simulator::models::AssetState,
    pub current_pack_id: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed_bps: u64,
    pub error: Option<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceStreamAddress {
    pub device_id: String,
    pub channel_id: Option<String>,
    pub stream: DeviceSimulatorStreamKind,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceIdentityPreviewDto {
    pub device_id: String,
    pub group_id: String,
    pub profile_id: String,
    pub device_kind: DeviceKind,
    pub ip: Ipv4Addr,
    pub mac: String,
    pub serial_number: String,
    pub hardware_id: String,
    pub streams: Vec<DeviceStreamAddress>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePreview {
    pub total_devices: u32,
    pub total_channels: u32,
    pub devices: Vec<DeviceIdentityPreviewDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCheckSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCheckStatus {
    Passed,
    Warning,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightCheck {
    pub id: String,
    pub severity: PreflightCheckSeverity,
    pub status: PreflightCheckStatus,
    pub message_key: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightReport {
    pub ok: bool,
    pub checks: Vec<PreflightCheck>,
    pub device_preview: DevicePreview,
    #[serde(default)]
    pub address_assessments: Vec<AddressConflictAssessment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SimulatorMetricsSnapshot {
    pub total_devices: u32,
    pub online_devices: u32,
    pub total_channels: u32,
    pub active_rtsp_clients: u32,
    pub outbound_bitrate_kbps: u64,
    pub active_alarm_jobs: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulatorStatusSnapshot {
    pub state: SessionState,
    pub session_id: Option<String>,
    pub started_at: Option<String>,
    pub phase_progress: Option<f32>,
    pub metrics: SimulatorMetricsSnapshot,
    pub cleanup_stage: Option<String>,
    pub recovery_session_id: Option<String>,
    pub last_error: Option<SimulatorErrorBody>,
}

impl From<SimulatorStatus> for SimulatorStatusSnapshot {
    fn from(status: SimulatorStatus) -> Self {
        let recovery_session_id = status
            .state
            .requires_recovery()
            .then(|| status.session_id.clone())
            .flatten();
        Self {
            state: status.state,
            session_id: status.session_id,
            started_at: None,
            phase_progress: None,
            metrics: SimulatorMetricsSnapshot::default(),
            cleanup_stage: None,
            recovery_session_id,
            last_error: status.error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlarmDispatchMode {
    Configured,
    Random,
    Sequential,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedAlarmImage {
    pub image_id: String,
    pub file_name: String,
    pub extension: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlarmJobRequest {
    pub target_device_ids: Vec<String>,
    pub alarm_profile_id: String,
    pub alarm_type_ids: Vec<String>,
    pub mode: AlarmDispatchMode,
    pub interval_ms: u64,
    pub send_count: Option<u64>,
    pub recovery_delay_secs: Option<u64>,
    pub image_variant: Option<String>,
    pub user_image_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlarmTriggerResult {
    pub attempted: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub unverified: u64,
    pub duration_ms: u64,
    pub errors: Vec<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryResult {
    pub session_id: String,
    pub recovered: bool,
    pub remaining_resources: Vec<String>,
    pub error: Option<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRuntimeStatusSnapshot {
    pub device_id: String,
    pub online: bool,
    pub active_http_connections: u32,
    pub active_rtsp_clients: u32,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceStatusBatch {
    pub session_id: String,
    pub sequence: u64,
    pub devices: Vec<DeviceRuntimeStatusSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RtspStatsSnapshot {
    pub session_id: String,
    pub active_clients: u32,
    pub bitrate_kbps: u64,
    pub bytes_sent: u64,
    pub disconnected_clients: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlarmJobStatsSnapshot {
    pub job_id: String,
    pub state: crate::device_simulator::models::AlarmJobState,
    pub attempted: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub unverified: u64,
    pub in_flight: u64,
    pub average_duration_ms: f64,
    pub last_http_status: Option<u16>,
    pub last_error: Option<SimulatorErrorBody>,
}

/// Where alarms are currently delivered, and whether that came from a platform
/// subscription or from the configured fallback.
///
/// Surfaced continuously so an operator can tell "the platform has subscribed
/// and I am pointed at its receiver" from "nothing has subscribed and I am
/// guessing" *before* sending an alarm and reading the failure back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlarmSubscriptionSnapshot {
    /// Absolute URLs alarms would currently be posted to.
    pub destinations: Vec<String>,
    /// `true` once the platform advertised a receiver via `Event/Subscription`.
    pub learned: bool,
    /// Receiver host from the subscription body, when the platform named one.
    pub host: Option<String>,
    /// Receiver port from the subscription body.
    pub port: Option<u16>,
    /// Subscription lifetime the platform declared, in seconds.
    pub duration_secs: Option<u32>,
    /// Wall-clock milliseconds when the subscription was last accepted.
    pub learned_at_ms: Option<u64>,
    /// Wall-clock milliseconds when the subscription lapses.
    pub expires_at_ms: Option<u64>,
    /// `true` when an explicit receiver URL is configured, which deliberately
    /// pins the destination and suppresses subscription learning.
    pub overridden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEventBatch {
    pub device_status: Option<DeviceStatusBatch>,
    pub rtsp_stats: Option<RtspStatsSnapshot>,
    pub alarm_stats: Vec<AlarmJobStatsSnapshot>,
    #[serde(default)]
    pub alarm_subscription: Option<AlarmSubscriptionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTelemetrySnapshot {
    pub status: SimulatorStatusSnapshot,
    pub events: RuntimeEventBatch,
}

#[derive(Debug, Default)]
pub struct RuntimeEventBatcher {
    sequence: u64,
    pending_devices: BTreeMap<String, DeviceRuntimeStatusSnapshot>,
    latest_rtsp: Option<RtspStatsSnapshot>,
    latest_alarm_jobs: BTreeMap<String, AlarmJobStatsSnapshot>,
    latest_alarm_subscription: Option<AlarmSubscriptionSnapshot>,
}

impl RuntimeEventBatcher {
    pub fn update_device(&mut self, status: DeviceRuntimeStatusSnapshot) {
        self.pending_devices
            .insert(status.device_id.clone(), status);
    }

    pub fn update_rtsp(&mut self, stats: RtspStatsSnapshot) {
        self.latest_rtsp = Some(stats);
    }

    pub fn update_alarm(&mut self, stats: AlarmJobStatsSnapshot) {
        self.latest_alarm_jobs.insert(stats.job_id.clone(), stats);
    }

    pub fn update_alarm_subscription(&mut self, subscription: AlarmSubscriptionSnapshot) {
        self.latest_alarm_subscription = Some(subscription);
    }

    pub fn drain(&mut self, session_id: &str, max_devices: usize) -> RuntimeEventBatch {
        let max_devices = max_devices.clamp(1, MAX_PREVIEW_DEVICES as usize);
        let device_ids = self
            .pending_devices
            .keys()
            .take(max_devices)
            .cloned()
            .collect::<Vec<_>>();
        let devices = device_ids
            .iter()
            .filter_map(|id| self.pending_devices.remove(id))
            .collect::<Vec<_>>();
        let device_status = if devices.is_empty() {
            None
        } else {
            self.sequence = self.sequence.saturating_add(1);
            Some(DeviceStatusBatch {
                session_id: session_id.to_owned(),
                sequence: self.sequence,
                devices,
            })
        };
        RuntimeEventBatch {
            device_status,
            rtsp_stats: self.latest_rtsp.take(),
            alarm_stats: std::mem::take(&mut self.latest_alarm_jobs)
                .into_values()
                .collect(),
            alarm_subscription: self.latest_alarm_subscription.take(),
        }
    }
}

pub fn list_first_release_profiles() -> Vec<DeviceProfileSummary> {
    use crate::device_simulator::profiles::scope::FIRST_RELEASE_PROFILES;
    FIRST_RELEASE_PROFILES
        .into_iter()
        .map(|profile| DeviceProfileSummary {
            id: profile.as_str().to_owned(),
            display_name_key: format!("deviceSimulator.profiles.{}", profile.as_str()),
            device_kind: profile.device_kind(),
            supported_platforms: vec![TargetPlatform::Ums],
            availability: DeviceProfileAvailability::Unavailable,
            installed_version: None,
            available_version: None,
            // Legacy source declarations are not real-platform verification.
            verified_platforms: vec![],
        })
        .collect()
}

pub fn preview_devices(
    request: &SimulatorStartRequest,
) -> Result<DevicePreview, SimulatorErrorBody> {
    validate_start_request(request)?;
    let network = Ipv4Net::new(request.start_ip, request.subnet_prefix).map_err(|source| {
        validation_error(
            "device_simulator.validation.network_invalid",
            format!("invalid device IPv4 network: {source}"),
        )
    })?;
    let mut current_ip = request.start_ip;
    let requested_device_count = request.groups.iter().try_fold(0_usize, |total, group| {
        total.checked_add(group.count as usize).ok_or_else(|| {
            validation_error(
                "device_simulator.validation.device_count_invalid",
                "device count overflowed",
            )
        })
    })?;
    if !request.device_ips.is_empty() && request.device_ips.len() != requested_device_count {
        return Err(validation_error(
            "device_simulator.validation.explicit_ip_count_mismatch",
            format!(
                "{} explicit addresses were provided for {requested_device_count} devices",
                request.device_ips.len()
            ),
        ));
    }
    let mut explicit_address_index = 0_usize;
    let mut devices = Vec::new();
    let mut total_channels = 0_u32;
    let mut occupied = HashSet::new();
    for group in &request.groups {
        let profile_id = parse_profile_id(&group.profile_id)?;
        let count = u16::try_from(group.count).map_err(|_| {
            validation_error(
                "device_simulator.validation.device_count_invalid",
                "device group count exceeds the first-release limit",
            )
        })?;
        let group_addresses = if request.device_ips.is_empty() {
            None
        } else {
            let end = explicit_address_index + count as usize;
            let addresses = request.device_ips[explicit_address_index..end].to_vec();
            explicit_address_index = end;
            Some(addresses)
        };
        let addresses = match group_addresses {
            Some(addresses) => addresses,
            None => (0..count)
                .map(|offset| {
                    u32::from(current_ip)
                        .checked_add(u32::from(offset))
                        .map(Ipv4Addr::from)
                        .ok_or_else(|| {
                            validation_error(
                                "device_simulator.validation.network_capacity_insufficient",
                                "device address range overflowed IPv4",
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        for (group_index, address) in addresses.into_iter().enumerate() {
            let identity_plan = IdentityPlan {
                profile_id,
                network,
                start_ip: address,
                device_count: 1,
                deterministic_seed: deterministic_seed(request, group),
                http_port: request.device_http_port,
            };
            let identity = generate_device_previews(&identity_plan, &occupied)
                .map_err(|source| validation_error(source.code, source.message))?
                .remove(0);
            occupied.insert(identity.ip);
            let device_id = format!("{}-{:04}", group.id, group_index + 1);
            total_channels = total_channels.saturating_add(1);
            let streams = stream_addresses(&device_id, identity.ip, request.rtsp_ports);
            devices.push(DeviceIdentityPreviewDto {
                device_id,
                group_id: group.id.clone(),
                profile_id: group.profile_id.clone(),
                device_kind: profile_id.device_kind(),
                ip: identity.ip,
                mac: identity.mac_compact,
                serial_number: identity.serial_number,
                hardware_id: identity.hardware_id,
                streams,
            });
        }
        if request.device_ips.is_empty() {
            current_ip =
                Ipv4Addr::from(u32::from(current_ip).checked_add(group.count).ok_or_else(
                    || {
                        validation_error(
                            "device_simulator.validation.network_capacity_insufficient",
                            "device address range overflowed IPv4",
                        )
                    },
                )?);
        }
    }
    Ok(DevicePreview {
        total_devices: devices.len() as u32,
        total_channels,
        devices,
    })
}

pub fn command_not_ready(operation: &str) -> SimulatorErrorBody {
    SimulatorErrorBody::new(
        "device_simulator.command.not_ready",
        "deviceSimulator.errors.commandNotReady",
    )
    .with_public_details(format!(
        "{operation} is registered but its native runtime integration is not ready"
    ))
    .retryable(false)
}

fn validate_start_request(request: &SimulatorStartRequest) -> Result<(), SimulatorErrorBody> {
    if request.interface_id.trim().is_empty() || request.interface_id.len() > 128 {
        return Err(validation_error(
            "device_simulator.validation.interface_id_invalid",
            "interface id is empty or too long",
        ));
    }
    if !(1..=30).contains(&request.subnet_prefix) {
        return Err(validation_error(
            "device_simulator.validation.subnet_prefix_invalid",
            "subnet prefix must be between 1 and 30",
        ));
    }
    validate_ports(request.device_http_port, request.rtsp_ports)?;
    if !is_safe_media_theme_id(&request.media_theme_id) {
        return Err(validation_error(
            "device_simulator.validation.media_theme_invalid",
            "media theme id must be a lowercase ASCII token",
        ));
    }
    if request.platform.alarm_receiver_port == Some(0) {
        return Err(validation_error(
            "device_simulator.validation.port_invalid",
            "alarm receiver port must be non-zero when configured",
        ));
    }
    // Restricted admission derives its allow list from the server entries, so an
    // empty list would silently block every caller including the intended one.
    if request.platform.access_mode == PlatformAccessMode::ConfiguredServersOnly
        && !request
            .platform
            .servers
            .iter()
            .any(|server| !server.host.trim().is_empty() && server.port != 0)
    {
        return Err(validation_error(
            "device_simulator.validation.platform_access_servers_required",
            "restricted platform access requires at least one configured server",
        ));
    }
    if request.stream.audio_enabled
        || request.stream.transport != StreamTransport::TcpInterleaved
        || request
            .stream
            .enabled_streams
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            != BTreeSet::from([
                DeviceSimulatorStreamKind::Main,
                DeviceSimulatorStreamKind::Sub,
                DeviceSimulatorStreamKind::Third,
            ])
    {
        return Err(validation_error(
            "device_simulator.validation.stream_policy_invalid",
            "first release requires TCP interleaving, main/sub/third streams, and no audio",
        ));
    }
    if request.groups.is_empty() {
        return Err(validation_error(
            "device_simulator.validation.device_groups_empty",
            "at least one device group is required",
        ));
    }
    let total = request.groups.iter().try_fold(0_u32, |sum, group| {
        sum.checked_add(group.count).ok_or_else(|| {
            validation_error(
                "device_simulator.validation.device_count_invalid",
                "total device count overflowed",
            )
        })
    })?;
    if total == 0 || total > u32::from(MAX_PREVIEW_DEVICES) {
        return Err(validation_error(
            "device_simulator.validation.device_count_invalid",
            format!("total device count must be between 1 and {MAX_PREVIEW_DEVICES}"),
        ));
    }
    let mut ids = BTreeSet::new();
    for group in &request.groups {
        if !is_safe_id(&group.id) || !ids.insert(group.id.as_str()) || group.count == 0 {
            return Err(validation_error(
                "device_simulator.validation.device_group_invalid",
                format!("device group '{}' is invalid or duplicated", group.id),
            ));
        }
        parse_profile_id(&group.profile_id)?;
    }
    Ok(())
}

fn is_safe_media_theme_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_ports(http: u16, rtsp: RtspPorts) -> Result<(), SimulatorErrorBody> {
    let ports = [http, rtsp.main, rtsp.sub, rtsp.third];
    if ports.contains(&0) || ports.into_iter().collect::<BTreeSet<_>>().len() != ports.len() {
        return Err(validation_error(
            "device_simulator.validation.port_invalid",
            "HTTP and RTSP ports must be non-zero and distinct",
        ));
    }
    Ok(())
}

fn parse_profile_id(value: &str) -> Result<FirstReleaseProfileId, SimulatorErrorBody> {
    match value {
        "ipc-structured" => Ok(FirstReleaseProfileId::IpcStructured),
        _ => Err(validation_error(
            "device_simulator.validation.profile_unknown",
            format!("unknown first-release profile '{value}'"),
        )),
    }
}

fn deterministic_seed(request: &SimulatorStartRequest, group: &DeviceGroupDraft) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"file-sync-tool/device-simulator/identity/v1");
    hasher.update(request.interface_id.as_bytes());
    hasher.update(request.start_ip.octets());
    hasher.update([request.subnet_prefix]);
    hasher.update(group.id.as_bytes());
    hasher.update(group.profile_id.as_bytes());
    hasher.finalize().into()
}

fn stream_addresses(device_id: &str, ip: Ipv4Addr, ports: RtspPorts) -> Vec<DeviceStreamAddress> {
    [
        (DeviceSimulatorStreamKind::Main, ports.main, 1_u8),
        (DeviceSimulatorStreamKind::Sub, ports.sub, 2),
        (DeviceSimulatorStreamKind::Third, ports.third, 3),
    ]
    .into_iter()
    .map(|(stream, port, number)| DeviceStreamAddress {
        device_id: device_id.to_owned(),
        channel_id: None,
        stream,
        url: format!("rtsp://{ip}:{port}/media/video{number}"),
    })
    .collect()
}

fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validation_error(code: &'static str, details: impl Into<String>) -> SimulatorErrorBody {
    SimulatorErrorBody::new(code, "deviceSimulator.errors.validationFailed")
        .with_public_details(details)
        .retryable(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::models::{AlarmJobState, SessionState};

    fn start_request() -> SimulatorStartRequest {
        SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Ums,
                servers: vec![],
                access_mode: PlatformAccessMode::Open,
                alarm_receiver_url: None,
                alarm_receiver_port: Some(55_025),
            },
            interface_id: "guid:a0b1c2d3-1234-5678-90ab-010203040506".into(),
            start_ip: "10.20.0.254".parse().unwrap(),
            device_ips: vec![],
            subnet_prefix: 23,
            device_http_port: 81,
            rtsp_ports: RtspPorts::default(),
            _legacy_allow_local_player_access: true,
            media_theme_id: DEFAULT_MEDIA_THEME_ID.into(),
            groups: vec![DeviceGroupDraft {
                id: "structured".into(),
                profile_id: "ipc-structured".into(),
                count: 3,
            }],
            stream: StreamRuntimeConfig {
                transport: StreamTransport::TcpInterleaved,
                enabled_streams: vec![
                    DeviceSimulatorStreamKind::Main,
                    DeviceSimulatorStreamKind::Sub,
                    DeviceSimulatorStreamKind::Third,
                ],
                audio_enabled: false,
                time_watermark_enabled: true,
            },
        }
    }

    #[test]
    fn target_platform_config_accepts_optional_alarm_receiver_port() {
        let config: TargetPlatformConfig = serde_json::from_value(serde_json::json!({
            "kind": "ums",
            "servers": [],
            "alarm_receiver_url": null,
            "alarm_receiver_port": 55025
        }))
        .expect("alarm receiver port is part of the persisted start contract");
        assert_eq!(config.alarm_receiver_port, Some(55_025));
    }

    #[test]
    fn legacy_local_player_flag_is_read_but_not_written() {
        let mut value = serde_json::to_value(start_request()).unwrap();
        assert!(value.get("allow_local_player_access").is_none());
        value["allow_local_player_access"] = serde_json::Value::Bool(false);

        let request: SimulatorStartRequest = serde_json::from_value(value).unwrap();
        assert!(!request._legacy_allow_local_player_access);
        assert!(serde_json::to_value(request)
            .unwrap()
            .get("allow_local_player_access")
            .is_none());
    }

    #[test]
    fn legacy_stream_request_defaults_time_watermark_on() {
        let mut value = serde_json::to_value(start_request()).unwrap();
        value["stream"]
            .as_object_mut()
            .unwrap()
            .remove("time_watermark_enabled");

        let request: SimulatorStartRequest = serde_json::from_value(value).unwrap();
        assert!(request.stream.time_watermark_enabled);
    }

    #[test]
    fn preview_is_deterministic_and_crosses_octet_boundary() {
        let request = start_request();
        let first = preview_devices(&request).unwrap();
        assert_eq!(first, preview_devices(&request).unwrap());
        assert_eq!(first.total_devices, 3);
        // One channel per structured camera.
        assert_eq!(first.total_channels, 3);
        assert_eq!(first.devices[1].ip.to_string(), "10.20.0.255");
        assert_eq!(first.devices[2].ip.to_string(), "10.20.1.0");
        assert_eq!(
            first.devices[2].streams[0].url,
            "rtsp://10.20.1.0:554/media/video1"
        );
    }

    #[test]
    fn preview_rejects_unapproved_streams_ports_profiles_and_channel_shapes() {
        let mut request = start_request();
        request.stream.audio_enabled = true;
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.stream_policy_invalid"
        );
        request = start_request();
        request.rtsp_ports.main = request.device_http_port;
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.port_invalid"
        );
        request = start_request();
        request.platform.alarm_receiver_port = Some(0);
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.port_invalid"
        );
        request = start_request();
        request.groups[0].profile_id = "downloaded-code".into();
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.profile_unknown"
        );
    }

    #[test]
    fn restricted_platform_access_requires_a_usable_server_entry() {
        let mut request = start_request();
        request.platform.access_mode = PlatformAccessMode::ConfiguredServersOnly;
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.platform_access_servers_required"
        );
        request.platform.servers = vec![TargetPlatformServer {
            id: "blank".into(),
            host: "   ".into(),
            port: 80,
        }];
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.platform_access_servers_required"
        );
        request.platform.servers = vec![TargetPlatformServer {
            id: "ums".into(),
            host: "192.0.2.10".into(),
            port: 80,
        }];
        assert!(preview_devices(&request).is_ok());
        // Open access keeps working without any server entry, as before.
        request.platform.access_mode = PlatformAccessMode::Open;
        request.platform.servers.clear();
        assert!(preview_devices(&request).is_ok());
    }

    #[test]
    fn platform_access_mode_defaults_to_open_for_payloads_written_before_it_existed() {
        let legacy = serde_json::json!({
            "kind": "ums",
            "servers": [],
            "alarm_receiver_url": null,
        });
        let parsed: TargetPlatformConfig = serde_json::from_value(legacy).unwrap();
        assert_eq!(parsed.access_mode, PlatformAccessMode::Open);
        assert_eq!(
            serde_json::to_value(PlatformAccessMode::ConfiguredServersOnly).unwrap(),
            serde_json::json!("configured_servers_only")
        );
    }

    #[test]
    fn preview_accepts_non_contiguous_explicit_addresses_and_rejects_count_mismatch() {
        let mut request = start_request();
        request.start_ip = "10.20.0.10".parse().unwrap();
        request.device_ips = ["10.20.0.10", "10.20.0.18", "10.20.1.7"]
            .map(|value| value.parse().unwrap())
            .to_vec();
        let preview = preview_devices(&request).unwrap();
        assert_eq!(
            preview
                .devices
                .iter()
                .map(|device| device.ip)
                .collect::<Vec<_>>(),
            request.device_ips
        );
        assert_eq!(preview.devices[0].device_id, "structured-0001");
        assert_eq!(preview.devices[2].device_id, "structured-0003");

        request.device_ips.pop();
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.explicit_ip_count_mismatch"
        );
    }

    #[test]
    fn status_snapshot_is_recoverable_without_persisting_worker_runtime_fields() {
        let status = SimulatorStatus {
            session_id: Some("session-1".into()),
            state: SessionState::RecoveryRequired,
            updated_at_ms: 123,
            error: None,
        };
        let json = serde_json::to_value(SimulatorStatusSnapshot::from(status)).unwrap();
        assert_eq!(json["recovery_session_id"], "session-1");
        assert_eq!(json["metrics"]["active_rtsp_clients"], 0);
        assert!(json.get("worker_process_id").is_none());
        assert!(json.get("heartbeat").is_none());
    }

    #[test]
    fn first_release_profiles_never_claim_platform_verification_or_local_assets() {
        let profiles = list_first_release_profiles();
        assert_eq!(profiles.len(), 1);
        assert!(profiles.iter().all(|profile| {
            profile.verified_platforms.is_empty()
                && profile.availability == DeviceProfileAvailability::Unavailable
        }));
    }

    #[test]
    fn event_batcher_deduplicates_devices_and_emits_aggregated_stats() {
        let mut batcher = RuntimeEventBatcher::default();
        for online in [false, true] {
            batcher.update_device(DeviceRuntimeStatusSnapshot {
                device_id: "device-1".into(),
                online,
                active_http_connections: 0,
                active_rtsp_clients: 0,
                last_error_code: None,
            });
        }
        batcher.update_rtsp(RtspStatsSnapshot {
            session_id: "session-1".into(),
            active_clients: 3,
            bitrate_kbps: 2048,
            bytes_sent: 100,
            disconnected_clients: 1,
        });
        batcher.update_alarm(AlarmJobStatsSnapshot {
            job_id: "alarm-1".into(),
            state: AlarmJobState::Running,
            attempted: 1,
            succeeded: 1,
            failed: 0,
            unverified: 0,
            in_flight: 0,
            average_duration_ms: 12.5,
            last_http_status: Some(202),
            last_error: None,
        });
        let batch = batcher.drain("session-1", 100);
        let devices = batch.device_status.unwrap();
        assert_eq!(devices.sequence, 1);
        assert_eq!(devices.devices.len(), 1);
        assert!(devices.devices[0].online);
        assert_eq!(batch.rtsp_stats.unwrap().active_clients, 3);
        assert_eq!(batch.alarm_stats.len(), 1);
        let alarm_json = serde_json::to_value(&batch.alarm_stats[0]).unwrap();
        assert_eq!(alarm_json["last_http_status"], 202);
    }

    #[test]
    fn command_not_ready_is_structured_and_never_reports_success() {
        let error = command_not_ready("start");
        assert_eq!(error.code, "device_simulator.command.not_ready");
        assert!(!error.retryable);
        assert!(error.details.unwrap().contains("start"));
    }
}
