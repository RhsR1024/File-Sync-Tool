//! Update checker, downloader, replacer, and release-history backend.
//!
//! See `docs/superpowers/specs/2026-04-25-update-checker-design.md`.

pub mod commands;
pub mod download;
pub mod installer;
pub mod manifest;
pub mod pending;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestVersion {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub released_at: String,
    pub changelog: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub latest: String,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingUpdate {
    pub target_version: String,
    pub temp_path: String,
    pub sha256: String,
    pub downloaded_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed_bps: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadCompletePayload {
    pub version: String,
    pub temp_path: String,
    pub sha256_ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckResult {
    pub has_update: bool,
    pub current: String,
    pub latest: Option<String>,
    pub manifest: Option<Manifest>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestServerResult {
    pub ok: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateState {
    pub current: String,
    pub server_url: String,
    pub manifest: Option<Manifest>,
    pub has_update: bool,
    pub last_checked_at: Option<String>,
    pub pending_update: Option<PendingUpdate>,
    pub debug_build: bool,
}

#[derive(Debug)]
pub enum UpdaterError {
    NotConfigured,
    Network(String),
    Http(u16),
    ManifestInvalid(String),
    Io(String),
    VerifyFailed,
    AlreadyInProgress,
    Cancelled,
    DebugBuild,
}

impl std::fmt::Display for UpdaterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdaterError::NotConfigured => write!(f, "server_not_configured"),
            UpdaterError::Network(message) => write!(f, "network: {message}"),
            UpdaterError::Http(status) => write!(f, "http_{status}"),
            UpdaterError::ManifestInvalid(message) => write!(f, "manifest_invalid: {message}"),
            UpdaterError::Io(message) => write!(f, "io: {message}"),
            UpdaterError::VerifyFailed => write!(f, "verify_failed"),
            UpdaterError::AlreadyInProgress => write!(f, "already_in_progress"),
            UpdaterError::Cancelled => write!(f, "cancelled"),
            UpdaterError::DebugBuild => write!(f, "debug_build"),
        }
    }
}

impl std::error::Error for UpdaterError {}

/// In-memory state attached to `AppState`.
pub struct UpdaterState {
    pub manifest: Mutex<Option<Manifest>>,
    pub last_checked_at: Mutex<Option<String>>,
    pub is_downloading: Mutex<bool>,
    pub cancel_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl UpdaterState {
    pub fn new() -> Self {
        Self {
            manifest: Mutex::new(None),
            last_checked_at: Mutex::new(None),
            is_downloading: Mutex::new(false),
            cancel_tx: Mutex::new(None),
        }
    }
}

impl Default for UpdaterState {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedUpdaterState = Arc<UpdaterState>;

/// Static helper.bat content. Written to `%TEMP%` on demand.
pub const HELPER_BAT: &str = include_str!("./updater.bat");

/// Build version string read from Cargo.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn is_debug_build() -> bool {
    cfg!(debug_assertions)
}
