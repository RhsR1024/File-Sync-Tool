use crate::device_simulator::worker_protocol::{
    handshake_response, HandshakeExpectation, HandshakeRequest, WorkerMessage,
};
use std::time::Duration;

const PIPE_PREFIX: &str = "FileSyncTool-DeviceSimulator-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeIdentity {
    pub session_id: String,
    pub pipe_name: String,
}

impl PipeIdentity {
    pub fn generate() -> Self {
        let session_id = uuid::Uuid::new_v4().simple().to_string();
        let pipe_entropy = uuid::Uuid::new_v4().simple().to_string();
        Self {
            session_id,
            pipe_name: format!("{PIPE_PREFIX}{pipe_entropy}"),
        }
    }

    pub fn pipe_path(&self) -> String {
        format!(r"\\.\pipe\{}", self.pipe_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeAcceptErrorKind {
    Unsupported,
    CreateFailed,
    StartupTimeout,
    Disconnected,
    UnexpectedMessage,
    HandshakeRejected,
    ProcessIdMismatch,
    ProtocolIo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeAcceptError {
    pub kind: PipeAcceptErrorKind,
    pub public_details: String,
}

fn pipe_error(kind: PipeAcceptErrorKind, details: impl Into<String>) -> PipeAcceptError {
    PipeAcceptError {
        kind,
        public_details: details.into(),
    }
}

#[cfg(target_os = "windows")]
pub fn create_secure_server(
    identity: &PipeIdentity,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, PipeAcceptError> {
    use std::mem::size_of;
    use windows::core::w;
    use windows::Win32::Foundation::{LocalFree, BOOL, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    // The object owner is the current user. OW grants that owner full access;
    // BA and SY permit the elevated Worker and Windows itself. The protected
    // DACL does not inherit broader permissions, and Tokio rejects remote
    // clients at the pipe layer.
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:P(A;;GA;;;OW)(A;;GA;;;BA)(A;;GA;;;SY)"),
            SDDL_REVISION,
            &mut descriptor,
            None,
        )
    }
    .map_err(|source| {
        pipe_error(
            PipeAcceptErrorKind::CreateFailed,
            format!("could not create private pipe security descriptor: {source}"),
        )
    })?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: BOOL(0),
    };
    let result = unsafe {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                identity.pipe_path(),
                &mut attributes as *mut SECURITY_ATTRIBUTES as *mut _,
            )
    };
    unsafe {
        LocalFree(HLOCAL(descriptor.0));
    }
    result.map_err(|source| {
        pipe_error(
            PipeAcceptErrorKind::CreateFailed,
            format!("could not create private Worker pipe: {source}"),
        )
    })
}

#[cfg(target_os = "windows")]
pub async fn accept_and_verify_worker(
    mut server: tokio::net::windows::named_pipe::NamedPipeServer,
    identity: &PipeIdentity,
    expected_process_id: u32,
    startup_timeout: Duration,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, PipeAcceptError> {
    if startup_timeout.is_zero() || expected_process_id == 0 {
        return Err(pipe_error(
            PipeAcceptErrorKind::HandshakeRejected,
            "worker startup timeout and process id must be valid",
        ));
    }
    tokio::time::timeout(startup_timeout, server.connect())
        .await
        .map_err(|_| {
            pipe_error(
                PipeAcceptErrorKind::StartupTimeout,
                "timed out waiting for elevated Worker pipe connection",
            )
        })?
        .map_err(|source| {
            pipe_error(
                PipeAcceptErrorKind::Disconnected,
                format!("Worker pipe connection failed: {source}"),
            )
        })?;

    let message = tokio::time::timeout(
        startup_timeout,
        crate::device_simulator::worker_protocol::read_frame::<_, WorkerMessage>(&mut server),
    )
    .await
    .map_err(|_| {
        pipe_error(
            PipeAcceptErrorKind::StartupTimeout,
            "timed out waiting for Worker handshake",
        )
    })?
    .map_err(|source| {
        pipe_error(
            PipeAcceptErrorKind::ProtocolIo,
            format!("Worker handshake could not be read: {source}"),
        )
    })?
    .ok_or_else(|| {
        pipe_error(
            PipeAcceptErrorKind::Disconnected,
            "Worker disconnected before handshake",
        )
    })?;
    let request = match message {
        WorkerMessage::HandshakeRequest(request) => request,
        _ => {
            return Err(pipe_error(
                PipeAcceptErrorKind::UnexpectedMessage,
                "Worker sent an unexpected first protocol message",
            ))
        }
    };
    let response = verified_handshake_response(&request, identity, expected_process_id);
    crate::device_simulator::worker_protocol::write_frame(
        &mut server,
        &WorkerMessage::HandshakeResponse(response.clone()),
    )
    .await
    .map_err(|source| {
        pipe_error(
            PipeAcceptErrorKind::ProtocolIo,
            format!("Worker handshake response could not be sent: {source}"),
        )
    })?;
    if !response.accepted {
        let kind = if request.hello.process_id != expected_process_id {
            PipeAcceptErrorKind::ProcessIdMismatch
        } else {
            PipeAcceptErrorKind::HandshakeRejected
        };
        return Err(pipe_error(kind, "Worker handshake was rejected"));
    }
    Ok(server)
}

fn verified_handshake_response(
    request: &HandshakeRequest,
    identity: &PipeIdentity,
    expected_process_id: u32,
) -> crate::device_simulator::worker_protocol::HandshakeResponse {
    let expectation = HandshakeExpectation::for_session(&identity.session_id);
    let mut response = handshake_response(request, &expectation);
    if response.accepted && request.hello.process_id != expected_process_id {
        response.accepted = false;
        response.error = Some(crate::device_simulator::errors::SimulatorErrorBody::new(
            "device_simulator.worker.pid_mismatch",
            "deviceSimulator.errors.workerPidMismatch",
        ));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::worker_protocol::{WorkerHello, WORKER_PROTOCOL_VERSION};

    fn request(identity: &PipeIdentity, process_id: u32) -> HandshakeRequest {
        HandshakeRequest {
            request_id: "request-1".into(),
            hello: WorkerHello {
                worker_protocol_version: WORKER_PROTOCOL_VERSION,
                app_version: env!("CARGO_PKG_VERSION").into(),
                session_id: identity.session_id.clone(),
                process_id,
                elevated: true,
            },
        }
    }

    #[test]
    fn generated_pipe_and_session_ids_are_random_and_cli_safe() {
        let first = PipeIdentity::generate();
        let second = PipeIdentity::generate();
        assert_ne!(first, second);
        assert!(first.pipe_name.starts_with(PIPE_PREFIX));
        assert!(first
            .pipe_name
            .bytes()
            .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') }));
        assert!(first.pipe_path().starts_with(r"\\.\pipe\"));
    }

    #[test]
    fn handshake_requires_launched_process_id_in_addition_to_session_and_version() {
        let identity = PipeIdentity::generate();
        assert!(verified_handshake_response(&request(&identity, 42), &identity, 42).accepted);
        let rejected = verified_handshake_response(&request(&identity, 7), &identity, 42);
        assert!(!rejected.accepted);
        assert_eq!(
            rejected.error.unwrap().code,
            "device_simulator.worker.pid_mismatch"
        );
    }
}
