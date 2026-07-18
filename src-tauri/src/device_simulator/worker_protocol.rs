use crate::device_simulator::errors::{
    worker_io_error, worker_json_error, SimulatorError, SimulatorErrorBody, SimulatorResult,
    WORKER_FRAME_TOO_LARGE, WORKER_FRAME_TRUNCATED, WORKER_HELLO_INVALID, WORKER_NOT_ELEVATED,
    WORKER_PROTOCOL_INCOMPATIBLE, WORKER_SESSION_MISMATCH,
};
use crate::device_simulator::events::WorkerEvent;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const WORKER_PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_MAX_FRAME_LEN: usize = 1024 * 1024;
const FRAME_PREFIX_LEN: usize = std::mem::size_of::<u32>();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerHello {
    pub worker_protocol_version: u32,
    pub app_version: String,
    pub session_id: String,
    pub process_id: u32,
    pub elevated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeRequest {
    pub request_id: String,
    pub hello: WorkerHello,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeResponse {
    pub request_id: String,
    pub accepted_protocol_version: u32,
    pub session_id: String,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<SimulatorErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeExpectation {
    pub session_id: String,
    pub protocol_version: u32,
    pub require_elevated: bool,
}

impl HandshakeExpectation {
    pub fn for_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            protocol_version: WORKER_PROTOCOL_VERSION,
            require_elevated: true,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub command: WorkerCommand,
}

impl fmt::Debug for WorkerRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkerRequest")
            .field("protocol_version", &self.protocol_version)
            .field("request_id", &self.request_id)
            .field("command", &self.command.name)
            .field(
                "payload",
                &self.command.payload.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCommand {
    pub name: WorkerCommandName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

impl fmt::Debug for WorkerCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkerCommand")
            .field("name", &self.name)
            .field("payload", &self.payload.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerCommandName {
    InitializeSession,
    RunPreflight,
    StartServices,
    StopServices,
    StartAlarmJob,
    StopAlarmJob,
    TriggerAlarmOnce,
    GetStatus,
    Shutdown,
    RecoverSession,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerResponse {
    pub protocol_version: u32,
    pub request_id: String,
    #[serde(flatten)]
    pub outcome: WorkerResponseOutcome,
}

impl WorkerResponse {
    pub fn success(request_id: impl Into<String>, payload: Option<Value>) -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id: request_id.into(),
            outcome: WorkerResponseOutcome::Success { payload },
        }
    }

    pub fn error(request_id: impl Into<String>, error: SimulatorErrorBody) -> Self {
        Self {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id: request_id.into(),
            outcome: WorkerResponseOutcome::Error { error },
        }
    }
}

impl fmt::Debug for WorkerResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let outcome = match &self.outcome {
            WorkerResponseOutcome::Success { .. } => "success",
            WorkerResponseOutcome::Error { .. } => "error",
        };
        formatter
            .debug_struct("WorkerResponse")
            .field("protocol_version", &self.protocol_version)
            .field("request_id", &self.request_id)
            .field("outcome", &outcome)
            .field("payload", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkerResponseOutcome {
    Success {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    Error {
        error: SimulatorErrorBody,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerHeartbeat {
    pub session_id: String,
    pub sequence: u64,
    pub sent_at_ms: u64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "message_type", content = "message", rename_all = "snake_case")]
pub enum WorkerMessage {
    HandshakeRequest(HandshakeRequest),
    HandshakeResponse(HandshakeResponse),
    Request(WorkerRequest),
    Response(WorkerResponse),
    Event(WorkerEvent),
    Heartbeat(WorkerHeartbeat),
}

impl fmt::Debug for WorkerMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HandshakeRequest(request) => formatter
                .debug_tuple("HandshakeRequest")
                .field(&request.request_id)
                .finish(),
            Self::HandshakeResponse(response) => formatter
                .debug_tuple("HandshakeResponse")
                .field(&response.request_id)
                .field(&response.accepted)
                .finish(),
            Self::Request(request) => request.fmt(formatter),
            Self::Response(response) => response.fmt(formatter),
            Self::Event(event) => formatter
                .debug_tuple("WorkerEvent")
                .field(&event.session_id)
                .field(&event.sequence)
                .finish(),
            Self::Heartbeat(heartbeat) => formatter
                .debug_tuple("WorkerHeartbeat")
                .field(&heartbeat.session_id)
                .field(&heartbeat.sequence)
                .finish(),
        }
    }
}

pub fn validate_handshake(
    request: &HandshakeRequest,
    expected: &HandshakeExpectation,
) -> SimulatorResult<HandshakeResponse> {
    let hello = &request.hello;
    if request.request_id.trim().is_empty()
        || hello.app_version.trim().is_empty()
        || hello.session_id.trim().is_empty()
        || hello.process_id == 0
    {
        return Err(SimulatorError::new(
            WORKER_HELLO_INVALID,
            "deviceSimulator.errors.workerHelloInvalid",
        ));
    }
    if hello.session_id != expected.session_id {
        return Err(SimulatorError::new(
            WORKER_SESSION_MISMATCH,
            "deviceSimulator.errors.workerSessionMismatch",
        ));
    }
    if hello.worker_protocol_version != expected.protocol_version {
        return Err(SimulatorError::new(
            WORKER_PROTOCOL_INCOMPATIBLE,
            "deviceSimulator.errors.workerProtocolIncompatible",
        )
        .with_public_details(format!(
            "expected protocol {}, received {}",
            expected.protocol_version, hello.worker_protocol_version
        )));
    }
    if expected.require_elevated && !hello.elevated {
        return Err(SimulatorError::new(
            WORKER_NOT_ELEVATED,
            "deviceSimulator.errors.workerNotElevated",
        ));
    }
    Ok(HandshakeResponse {
        request_id: request.request_id.clone(),
        accepted_protocol_version: expected.protocol_version,
        session_id: expected.session_id.clone(),
        accepted: true,
        error: None,
    })
}

pub fn handshake_response(
    request: &HandshakeRequest,
    expected: &HandshakeExpectation,
) -> HandshakeResponse {
    match validate_handshake(request, expected) {
        Ok(response) => response,
        Err(error) => HandshakeResponse {
            request_id: request.request_id.clone(),
            accepted_protocol_version: expected.protocol_version,
            session_id: expected.session_id.clone(),
            accepted: false,
            error: Some(error.into_body()),
        },
    }
}

pub fn encode_frame<T: Serialize>(value: &T) -> SimulatorResult<Vec<u8>> {
    encode_frame_with_limit(value, DEFAULT_MAX_FRAME_LEN)
}

pub fn encode_frame_with_limit<T: Serialize>(
    value: &T,
    max_frame_len: usize,
) -> SimulatorResult<Vec<u8>> {
    let payload = serde_json::to_vec(value).map_err(worker_json_error)?;
    validate_frame_len(payload.len(), max_frame_len)?;
    let mut frame = Vec::with_capacity(FRAME_PREFIX_LEN + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> SimulatorResult<T> {
    decode_frame_with_limit(frame, DEFAULT_MAX_FRAME_LEN)
}

pub fn decode_frame_with_limit<T: DeserializeOwned>(
    frame: &[u8],
    max_frame_len: usize,
) -> SimulatorResult<T> {
    if frame.len() < FRAME_PREFIX_LEN {
        return Err(truncated_frame_error());
    }
    let payload_len = u32::from_be_bytes(frame[..FRAME_PREFIX_LEN].try_into().unwrap()) as usize;
    validate_frame_len(payload_len, max_frame_len)?;
    let expected_len = FRAME_PREFIX_LEN + payload_len;
    if frame.len() != expected_len {
        return Err(truncated_frame_error());
    }
    serde_json::from_slice(&frame[FRAME_PREFIX_LEN..]).map_err(worker_json_error)
}

pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> SimulatorResult<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let frame = encode_frame(value)?;
    writer.write_all(&frame).await.map_err(worker_io_error)?;
    writer.flush().await.map_err(worker_io_error)
}

/// Reads one frame. `Ok(None)` means clean EOF before the next frame prefix.
/// EOF after any prefix or payload byte is a truncated-frame protocol error.
pub async fn read_frame<R, T>(reader: &mut R) -> SimulatorResult<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    read_frame_with_limit(reader, DEFAULT_MAX_FRAME_LEN).await
}

pub async fn read_frame_with_limit<R, T>(
    reader: &mut R,
    max_frame_len: usize,
) -> SimulatorResult<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut prefix = [0_u8; FRAME_PREFIX_LEN];
    let first = reader
        .read(&mut prefix[..1])
        .await
        .map_err(worker_io_error)?;
    if first == 0 {
        return Ok(None);
    }
    reader
        .read_exact(&mut prefix[1..])
        .await
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::UnexpectedEof {
                truncated_frame_error()
            } else {
                worker_io_error(source)
            }
        })?;
    let payload_len = u32::from_be_bytes(prefix) as usize;
    validate_frame_len(payload_len, max_frame_len)?;
    let mut payload = vec![0_u8; payload_len];
    reader.read_exact(&mut payload).await.map_err(|source| {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            truncated_frame_error()
        } else {
            worker_io_error(source)
        }
    })?;
    let value = serde_json::from_slice(&payload).map_err(worker_json_error)?;
    Ok(Some(value))
}

fn validate_frame_len(payload_len: usize, max_frame_len: usize) -> SimulatorResult<()> {
    if payload_len == 0 || payload_len > max_frame_len || payload_len > u32::MAX as usize {
        return Err(SimulatorError::new(
            WORKER_FRAME_TOO_LARGE,
            "deviceSimulator.errors.workerFrameTooLarge",
        )
        .with_public_details(format!(
            "frame length {payload_len} is outside the allowed range 1..={max_frame_len}"
        )));
    }
    Ok(())
}

fn truncated_frame_error() -> SimulatorError {
    SimulatorError::new(
        WORKER_FRAME_TRUNCATED,
        "deviceSimulator.errors.workerFrameTruncated",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{duplex, AsyncWriteExt};

    fn hello(version: u32) -> HandshakeRequest {
        HandshakeRequest {
            request_id: "request-1".into(),
            hello: WorkerHello {
                worker_protocol_version: version,
                app_version: "1.2.3".into(),
                session_id: "session-1".into(),
                process_id: 42,
                elevated: true,
            },
        }
    }

    #[test]
    fn frame_round_trip_preserves_newlines_and_message_boundary() {
        let message = WorkerMessage::Request(WorkerRequest {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id: "request-1".into(),
            command: WorkerCommand {
                name: WorkerCommandName::InitializeSession,
                payload: Some(json!({"template": "line one\nline two"})),
            },
        });
        let encoded = encode_frame(&message).unwrap();
        let payload_len = u32::from_be_bytes(encoded[..4].try_into().unwrap()) as usize;
        assert_eq!(payload_len, encoded.len() - 4);
        let decoded: WorkerMessage = decode_frame(&encoded).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn debug_output_redacts_command_and_response_payloads() {
        let secret = "credential-that-must-not-be-logged";
        let request = WorkerRequest {
            protocol_version: WORKER_PROTOCOL_VERSION,
            request_id: "request-1".into(),
            command: WorkerCommand {
                name: WorkerCommandName::InitializeSession,
                payload: Some(json!({"password": secret})),
            },
        };
        assert!(!format!("{request:?}").contains(secret));

        let response = WorkerResponse::success("request-1", Some(json!({"token": secret})));
        assert!(!format!("{response:?}").contains(secret));
    }

    #[test]
    fn handshake_accepts_matching_version_session_and_elevation() {
        let expected = HandshakeExpectation::for_session("session-1");
        let response = handshake_response(&hello(WORKER_PROTOCOL_VERSION), &expected);
        assert!(response.accepted);
        assert_eq!(response.request_id, "request-1");
        assert!(response.error.is_none());
    }

    #[test]
    fn incompatible_version_returns_correlated_error_response() {
        let expected = HandshakeExpectation::for_session("session-1");
        let response = handshake_response(&hello(WORKER_PROTOCOL_VERSION + 1), &expected);
        assert!(!response.accepted);
        assert_eq!(response.request_id, "request-1");
        assert_eq!(response.error.unwrap().code, WORKER_PROTOCOL_INCOMPATIBLE);
    }

    #[test]
    fn handshake_rejects_session_mismatch_and_non_elevated_worker() {
        let expected = HandshakeExpectation::for_session("other-session");
        let mismatch = handshake_response(&hello(WORKER_PROTOCOL_VERSION), &expected);
        assert_eq!(mismatch.error.unwrap().code, WORKER_SESSION_MISMATCH);

        let mut request = hello(WORKER_PROTOCOL_VERSION);
        request.hello.elevated = false;
        let expected = HandshakeExpectation::for_session("session-1");
        let not_elevated = handshake_response(&request, &expected);
        assert_eq!(not_elevated.error.unwrap().code, WORKER_NOT_ELEVATED);
    }

    #[tokio::test]
    async fn async_codec_reads_back_to_back_frames_then_clean_eof() {
        let (mut writer, mut reader) = duplex(4096);
        let first = WorkerMessage::Heartbeat(WorkerHeartbeat {
            session_id: "session-1".into(),
            sequence: 1,
            sent_at_ms: 100,
        });
        let second = WorkerMessage::Heartbeat(WorkerHeartbeat {
            session_id: "session-1".into(),
            sequence: 2,
            sent_at_ms: 200,
        });
        write_frame(&mut writer, &first).await.unwrap();
        write_frame(&mut writer, &second).await.unwrap();
        writer.shutdown().await.unwrap();

        assert_eq!(read_frame(&mut reader).await.unwrap(), Some(first));
        assert_eq!(read_frame(&mut reader).await.unwrap(), Some(second));
        assert_eq!(
            read_frame::<_, WorkerMessage>(&mut reader).await.unwrap(),
            None
        );
    }

    #[test]
    fn command_error_response_preserves_request_id_and_stable_error_shape() {
        let response = WorkerResponse::error(
            "request-9",
            SimulatorErrorBody::new(
                "device_simulator.worker.command_rejected",
                "deviceSimulator.errors.workerCommandRejected",
            )
            .with_public_details("command is not valid in the current session state"),
        );
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["request_id"], "request-9");
        assert_eq!(json["status"], "error");
        assert_eq!(
            json["error"]["code"],
            "device_simulator.worker.command_rejected"
        );
        assert!(!format!("{response:?}").contains("current session state"));
    }

    #[tokio::test]
    async fn partial_prefix_and_partial_payload_are_protocol_errors() {
        let mut partial_prefix = &b"\0\0"[..];
        let error = read_frame::<_, WorkerMessage>(&mut partial_prefix)
            .await
            .unwrap_err();
        assert_eq!(error.body().code, WORKER_FRAME_TRUNCATED);

        let mut partial_payload = &[0, 0, 0, 5, b'{', b'}'][..];
        let error = read_frame::<_, WorkerMessage>(&mut partial_payload)
            .await
            .unwrap_err();
        assert_eq!(error.body().code, WORKER_FRAME_TRUNCATED);
    }

    #[test]
    fn oversized_and_invalid_json_frames_have_stable_errors() {
        let oversized = [0, 0, 0, 9];
        let error = decode_frame_with_limit::<WorkerMessage>(&oversized, 8).unwrap_err();
        assert_eq!(error.body().code, WORKER_FRAME_TOO_LARGE);

        let invalid = [0, 0, 0, 1, b'{'];
        let error = decode_frame::<WorkerMessage>(&invalid).unwrap_err();
        assert_eq!(
            error.body().code,
            crate::device_simulator::errors::WORKER_JSON_INVALID
        );
    }
}
