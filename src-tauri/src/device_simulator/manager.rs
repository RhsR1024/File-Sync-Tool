use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::{SessionState, SimulatorStatus};
use crate::device_simulator::windows::named_pipe::PipeIdentity;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedSession {
    pub identity: PipeIdentity,
    pub status: SimulatorStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerTimeoutPolicy {
    pub startup: Duration,
    pub request: Duration,
    pub stop: Duration,
}

impl Default for WorkerTimeoutPolicy {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(20),
            request: Duration::from_secs(15),
            stop: Duration::from_secs(15),
        }
    }
}

impl WorkerTimeoutPolicy {
    pub fn validate(self) -> Result<Self, ManagerError> {
        let durations = [self.startup, self.request, self.stop];
        if durations
            .iter()
            .any(|duration| duration.is_zero() || *duration > Duration::from_secs(120))
        {
            return Err(ManagerError::new(
                "device_simulator.worker.timeout_invalid",
                "worker timeouts must be finite and within 1ms..=120s",
            ));
        }
        Ok(self)
    }
}

impl ManagerError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct ManagerInner {
    status: SimulatorStatus,
    resources_may_be_owned: bool,
    worker_process_id: Option<u32>,
    last_heartbeat_ms: Option<u64>,
}

impl Default for ManagerInner {
    fn default() -> Self {
        Self {
            status: SimulatorStatus::default(),
            resources_may_be_owned: false,
            worker_process_id: None,
            last_heartbeat_ms: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct SimulatorManager {
    inner: Mutex<ManagerInner>,
}

impl SimulatorManager {
    pub fn status(&self) -> SimulatorStatus {
        self.inner
            .lock()
            .expect("simulator manager poisoned")
            .status
            .clone()
    }

    pub fn begin_session(&self, session_id: String) -> Result<SimulatorStatus, ManagerError> {
        if session_id.trim().is_empty() || session_id.len() > 128 {
            return Err(ManagerError::new(
                "device_simulator.session.id_invalid",
                "session id is empty or too long",
            ));
        }
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        if !matches!(
            inner.status.state,
            SessionState::Idle | SessionState::Stopped | SessionState::Failed
        ) || inner.resources_may_be_owned
        {
            return Err(ManagerError::new(
                "device_simulator.session.already_active",
                "another simulator session is active or still owns resources",
            ));
        }
        inner.status = SimulatorStatus {
            session_id: Some(session_id),
            state: SessionState::Validating,
            updated_at_ms: now_ms(),
            error: None,
        };
        inner.worker_process_id = None;
        inner.last_heartbeat_ms = None;
        Ok(inner.status.clone())
    }

    pub fn begin_random_session(&self) -> Result<StartedSession, ManagerError> {
        let identity = PipeIdentity::generate();
        let status = self.begin_session(identity.session_id.clone())?;
        Ok(StartedSession { identity, status })
    }

    pub fn transition(
        &self,
        session_id: &str,
        next: SessionState,
    ) -> Result<SimulatorStatus, ManagerError> {
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        let current = inner.status.state;
        if !transition_allowed(current, next) {
            return Err(ManagerError::new(
                "device_simulator.session.transition_invalid",
                format!("invalid simulator transition {current:?} -> {next:?}"),
            ));
        }
        inner.status.state = next;
        inner.status.updated_at_ms = now_ms();
        inner.status.error = None;
        if matches!(
            next,
            SessionState::AddingIps | SessionState::StartingServices | SessionState::Running
        ) {
            inner.resources_may_be_owned = true;
        }
        if matches!(next, SessionState::Stopped | SessionState::Failed) {
            inner.resources_may_be_owned = false;
            inner.worker_process_id = None;
            inner.last_heartbeat_ms = None;
        }
        Ok(inner.status.clone())
    }

    pub fn record_worker_connected(
        &self,
        session_id: &str,
        process_id: u32,
        at_ms: u64,
    ) -> Result<(), ManagerError> {
        if process_id == 0 {
            return Err(ManagerError::new(
                "device_simulator.worker.pid_invalid",
                "worker process id is zero",
            ));
        }
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        if inner.status.state != SessionState::StartingWorker {
            return Err(ManagerError::new(
                "device_simulator.session.transition_invalid",
                "worker connected outside starting_worker state",
            ));
        }
        inner.worker_process_id = Some(process_id);
        inner.last_heartbeat_ms = Some(at_ms);
        Ok(())
    }

    pub fn record_heartbeat(
        &self,
        session_id: &str,
        process_id: u32,
        at_ms: u64,
    ) -> Result<(), ManagerError> {
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        if inner.worker_process_id != Some(process_id) {
            return Err(ManagerError::new(
                "device_simulator.worker.pid_mismatch",
                "heartbeat process does not match the connected worker",
            ));
        }
        if inner
            .last_heartbeat_ms
            .is_some_and(|previous| at_ms < previous)
        {
            return Err(ManagerError::new(
                "device_simulator.worker.heartbeat_stale",
                "worker heartbeat timestamp moved backwards",
            ));
        }
        inner.last_heartbeat_ms = Some(at_ms);
        Ok(())
    }

    pub fn heartbeat_expired(&self, now_ms: u64, timeout: Duration) -> bool {
        let inner = self.inner.lock().expect("simulator manager poisoned");
        inner.last_heartbeat_ms.is_some_and(|last| {
            now_ms.saturating_sub(last) > timeout.as_millis().try_into().unwrap_or(u64::MAX)
        })
    }

    pub fn fail(
        &self,
        session_id: &str,
        error: SimulatorErrorBody,
    ) -> Result<SimulatorStatus, ManagerError> {
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        inner.status.state = if inner.resources_may_be_owned {
            SessionState::RecoveryRequired
        } else {
            SessionState::Failed
        };
        inner.status.error = Some(error);
        inner.status.updated_at_ms = now_ms();
        Ok(inner.status.clone())
    }

    pub fn mark_resources_released(&self, session_id: &str) -> Result<(), ManagerError> {
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        inner.resources_may_be_owned = false;
        Ok(())
    }

    /// A disconnected or crashed Worker is never restarted automatically:
    /// ownership must first be reconciled from the durable session journal.
    pub fn record_worker_loss(
        &self,
        session_id: &str,
        reason_code: &'static str,
    ) -> Result<SimulatorStatus, ManagerError> {
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        inner.status.state = if inner.resources_may_be_owned {
            SessionState::RecoveryRequired
        } else {
            SessionState::Failed
        };
        inner.status.error = Some(
            SimulatorErrorBody::new(reason_code, "deviceSimulator.errors.workerDisconnected")
                .retryable(false),
        );
        inner.status.updated_at_ms = now_ms();
        inner.worker_process_id = None;
        inner.last_heartbeat_ms = None;
        Ok(inner.status.clone())
    }

    pub fn record_stop_timeout(&self, session_id: &str) -> Result<SimulatorStatus, ManagerError> {
        let mut inner = self.inner.lock().expect("simulator manager poisoned");
        ensure_session(&inner, session_id)?;
        inner.resources_may_be_owned = true;
        inner.status.state = SessionState::RecoveryRequired;
        inner.status.error = Some(
            SimulatorErrorBody::new(
                "device_simulator.worker.stop_timeout",
                "deviceSimulator.errors.workerStopTimeout",
            )
            .retryable(false),
        );
        inner.status.updated_at_ms = now_ms();
        Ok(inner.status.clone())
    }
}

fn ensure_session(inner: &ManagerInner, session_id: &str) -> Result<(), ManagerError> {
    if inner.status.session_id.as_deref() != Some(session_id) {
        return Err(ManagerError::new(
            "device_simulator.session.mismatch",
            "request does not belong to the active simulator session",
        ));
    }
    Ok(())
}

fn transition_allowed(current: SessionState, next: SessionState) -> bool {
    use SessionState::*;
    matches!(
        (current, next),
        (Validating, AssetsRequired | Preflighting | Failed)
            | (AssetsRequired, DownloadingAssets | Failed)
            | (DownloadingAssets, Preflighting | Failed)
            | (Preflighting, StartingWorker | Failed)
            | (
                StartingWorker,
                AddingIps | StoppingServices | Failed | RecoveryRequired
            )
            | (AddingIps, StartingServices | RemovingIps | RecoveryRequired)
            | (
                StartingServices,
                Running | StoppingServices | RecoveryRequired
            )
            | (Running, StoppingAlarms | RecoveryRequired)
            | (StoppingAlarms, StoppingServices | RecoveryRequired)
            | (StoppingServices, RemovingFirewall | RecoveryRequired)
            | (RemovingFirewall, RemovingIps | RecoveryRequired)
            | (RemovingIps, Stopped | RecoveryRequired)
            | (RecoveryRequired, Recovering)
            | (
                Recovering,
                StoppingServices | RemovingFirewall | RemovingIps | Stopped | RecoveryRequired
            )
    )
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

    fn error() -> SimulatorErrorBody {
        SimulatorErrorBody::new("device_simulator.test", "deviceSimulator.errors.test")
    }

    #[test]
    fn enforces_single_session_and_full_happy_path() {
        let manager = SimulatorManager::default();
        manager.begin_session("session-1".into()).unwrap();
        assert_eq!(
            manager.begin_session("session-2".into()).unwrap_err().code,
            "device_simulator.session.already_active"
        );
        for state in [
            SessionState::Preflighting,
            SessionState::StartingWorker,
            SessionState::AddingIps,
            SessionState::StartingServices,
            SessionState::Running,
            SessionState::StoppingAlarms,
            SessionState::StoppingServices,
            SessionState::RemovingFirewall,
            SessionState::RemovingIps,
            SessionState::Stopped,
        ] {
            manager.transition("session-1", state).unwrap();
        }
        assert_eq!(manager.status().state, SessionState::Stopped);
        manager.begin_session("session-2".into()).unwrap();
    }

    #[test]
    fn failures_with_possible_resources_require_recovery() {
        let manager = SimulatorManager::default();
        manager.begin_session("session-1".into()).unwrap();
        manager
            .transition("session-1", SessionState::Preflighting)
            .unwrap();
        manager
            .transition("session-1", SessionState::StartingWorker)
            .unwrap();
        manager
            .transition("session-1", SessionState::AddingIps)
            .unwrap();
        assert_eq!(
            manager.fail("session-1", error()).unwrap().state,
            SessionState::RecoveryRequired
        );
        assert_eq!(
            manager.begin_session("session-2".into()).unwrap_err().code,
            "device_simulator.session.already_active"
        );
    }

    #[test]
    fn validates_worker_identity_heartbeat_and_transitions() {
        let manager = SimulatorManager::default();
        manager.begin_session("session-1".into()).unwrap();
        assert_eq!(
            manager
                .transition("session-1", SessionState::Running)
                .unwrap_err()
                .code,
            "device_simulator.session.transition_invalid"
        );
        manager
            .transition("session-1", SessionState::Preflighting)
            .unwrap();
        manager
            .transition("session-1", SessionState::StartingWorker)
            .unwrap();
        manager
            .record_worker_connected("session-1", 42, 100)
            .unwrap();
        manager.record_heartbeat("session-1", 42, 200).unwrap();
        assert!(manager.heartbeat_expired(1_500, Duration::from_secs(1)));
        assert_eq!(
            manager
                .record_heartbeat("session-1", 7, 300)
                .unwrap_err()
                .code,
            "device_simulator.worker.pid_mismatch"
        );
    }

    #[test]
    fn creates_random_launch_identity_and_rejects_unbounded_timeouts() {
        let manager = SimulatorManager::default();
        let started = manager.begin_random_session().unwrap();
        assert_eq!(
            started.status.session_id.as_deref(),
            Some(started.identity.session_id.as_str())
        );
        assert!(started.identity.pipe_name.contains("DeviceSimulator"));
        assert!(WorkerTimeoutPolicy::default().validate().is_ok());
        assert!(WorkerTimeoutPolicy {
            startup: Duration::ZERO,
            ..WorkerTimeoutPolicy::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn worker_loss_never_restarts_and_stop_timeout_requires_recovery() {
        let manager = SimulatorManager::default();
        manager.begin_session("session-1".into()).unwrap();
        manager
            .transition("session-1", SessionState::Preflighting)
            .unwrap();
        manager
            .transition("session-1", SessionState::StartingWorker)
            .unwrap();
        assert_eq!(
            manager
                .record_worker_loss("session-1", "device_simulator.worker.panicked")
                .unwrap()
                .state,
            SessionState::Failed
        );

        manager.begin_session("session-2".into()).unwrap();
        manager
            .transition("session-2", SessionState::Preflighting)
            .unwrap();
        manager
            .transition("session-2", SessionState::StartingWorker)
            .unwrap();
        manager
            .transition("session-2", SessionState::AddingIps)
            .unwrap();
        assert_eq!(
            manager.record_stop_timeout("session-2").unwrap().state,
            SessionState::RecoveryRequired
        );
        assert_eq!(
            manager.begin_random_session().unwrap_err().code,
            "device_simulator.session.already_active"
        );
    }
}
