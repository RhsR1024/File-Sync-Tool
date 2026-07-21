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

pub const DEVICE_SIMULATOR_EVENT_STATUS: &str = "device-simulator-status";
pub const DEVICE_SIMULATOR_EVENT_LOG: &str = "device-simulator-log";
pub const DEVICE_SIMULATOR_EVENT_ASSET_PROGRESS: &str = "device-simulator-asset-progress";
pub const DEVICE_SIMULATOR_EVENT_DEVICE_STATUS: &str = "device-simulator-device-status";
pub const DEVICE_SIMULATOR_EVENT_RTSP_STATS: &str = "device-simulator-rtsp-stats";
pub const DEVICE_SIMULATOR_EVENT_ALARM_STATS: &str = "device-simulator-alarm-stats";
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceGroupDraft {
    pub id: String,
    pub profile_id: String,
    pub count: u32,
    pub nvr_channel_count: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlatformServer {
    pub id: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPlatformConfig {
    pub kind: TargetPlatform,
    pub servers: Vec<TargetPlatformServer>,
    pub alarm_receiver_url: Option<String>,
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
    pub groups: Vec<DeviceGroupDraft>,
    pub stream: StreamRuntimeConfig,
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
    pub channel_count: Option<u16>,
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
    pub last_error: Option<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEventBatch {
    pub device_status: Option<DeviceStatusBatch>,
    pub rtsp_stats: Option<RtspStatsSnapshot>,
    pub alarm_stats: Vec<AlarmJobStatsSnapshot>,
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
                nvr_channel_count: group.nvr_channel_count,
            };
            let identity = generate_device_previews(&identity_plan, &occupied)
                .map_err(|source| validation_error(source.code, source.message))?
                .remove(0);
            occupied.insert(identity.ip);
            let device_id = format!("{}-{:04}", group.id, group_index + 1);
            let channel_count = identity.nvr_channel_count;
            total_channels = total_channels.saturating_add(u32::from(channel_count.unwrap_or(1)));
            let streams = stream_addresses(
                &device_id,
                profile_id.device_kind(),
                identity.ip,
                request.rtsp_ports,
            );
            devices.push(DeviceIdentityPreviewDto {
                device_id,
                group_id: group.id.clone(),
                profile_id: group.profile_id.clone(),
                device_kind: profile_id.device_kind(),
                ip: identity.ip,
                mac: identity.mac_compact,
                serial_number: identity.serial_number,
                hardware_id: identity.hardware_id,
                channel_count,
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
        let profile = parse_profile_id(&group.profile_id)?;
        match (profile.device_kind(), group.nvr_channel_count) {
            (DeviceKind::Ipc, None) => {}
            (DeviceKind::Nvr, Some(channels)) => {
                crate::device_simulator::profiles::scope::validate_nvr_channel_count(channels)
                    .map_err(|code| validation_error(code, "invalid NVR channel count"))?;
            }
            (DeviceKind::Ipc, Some(_)) => {
                return Err(validation_error(
                    "device_simulator.validation.ipc_channel_count_invalid",
                    "IPC group must not declare an NVR channel count",
                ));
            }
            (DeviceKind::Nvr, None) => {
                return Err(validation_error(
                    "device_simulator.validation.nvr_channel_count_missing",
                    "NVR group requires a channel count",
                ));
            }
        }
    }
    Ok(())
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
        "ipc-custom" => Ok(FirstReleaseProfileId::IpcCustom),
        "ipc-smart" => Ok(FirstReleaseProfileId::IpcSmart),
        "ipc-structured" => Ok(FirstReleaseProfileId::IpcStructured),
        "ipc-face-access" => Ok(FirstReleaseProfileId::IpcFaceAccess),
        "nvr-common" => Ok(FirstReleaseProfileId::NvrCommon),
        "nvr-vehicle" => Ok(FirstReleaseProfileId::NvrVehicle),
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

fn stream_addresses(
    device_id: &str,
    kind: DeviceKind,
    ip: Ipv4Addr,
    ports: RtspPorts,
) -> Vec<DeviceStreamAddress> {
    [
        (DeviceSimulatorStreamKind::Main, ports.main, 1_u8),
        (DeviceSimulatorStreamKind::Sub, ports.sub, 2),
        (DeviceSimulatorStreamKind::Third, ports.third, 3),
    ]
    .into_iter()
    .map(|(stream, port, number)| {
        let (channel_id, url) = match kind {
            DeviceKind::Ipc => (None, format!("rtsp://{ip}:{port}/media/video{number}")),
            // Legacy parity: NVR channel metadata is configurable, while the
            // old runtime only advertises c1 for its three actual RTSP streams.
            DeviceKind::Nvr => (
                Some("1".to_owned()),
                format!("rtsp://{ip}:{port}/unicast/c1/s{}/live", number - 1),
            ),
        };
        DeviceStreamAddress {
            device_id: device_id.to_owned(),
            channel_id,
            stream,
            url,
        }
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
                alarm_receiver_url: None,
            },
            interface_id: "guid:a0b1c2d3-1234-5678-90ab-010203040506".into(),
            start_ip: "10.20.0.254".parse().unwrap(),
            device_ips: vec![],
            subnet_prefix: 23,
            device_http_port: 81,
            rtsp_ports: RtspPorts::default(),
            groups: vec![
                DeviceGroupDraft {
                    id: "smart".into(),
                    profile_id: "ipc-smart".into(),
                    count: 2,
                    nvr_channel_count: None,
                },
                DeviceGroupDraft {
                    id: "nvr".into(),
                    profile_id: "nvr-common".into(),
                    count: 1,
                    nvr_channel_count: Some(8),
                },
            ],
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

    #[test]
    fn preview_is_deterministic_crosses_octet_boundary_and_keeps_nvr_url_evidence_boundary() {
        let request = start_request();
        let first = preview_devices(&request).unwrap();
        assert_eq!(first, preview_devices(&request).unwrap());
        assert_eq!(first.total_devices, 3);
        assert_eq!(first.total_channels, 10);
        assert_eq!(first.devices[1].ip.to_string(), "10.20.0.255");
        assert_eq!(first.devices[2].ip.to_string(), "10.20.1.0");
        assert_eq!(
            first.devices[2].streams[0].url,
            "rtsp://10.20.1.0:554/unicast/c1/s0/live"
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
        request.groups[0].profile_id = "downloaded-code".into();
        assert_eq!(
            preview_devices(&request).unwrap_err().code,
            "device_simulator.validation.profile_unknown"
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
        assert_eq!(preview.devices[0].device_id, "smart-0001");
        assert_eq!(preview.devices[2].device_id, "nvr-0001");

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
        assert_eq!(profiles.len(), 6);
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
            last_error: None,
        });
        let batch = batcher.drain("session-1", 100);
        let devices = batch.device_status.unwrap();
        assert_eq!(devices.sequence, 1);
        assert_eq!(devices.devices.len(), 1);
        assert!(devices.devices[0].online);
        assert_eq!(batch.rtsp_stats.unwrap().active_clients, 3);
        assert_eq!(batch.alarm_stats.len(), 1);
    }

    #[test]
    fn command_not_ready_is_structured_and_never_reports_success() {
        let error = command_not_ready("start");
        assert_eq!(error.code, "device_simulator.command.not_ready");
        assert!(!error.retryable);
        assert!(error.details.unwrap().contains("start"));
    }
}
