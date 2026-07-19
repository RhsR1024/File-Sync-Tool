use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::events::WorkerEvent;
use crate::device_simulator::models::{SessionState, SimulatorStatus};
use crate::device_simulator::windows::named_pipe::PipeIdentity;
use crate::device_simulator::worker_protocol::{
    WorkerCommand, WorkerCommandName, WorkerHeartbeat, WorkerMessage, WorkerRequest,
    WorkerResponseOutcome, WORKER_PROTOCOL_VERSION,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, watch, Mutex as AsyncMutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerError {
    pub code: &'static str,
    pub message: String,
    pub worker_error: Option<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedSession {
    pub identity: PipeIdentity,
    pub status: SimulatorStatus,
}

#[derive(Debug, Clone)]
pub enum ManagerNotification {
    Heartbeat {
        process_id: u32,
        heartbeat: WorkerHeartbeat,
    },
    Event(WorkerEvent),
    WorkerLost {
        session_id: String,
        process_id: u32,
        code: &'static str,
        details: String,
    },
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
            worker_error: None,
        }
    }

    fn from_worker(error: SimulatorErrorBody) -> Self {
        Self {
            code: "device_simulator.worker.command_failed",
            message: error.details.clone().unwrap_or_else(|| error.code.clone()),
            worker_error: Some(error),
        }
    }

    pub fn into_body(self) -> SimulatorErrorBody {
        self.worker_error.unwrap_or_else(|| {
            SimulatorErrorBody::new(self.code, "deviceSimulator.errors.workerCommandFailed")
                .with_public_details(self.message)
                .retryable(false)
        })
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

pub struct SimulatorManager {
    inner: Mutex<ManagerInner>,
    worker: AsyncMutex<Option<WorkerClient>>,
    timeout_policy: WorkerTimeoutPolicy,
}

impl Default for SimulatorManager {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ManagerInner::default()),
            worker: AsyncMutex::new(None),
            timeout_policy: WorkerTimeoutPolicy::default(),
        }
    }
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

    pub async fn has_worker(&self) -> bool {
        self.worker.lock().await.is_some()
    }

    #[cfg(target_os = "windows")]
    pub async fn launch_worker(
        &self,
        identity: &PipeIdentity,
        notifications: mpsc::UnboundedSender<ManagerNotification>,
    ) -> Result<u32, ManagerError> {
        use crate::device_simulator::windows::elevation::{
            launch_elevated_worker, WorkerLaunchSpec,
        };
        use crate::device_simulator::windows::named_pipe::{
            accept_and_verify_worker, create_secure_server,
        };

        self.timeout_policy.validate()?;
        let mut worker = self.worker.lock().await;
        if worker.is_some() {
            return Err(ManagerError::new(
                "device_simulator.worker.already_connected",
                "an elevated Worker is already connected",
            ));
        }
        let executable = std::env::current_exe().map_err(|source| {
            ManagerError::new(
                "device_simulator.worker.executable_unavailable",
                format!("could not resolve current executable: {source}"),
            )
        })?;
        let server = create_secure_server(identity).map_err(pipe_accept_error)?;
        let spec = WorkerLaunchSpec::new(
            executable,
            identity.session_id.clone(),
            identity.pipe_name.clone(),
        )
        .map_err(|source| {
            ManagerError::new(
                "device_simulator.worker.launch_spec_invalid",
                source.public_details,
            )
        })?;
        let process = launch_elevated_worker(&spec).map_err(|source| {
            let code = match source.kind {
                crate::device_simulator::windows::elevation::ElevationErrorKind::UacCancelled => {
                    "device_simulator.worker.uac_cancelled"
                }
                _ => "device_simulator.worker.launch_failed",
            };
            ManagerError::new(code, source.public_details)
        })?;
        let process_id = process.process_id();
        let pipe =
            accept_and_verify_worker(server, identity, process_id, self.timeout_policy.startup)
                .await
                .map_err(pipe_accept_error)?;
        self.record_worker_connected(&identity.session_id, process_id, now_ms())?;
        let transport = Arc::new(WorkerTransport {
            pipe: AsyncMutex::new(pipe),
            session_id: identity.session_id.clone(),
            process_id,
            request_timeout: self.timeout_policy.request,
            notifications: notifications.clone(),
        });
        let (cancel, cancel_rx) = watch::channel(false);
        let monitor = tokio::spawn(monitor_worker(
            Arc::clone(&transport),
            cancel_rx,
            notifications,
        ));
        *worker = Some(WorkerClient {
            transport,
            cancel,
            monitor,
            process,
        });
        Ok(process_id)
    }

    #[cfg(not(target_os = "windows"))]
    pub async fn launch_worker(
        &self,
        _identity: &PipeIdentity,
        _notifications: mpsc::UnboundedSender<ManagerNotification>,
    ) -> Result<u32, ManagerError> {
        Err(ManagerError::new(
            "device_simulator.worker.unsupported_platform",
            "elevated Worker launch is only supported on Windows",
        ))
    }

    #[cfg(target_os = "windows")]
    pub async fn request_worker<P: Serialize>(
        &self,
        command: WorkerCommandName,
        payload: Option<&P>,
    ) -> Result<Option<Value>, ManagerError> {
        let payload = payload
            .map(serde_json::to_value)
            .transpose()
            .map_err(|source| {
                ManagerError::new(
                    "device_simulator.worker.request_serialize_failed",
                    format!("could not serialize Worker request: {source}"),
                )
            })?;
        let worker = self.worker.lock().await;
        let client = worker.as_ref().ok_or_else(|| {
            ManagerError::new(
                "device_simulator.worker.not_connected",
                "there is no connected elevated Worker",
            )
        })?;
        client.transport.request(command, payload).await
    }

    #[cfg(not(target_os = "windows"))]
    pub async fn request_worker<P: Serialize>(
        &self,
        _command: WorkerCommandName,
        _payload: Option<&P>,
    ) -> Result<Option<Value>, ManagerError> {
        Err(ManagerError::new(
            "device_simulator.worker.unsupported_platform",
            "Worker requests are only supported on Windows",
        ))
    }

    #[cfg(target_os = "windows")]
    pub async fn shutdown_worker(&self) -> Result<(), ManagerError> {
        let mut slot = self.worker.lock().await;
        let Some(worker) = slot.take() else {
            return Ok(());
        };
        let _ = worker.cancel.send(true);
        let shutdown = worker
            .transport
            .request(WorkerCommandName::Shutdown, None)
            .await;
        worker.monitor.abort();
        let _ = worker.monitor.await;
        if let Err(source) = shutdown {
            return Err(source);
        }
        let deadline = tokio::time::Instant::now() + self.timeout_policy.stop;
        loop {
            if worker
                .process
                .try_exit_code()
                .map_err(|source| {
                    ManagerError::new(
                        "device_simulator.worker.exit_query_failed",
                        source.public_details,
                    )
                })?
                .is_some()
            {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ManagerError::new(
                    "device_simulator.worker.stop_timeout",
                    "elevated Worker did not exit within the finite stop timeout",
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub async fn shutdown_worker(&self) -> Result<(), ManagerError> {
        *self.worker.lock().await = None;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
struct WorkerTransport {
    pipe: AsyncMutex<tokio::net::windows::named_pipe::NamedPipeServer>,
    session_id: String,
    process_id: u32,
    request_timeout: Duration,
    notifications: mpsc::UnboundedSender<ManagerNotification>,
}

#[cfg(target_os = "windows")]
impl WorkerTransport {
    async fn request(
        &self,
        command: WorkerCommandName,
        payload: Option<Value>,
    ) -> Result<Option<Value>, ManagerError> {
        let request_id = uuid::Uuid::new_v4().simple().to_string();
        let request = WorkerMessage::Request(WorkerRequest {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id: request_id.clone(),
            command: WorkerCommand {
                name: command,
                payload,
            },
        });
        let operation = async {
            let mut pipe = self.pipe.lock().await;
            crate::device_simulator::worker_protocol::write_frame(&mut *pipe, &request)
                .await
                .map_err(|source| worker_io_error("write", source.to_string()))?;
            loop {
                let message = crate::device_simulator::worker_protocol::read_frame::<
                    _,
                    WorkerMessage,
                >(&mut *pipe)
                .await
                .map_err(|source| worker_io_error("read", source.to_string()))?
                .ok_or_else(|| {
                    worker_io_error("read", "Worker closed the named pipe before responding")
                })?;
                match message {
                    WorkerMessage::Response(response) if response.request_id == request_id => {
                        if response.protocol_version != WORKER_PROTOCOL_VERSION {
                            return Err(ManagerError::new(
                                "device_simulator.worker.protocol_incompatible",
                                "Worker response protocol version does not match",
                            ));
                        }
                        return match response.outcome {
                            WorkerResponseOutcome::Success { payload } => Ok(payload),
                            WorkerResponseOutcome::Error { error } => {
                                Err(ManagerError::from_worker(error))
                            }
                        };
                    }
                    WorkerMessage::Heartbeat(heartbeat) => {
                        if heartbeat.session_id != self.session_id {
                            return Err(ManagerError::new(
                                "device_simulator.worker.session_mismatch",
                                "Worker heartbeat belongs to another session",
                            ));
                        }
                        let _ = self.notifications.send(ManagerNotification::Heartbeat {
                            process_id: self.process_id,
                            heartbeat,
                        });
                    }
                    WorkerMessage::Event(event) => {
                        if event.session_id != self.session_id {
                            return Err(ManagerError::new(
                                "device_simulator.worker.session_mismatch",
                                "Worker event belongs to another session",
                            ));
                        }
                        let _ = self.notifications.send(ManagerNotification::Event(event));
                    }
                    WorkerMessage::Response(_) => {
                        return Err(ManagerError::new(
                            "device_simulator.worker.response_mismatch",
                            "Worker response request ID does not match the active request",
                        ));
                    }
                    _ => {
                        return Err(ManagerError::new(
                            "device_simulator.worker.message_unexpected",
                            "Worker sent an unexpected protocol message",
                        ));
                    }
                }
            }
        };
        match tokio::time::timeout(self.request_timeout, operation).await {
            Ok(result) => result,
            Err(_) => Err(ManagerError::new(
                "device_simulator.worker.request_timeout",
                format!("Worker command {command:?} timed out"),
            )),
        }
    }
}

#[cfg(target_os = "windows")]
struct WorkerClient {
    transport: Arc<WorkerTransport>,
    cancel: watch::Sender<bool>,
    monitor: tokio::task::JoinHandle<()>,
    process: crate::device_simulator::windows::elevation::ElevatedWorkerProcess,
}

#[cfg(not(target_os = "windows"))]
struct WorkerClient;

#[cfg(target_os = "windows")]
async fn monitor_worker(
    transport: Arc<WorkerTransport>,
    mut cancel: watch::Receiver<bool>,
    notifications: mpsc::UnboundedSender<ManagerNotification>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                if let Err(source) = transport.request(WorkerCommandName::GetStatus, None).await {
                    let _ = notifications.send(ManagerNotification::WorkerLost {
                        session_id: transport.session_id.clone(),
                        process_id: transport.process_id,
                        code: "device_simulator.worker.disconnected",
                        details: source.message,
                    });
                    break;
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn pipe_accept_error(
    source: crate::device_simulator::windows::named_pipe::PipeAcceptError,
) -> ManagerError {
    use crate::device_simulator::windows::named_pipe::PipeAcceptErrorKind;
    let code = match source.kind {
        PipeAcceptErrorKind::StartupTimeout => "device_simulator.worker.startup_timeout",
        PipeAcceptErrorKind::ProcessIdMismatch => "device_simulator.worker.pid_mismatch",
        PipeAcceptErrorKind::HandshakeRejected => "device_simulator.worker.handshake_rejected",
        PipeAcceptErrorKind::CreateFailed => "device_simulator.worker.pipe_create_failed",
        _ => "device_simulator.worker.pipe_failed",
    };
    ManagerError::new(code, source.public_details)
}

fn worker_io_error(action: &'static str, details: impl Into<String>) -> ManagerError {
    ManagerError::new(
        "device_simulator.worker.io_failed",
        format!("Worker pipe {action} failed: {}", details.into()),
    )
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
