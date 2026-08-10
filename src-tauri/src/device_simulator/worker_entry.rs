use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::events::{WorkerEvent, WorkerEventPayload};
use crate::device_simulator::session_journal::WorkerProcessIdentity;
use crate::device_simulator::telemetry::ProtocolDiagnosticSink;
use crate::device_simulator::worker_protocol::{
    read_frame, write_frame, AlarmJobCommandPayload, HandshakeRequest, InitializeSessionPayload,
    RecoverSessionPayload, StopAlarmJobPayload, WorkerCommandName, WorkerHeartbeat, WorkerHello,
    WorkerMessage, WorkerRequest, WorkerResponse, WORKER_PROTOCOL_VERSION,
};
use crate::device_simulator::worker_runtime::WorkerRuntime;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{to_value, Value};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WORKER_FLAG: &str = "--simulator-worker";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerLaunchArgs {
    pub session_id: String,
    pub pipe_name: String,
}

pub fn try_run_from_env() -> Option<i32> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !args.iter().any(|argument| argument == WORKER_FLAG) {
        return None;
    }
    let launch = match WorkerLaunchArgs::parse(&args) {
        Ok(launch) => launch,
        Err(_) => return Some(2),
    };
    Some(run_worker_process(launch))
}

impl WorkerLaunchArgs {
    pub fn parse(args: &[String]) -> Result<Self, &'static str> {
        let mut worker = false;
        let mut session_id = None;
        let mut pipe_name = None;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                WORKER_FLAG if !worker => worker = true,
                "--session-id" if session_id.is_none() => {
                    index += 1;
                    session_id = args.get(index).cloned();
                }
                "--pipe-name" if pipe_name.is_none() => {
                    index += 1;
                    pipe_name = args.get(index).cloned();
                }
                _ => return Err("unknown or duplicate worker argument"),
            }
            index += 1;
        }
        if !worker {
            return Err("worker flag is missing");
        }
        let session_id = session_id.ok_or("session id is missing")?;
        let pipe_name = pipe_name.ok_or("pipe name is missing")?;
        if !valid_identifier(&session_id, 128) || !valid_identifier(&pipe_name, 180) {
            return Err("worker identifiers are invalid");
        }
        Ok(Self {
            session_id,
            pipe_name,
        })
    }
}

fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn run_worker_process(launch: WorkerLaunchArgs) -> i32 {
    #[cfg(target_os = "windows")]
    {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return 3,
        };
        match runtime.block_on(run_windows_worker(launch)) {
            Ok(()) => 0,
            Err(_) => 4,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = launch;
        5
    }
}

#[cfg(target_os = "windows")]
async fn run_windows_worker(launch: WorkerLaunchArgs) -> Result<(), String> {
    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::sync::mpsc;
    use tokio::time::{sleep, Instant};

    let path = format!(r"\\.\pipe\{}", launch.pipe_name);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut pipe = loop {
        match ClientOptions::new().open(&path) {
            Ok(client) => break client,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                sleep(Duration::from_millis(100)).await;
            }
            Err(error) => return Err(format!("worker pipe connection failed: {error}")),
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();
    let handshake = HandshakeRequest {
        request_id: request_id.clone(),
        hello: WorkerHello {
            worker_protocol_version: WORKER_PROTOCOL_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            session_id: launch.session_id.clone(),
            process_id: std::process::id(),
            elevated: is_elevated(),
        },
    };
    write_frame(&mut pipe, &WorkerMessage::HandshakeRequest(handshake))
        .await
        .map_err(|error| error.to_string())?;
    let response = read_frame::<_, WorkerMessage>(&mut pipe)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "worker pipe closed during handshake".to_string())?;
    match response {
        WorkerMessage::HandshakeResponse(response)
            if response.request_id == request_id
                && response.accepted
                && response.session_id == launch.session_id => {}
        _ => return Err("worker handshake was rejected".into()),
    }

    let worker_process = tokio::task::spawn_blocking(current_worker_identity)
        .await
        .map_err(|error| format!("worker identity task failed: {error}"))??;
    let (diagnostic_tx, mut diagnostic_rx) = mpsc::unbounded_channel();
    let mut runtime = WorkerRuntime::system_with_diagnostics(
        launch.session_id.clone(),
        Some(worker_process),
        ProtocolDiagnosticSink::new(diagnostic_tx),
    );
    let (mut reader, mut writer) = tokio::io::split(pipe);
    let (outgoing, mut outgoing_rx) = mpsc::channel::<WorkerMessage>(256);
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            write_frame(&mut writer, &message)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    });
    let event_sequence = Arc::new(AtomicU64::new(0));
    let diagnostic_outgoing = outgoing.clone();
    let diagnostic_session = launch.session_id.clone();
    let diagnostic_sequence = Arc::clone(&event_sequence);
    let diagnostic_task = tokio::spawn(async move {
        while let Some(payload) = diagnostic_rx.recv().await {
            let sequence = diagnostic_sequence
                .fetch_add(1, Ordering::Relaxed)
                .saturating_add(1);
            if diagnostic_outgoing
                .send(WorkerMessage::Event(WorkerEvent::new(
                    diagnostic_session.clone(),
                    sequence,
                    now_ms(),
                    payload,
                )))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let heartbeat_tx = outgoing.clone();
    let heartbeat_session = launch.session_id.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut sequence = 0_u64;
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            sequence = sequence.saturating_add(1);
            if heartbeat_tx
                .send(WorkerMessage::Heartbeat(WorkerHeartbeat {
                    session_id: heartbeat_session.clone(),
                    sequence,
                    sent_at_ms: now_ms(),
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let loop_result = loop {
        let message = match read_frame::<_, WorkerMessage>(&mut reader).await {
            Ok(Some(message)) => message,
            Ok(None) => break Ok(()),
            Err(error) => break Err(error.to_string()),
        };
        match message {
            WorkerMessage::Request(request) => {
                if request.protocol_version != WORKER_PROTOCOL_VERSION {
                    let response = WorkerResponse::error(
                        request.request_id,
                        SimulatorErrorBody::new(
                            "device_simulator.worker.protocol_incompatible",
                            "deviceSimulator.errors.workerProtocolIncompatible",
                        ),
                    );
                    outgoing
                        .send(WorkerMessage::Response(response))
                        .await
                        .map_err(|_| "worker response channel closed".to_string())?;
                    continue;
                }
                let shutdown = request.command.name == WorkerCommandName::Shutdown;
                let previous = runtime.state();
                let response = handle_worker_request(&mut runtime, request).await;
                let current = runtime.state();
                if current != previous {
                    let sequence = event_sequence
                        .fetch_add(1, Ordering::Relaxed)
                        .saturating_add(1);
                    let _ = outgoing
                        .send(WorkerMessage::Event(WorkerEvent::new(
                            launch.session_id.clone(),
                            sequence,
                            now_ms(),
                            WorkerEventPayload::StatusChanged { previous, current },
                        )))
                        .await;
                }
                outgoing
                    .send(WorkerMessage::Response(response))
                    .await
                    .map_err(|_| "worker response channel closed".to_string())?;
                if shutdown {
                    break Ok(());
                }
            }
            WorkerMessage::Heartbeat(_) => {}
            _ => break Err("unexpected worker protocol message".into()),
        }
    };
    if !matches!(
        runtime.state(),
        crate::device_simulator::models::SessionState::Stopped
    ) {
        let _ = runtime.stop_services().await;
    }
    drop(runtime);
    let _ = diagnostic_task.await;
    heartbeat_task.abort();
    drop(outgoing);
    let writer_result = writer_task
        .await
        .map_err(|error| format!("worker writer task failed: {error}"))?;
    loop_result.and(writer_result)
}

async fn handle_worker_request(
    runtime: &mut WorkerRuntime,
    request: WorkerRequest,
) -> WorkerResponse {
    let request_id = request.request_id;
    let result = match request.command.name {
        WorkerCommandName::InitializeSession => {
            let payload = decode_payload::<InitializeSessionPayload>(request.command.payload);
            match payload {
                Ok(payload) => runtime
                    .initialize_session(payload)
                    .await
                    .and_then(response_value),
                Err(error) => Err(error),
            }
        }
        WorkerCommandName::RunPreflight => runtime.run_preflight().await.and_then(response_value),
        WorkerCommandName::StartServices => runtime.start_services().await.and_then(response_value),
        WorkerCommandName::StopServices => runtime.stop_services().await.and_then(response_value),
        WorkerCommandName::StartAlarmJob => {
            match decode_payload::<AlarmJobCommandPayload>(request.command.payload) {
                Ok(payload) => runtime
                    .start_alarm_job(payload.request)
                    .await
                    .and_then(response_value),
                Err(error) => Err(error),
            }
        }
        WorkerCommandName::TriggerAlarmOnce => {
            match decode_payload::<AlarmJobCommandPayload>(request.command.payload) {
                Ok(payload) => runtime
                    .trigger_alarm_once(payload.request)
                    .await
                    .and_then(response_value),
                Err(error) => Err(error),
            }
        }
        WorkerCommandName::StopAlarmJob => {
            match decode_payload::<StopAlarmJobPayload>(request.command.payload) {
                Ok(payload) => runtime.stop_alarm_job(payload.job_id).await.map(|_| None),
                Err(error) => Err(error),
            }
        }
        WorkerCommandName::GetStatus => Ok(to_value(runtime.status_snapshot().await).ok()),
        WorkerCommandName::GetRuntimeTelemetry => {
            Ok(to_value(runtime.telemetry_snapshot().await).ok())
        }
        WorkerCommandName::Shutdown
            if matches!(
                runtime.state(),
                crate::device_simulator::models::SessionState::Idle
                    | crate::device_simulator::models::SessionState::Stopped
                    | crate::device_simulator::models::SessionState::Failed
            ) =>
        {
            Ok(None)
        }
        WorkerCommandName::Shutdown => runtime.stop_services().await.map(|_| None),
        WorkerCommandName::RecoverSession => {
            let payload = decode_payload::<RecoverSessionPayload>(request.command.payload);
            match payload {
                Ok(payload) => {
                    let recovery = tokio::task::spawn_blocking(move || {
                        crate::device_simulator::windows::recovery::recover_recorded_session(
                            &payload.app_data_dir,
                            &payload.session_id,
                        )
                    })
                    .await;
                    match recovery {
                        Ok(Ok(result)) => response_value(result),
                        Ok(Err(error)) => {
                            return WorkerResponse::error(request_id, error.into_body())
                        }
                        Err(error) => {
                            return WorkerResponse::error(
                                request_id,
                                SimulatorErrorBody::new(
                                    "device_simulator.recovery.task_failed",
                                    "deviceSimulator.errors.recoveryFailed",
                                )
                                .with_public_details(error.to_string()),
                            )
                        }
                    }
                }
                Err(error) => Err(error),
            }
        }
    };
    match result {
        Ok(payload) => WorkerResponse::success(request_id, payload),
        Err(error) => WorkerResponse::error(request_id, error.into_body()),
    }
}

fn decode_payload<T: DeserializeOwned>(
    payload: Option<Value>,
) -> Result<T, crate::device_simulator::worker_runtime::WorkerRuntimeError> {
    let payload =
        payload.ok_or_else(
            || crate::device_simulator::worker_runtime::WorkerRuntimeError {
                code: "device_simulator.worker.payload_missing",
                message: "Worker command payload is missing".into(),
            },
        )?;
    serde_json::from_value(payload).map_err(|source| {
        crate::device_simulator::worker_runtime::WorkerRuntimeError {
            code: "device_simulator.worker.payload_invalid",
            message: format!("Worker command payload is invalid: {source}"),
        }
    })
}

fn response_value<T: Serialize>(
    value: T,
) -> Result<Option<Value>, crate::device_simulator::worker_runtime::WorkerRuntimeError> {
    to_value(value).map(Some).map_err(|source| {
        crate::device_simulator::worker_runtime::WorkerRuntimeError {
            code: "device_simulator.worker.response_serialize_failed",
            message: format!("Worker response could not be serialized: {source}"),
        }
    })
}

#[cfg(target_os = "windows")]
fn current_worker_identity() -> Result<WorkerProcessIdentity, String> {
    use sha2::{Digest, Sha256};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    }
    .map_err(|source| format!("could not read Worker process creation time: {source}"))?;
    let creation_time_100ns =
        (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    let executable = std::env::current_exe()
        .map_err(|source| format!("could not resolve Worker executable: {source}"))?;
    let mut file = std::fs::File::open(&executable)
        .map_err(|source| format!("could not open Worker executable: {source}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|source| format!("could not hash Worker executable: {source}"))?;
        if length == 0 {
            break;
        }
        hasher.update(&buffer[..length]);
    }
    let executable_identity = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(WorkerProcessIdentity {
        pid: std::process::id(),
        creation_time_100ns,
        executable_identity,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(target_os = "windows")]
fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::worker_protocol::WorkerResponseOutcome;

    #[test]
    fn parses_only_minimal_non_secret_worker_arguments() {
        let args = [
            WORKER_FLAG,
            "--session-id",
            "session-123",
            "--pipe-name",
            "FileSyncTool-DeviceSimulator-abc",
        ]
        .map(str::to_string);
        assert_eq!(
            WorkerLaunchArgs::parse(&args).unwrap(),
            WorkerLaunchArgs {
                session_id: "session-123".into(),
                pipe_name: "FileSyncTool-DeviceSimulator-abc".into(),
            }
        );
        let mut secret = args.to_vec();
        secret.extend(["--password".into(), "secret".into()]);
        assert!(WorkerLaunchArgs::parse(&secret).is_err());
    }

    #[tokio::test]
    async fn status_and_shutdown_are_the_only_pre_initialization_commands() {
        let mut runtime = WorkerRuntime::system("session-1", None);
        let request = |request_id: &str, name| WorkerRequest {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id: request_id.into(),
            command: crate::device_simulator::worker_protocol::WorkerCommand {
                name,
                payload: None,
            },
        };
        assert!(matches!(
            handle_worker_request(&mut runtime, request("r1", WorkerCommandName::GetStatus),)
                .await
                .outcome,
            WorkerResponseOutcome::Success { .. }
        ));
        assert!(matches!(
            handle_worker_request(
                &mut runtime,
                request("r2", WorkerCommandName::StartServices),
            )
            .await
            .outcome,
            WorkerResponseOutcome::Error { .. }
        ));
        assert!(matches!(
            handle_worker_request(&mut runtime, request("r3", WorkerCommandName::Shutdown))
                .await
                .outcome,
            WorkerResponseOutcome::Success { .. }
        ));
    }
}
