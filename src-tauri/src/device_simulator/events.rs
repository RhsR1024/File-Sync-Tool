use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::{
    AlarmJobStats, DeviceStatusUpdate, ServiceKind, SessionState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerEvent {
    pub session_id: String,
    pub sequence: u64,
    pub emitted_at_ms: u64,
    #[serde(flatten)]
    pub payload: WorkerEventPayload,
}

impl WorkerEvent {
    pub fn new(
        session_id: impl Into<String>,
        sequence: u64,
        emitted_at_ms: u64,
        payload: WorkerEventPayload,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            sequence,
            emitted_at_ms,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum WorkerEventPayload {
    StatusChanged {
        previous: SessionState,
        current: SessionState,
    },
    ServiceReady {
        service: ServiceKind,
        bind_addresses: Vec<String>,
    },
    DeviceStatus {
        updates: Vec<DeviceStatusUpdate>,
    },
    RtspClientChanged {
        rtsp_session_id: String,
        device_id: String,
        channel_id: String,
        connected: bool,
        active_clients: u32,
    },
    AlarmStats {
        stats: AlarmJobStats,
    },
    Log {
        level: WorkerLogLevel,
        component: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
    },
    CleanupProgress {
        stage: CleanupStage,
        completed: u32,
        total: u32,
    },
    FatalError {
        error: SimulatorErrorBody,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStage {
    StoppingAlarms,
    StoppingServices,
    RemovingFirewall,
    RemovingIps,
    Complete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proactive_event_has_session_sequence_and_tag() {
        let event = WorkerEvent::new(
            "session-1",
            7,
            1234,
            WorkerEventPayload::StatusChanged {
                previous: SessionState::StartingServices,
                current: SessionState::Running,
            },
        );
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["session_id"], "session-1");
        assert_eq!(json["sequence"], 7);
        assert_eq!(json["event"], "status_changed");
    }
}
