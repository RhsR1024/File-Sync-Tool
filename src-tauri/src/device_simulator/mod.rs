//! Video device simulator infrastructure.
//!
//! Protocol-specific behavior must remain traceable to the evidence matrix in
//! `docs/superpowers/specs/2026-07-18-video-device-simulator-evidence-matrix.md`.

pub mod access_control;
pub mod alarm_runtime;
pub mod alarms;
pub mod api;
pub mod assets;
pub mod discovery;
pub mod errors;
pub mod events;
pub mod http;
pub mod manager;
pub mod media;
pub mod models;
pub mod preflight;
pub mod profiles;
pub mod protocol_runtime;
pub mod release_policy;
pub mod rtsp;
pub mod runtime_assets;
pub mod session_journal;
pub mod telemetry;
pub mod template;
pub mod windows;
pub mod worker_entry;
pub mod worker_protocol;
pub mod worker_runtime;
