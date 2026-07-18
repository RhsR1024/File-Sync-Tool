use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::fmt;

pub const WORKER_IO_ERROR: &str = "device_simulator.worker.io";
pub const WORKER_FRAME_TOO_LARGE: &str = "device_simulator.worker.frame_too_large";
pub const WORKER_FRAME_TRUNCATED: &str = "device_simulator.worker.frame_truncated";
pub const WORKER_JSON_INVALID: &str = "device_simulator.worker.json_invalid";
pub const WORKER_PROTOCOL_INCOMPATIBLE: &str = "device_simulator.worker.protocol_incompatible";
pub const WORKER_SESSION_MISMATCH: &str = "device_simulator.worker.session_mismatch";
pub const WORKER_NOT_ELEVATED: &str = "device_simulator.privilege.worker_not_elevated";
pub const WORKER_HELLO_INVALID: &str = "device_simulator.worker.hello_invalid";

/// Serializable, user-safe error information crossing the Worker boundary.
///
/// `details` must already be sanitized. Passwords, access tokens, complete
/// command lines, and raw protocol messages must never be stored here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulatorErrorBody {
    pub code: String,
    pub message_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

impl fmt::Debug for SimulatorErrorBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SimulatorErrorBody")
            .field("code", &self.code)
            .field("message_key", &self.message_key)
            .field("details", &self.details.as_ref().map(|_| "<redacted>"))
            .field("retryable", &self.retryable)
            .finish()
    }
}

impl SimulatorErrorBody {
    pub fn new(code: impl Into<String>, message_key: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message_key: message_key.into(),
            details: None,
            retryable: false,
        }
    }

    pub fn with_public_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

/// Internal error retaining an optional source chain while exposing only a
/// stable, sanitized body to the Worker protocol and UI layers.
pub struct SimulatorError {
    body: SimulatorErrorBody,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl SimulatorError {
    pub fn new(code: impl Into<String>, message_key: impl Into<String>) -> Self {
        Self {
            body: SimulatorErrorBody::new(code, message_key),
            source: None,
        }
    }

    pub fn with_public_details(mut self, details: impl Into<String>) -> Self {
        self.body = self.body.with_public_details(details);
        self
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.body = self.body.retryable(retryable);
        self
    }

    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    pub fn body(&self) -> &SimulatorErrorBody {
        &self.body
    }

    pub fn into_body(self) -> SimulatorErrorBody {
        self.body
    }
}

impl fmt::Debug for SimulatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SimulatorError")
            .field("body", &self.body)
            .field("source", &self.source.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl fmt::Display for SimulatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({})", self.body.code, self.body.message_key)
    }
}

impl StdError for SimulatorError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn StdError + 'static))
    }
}

pub type SimulatorResult<T> = Result<T, SimulatorError>;

pub(crate) fn worker_io_error(source: std::io::Error) -> SimulatorError {
    SimulatorError::new(WORKER_IO_ERROR, "deviceSimulator.errors.workerIo")
        .retryable(source.kind() == std::io::ErrorKind::Interrupted)
        .with_source(source)
}

pub(crate) fn worker_json_error(source: serde_json::Error) -> SimulatorError {
    SimulatorError::new(
        WORKER_JSON_INVALID,
        "deviceSimulator.errors.workerJsonInvalid",
    )
    .with_source(source)
}
