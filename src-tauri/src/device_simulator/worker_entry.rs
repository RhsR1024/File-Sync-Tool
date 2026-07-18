use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::{SessionState, SimulatorStatus};
use crate::device_simulator::worker_protocol::{
    read_frame, write_frame, HandshakeRequest, WorkerCommandName, WorkerHello, WorkerMessage,
    WorkerResponse, WorkerResponseOutcome, WORKER_PROTOCOL_VERSION,
};
use serde_json::to_value;
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

    while let Some(message) = read_frame::<_, WorkerMessage>(&mut pipe)
        .await
        .map_err(|error| error.to_string())?
    {
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
                    write_frame(&mut pipe, &WorkerMessage::Response(response))
                        .await
                        .map_err(|error| error.to_string())?;
                    continue;
                }
                let shutdown = request.command.name == WorkerCommandName::Shutdown;
                let response =
                    handle_worker_request(&launch, request.request_id, request.command.name);
                write_frame(&mut pipe, &WorkerMessage::Response(response))
                    .await
                    .map_err(|error| error.to_string())?;
                if shutdown {
                    break;
                }
            }
            WorkerMessage::Heartbeat(_) => {}
            _ => return Err("unexpected worker protocol message".into()),
        }
    }
    Ok(())
}

fn handle_worker_request(
    launch: &WorkerLaunchArgs,
    request_id: String,
    command: WorkerCommandName,
) -> WorkerResponse {
    match command {
        WorkerCommandName::GetStatus => {
            let status = SimulatorStatus {
                session_id: Some(launch.session_id.clone()),
                state: SessionState::StartingWorker,
                updated_at_ms: now_ms(),
                error: None,
            };
            WorkerResponse::success(request_id, to_value(status).ok())
        }
        WorkerCommandName::Shutdown => WorkerResponse::success(request_id, None),
        _ => WorkerResponse {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id,
            outcome: WorkerResponseOutcome::Error {
                error: SimulatorErrorBody::new(
                    "device_simulator.worker.command_not_ready",
                    "deviceSimulator.errors.workerCommandNotReady",
                )
                .retryable(true),
            },
        },
    }
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

    #[test]
    fn status_and_shutdown_are_the_only_pre_service_commands() {
        let launch = WorkerLaunchArgs {
            session_id: "session-1".into(),
            pipe_name: "pipe-1".into(),
        };
        assert!(matches!(
            handle_worker_request(&launch, "r1".into(), WorkerCommandName::GetStatus).outcome,
            WorkerResponseOutcome::Success { .. }
        ));
        assert!(matches!(
            handle_worker_request(&launch, "r2".into(), WorkerCommandName::StartServices).outcome,
            WorkerResponseOutcome::Error { .. }
        ));
    }
}
