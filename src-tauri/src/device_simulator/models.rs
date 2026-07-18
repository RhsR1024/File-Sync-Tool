use crate::device_simulator::errors::SimulatorErrorBody;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    #[default]
    Idle,
    Validating,
    AssetsRequired,
    DownloadingAssets,
    Preflighting,
    StartingWorker,
    AddingIps,
    StartingServices,
    Running,
    StoppingAlarms,
    StoppingServices,
    RemovingFirewall,
    RemovingIps,
    Stopped,
    Failed,
    RecoveryRequired,
    Recovering,
}

impl SessionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Failed)
    }

    pub fn requires_recovery(self) -> bool {
        matches!(self, Self::RecoveryRequired | Self::Recovering)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetState {
    #[default]
    Unknown,
    Checking,
    Missing,
    Downloading,
    Verifying,
    Installing,
    Ready,
    UpdateAvailable,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlarmJobState {
    #[default]
    Idle,
    Starting,
    Running,
    Stopping,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Discovery,
    Http,
    Rtsp,
    Alarm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRuntimeState {
    Starting,
    Online,
    Degraded,
    Offline,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulatorStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub state: SessionState,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulatorErrorBody>,
}

impl Default for SimulatorStatus {
    fn default() -> Self {
        Self {
            session_id: None,
            state: SessionState::Idle,
            updated_at_ms: 0,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceStatusUpdate {
    pub device_id: String,
    pub device_ip: String,
    pub state: DeviceRuntimeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlarmJobStats {
    pub alarm_job_id: String,
    pub state: AlarmJobState,
    pub total: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub in_flight: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_states_use_stable_snake_case_names() {
        assert_eq!(
            serde_json::to_string(&SessionState::RecoveryRequired).unwrap(),
            "\"recovery_required\""
        );
        assert!(SessionState::Recovering.requires_recovery());
        assert!(SessionState::Stopped.is_terminal());
        assert!(!SessionState::Running.is_terminal());
    }
}
