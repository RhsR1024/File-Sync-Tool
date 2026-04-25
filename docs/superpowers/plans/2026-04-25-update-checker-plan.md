# Update Checker & Release History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-app update checker that pulls a JSON manifest from a configurable internal HTTP server, downloads + SHA-256-verifies a new `.exe`, then self-replaces via a `helper.bat` and restarts. Includes a `/about` page that doubles as the release-history viewer.

**Architecture:** New Rust module `src-tauri/src/updater/` with submodules for manifest fetch, streaming download, helper-bat installer, and pending-update persistence. Six Tauri commands expose the flow to the frontend. New Vue page `AboutPage.vue` plus a global `UpdateDialog.vue` modal driven by a `useUpdater` composable. AppConfig gains four fields, populated via serde defaults so existing config files migrate transparently.

**Tech Stack:** Rust (`reqwest` streaming + `sha2` + `semver` + `chrono` + `zip` already there); Vue 3 `<script setup>` + Tailwind + lucide-vue-next + vue-i18n; Tauri 2.x; embedded `helper.bat` via `include_str!`.

**Companion design spec:** `docs/superpowers/specs/2026-04-25-update-checker-design.md` — re-read it before starting.

---

## File Structure

**Backend (`src-tauri/`):**

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Add `semver = "1"` runtime dep + `wiremock = "0.6"` dev-dep |
| `src/updater/mod.rs` | `Manifest`, `ManifestVersion`, `PendingUpdate`, `UpdaterState`, `UpdateCheckResult`, `TestServerResult`, `DownloadProgress`, `UpdateState`, error enum |
| `src/updater/manifest.rs` | URL builder, JSON parse with malformed-entry tolerance, fetch via reqwest, semver compare, relative→absolute URL resolution |
| `src/updater/download.rs` | Streaming download with progress events, SHA-256 verification, cancel flag |
| `src/updater/installer.rs` | `helper.bat` template (`include_str!`), spawn helper, write to `%TEMP%`, `process::exit` orchestration |
| `src/updater/pending.rs` | Validate / restore `PendingUpdate` from config + temp file across restarts |
| `src/updater/commands.rs` | 6 Tauri commands |
| `src/updater/updater.bat` | Embedded helper script (raw `.bat`) |
| `src/config.rs` | Add 4 fields with `#[serde(default)]` (no logic change beyond field additions) |
| `src/main.rs` | `mod updater;`, `AppState.updater`, `invoke_handler` registration, startup background task |

**Frontend (`src/`):**

| Path | Responsibility |
|---|---|
| `lib/tauri.ts` | Types from spec §4.3 + `updaterApi` |
| `composables/useUpdater.ts` | Reactive state + 4 event listeners, mounted once on app boot |
| `pages/AboutPage.vue` | Route `/about`: version info, server, banner, history list, dev-mode badge |
| `pages/about/version.ts` | Pure helpers: `formatReleaseDate`, `compareVersionsAsc`, `isCurrentVersion` |
| `pages/about/version.test.mjs` | `node --test` cases for the helpers |
| `components/UpdateDialog.vue` | 5-state modal (found / downloading / ready / verify_failed / network_error) |
| `components/UpdateRedDot.vue` | Tiny red-dot + up-arrow indicator |
| `components/Sidebar.vue` | Make existing version chip a button that pushes `/about`; mount `UpdateRedDot` |
| `pages/SettingsPage.vue` | New "更新检查" section: toggle + URL input |
| `router/index.ts` | Register `/about` |
| `App.vue` | Call `useUpdater()` once on mount so listeners are wired globally |
| `locales/messages.ts` | All `about.*`, `updater.*`, `settings.update.*`, `sidebar.versionChipTooltip` (zh + en) |

**Other:**

| Path | Responsibility |
|---|---|
| `scripts/release-server/serve.py` | Bundled 30-line static-file server (optional companion to spec §2.2) |

---

## Task 1: Add `semver` and `wiremock` dependencies

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add the runtime dep**

In `src-tauri/Cargo.toml`, in the `[dependencies]` block, add:

```toml
semver = "1"
```

Place it alphabetically near `serde` / `sha2` (order is not strictly enforced in this repo; keep it readable).

- [ ] **Step 2: Add the dev dep**

Inside the `[dev-dependencies]` block (already contains `tower`, `tempfile`), add:

```toml
wiremock = "0.6"
```

- [ ] **Step 3: Verify cargo can resolve everything**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: completes with no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore(updater): add semver runtime dep and wiremock dev dep"
```

---

## Task 2: `updater` module skeleton + embedded `helper.bat`

**Files:**
- Create: `src-tauri/src/updater/mod.rs`
- Create: `src-tauri/src/updater/manifest.rs` (empty stub)
- Create: `src-tauri/src/updater/download.rs` (empty stub)
- Create: `src-tauri/src/updater/installer.rs` (empty stub)
- Create: `src-tauri/src/updater/pending.rs` (empty stub)
- Create: `src-tauri/src/updater/commands.rs` (empty stub)
- Create: `src-tauri/src/updater/updater.bat`
- Modify: `src-tauri/src/main.rs` (add `mod updater;`)

- [ ] **Step 1: Create the embedded bat file**

Create `src-tauri/src/updater/updater.bat` with the exact content (no trailing
spaces, CRLF line endings):

```bat
@echo off
setlocal
:wait
tasklist /FI "PID eq %~1" /NH 2>nul | find " %~1 " >nul
if %errorlevel% equ 0 ( timeout /t 1 /nobreak >nul & goto wait )
timeout /t 1 /nobreak >nul
if exist "%~3.old" del /f /q "%~3.old" 2>nul
if exist "%~3" move /y "%~3" "%~3.old" >nul 2>nul
move /y "%~2" "%~3" >nul
start "" "%~3"
(goto) 2>nul & del "%~f0"
```

- [ ] **Step 2: Create `mod.rs` with shared types**

Create `src-tauri/src/updater/mod.rs`:

```rust
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
    Auth,
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
            UpdaterError::Network(m) => write!(f, "network: {m}"),
            UpdaterError::Auth => write!(f, "auth_failed"),
            UpdaterError::Http(s) => write!(f, "http_{s}"),
            UpdaterError::ManifestInvalid(m) => write!(f, "manifest_invalid: {m}"),
            UpdaterError::Io(m) => write!(f, "io: {m}"),
            UpdaterError::VerifyFailed => write!(f, "verify_failed"),
            UpdaterError::AlreadyInProgress => write!(f, "already_in_progress"),
            UpdaterError::Cancelled => write!(f, "cancelled"),
            UpdaterError::DebugBuild => write!(f, "debug_build"),
        }
    }
}

impl std::error::Error for UpdaterError {}

/// In-memory state attached to AppState.
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

/// Static helper.bat content. Written to %TEMP% on demand.
pub const HELPER_BAT: &str = include_str!("./updater.bat");

/// Build version string read from Cargo.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn is_debug_build() -> bool {
    cfg!(debug_assertions)
}
```

- [ ] **Step 3: Create empty stub files**

```bash
: > src-tauri/src/updater/manifest.rs
: > src-tauri/src/updater/download.rs
: > src-tauri/src/updater/installer.rs
: > src-tauri/src/updater/pending.rs
: > src-tauri/src/updater/commands.rs
```

(On Windows bash they create empty files. If the shell complains, create the
files via your editor instead.)

- [ ] **Step 4: Add `mod updater;` to `main.rs`**

In `src-tauri/src/main.rs`, find the alphabetical block of `mod` declarations
near the top. Insert after `mod task_runtime;`:

```rust
mod updater;
```

- [ ] **Step 5: Verify build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: compiles. Dead-code warnings about unused types are expected at
this stage — ignore them.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/updater src-tauri/src/main.rs
git commit -m "feat(updater): module skeleton, shared types, embedded helper.bat"
```

---

## Task 3: AppConfig migration (4 new fields)

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Add fields to `AppConfig`**

Open `src-tauri/src/config.rs`. Locate the `pub struct AppConfig { … }`
definition. After the last existing field but before the closing `}`, add:

```rust
    #[serde(default = "default_update_server_url")]
    pub update_server_url: String,
    #[serde(default)]
    pub notify_on_new_version: bool,
    #[serde(default)]
    pub last_update_check_at: Option<String>,
    #[serde(default)]
    pub pending_update: Option<crate::updater::PendingUpdate>,
```

- [ ] **Step 2: Add the default function**

Below the `AppConfig` struct (anywhere outside it), add:

```rust
fn default_update_server_url() -> String {
    "http://192.115.1.3:8080".to_string()
}
```

- [ ] **Step 3: Update `impl Default for AppConfig`**

Find the existing `impl Default for AppConfig { fn default() -> Self { Self { … } } }`. Add the four new fields to the `Self { … }` initializer:

```rust
            update_server_url: default_update_server_url(),
            notify_on_new_version: false,
            last_update_check_at: None,
            pending_update: None,
```

Match the indentation style of surrounding fields.

- [ ] **Step 4: Add a migration round-trip test**

Inside `src-tauri/src/config.rs`, locate (or create) the `#[cfg(test)] mod tests { … }` block. Append:

```rust
    #[test]
    fn legacy_config_without_update_fields_migrates_to_defaults() {
        let legacy_json = r#"{
            "tasks": [],
            "local_path": "C:\\local",
            "interval_minutes": 5,
            "time_ranges": [],
            "file_extensions": [],
            "filename_includes": [],
            "deploy_enabled": false,
            "servers": [],
            "command_groups": [],
            "stability_check_secs": 60,
            "recent_file_guard_mins": 3,
            "launch_and_auto_scan": false,
            "close_to_tray": false,
            "max_log_lines": 200
        }"#;
        let cfg: AppConfig = serde_json::from_str(legacy_json).expect("parse");
        assert_eq!(cfg.update_server_url, "http://192.115.1.3:8080");
        assert!(!cfg.notify_on_new_version);
        assert!(cfg.last_update_check_at.is_none());
        assert!(cfg.pending_update.is_none());
    }

    #[test]
    fn config_round_trip_preserves_pending_update() {
        let mut cfg = AppConfig::default();
        cfg.pending_update = Some(crate::updater::PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: r"C:\Users\u\AppData\Local\Temp\fst-update.exe".into(),
            sha256: "ab".repeat(32),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        });
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pending_update, cfg.pending_update);
    }
```

If the test module does not exist in this file, create it at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // (the two tests above)
}
```

- [ ] **Step 5: Run the tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app config::
```

Expected: 2 passed (plus any pre-existing config tests).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat(updater): AppConfig fields for server URL, notify, throttle, pending"
```

---

## Task 4: `manifest::resolve_url` + `compare_versions` (TDD)

**Files:**
- Modify: `src-tauri/src/updater/manifest.rs`

- [ ] **Step 1: Write failing tests**

Replace `src-tauri/src/updater/manifest.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_url_strips_trailing_slash_and_appends_path() {
        assert_eq!(
            manifest_url("http://1.2.3.4:8080/"),
            "http://1.2.3.4:8080/manifest.json"
        );
        assert_eq!(
            manifest_url("http://1.2.3.4:8080"),
            "http://1.2.3.4:8080/manifest.json"
        );
        assert_eq!(
            manifest_url("http://srv/releases/"),
            "http://srv/releases/manifest.json"
        );
    }

    #[test]
    fn resolve_download_url_keeps_absolute() {
        assert_eq!(
            resolve_download_url("http://srv:8080", "http://other/foo.exe"),
            "http://other/foo.exe"
        );
        assert_eq!(
            resolve_download_url("http://srv:8080/", "https://other/foo.exe"),
            "https://other/foo.exe"
        );
    }

    #[test]
    fn resolve_download_url_joins_relative() {
        assert_eq!(
            resolve_download_url("http://srv:8080", "foo.exe"),
            "http://srv:8080/foo.exe"
        );
        assert_eq!(
            resolve_download_url("http://srv:8080/", "/abs/foo.exe"),
            "http://srv:8080/abs/foo.exe"
        );
        assert_eq!(
            resolve_download_url("http://srv:8080/dir/", "foo.exe"),
            "http://srv:8080/dir/foo.exe"
        );
    }

    #[test]
    fn compare_versions_basic_ordering() {
        assert!(is_newer("1.0.8", "1.0.7"));
        assert!(!is_newer("1.0.7", "1.0.7"));
        assert!(!is_newer("1.0.6", "1.0.7"));
        assert!(is_newer("2.0.0", "1.99.99"));
    }

    #[test]
    fn compare_versions_handles_pre_release() {
        assert!(is_newer("1.0.8", "1.0.8-beta.1"));
        assert!(!is_newer("1.0.8-beta.1", "1.0.8"));
    }

    #[test]
    fn compare_versions_invalid_returns_false() {
        // Garbage input must never claim "newer" — fail closed.
        assert!(!is_newer("not-a-version", "1.0.0"));
        assert!(!is_newer("1.0.0", "garbage"));
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest
```

Expected: compile error — `manifest_url`, `resolve_download_url`, `is_newer` not defined.

- [ ] **Step 3: Implement**

Prepend to `manifest.rs` (above the test module):

```rust
use semver::Version;

/// Build the manifest endpoint from a configured base URL.
/// Trailing `/` is normalized away so we don't produce double slashes.
pub fn manifest_url(server_url: &str) -> String {
    let trimmed = server_url.trim_end_matches('/');
    format!("{trimmed}/manifest.json")
}

/// Turn a (possibly relative) `url` from the manifest into an absolute URL.
/// Heuristic: starts with `http://` or `https://` -> absolute.
/// Starts with `/` -> joined to `<scheme>://<host>[:port]` (root-relative).
/// Otherwise -> joined to the server_url's directory (path-relative).
pub fn resolve_download_url(server_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    let base = server_url.trim_end_matches('/');
    if let Some(stripped) = url.strip_prefix('/') {
        // Root-relative — keep scheme+host+port from base.
        if let Some(authority_end) = find_authority_end(base) {
            let authority = &base[..authority_end];
            return format!("{authority}/{stripped}");
        }
        return format!("{base}/{stripped}");
    }
    format!("{base}/{url}")
}

fn find_authority_end(base: &str) -> Option<usize> {
    let scheme_end = base.find("://")? + 3;
    match base[scheme_end..].find('/') {
        Some(idx) => Some(scheme_end + idx),
        None => Some(base.len()),
    }
}

/// Returns true iff `latest` is strictly newer than `current` per semver.
/// Returns false on any parse error to fail closed.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let l = match Version::parse(latest) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = match Version::parse(current) {
        Ok(v) => v,
        Err(_) => return false,
    };
    l > c
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest
```

Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/manifest.rs
git commit -m "feat(updater): URL resolver and semver comparison helpers"
```

---

## Task 5: Manifest JSON parsing with malformed-entry tolerance (TDD)

**Files:**
- Modify: `src-tauri/src/updater/manifest.rs`

- [ ] **Step 1: Write failing tests**

Inside the existing `#[cfg(test)] mod tests` block in `manifest.rs`, append:

```rust
    #[test]
    fn parse_manifest_strips_invalid_entries_and_normalizes_urls() {
        let raw = r#"{
            "latest": "1.0.8",
            "versions": [
                {"version":"1.0.8","url":"file-sync-tool-1.0.8.exe","sha256":"AB","released_at":"2026-04-26","changelog":["a","b"]},
                {"version":"1.0.7","url":"http://other/x.exe","sha256":"CD","released_at":"2026-04-19","changelog":["c"]},
                {"version":"bad-version","url":"x","sha256":"EF","released_at":"2026-04-10","changelog":[]},
                {"version":"1.0.5"}
            ]
        }"#;
        let m = parse_manifest(raw, "http://srv:8080/").expect("parse");
        assert_eq!(m.latest, "1.0.8");
        assert_eq!(m.versions.len(), 2);
        assert_eq!(m.versions[0].url, "http://srv:8080/file-sync-tool-1.0.8.exe");
        assert_eq!(m.versions[0].sha256, "ab"); // normalized to lowercase
        assert_eq!(m.versions[1].url, "http://other/x.exe");
    }

    #[test]
    fn parse_manifest_rejects_non_object_root() {
        let err = parse_manifest("[1,2,3]", "http://srv").unwrap_err();
        assert!(matches!(err, crate::updater::UpdaterError::ManifestInvalid(_)));
    }

    #[test]
    fn parse_manifest_accepts_empty_versions_array() {
        let m = parse_manifest(r#"{"latest":"1.0.0","versions":[]}"#, "http://srv").expect("parse");
        assert!(m.versions.is_empty());
    }

    #[test]
    fn parse_manifest_drops_all_invalid_returns_err() {
        let raw = r#"{"latest":"1.0.0","versions":[{"version":"abc"}]}"#;
        let err = parse_manifest(raw, "http://srv").unwrap_err();
        assert!(matches!(err, crate::updater::UpdaterError::ManifestInvalid(_)));
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest
```

Expected: compile error — `parse_manifest` not defined.

- [ ] **Step 3: Implement**

Append to `manifest.rs` above the test module:

```rust
use crate::updater::{Manifest, ManifestVersion, UpdaterError};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawManifest {
    latest: String,
    versions: Vec<serde_json::Value>,
}

/// Parse manifest text. Drops `versions[]` entries that fail validation
/// (missing fields / invalid semver). If **every** entry is dropped, returns
/// `ManifestInvalid` so the UI can surface a real error.
pub fn parse_manifest(text: &str, server_url: &str) -> Result<Manifest, UpdaterError> {
    let raw: RawManifest = serde_json::from_str(text)
        .map_err(|e| UpdaterError::ManifestInvalid(e.to_string()))?;

    let mut versions = Vec::with_capacity(raw.versions.len());
    let total = raw.versions.len();
    for (idx, value) in raw.versions.into_iter().enumerate() {
        match parse_version_entry(value, server_url) {
            Ok(v) => versions.push(v),
            Err(e) => log::warn!("[updater] manifest versions[{idx}] 丢弃: {e}"),
        }
    }

    if total > 0 && versions.is_empty() {
        return Err(UpdaterError::ManifestInvalid(
            "all version entries were invalid".into(),
        ));
    }

    Ok(Manifest {
        latest: raw.latest,
        versions,
    })
}

fn parse_version_entry(value: serde_json::Value, server_url: &str) -> Result<ManifestVersion, String> {
    let obj = value.as_object().ok_or_else(|| "not an object".to_string())?;
    let version = obj
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing version".to_string())?;
    semver::Version::parse(version).map_err(|e| format!("invalid semver: {e}"))?;
    let url = obj
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing url".to_string())?;
    let sha256 = obj
        .get("sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing sha256".to_string())?;
    let released_at = obj
        .get("released_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let changelog = obj
        .get("changelog")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(ManifestVersion {
        version: version.to_string(),
        url: resolve_download_url(server_url, url),
        sha256: sha256.to_ascii_lowercase(),
        released_at,
        changelog,
    })
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest
```

Expected: 10 passed (4 new + 6 from Task 4).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/manifest.rs
git commit -m "feat(updater): parse manifest JSON with malformed-entry tolerance"
```

---

## Task 6: `manifest::fetch` against a wiremock server

**Files:**
- Modify: `src-tauri/src/updater/manifest.rs`

- [ ] **Step 1: Write a failing test**

Append to the `#[cfg(test)] mod tests { … }` block:

```rust
    #[tokio::test]
    async fn fetch_manifest_round_trip_against_wiremock() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = r#"{
            "latest":"1.0.8",
            "versions":[
                {"version":"1.0.8","url":"foo.exe","sha256":"ab","released_at":"2026-04-26","changelog":["x"]}
            ]
        }"#;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let m = fetch_manifest(server.uri().as_str()).await.expect("ok");
        assert_eq!(m.latest, "1.0.8");
        assert_eq!(m.versions.len(), 1);
        assert!(m.versions[0].url.starts_with(server.uri().as_str()));
    }

    #[tokio::test]
    async fn fetch_manifest_maps_404_to_http_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let err = fetch_manifest(server.uri().as_str()).await.unwrap_err();
        assert!(matches!(err, crate::updater::UpdaterError::Http(404)));
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest::tests::fetch_
```

Expected: compile error — `fetch_manifest` not defined.

- [ ] **Step 3: Implement**

Append to `manifest.rs` above the test module:

```rust
use std::time::Duration;

pub async fn fetch_manifest(server_url: &str) -> Result<Manifest, UpdaterError> {
    if server_url.trim().is_empty() {
        return Err(UpdaterError::NotConfigured);
    }
    let url = manifest_url(server_url);
    log::info!("[updater] 拉取 manifest {}", url);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| UpdaterError::Network(e.to_string()))?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(UpdaterError::Auth);
    }
    if !status.is_success() {
        return Err(UpdaterError::Http(status.as_u16()));
    }

    let body = response
        .text()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;

    parse_manifest(&body, server_url)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest
```

Expected: 12 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/manifest.rs
git commit -m "feat(updater): async fetch_manifest with reqwest + wiremock tests"
```

---

## Task 7: SHA-256 verifier (TDD)

**Files:**
- Modify: `src-tauri/src/updater/download.rs`

- [ ] **Step 1: Write failing tests**

Replace `src-tauri/src/updater/download.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_of_empty_input() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_of_known_string() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn verify_accepts_lowercase_or_uppercase_expected() {
        let bytes = b"abc";
        let lower = sha256_hex(bytes);
        let upper = lower.to_uppercase();
        assert!(verify_bytes(bytes, &lower));
        assert!(verify_bytes(bytes, &upper));
        assert!(!verify_bytes(bytes, "deadbeef"));
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::download
```

Expected: compile error — `sha256_hex`, `verify_bytes` not defined.

- [ ] **Step 3: Implement**

Prepend to `download.rs`:

```rust
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

pub fn verify_bytes(bytes: &[u8], expected_hex: &str) -> bool {
    sha256_hex(bytes).eq_ignore_ascii_case(expected_hex)
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(TABLE[(b >> 4) as usize] as char);
        out.push(TABLE[(b & 0xF) as usize] as char);
    }
    out
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::download
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/download.rs
git commit -m "feat(updater): SHA-256 hex helper and equality check"
```

---

## Task 8: Streaming downloader with progress + cancel (TDD with wiremock)

**Files:**
- Modify: `src-tauri/src/updater/download.rs`

- [ ] **Step 1: Write a failing test**

Append to the `#[cfg(test)] mod tests { … }` block:

```rust
    #[tokio::test]
    async fn download_writes_file_and_verifies_sha() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let payload: Vec<u8> = (0u8..=255).cycle().take(50_000).collect();
        let expected = sha256_hex(&payload);

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
            .mount(&server)
            .await;

        let url = format!("{}/file.exe", server.uri());
        let dest = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let dest_path = dest.to_path_buf();
        drop(dest); // we want the path, not the file handle

        let cancel = tokio::sync::watch::channel(false).1;
        let progress = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u64>::new()));
        let progress_clone = progress.clone();

        let result = download_to_file(
            &url,
            &dest_path,
            &expected,
            cancel,
            move |downloaded, _total| {
                progress_clone.lock().unwrap().push(downloaded);
            },
        )
        .await;

        result.expect("download should succeed");
        let written = std::fs::read(&dest_path).unwrap();
        assert_eq!(written, payload);
        assert!(progress.lock().unwrap().last().copied().unwrap_or(0) >= 50_000);
        let _ = std::fs::remove_file(&dest_path);
    }

    #[tokio::test]
    async fn download_aborts_on_cancel_and_cleans_up() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let payload = vec![0u8; 1_000_000];
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/big.exe"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(payload)
                    .set_delay(std::time::Duration::from_millis(50)),
            )
            .mount(&server)
            .await;

        let url = format!("{}/big.exe", server.uri());
        let dest = std::env::temp_dir().join(format!("fst-cancel-{}.bin", std::process::id()));
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        let dest_clone = dest.clone();
        let task = tokio::spawn(async move {
            download_to_file(
                &url,
                &dest_clone,
                "deadbeef",
                cancel_rx,
                |_d, _t| {},
            )
            .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        cancel_tx.send(true).unwrap();
        let result = task.await.unwrap();
        assert!(matches!(result, Err(crate::updater::UpdaterError::Cancelled)));
        assert!(!dest.exists(), "partial file should be cleaned up");
    }

    #[tokio::test]
    async fn download_verify_failure_deletes_file() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
            .mount(&server)
            .await;

        let url = format!("{}/x.exe", server.uri());
        let dest = std::env::temp_dir().join(format!("fst-verify-{}.bin", std::process::id()));
        let cancel = tokio::sync::watch::channel(false).1;

        let result = download_to_file(&url, &dest, "deadbeef", cancel, |_, _| {}).await;
        assert!(matches!(result, Err(crate::updater::UpdaterError::VerifyFailed)));
        assert!(!dest.exists());
    }
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::download
```

Expected: compile error — `download_to_file` not defined.

- [ ] **Step 3: Implement**

Append to `download.rs` above the test module:

```rust
use crate::updater::UpdaterError;
use futures_util::StreamExt;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Download `url` to `dest`, computing SHA-256 as bytes stream in.
/// Calls `on_progress(downloaded, total)` with throttling left to the caller.
/// Aborts immediately when `cancel` flips to `true`. Verifies the final hash
/// against `expected_sha256_hex` (case-insensitive). On any failure the
/// partial file at `dest` is deleted.
pub async fn download_to_file<F>(
    url: &str,
    dest: &Path,
    expected_sha256_hex: &str,
    mut cancel: watch::Receiver<bool>,
    mut on_progress: F,
) -> Result<(), UpdaterError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60 * 30))
        .build()
        .map_err(|e| UpdaterError::Network(e.to_string()))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(UpdaterError::Http(status.as_u16()));
    }
    let total = response.content_length();

    let mut file = std::fs::File::create(dest).map_err(|e| UpdaterError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut last_report = Instant::now();
    let mut stream = response.bytes_stream();

    let abort_with = |reason: UpdaterError, dest: &Path| -> UpdaterError {
        let _ = std::fs::remove_file(dest);
        reason
    };

    while let Some(chunk) = stream.next().await {
        if *cancel.borrow() {
            drop(file);
            return Err(abort_with(UpdaterError::Cancelled, dest));
        }
        let bytes = match chunk {
            Ok(b) => b,
            Err(e) => return Err(abort_with(UpdaterError::Network(e.to_string()), dest)),
        };
        if let Err(e) = file.write_all(&bytes) {
            return Err(abort_with(UpdaterError::Io(e.to_string()), dest));
        }
        hasher.update(&bytes);
        downloaded = downloaded.saturating_add(bytes.len() as u64);
        if last_report.elapsed() >= Duration::from_millis(100) {
            on_progress(downloaded, total);
            last_report = Instant::now();
        }
    }
    on_progress(downloaded, total);

    if let Err(e) = file.flush() {
        return Err(abort_with(UpdaterError::Io(e.to_string()), dest));
    }
    drop(file);

    let actual = hex_encode(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256_hex) {
        log::warn!(
            "[updater] SHA256 校验失败 期望={} 实际={}",
            expected_sha256_hex,
            actual
        );
        return Err(abort_with(UpdaterError::VerifyFailed, dest));
    }
    log::info!("[updater] 下载完成 {} 字节 sha256 校验通过", downloaded);
    Ok(())
}
```

- [ ] **Step 4: Add `futures-util` to Cargo.toml if missing**

`reqwest::Response::bytes_stream` returns a stream needing `futures_util::StreamExt`. Check `src-tauri/Cargo.toml`:

```bash
cargo check --manifest-path src-tauri/Cargo.toml 2>&1 | head -20
```

If you see `unresolved import futures_util` or similar, add to `[dependencies]`:

```toml
futures-util = "0.3"
```

The repo already has `futures-lite`; check whether `futures-util` is also there and add only if not. (It is **not** present per the current `Cargo.toml`, so add it.)

- [ ] **Step 5: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::download
```

Expected: 6 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/updater/download.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(updater): streaming download with progress, cancel, sha256 verify"
```

---

## Task 9: `installer::write_helper` + `installer::spawn_helper` (TDD)

**Files:**
- Modify: `src-tauri/src/updater/installer.rs`

- [ ] **Step 1: Write failing tests**

Replace `src-tauri/src/updater/installer.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_bat_template_is_present_and_uses_positional_args() {
        let bat = crate::updater::HELPER_BAT;
        assert!(bat.contains("tasklist"));
        assert!(bat.contains("%~1"));
        assert!(bat.contains("%~2"));
        assert!(bat.contains("%~3"));
        assert!(bat.contains("move /y \"%~2\" \"%~3\""));
        assert!(bat.contains("start \"\" \"%~3\""));
        assert!(bat.contains("del \"%~f0\""));
    }

    #[test]
    fn write_helper_creates_a_unique_bat_under_temp() {
        let p1 = write_helper().expect("write");
        let p2 = write_helper().expect("write");
        assert!(p1 != p2);
        assert!(p1.exists());
        assert!(p2.exists());
        assert_eq!(p1.extension().unwrap(), "bat");
        let written = std::fs::read_to_string(&p1).unwrap();
        assert!(written.contains("tasklist"));
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn build_helper_args_quotes_paths() {
        let args = build_helper_args(
            12345,
            std::path::Path::new(r"C:\Temp\with space\new.exe"),
            std::path::Path::new(r"C:\Program Files\app.exe"),
        );
        assert_eq!(args[0], "/c");
        // start min hides the helper window
        assert_eq!(args[1], "start");
        assert_eq!(args[2], "");
        assert_eq!(args[3], "/min");
        // bat path follows; PID, source, destination are positional
        assert_eq!(args.last().unwrap(), r"C:\Program Files\app.exe");
        let pid_idx = args.iter().position(|a| a == "12345").unwrap();
        // PID, src, dst are the last three meaningful args
        assert_eq!(args[pid_idx + 1], r"C:\Temp\with space\new.exe");
        assert_eq!(args[pid_idx + 2], r"C:\Program Files\app.exe");
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::installer
```

Expected: compile error — `write_helper`, `build_helper_args` not defined.

- [ ] **Step 3: Implement**

Prepend to `installer.rs`:

```rust
use crate::updater::{HELPER_BAT, UpdaterError};
use std::path::{Path, PathBuf};

/// Write the embedded helper bat to a unique path under `%TEMP%`.
pub fn write_helper() -> Result<PathBuf, UpdaterError> {
    let mut path = std::env::temp_dir();
    let name = format!("fst-update-{}-{}.bat", std::process::id(), random_suffix());
    path.push(name);
    std::fs::write(&path, HELPER_BAT).map_err(|e| UpdaterError::Io(e.to_string()))?;
    Ok(path)
}

/// Build the argv passed to `cmd.exe`. We use `cmd /c start "" /min <bat> <pid> <src> <dst>`
/// so the helper window is minimized and detached.
pub fn build_helper_args(pid: u32, src: &Path, dst: &Path) -> Vec<String> {
    vec![
        "/c".to_string(),
        "start".to_string(),
        "".to_string(),
        "/min".to_string(),
        // The bat path itself is appended by `spawn_helper` since that's where
        // the on-disk path is materialized. For tests we exercise `build_helper_args`
        // alongside a synthesized bat path.
        // For Windows `start` with quoted title (the "" above) the next quoted arg
        // is treated as the program. We pass the bat path next.
        // For testability we accept the bat as the first variadic.
        // (kept simple — actual spawning passes [.., bat_path, pid, src, dst])
        pid.to_string(),
        src.display().to_string(),
        dst.display().to_string(),
    ]
}

fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:08x}")
}

/// Spawn the helper bat, hand off control, and **exit the current process**.
/// Returns only on failure to spawn.
pub fn spawn_helper_and_exit(
    bat_path: &Path,
    pid: u32,
    src: &Path,
    dst: &Path,
) -> Result<std::convert::Infallible, UpdaterError> {
    let mut cmd = std::process::Command::new("cmd.exe");
    cmd.arg("/c")
        .arg("start")
        .arg("")
        .arg("/min")
        .arg(bat_path)
        .arg(pid.to_string())
        .arg(src)
        .arg(dst);

    cmd.spawn().map_err(|e| UpdaterError::Io(e.to_string()))?;

    log::info!(
        "[updater] 启动 helper {} pid={} src={} dst={} 主进程退出",
        bat_path.display(),
        pid,
        src.display(),
        dst.display(),
    );

    // Give cmd a moment to launch before we vanish.
    std::thread::sleep(std::time::Duration::from_millis(200));
    std::process::exit(0);
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::installer
```

Expected: 3 passed. (Note: `spawn_helper_and_exit` is **not** unit-tested
because it intentionally calls `process::exit`; it is exercised by manual QA.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/installer.rs
git commit -m "feat(updater): write helper.bat to TEMP and spawn cmd.exe with positional args"
```

---

## Task 10: `pending::validate_or_clear` (TDD)

**Files:**
- Modify: `src-tauri/src/updater/pending.rs`

- [ ] **Step 1: Write failing tests**

Replace `src-tauri/src/updater/pending.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::PendingUpdate;

    fn write_temp(bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "fst-pending-{}-{}.bin",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn returns_none_when_pending_is_none() {
        assert!(validate(None).is_none());
    }

    #[test]
    fn returns_pending_when_file_exists_and_sha_matches() {
        let bytes = b"hello world";
        let path = write_temp(bytes);
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            sha256: crate::updater::download::sha256_hex(bytes),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };
        let result = validate(Some(pending.clone()));
        assert_eq!(result, Some(pending));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn returns_none_and_deletes_when_sha_mismatches() {
        let path = write_temp(b"correct content");
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            sha256: "deadbeef".into(),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };
        assert!(validate(Some(pending)).is_none());
        assert!(!path.exists(), "stale file with mismatched sha must be deleted");
    }

    #[test]
    fn returns_none_when_temp_file_missing() {
        let path = std::env::temp_dir().join("fst-pending-nonexistent-xxxx.bin");
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            sha256: "ab".repeat(32),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };
        assert!(validate(Some(pending)).is_none());
    }
}
```

- [ ] **Step 2: Run to confirm failure**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::pending
```

Expected: compile error — `validate` not defined.

- [ ] **Step 3: Implement**

Prepend to `pending.rs`:

```rust
use crate::updater::PendingUpdate;
use std::path::Path;

/// Returns the pending update **only if** the temp file still exists and its
/// SHA-256 still matches. On mismatch / missing, deletes any stray file and
/// returns `None` so the caller knows to clear `AppConfig.pending_update`.
pub fn validate(pending: Option<PendingUpdate>) -> Option<PendingUpdate> {
    let pending = pending?;
    let path = Path::new(&pending.temp_path);
    if !path.exists() {
        log::info!(
            "[updater] pending_update 临时文件已不存在: {}",
            pending.temp_path
        );
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(err) => {
            log::warn!(
                "[updater] 无法读取 pending_update 文件 {}: {}",
                pending.temp_path,
                err
            );
            let _ = std::fs::remove_file(path);
            return None;
        }
    };
    if !crate::updater::download::verify_bytes(&bytes, &pending.sha256) {
        log::warn!(
            "[updater] pending_update 文件 sha256 不匹配，已删除: {}",
            pending.temp_path
        );
        let _ = std::fs::remove_file(path);
        return None;
    }
    log::info!(
        "[updater] 检测到有效 pending_update version={} path={}",
        pending.target_version,
        pending.temp_path
    );
    Some(pending)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater::pending
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/updater/pending.rs
git commit -m "feat(updater): validate pending_update against temp file + sha256"
```

---

## Task 11: Tauri command handlers

**Files:**
- Modify: `src-tauri/src/updater/commands.rs`

- [ ] **Step 1: Implement all six commands**

Replace `src-tauri/src/updater/commands.rs` with:

```rust
use crate::updater::{
    download, installer, manifest as manifest_mod, pending, DownloadProgress, Manifest,
    PendingUpdate, TestServerResult, UpdateCheckResult, UpdateState, UpdaterError, CURRENT_VERSION,
};
use crate::AppState;
use chrono::Local;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, State};
use tokio::sync::watch;

fn server_url(state: &State<'_, AppState>) -> String {
    let cfg = state.config.lock().expect("config mutex");
    cfg.update_server_url.trim().to_string()
}

fn assert_release_build() -> Result<(), String> {
    if crate::updater::is_debug_build() {
        return Err(UpdaterError::DebugBuild.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn check_update(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateCheckResult, String> {
    if crate::updater::is_debug_build() {
        return Ok(UpdateCheckResult {
            has_update: false,
            current: CURRENT_VERSION.to_string(),
            latest: None,
            manifest: None,
        });
    }
    let url = server_url(&state);
    if url.is_empty() {
        return Err(UpdaterError::NotConfigured.to_string());
    }

    let manifest = manifest_mod::fetch_manifest(&url)
        .await
        .map_err(|e| e.to_string())?;
    let latest = manifest.latest.clone();
    let has_update = manifest_mod::is_newer(&latest, CURRENT_VERSION);
    let now = Local::now().to_rfc3339();

    {
        let updater = state.updater.clone();
        *updater.manifest.lock().unwrap() = Some(manifest.clone());
        *updater.last_checked_at.lock().unwrap() = Some(now.clone());
    }
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.last_update_check_at = Some(now);
        crate::config::save_config(&cfg).ok();
    }
    let _ = app_handle.emit("update-state-changed", build_update_state(&state));

    Ok(UpdateCheckResult {
        has_update,
        current: CURRENT_VERSION.to_string(),
        latest: Some(latest),
        manifest: Some(manifest),
    })
}

#[tauri::command]
pub async fn start_update_download(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    assert_release_build().map_err(|e| e)?;
    let updater = state.updater.clone();

    {
        let mut flag = updater.is_downloading.lock().unwrap();
        if *flag {
            return Err(UpdaterError::AlreadyInProgress.to_string());
        }
        *flag = true;
    }

    let manifest_opt = updater.manifest.lock().unwrap().clone();
    let manifest = match manifest_opt {
        Some(m) => m,
        None => {
            *updater.is_downloading.lock().unwrap() = false;
            return Err(UpdaterError::ManifestInvalid("no manifest cached".into()).to_string());
        }
    };
    let target = match manifest
        .versions
        .iter()
        .find(|v| v.version == manifest.latest)
        .cloned()
    {
        Some(v) => v,
        None => {
            *updater.is_downloading.lock().unwrap() = false;
            return Err(
                UpdaterError::ManifestInvalid("latest entry missing".into()).to_string()
            );
        }
    };

    let (cancel_tx, cancel_rx) = watch::channel(false);
    *updater.cancel_tx.lock().unwrap() = Some(cancel_tx);

    let dest = unique_temp_exe_path();
    let app = app_handle.clone();
    let updater_for_task = updater.clone();
    let state_arc = state.config.clone();

    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let progress_app = app.clone();
        let mut last_emit = Instant::now() - Duration::from_secs(1);

        let on_progress = move |downloaded: u64, total: Option<u64>| {
            let _ = downloaded;
            // throttling is also done inside download_to_file, but we throttle event emission here too.
            let elapsed = started.elapsed().as_secs_f64().max(0.001);
            let speed = (downloaded as f64 / elapsed) as u64;
            if last_emit.elapsed() >= Duration::from_millis(100) {
                let _ = progress_app.emit(
                    "update-download-progress",
                    DownloadProgress {
                        downloaded,
                        total,
                        speed_bps: speed,
                    },
                );
                last_emit = Instant::now();
            }
        };

        let result = download::download_to_file(
            &target.url,
            &dest,
            &target.sha256,
            cancel_rx,
            on_progress,
        )
        .await;

        *updater_for_task.is_downloading.lock().unwrap() = false;
        *updater_for_task.cancel_tx.lock().unwrap() = None;

        let payload = match &result {
            Ok(()) => {
                let pending = PendingUpdate {
                    target_version: target.version.clone(),
                    temp_path: dest.to_string_lossy().into_owned(),
                    sha256: target.sha256.clone(),
                    downloaded_at: Local::now().to_rfc3339(),
                };
                {
                    let mut cfg = state_arc.lock().unwrap();
                    cfg.pending_update = Some(pending.clone());
                    crate::config::save_config(&cfg).ok();
                }
                serde_json::json!({
                    "ok": true,
                    "version": target.version,
                    "temp_path": pending.temp_path,
                    "error": null,
                })
            }
            Err(err) => serde_json::json!({
                "ok": false,
                "version": target.version,
                "temp_path": null,
                "error": err.to_string(),
            }),
        };
        let _ = app.emit("update-download-complete", payload);
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_update_download(state: State<'_, AppState>) -> Result<(), String> {
    let updater = state.updater.clone();
    if let Some(tx) = updater.cancel_tx.lock().unwrap().as_ref() {
        let _ = tx.send(true);
    }
    Ok(())
}

#[tauri::command]
pub async fn apply_update_now(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    assert_release_build()?;
    let pending = {
        let cfg = state.config.lock().unwrap();
        cfg.pending_update.clone()
    };
    let pending = pending.ok_or_else(|| "no_pending_update".to_string())?;

    let exe_path = std::env::current_exe()
        .map_err(|e| UpdaterError::Io(format!("current_exe: {e}")).to_string())?;

    let bat_path = installer::write_helper().map_err(|e| e.to_string())?;
    let pid = std::process::id();

    {
        let mut cfg = state.config.lock().unwrap();
        cfg.pending_update = None;
        crate::config::save_config(&cfg).ok();
    }

    log::info!(
        "[updater] 准备启动 helper：bat={} pid={} src={} dst={}",
        bat_path.display(),
        pid,
        pending.temp_path,
        exe_path.display(),
    );

    // Best-effort: close all webview windows so the user sees the app vanish quickly.
    for (_, w) in app_handle.webview_windows() {
        let _ = w.close();
    }

    installer::spawn_helper_and_exit(
        &bat_path,
        pid,
        std::path::Path::new(&pending.temp_path),
        &exe_path,
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_update_server(state: State<'_, AppState>) -> Result<TestServerResult, String> {
    let url = server_url(&state);
    if url.is_empty() {
        return Ok(TestServerResult {
            ok: false,
            status: None,
            error: Some(UpdaterError::NotConfigured.to_string()),
        });
    }
    let manifest_url = manifest_mod::manifest_url(&url);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return Ok(TestServerResult {
                ok: false,
                status: None,
                error: Some(e.to_string()),
            })
        }
    };
    match client.get(&manifest_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                return Ok(TestServerResult {
                    ok: false,
                    status: Some(status.as_u16()),
                    error: Some(format!("http_{}", status.as_u16())),
                });
            }
            match manifest_mod::parse_manifest(&body, &url) {
                Ok(_) => Ok(TestServerResult {
                    ok: true,
                    status: Some(status.as_u16()),
                    error: None,
                }),
                Err(e) => Ok(TestServerResult {
                    ok: false,
                    status: Some(status.as_u16()),
                    error: Some(e.to_string()),
                }),
            }
        }
        Err(e) => Ok(TestServerResult {
            ok: false,
            status: None,
            error: Some(e.to_string()),
        }),
    }
}

#[tauri::command]
pub fn get_update_state(state: State<'_, AppState>) -> Result<UpdateState, String> {
    Ok(build_update_state(&state))
}

fn build_update_state(state: &State<'_, AppState>) -> UpdateState {
    let updater = state.updater.clone();
    let manifest = updater.manifest.lock().unwrap().clone();
    let last_checked_at = updater.last_checked_at.lock().unwrap().clone();
    let pending = {
        let cfg = state.config.lock().unwrap();
        cfg.pending_update.clone()
    };
    let pending_update = pending::validate(pending);

    if pending_update.is_none() {
        // Validation may have just deleted a stale file — clear it from config.
        let mut cfg = state.config.lock().unwrap();
        if cfg.pending_update.is_some() {
            cfg.pending_update = None;
            crate::config::save_config(&cfg).ok();
        }
    }

    let server_url = state
        .config
        .lock()
        .unwrap()
        .update_server_url
        .trim()
        .to_string();

    let has_update = match (&manifest, crate::updater::is_debug_build()) {
        (_, true) => false,
        (Some(m), false) => manifest_mod::is_newer(&m.latest, CURRENT_VERSION),
        _ => false,
    };

    UpdateState {
        current: CURRENT_VERSION.to_string(),
        server_url,
        manifest,
        has_update,
        last_checked_at,
        pending_update,
        debug_build: crate::updater::is_debug_build(),
    }
}

fn unique_temp_exe_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    p.push(format!(
        "fst-update-{}-{:08x}.exe",
        std::process::id(),
        nanos
    ));
    p
}

// Suppress unused-import warning for Arc when only used via state.config field.
#[allow(dead_code)]
fn _arc_marker() -> Arc<()> {
    Arc::new(())
}
```

- [ ] **Step 2: Verify build (will fail because AppState doesn't yet have `updater` field)**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: errors about `state.updater` and `state.config` being non-shareable.
Task 12 fixes them.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/updater/commands.rs
git commit -m "feat(updater): tauri command handlers for check, download, apply, test, state"
```

---

## Task 12: Wire `AppState`, register commands, add startup background task

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Make `AppState.config` an `Arc<Mutex<…>>` (already is) and add `updater` field**

In `src-tauri/src/main.rs`, find the `struct AppState { … }` block. After the
existing fields, append:

```rust
    pub updater: crate::updater::SharedUpdaterState,
```

Locate where `AppState` is constructed (search for `AppState {`). For each
construction site append:

```rust
            updater: std::sync::Arc::new(crate::updater::UpdaterState::new()),
```

Match the indentation of surrounding fields.

If `AppState.config` is currently `Arc<Mutex<AppConfig>>` already (it is in
this repo per CLAUDE.md), you do not need to change its type. The
`commands.rs` reads it as `state.config.lock()`.

- [ ] **Step 2: Register the six commands in `invoke_handler`**

Find the `tauri::generate_handler![ … ]` macro call. Append the six command
references to the handler list (before the closing `]`):

```rust
            updater::commands::check_update,
            updater::commands::start_update_download,
            updater::commands::cancel_update_download,
            updater::commands::apply_update_now,
            updater::commands::test_update_server,
            updater::commands::get_update_state,
```

- [ ] **Step 3: Add the startup background task**

Find the `setup` closure in the Tauri builder (search for `.setup(`). At the
end of the closure body but before the existing `Ok(())` return, insert:

```rust
            // ----- Updater: post-launch auto check (release builds only) -----
            if !crate::updater::is_debug_build() {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                    let state = app_handle.state::<AppState>();
                    let server_url = state.config.lock().unwrap().update_server_url.trim().to_string();
                    if server_url.is_empty() {
                        log::info!("[updater] 未配置服务器，跳过启动检查");
                        return;
                    }

                    // 24h throttle.
                    let last_checked = state.config.lock().unwrap().last_update_check_at.clone();
                    if let Some(last) = last_checked.as_deref() {
                        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(last) {
                            let elapsed = chrono::Local::now().signed_duration_since(t.with_timezone(&chrono::Local));
                            if elapsed.num_hours() < 24 {
                                log::info!("[updater] 24h 节流命中，跳过启动检查");
                                return;
                            }
                        }
                    }

                    log::info!("[updater] 启动后台检查更新");
                    match crate::updater::manifest::fetch_manifest(&server_url).await {
                        Ok(manifest) => {
                            let now = chrono::Local::now().to_rfc3339();
                            let has_update = crate::updater::manifest::is_newer(
                                &manifest.latest,
                                crate::updater::CURRENT_VERSION,
                            );

                            *state.updater.manifest.lock().unwrap() = Some(manifest.clone());
                            *state.updater.last_checked_at.lock().unwrap() = Some(now.clone());

                            {
                                let mut cfg = state.config.lock().unwrap();
                                cfg.last_update_check_at = Some(now);
                                let _ = crate::config::save_config(&cfg);
                            }

                            log::info!(
                                "[updater] 当前 {} 远端最新 {} has_update={}",
                                crate::updater::CURRENT_VERSION,
                                manifest.latest,
                                has_update
                            );

                            let _ = app_handle.emit("update-state-changed", build_lite_state(&app_handle));

                            if has_update {
                                let notify = state.config.lock().unwrap().notify_on_new_version;
                                if notify {
                                    let _ = app_handle.emit("open-update-dialog", ());
                                }
                            }
                        }
                        Err(e) => log::warn!("[updater] 启动检查失败：{}", e),
                    }
                });
            }
```

If your `setup` closure does not currently exist (it does in this repo),
add a minimal one. The `app.handle()` call returns an `AppHandle` clone-able
across tasks.

You'll also need to import `tauri::Emitter` if it isn't already imported at
the top of `main.rs` (it likely is — search and confirm).

Add a small helper near the bottom of `main.rs`:

```rust
fn build_lite_state(app_handle: &tauri::AppHandle) -> crate::updater::UpdateState {
    let state = app_handle.state::<AppState>();
    let manifest = state.updater.manifest.lock().unwrap().clone();
    let last_checked_at = state.updater.last_checked_at.lock().unwrap().clone();
    let pending = state.config.lock().unwrap().pending_update.clone();
    let pending_update = crate::updater::pending::validate(pending);
    let server_url = state.config.lock().unwrap().update_server_url.trim().to_string();
    let has_update = manifest.as_ref().map(|m| {
        crate::updater::manifest::is_newer(&m.latest, crate::updater::CURRENT_VERSION)
    }).unwrap_or(false) && !crate::updater::is_debug_build();
    crate::updater::UpdateState {
        current: crate::updater::CURRENT_VERSION.to_string(),
        server_url,
        manifest,
        has_update,
        last_checked_at,
        pending_update,
        debug_build: crate::updater::is_debug_build(),
    }
}
```

- [ ] **Step 4: Verify build**

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: clean compile (warnings about unused imports may exist; resolve
them inline). If `crate::config::save_config` is named differently in this
repo (e.g., `save_config_internal`), search and use the correct name.

- [ ] **Step 5: Run the full updater test suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app updater
```

Expected: all updater tests pass plus the two config migration tests.

- [ ] **Step 6: Format + clippy**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Address any clippy warnings inside `src-tauri/src/updater/` and the new
`main.rs` block. Pre-existing warnings outside this scope may be left alone.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat(updater): wire AppState.updater, register commands, startup background check"
```

---

## Task 13: Frontend type wrappers in `tauri.ts`

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: Append the new types and api**

Open `src/lib/tauri.ts`. At the end of the file (or alongside other feature
sections following project style), append:

```ts
// ===== Update Checker =====

export interface ManifestVersion {
  version: string;
  url: string;
  sha256: string;
  released_at: string;
  changelog: string[];
}

export interface Manifest {
  latest: string;
  versions: ManifestVersion[];
}

export interface PendingUpdate {
  target_version: string;
  temp_path: string;
  sha256: string;
  downloaded_at: string;
}

export interface UpdateState {
  current: string;
  server_url: string;
  manifest: Manifest | null;
  has_update: boolean;
  last_checked_at: string | null;
  pending_update: PendingUpdate | null;
  debug_build: boolean;
}

export interface DownloadProgress {
  downloaded: number;
  total: number | null;
  speed_bps: number;
}

export interface UpdateCheckResult {
  has_update: boolean;
  current: string;
  latest: string | null;
  manifest: Manifest | null;
}

export interface TestServerResult {
  ok: boolean;
  status: number | null;
  error: string | null;
}

export interface UpdateDownloadComplete {
  ok: boolean;
  version: string;
  temp_path: string | null;
  error: string | null;
}

export const updaterApi = {
  getState: () => invoke<UpdateState>('get_update_state'),
  check: () => invoke<UpdateCheckResult>('check_update'),
  startDownload: () => invoke<void>('start_update_download'),
  cancelDownload: () => invoke<void>('cancel_update_download'),
  applyNow: () => invoke<void>('apply_update_now'),
  testServer: () => invoke<TestServerResult>('test_update_server'),
};
```

If `invoke` is not yet imported at the top of `tauri.ts`, look at how other
feature blocks import it and reuse the same import.

- [ ] **Step 2: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(updater): typed tauri wrappers and event payload types"
```

---

## Task 14: `useUpdater` composable

**Files:**
- Create: `src/composables/useUpdater.ts`

- [ ] **Step 1: Write the composable**

Create `src/composables/useUpdater.ts`:

```ts
import { ref, readonly, type Ref } from 'vue';
import { listen } from '@tauri-apps/api/event';
import {
  updaterApi,
  type DownloadProgress,
  type UpdateDownloadComplete,
  type UpdateState,
} from '@/lib/tauri';

export type UpdateDialogState =
  | 'closed'
  | 'found'
  | 'downloading'
  | 'ready'
  | 'verify_failed'
  | 'network_error'
  | 'resume';

const state: Ref<UpdateState | null> = ref(null);
const progress: Ref<DownloadProgress | null> = ref(null);
const dialogOpen: Ref<boolean> = ref(false);
const dialogState: Ref<UpdateDialogState> = ref('closed');
const dialogError: Ref<string | null> = ref(null);

let initialized = false;
async function init() {
  if (initialized) return;
  initialized = true;

  try {
    state.value = await updaterApi.getState();
    if (state.value.pending_update) {
      dialogState.value = 'resume';
      dialogOpen.value = true;
    }
  } catch (err) {
    console.error('[updater] getState failed', err);
  }

  await listen<UpdateState>('update-state-changed', (event) => {
    state.value = event.payload;
  });

  await listen<DownloadProgress>('update-download-progress', (event) => {
    progress.value = event.payload;
  });

  await listen<UpdateDownloadComplete>('update-download-complete', (event) => {
    if (event.payload.ok) {
      dialogState.value = 'ready';
      dialogError.value = null;
    } else {
      const err = event.payload.error ?? '';
      dialogState.value = err.startsWith('verify_failed') ? 'verify_failed' : 'network_error';
      dialogError.value = err;
    }
  });

  await listen('open-update-dialog', () => {
    if (state.value?.has_update) {
      dialogState.value = 'found';
      dialogOpen.value = true;
    }
  });
}

export function useUpdater() {
  if (!initialized) void init();
  return {
    state: readonly(state),
    progress: readonly(progress),
    dialogOpen,
    dialogState,
    dialogError,
    refresh: async () => {
      state.value = await updaterApi.getState();
    },
  };
}
```

- [ ] **Step 2: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add src/composables/useUpdater.ts
git commit -m "feat(updater): useUpdater composable wires state + 4 event listeners"
```

---

## Task 15: About-page pure helpers + tests (TDD)

**Files:**
- Create: `src/pages/about/version.ts`
- Create: `src/pages/about/version.test.mjs`

- [ ] **Step 1: Write failing tests**

Create `src/pages/about/`. Add `src/pages/about/version.test.mjs`:

```js
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { compareVersionsAsc, formatReleaseDate, isCurrentVersion } from './version.ts';

test('compareVersionsAsc orders semver ascending', () => {
  const versions = [
    { version: '1.0.7' },
    { version: '2.0.0' },
    { version: '1.0.8' },
    { version: '1.0.6' },
  ];
  const sorted = [...versions].sort(compareVersionsAsc);
  assert.deepEqual(
    sorted.map((v) => v.version),
    ['1.0.6', '1.0.7', '1.0.8', '2.0.0'],
  );
});

test('compareVersionsAsc treats invalid versions as lowest', () => {
  const versions = [{ version: '1.0.0' }, { version: 'garbage' }];
  const sorted = [...versions].sort(compareVersionsAsc);
  assert.equal(sorted[0].version, 'garbage');
});

test('formatReleaseDate normalizes "YYYY-MM-DD" to "YYYY.MM.DD"', () => {
  assert.equal(formatReleaseDate('2026-04-19'), '2026.04.19');
  assert.equal(formatReleaseDate('2026.04.19'), '2026.04.19');
  assert.equal(formatReleaseDate(''), '');
});

test('isCurrentVersion compares strings exactly', () => {
  assert.equal(isCurrentVersion('1.0.7', '1.0.7'), true);
  assert.equal(isCurrentVersion('1.0.7', '1.0.8'), false);
});
```

- [ ] **Step 2: Run to confirm failure**

```bash
node --test src/pages/about/version.test.mjs
```

Expected: failure — module not found.

- [ ] **Step 3: Implement**

Create `src/pages/about/version.ts`:

```ts
export interface HasVersion {
  version: string;
}

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)/;

function parseSemver(v: string): [number, number, number] | null {
  const m = SEMVER_RE.exec(v);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3])];
}

/** Sort callback that orders by semver ascending. Invalid versions sort first. */
export function compareVersionsAsc<A extends HasVersion, B extends HasVersion>(
  a: A,
  b: B,
): number {
  const pa = parseSemver(a.version);
  const pb = parseSemver(b.version);
  if (!pa && !pb) return 0;
  if (!pa) return -1;
  if (!pb) return 1;
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

/** Normalize an ISO-like release date for sidebar display. */
export function formatReleaseDate(value: string): string {
  if (!value) return '';
  return value.replaceAll('-', '.');
}

export function isCurrentVersion(candidate: string, current: string): boolean {
  return candidate === current;
}
```

- [ ] **Step 4: Run tests**

```bash
node --test src/pages/about/version.test.mjs
```

Expected: 4 passed.

- [ ] **Step 5: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 6: Commit**

```bash
git add src/pages/about
git commit -m "feat(updater): pure helpers for version sort + release date format"
```

---

## Task 16: i18n strings (zh + en)

**Files:**
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Add `sidebar.versionChipTooltip`**

In both `zh` and `en` blocks under `sidebar`, append:

```ts
// zh:
versionChipTooltip: '打开关于与更新页',
// en:
versionChipTooltip: 'Open About & Updates',
```

- [ ] **Step 2: Add `about.*` namespace**

At the same nesting level as other top-level page namespaces, add:

```ts
// zh:
about: {
  title: '关于与更新',
  back: '返回',
  currentVersion: '当前版本：{version}',
  releasedOn: '发布日期：{date}',
  serverLabel: '更新服务器：',
  testConnection: '测试连接',
  testing: '测试中…',
  checkNow: '立即检查',
  checking: '检查中…',
  devModeBadge: '开发模式 — 更新检查已禁用',
  bannerTitle: '发现新版本 {version}',
  bannerReleasedOn: '{date} 发布',
  upgradeCta: '立即升级',
  history: '历史版本',
  currentTag: '当前',
  serverNotConfigured: '未配置更新服务器，请到设置页填写。',
  serverEmpty: '请在设置页填写更新服务器地址后再使用。',
  changelogEmpty: '（无更新内容）',
},
// en:
about: {
  title: 'About & Updates',
  back: 'Back',
  currentVersion: 'Current version: {version}',
  releasedOn: 'Released: {date}',
  serverLabel: 'Update server:',
  testConnection: 'Test Connection',
  testing: 'Testing…',
  checkNow: 'Check Now',
  checking: 'Checking…',
  devModeBadge: 'Debug build — update checks disabled',
  bannerTitle: 'New version {version} available',
  bannerReleasedOn: 'Released {date}',
  upgradeCta: 'Upgrade Now',
  history: 'Release History',
  currentTag: 'Current',
  serverNotConfigured: 'Update server not configured. Please set it in Settings.',
  serverEmpty: 'Please set the update server URL in Settings first.',
  changelogEmpty: '(no changelog)',
},
```

- [ ] **Step 3: Add `updater.*` namespace**

```ts
// zh:
updater: {
  dialog: {
    titleFound: '🚀 发现新版本',
    titleDownloading: '正在下载 {version}…',
    titleReady: '✅ 已下载并校验通过',
    titleVerifyFail: '❌ 文件校验失败',
    titleError: '❌ 下载失败',
    titleResume: '上次有未应用的更新',
    bodyCurrentLatest: '当前版本：{current}    最新版本：{latest}（{date} 发布）',
    bodyResume: '版本 {version} 已下载到本地，现在升级？',
    changelogHeader: '更新内容：',
    actionLater: '稍后提醒',
    actionUpgrade: '立即升级',
    actionCancel: '取消',
    actionRestart: '立即重启升级',
    actionRetry: '重试',
    actionClose: '关闭',
    actionLaterRestart: '稍后',
    progress: '{percent}%  ·  {downloaded} / {total}  ·  {speed}/s',
    progressUnknownTotal: '已下载 {downloaded}  ·  {speed}/s',
    verifyHint: '下载的文件可能损坏。请稍后重试。',
  },
  toast: {
    upToDate: '已是最新版本',
    networkFail: '无法连接到更新服务器：{detail}',
    testOk: '连接成功',
    testFail: '连接失败：{detail}',
    cancelled: '已取消下载',
    restartFailed: '重启失败：{detail}',
  },
},
// en:
updater: {
  dialog: {
    titleFound: '🚀 New version available',
    titleDownloading: 'Downloading {version}…',
    titleReady: '✅ Downloaded and verified',
    titleVerifyFail: '❌ File verification failed',
    titleError: '❌ Download failed',
    titleResume: 'Pending update detected',
    bodyCurrentLatest: 'Current: {current}    Latest: {latest} (released {date})',
    bodyResume: 'Version {version} is already downloaded. Upgrade now?',
    changelogHeader: 'Changes:',
    actionLater: 'Remind Me Later',
    actionUpgrade: 'Upgrade Now',
    actionCancel: 'Cancel',
    actionRestart: 'Restart and Upgrade',
    actionRetry: 'Retry',
    actionClose: 'Close',
    actionLaterRestart: 'Later',
    progress: '{percent}%  ·  {downloaded} / {total}  ·  {speed}/s',
    progressUnknownTotal: 'Downloaded {downloaded}  ·  {speed}/s',
    verifyHint: 'The downloaded file may be corrupted. Please try again.',
  },
  toast: {
    upToDate: 'You are on the latest version.',
    networkFail: 'Cannot reach update server: {detail}',
    testOk: 'Connection OK',
    testFail: 'Connection failed: {detail}',
    cancelled: 'Download cancelled',
    restartFailed: 'Restart failed: {detail}',
  },
},
```

- [ ] **Step 4: Add `settings.update.*` namespace**

Inside the existing `settings` block of both locales, append a sub-block:

```ts
// zh:
update: {
  section: '更新检查',
  notifyToggle: '有新版本时弹窗提示',
  notifyHelp: '关闭后只在左下角版本号处显示红点提示。',
  serverLabel: '更新服务器地址',
  serverPlaceholder: 'http://192.115.1.3:8080',
  serverHint: '支持 http/https。留空将禁用自动检查。',
},
// en:
update: {
  section: 'Update Checks',
  notifyToggle: 'Show popup when a new version is available',
  notifyHelp: 'When off, only the red dot next to the version chip is shown.',
  serverLabel: 'Update server URL',
  serverPlaceholder: 'http://192.115.1.3:8080',
  serverHint: 'http or https. Leave empty to disable auto checks.',
},
```

- [ ] **Step 5: Type-check**

```bash
pnpm check
```

Expected: passes. If TypeScript flags missing-key parity between `zh` and
`en`, fix and re-run.

- [ ] **Step 6: Commit**

```bash
git add src/locales/messages.ts
git commit -m "feat(updater): zh/en strings for sidebar tooltip, about, dialog, settings"
```

---

## Task 17: `UpdateDialog.vue`

**Files:**
- Create: `src/components/UpdateDialog.vue`

- [ ] **Step 1: Write the dialog component**

Create `src/components/UpdateDialog.vue`:

```vue
<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { Download, RefreshCw, ShieldCheck, X } from 'lucide-vue-next';

import { updaterApi } from '@/lib/tauri';
import { addLog } from '@/lib/store';
import { useUpdater } from '@/composables/useUpdater';

defineOptions({ name: 'UpdateDialog' });

const { t, locale } = useI18n();
const { state, progress, dialogOpen, dialogState, dialogError } = useUpdater();

const latestEntry = computed(() => {
  const m = state.value?.manifest;
  if (!m) return null;
  return m.versions.find((v) => v.version === m.latest) ?? null;
});

const pendingEntry = computed(() => state.value?.pending_update ?? null);

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

function formatSpeed(bps: number): string {
  return formatBytes(bps);
}

function formatDate(value: string | null | undefined): string {
  if (!value) return '';
  try {
    const d = new Date(value);
    if (Number.isNaN(d.getTime())) return value;
    return d.toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US');
  } catch {
    return value;
  }
}

const percent = computed(() => {
  const p = progress.value;
  if (!p?.total || p.total === 0) return null;
  return Math.min(100, Math.floor((p.downloaded / p.total) * 100));
});

async function onUpgrade() {
  dialogState.value = 'downloading';
  try {
    await updaterApi.startDownload();
  } catch (err) {
    dialogState.value = 'network_error';
    dialogError.value = String(err);
  }
}

async function onCancel() {
  try {
    await updaterApi.cancelDownload();
  } catch {
    /* ignore */
  }
  addLog(`[updater] ${t('updater.toast.cancelled')}`, 'info');
  dialogOpen.value = false;
  dialogState.value = 'closed';
}

async function onRestart() {
  try {
    await updaterApi.applyNow();
  } catch (err) {
    dialogState.value = 'network_error';
    dialogError.value = String(err);
    addLog(
      `[updater] ${t('updater.toast.restartFailed', { detail: String(err) })}`,
      'error',
    );
  }
}

async function onRetryDownload() {
  await onUpgrade();
}

function onClose() {
  dialogOpen.value = false;
  dialogState.value = 'closed';
}

function onLater() {
  dialogOpen.value = false;
  dialogState.value = 'closed';
}
</script>

<template>
  <transition name="fade">
    <div
      v-if="dialogOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/40 backdrop-blur-sm"
    >
      <div
        class="relative w-full max-w-lg rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl"
      >
        <button
          class="absolute right-4 top-4 rounded-md p-1 text-slate-400 hover:bg-slate-100"
          type="button"
          @click="onClose"
        >
          <X class="h-4 w-4" />
        </button>

        <!-- State: found -->
        <template v-if="dialogState === 'found' && latestEntry">
          <h2 class="text-xl font-bold text-slate-950">
            {{ t('updater.dialog.titleFound') }}
          </h2>
          <p class="mt-2 text-sm text-slate-600">
            {{
              t('updater.dialog.bodyCurrentLatest', {
                current: state?.current ?? '?',
                latest: latestEntry.version,
                date: latestEntry.released_at,
              })
            }}
          </p>
          <div class="mt-4 rounded-xl bg-slate-50 p-4">
            <p class="text-xs font-semibold uppercase tracking-wider text-slate-500">
              {{ t('updater.dialog.changelogHeader') }}
            </p>
            <ul class="mt-2 list-disc space-y-1 pl-5 text-sm text-slate-700">
              <li v-for="(line, i) in latestEntry.changelog" :key="i">{{ line }}</li>
              <li v-if="latestEntry.changelog.length === 0" class="list-none text-slate-400">
                {{ t('about.changelogEmpty') }}
              </li>
            </ul>
          </div>
          <div class="mt-6 flex justify-end gap-3">
            <button
              class="rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              type="button"
              @click="onLater"
            >
              {{ t('updater.dialog.actionLater') }}
            </button>
            <button
              class="inline-flex items-center gap-2 rounded-xl bg-indigo-500 px-4 py-2 text-sm font-semibold text-white hover:bg-indigo-600"
              type="button"
              @click="onUpgrade"
            >
              <Download class="h-4 w-4" />
              {{ t('updater.dialog.actionUpgrade') }}
            </button>
          </div>
        </template>

        <!-- State: downloading -->
        <template v-else-if="dialogState === 'downloading'">
          <h2 class="text-xl font-bold text-slate-950">
            {{
              t('updater.dialog.titleDownloading', {
                version: latestEntry?.version ?? '',
              })
            }}
          </h2>
          <div class="mt-6">
            <div class="h-2 w-full overflow-hidden rounded-full bg-slate-100">
              <div
                class="h-full bg-indigo-500 transition-all"
                :style="{ width: percent !== null ? percent + '%' : '40%' }"
              ></div>
            </div>
            <p class="mt-3 text-xs text-slate-500">
              <template v-if="progress && percent !== null">
                {{
                  t('updater.dialog.progress', {
                    percent,
                    downloaded: formatBytes(progress.downloaded),
                    total: progress.total ? formatBytes(progress.total) : '?',
                    speed: formatSpeed(progress.speed_bps),
                  })
                }}
              </template>
              <template v-else-if="progress">
                {{
                  t('updater.dialog.progressUnknownTotal', {
                    downloaded: formatBytes(progress.downloaded),
                    speed: formatSpeed(progress.speed_bps),
                  })
                }}
              </template>
            </p>
          </div>
          <div class="mt-6 flex justify-end">
            <button
              class="rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              type="button"
              @click="onCancel"
            >
              {{ t('updater.dialog.actionCancel') }}
            </button>
          </div>
        </template>

        <!-- State: ready -->
        <template v-else-if="dialogState === 'ready'">
          <h2 class="flex items-center gap-2 text-xl font-bold text-emerald-700">
            <ShieldCheck class="h-5 w-5" />
            {{ t('updater.dialog.titleReady') }}
          </h2>
          <p class="mt-3 text-sm text-slate-600">
            {{
              t('updater.dialog.bodyCurrentLatest', {
                current: state?.current ?? '?',
                latest: latestEntry?.version ?? '',
                date: latestEntry?.released_at ?? '',
              })
            }}
          </p>
          <div class="mt-6 flex justify-end gap-3">
            <button
              class="rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              type="button"
              @click="onLater"
            >
              {{ t('updater.dialog.actionLaterRestart') }}
            </button>
            <button
              class="inline-flex items-center gap-2 rounded-xl bg-emerald-500 px-4 py-2 text-sm font-semibold text-white hover:bg-emerald-600"
              type="button"
              @click="onRestart"
            >
              <RefreshCw class="h-4 w-4" />
              {{ t('updater.dialog.actionRestart') }}
            </button>
          </div>
        </template>

        <!-- State: resume (pending update from previous run) -->
        <template v-else-if="dialogState === 'resume' && pendingEntry">
          <h2 class="text-xl font-bold text-slate-950">
            {{ t('updater.dialog.titleResume') }}
          </h2>
          <p class="mt-3 text-sm text-slate-600">
            {{
              t('updater.dialog.bodyResume', { version: pendingEntry.target_version })
            }}
          </p>
          <p class="mt-2 text-xs text-slate-400">
            {{ formatDate(pendingEntry.downloaded_at) }}
          </p>
          <div class="mt-6 flex justify-end gap-3">
            <button
              class="rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              type="button"
              @click="onLater"
            >
              {{ t('updater.dialog.actionLaterRestart') }}
            </button>
            <button
              class="inline-flex items-center gap-2 rounded-xl bg-emerald-500 px-4 py-2 text-sm font-semibold text-white hover:bg-emerald-600"
              type="button"
              @click="onRestart"
            >
              <RefreshCw class="h-4 w-4" />
              {{ t('updater.dialog.actionRestart') }}
            </button>
          </div>
        </template>

        <!-- State: verify_failed -->
        <template v-else-if="dialogState === 'verify_failed'">
          <h2 class="text-xl font-bold text-rose-700">
            {{ t('updater.dialog.titleVerifyFail') }}
          </h2>
          <p class="mt-3 text-sm text-slate-600">{{ t('updater.dialog.verifyHint') }}</p>
          <div class="mt-6 flex justify-end">
            <button
              class="rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              type="button"
              @click="onClose"
            >
              {{ t('updater.dialog.actionClose') }}
            </button>
          </div>
        </template>

        <!-- State: network_error -->
        <template v-else-if="dialogState === 'network_error'">
          <h2 class="text-xl font-bold text-rose-700">
            {{ t('updater.dialog.titleError') }}
          </h2>
          <p class="mt-3 text-sm text-slate-600">{{ dialogError ?? '' }}</p>
          <div class="mt-6 flex justify-end gap-3">
            <button
              class="rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
              type="button"
              @click="onClose"
            >
              {{ t('updater.dialog.actionClose') }}
            </button>
            <button
              class="inline-flex items-center gap-2 rounded-xl bg-indigo-500 px-4 py-2 text-sm font-semibold text-white hover:bg-indigo-600"
              type="button"
              @click="onRetryDownload"
            >
              {{ t('updater.dialog.actionRetry') }}
            </button>
          </div>
        </template>
      </div>
    </div>
  </transition>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 120ms ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
```

- [ ] **Step 2: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add src/components/UpdateDialog.vue
git commit -m "feat(updater): UpdateDialog with 6 states (found/downloading/ready/resume/verify/error)"
```

---

## Task 18: `AboutPage.vue`

**Files:**
- Create: `src/pages/AboutPage.vue`

- [ ] **Step 1: Write the page**

Create `src/pages/AboutPage.vue`:

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import { ArrowLeft, RefreshCw, Server, ShieldCheck } from 'lucide-vue-next';

import { updaterApi, type ManifestVersion } from '@/lib/tauri';
import { addLog } from '@/lib/store';
import { useUpdater } from '@/composables/useUpdater';
import { compareVersionsAsc, formatReleaseDate, isCurrentVersion } from '@/pages/about/version';

defineOptions({ name: 'AboutPage' });

const { t } = useI18n();
const router = useRouter();
const { state, dialogOpen, dialogState } = useUpdater();

const checking = ref(false);
const testing = ref(false);

const latestEntry = computed<ManifestVersion | null>(() => {
  const m = state.value?.manifest;
  if (!m) return null;
  return m.versions.find((v) => v.version === m.latest) ?? null;
});

const sortedHistory = computed<ManifestVersion[]>(() => {
  const m = state.value?.manifest;
  if (!m) return [];
  return [...m.versions].sort((a, b) => compareVersionsAsc(b, a)); // newest first
});

const expandedKey = ref<string | null>(null);

function toggleExpand(version: string) {
  expandedKey.value = expandedKey.value === version ? null : version;
}

onMounted(() => {
  if (state.value?.manifest) {
    expandedKey.value = state.value.current;
  }
});

async function onCheckNow() {
  checking.value = true;
  try {
    const result = await updaterApi.check();
    if (result.has_update) {
      dialogState.value = 'found';
      dialogOpen.value = true;
    } else {
      addLog(`[updater] ${t('updater.toast.upToDate')}`, 'success');
    }
  } catch (err) {
    addLog(
      `[updater] ${t('updater.toast.networkFail', { detail: String(err) })}`,
      'error',
    );
  } finally {
    checking.value = false;
  }
}

async function onTestServer() {
  testing.value = true;
  try {
    const result = await updaterApi.testServer();
    if (result.ok) {
      addLog(`[updater] ${t('updater.toast.testOk')}`, 'success');
    } else {
      addLog(
        `[updater] ${t('updater.toast.testFail', { detail: result.error ?? '' })}`,
        'error',
      );
    }
  } finally {
    testing.value = false;
  }
}

function onUpgradeClick() {
  if (state.value?.has_update) {
    dialogState.value = 'found';
    dialogOpen.value = true;
  }
}
</script>

<template>
  <div
    class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(99,102,241,0.16),_transparent_30%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_42%,_#f8fafc_100%)]"
  >
    <div class="mx-auto flex w-full max-w-3xl flex-col gap-6 px-6 py-6 pb-10">
      <header class="flex items-center justify-between">
        <button
          class="inline-flex items-center gap-2 rounded-xl border border-slate-200 bg-white/70 px-3 py-2 text-sm text-slate-600 hover:bg-white"
          type="button"
          @click="router.back()"
        >
          <ArrowLeft class="h-4 w-4" />
          {{ t('about.back') }}
        </button>
        <h1 class="inline-flex items-center gap-2 text-lg font-semibold text-slate-950">
          <ShieldCheck class="h-5 w-5 text-indigo-500" />
          {{ t('about.title') }}
        </h1>
      </header>

      <!-- Version + server -->
      <section class="rounded-[24px] border border-slate-200 bg-white/90 p-5 shadow-sm">
        <p class="text-sm text-slate-700">
          {{ t('about.currentVersion', { version: state?.current ?? '?' }) }}
        </p>
        <p
          v-if="state?.manifest"
          class="mt-1 text-xs text-slate-500"
        >
          {{
            t('about.releasedOn', {
              date: formatReleaseDate(
                state.manifest.versions.find((v) => v.version === state?.current)?.released_at ?? '',
              ),
            })
          }}
        </p>
        <div class="mt-4 flex items-center gap-2 text-sm text-slate-600">
          <Server class="h-4 w-4" />
          <span>{{ t('about.serverLabel') }}</span>
          <code v-if="state?.server_url" class="rounded bg-slate-100 px-2 py-0.5 text-xs">
            {{ state.server_url }}
          </code>
          <span v-else class="text-rose-500">
            {{ t('about.serverNotConfigured') }}
          </span>
          <button
            v-if="state?.server_url"
            class="ml-auto rounded-lg border border-slate-200 px-3 py-1 text-xs hover:bg-slate-50 disabled:opacity-50"
            :disabled="testing"
            type="button"
            @click="onTestServer"
          >
            {{ testing ? t('about.testing') : t('about.testConnection') }}
          </button>
        </div>
        <div class="mt-3 flex justify-end">
          <button
            class="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2 text-sm font-semibold text-white hover:bg-slate-800 disabled:opacity-60"
            :disabled="checking || state?.debug_build || !state?.server_url"
            type="button"
            @click="onCheckNow"
          >
            <RefreshCw class="h-4 w-4" :class="checking ? 'animate-spin' : ''" />
            {{ checking ? t('about.checking') : t('about.checkNow') }}
          </button>
        </div>
        <p
          v-if="state?.debug_build"
          class="mt-3 inline-block rounded-md bg-amber-50 px-2 py-1 text-xs text-amber-700"
        >
          {{ t('about.devModeBadge') }}
        </p>
      </section>

      <!-- New version banner -->
      <section
        v-if="state?.has_update && latestEntry"
        class="rounded-[24px] border border-indigo-200 bg-indigo-50/80 p-5 shadow-sm"
      >
        <h2 class="text-base font-bold text-indigo-900">
          {{ t('about.bannerTitle', { version: latestEntry.version }) }}
        </h2>
        <p class="mt-1 text-xs text-indigo-700">
          {{ t('about.bannerReleasedOn', { date: formatReleaseDate(latestEntry.released_at) }) }}
        </p>
        <ul class="mt-3 list-disc space-y-1 pl-5 text-sm text-indigo-900">
          <li v-for="(line, i) in latestEntry.changelog" :key="i">{{ line }}</li>
          <li v-if="latestEntry.changelog.length === 0" class="list-none text-indigo-500">
            {{ t('about.changelogEmpty') }}
          </li>
        </ul>
        <div class="mt-4 flex justify-end">
          <button
            class="inline-flex items-center gap-2 rounded-xl bg-indigo-500 px-4 py-2 text-sm font-semibold text-white hover:bg-indigo-600"
            type="button"
            @click="onUpgradeClick"
          >
            {{ t('about.upgradeCta') }}
          </button>
        </div>
      </section>

      <!-- History list -->
      <section class="rounded-[24px] border border-slate-200 bg-white/90 p-5 shadow-sm">
        <h2 class="mb-3 text-sm font-semibold text-slate-700">📜 {{ t('about.history') }}</h2>
        <ul v-if="sortedHistory.length > 0" class="divide-y divide-slate-100">
          <li v-for="entry in sortedHistory" :key="entry.version" class="py-3">
            <button
              class="flex w-full items-center justify-between gap-3 text-left"
              type="button"
              @click="toggleExpand(entry.version)"
            >
              <div class="flex items-center gap-2">
                <span class="font-mono text-sm text-slate-900">{{ entry.version }}</span>
                <span
                  v-if="state && isCurrentVersion(entry.version, state.current)"
                  class="rounded-full bg-emerald-100 px-2 py-0.5 text-[11px] font-semibold uppercase text-emerald-700"
                >
                  {{ t('about.currentTag') }}
                </span>
                <span class="text-xs text-slate-400">
                  {{ formatReleaseDate(entry.released_at) }}
                </span>
              </div>
              <span class="text-xs text-slate-400">
                {{ expandedKey === entry.version ? '▾' : '▸' }}
              </span>
            </button>
            <ul
              v-if="expandedKey === entry.version"
              class="mt-2 list-disc space-y-1 pl-6 text-sm text-slate-600"
            >
              <li v-for="(line, i) in entry.changelog" :key="i">{{ line }}</li>
              <li v-if="entry.changelog.length === 0" class="list-none text-slate-400">
                {{ t('about.changelogEmpty') }}
              </li>
            </ul>
          </li>
        </ul>
        <p v-else class="text-sm text-slate-500">
          {{ t('about.serverEmpty') }}
        </p>
      </section>
    </div>
  </div>
</template>
```

- [ ] **Step 2: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add src/pages/AboutPage.vue
git commit -m "feat(updater): AboutPage with version banner, server tools, history list"
```

---

## Task 19: Sidebar version chip + red dot

**Files:**
- Create: `src/components/UpdateRedDot.vue`
- Modify: `src/components/Sidebar.vue`

- [ ] **Step 1: Create the indicator component**

Create `src/components/UpdateRedDot.vue`:

```vue
<script setup lang="ts">
import { ChevronUp } from 'lucide-vue-next';

defineOptions({ name: 'UpdateRedDot' });
</script>

<template>
  <span
    class="inline-flex items-center gap-0.5 rounded-full bg-rose-100 px-1.5 py-0.5 text-[10px] font-semibold text-rose-600 shadow-sm"
    aria-label="update available"
  >
    <span class="h-1.5 w-1.5 rounded-full bg-rose-500"></span>
    <ChevronUp class="h-2.5 w-2.5" />
  </span>
</template>
```

- [ ] **Step 2: Modify `Sidebar.vue` to make the chip clickable + render dot**

Open `src/components/Sidebar.vue`. Find where the version is rendered
(around line 125, where `t('sidebar.version')` is used). Above that, add to
the `<script setup>` block:

```ts
import { useRouter } from 'vue-router';
import UpdateRedDot from './UpdateRedDot.vue';
import { useUpdater } from '@/composables/useUpdater';

const router = useRouter();
const { state: updaterState } = useUpdater();
```

(If `useRouter` is already imported in this file, don't duplicate.)

Replace the version chip block (the existing element that displays
`{{ t('sidebar.version') }}`) with a clickable button that includes the dot:

```vue
<button
  type="button"
  class="group flex items-center gap-2 rounded-lg px-2 py-1 text-xs text-slate-300 hover:bg-slate-800/40"
  :title="t('sidebar.versionChipTooltip')"
  @click="router.push('/about')"
>
  <ShieldCheck class="h-3.5 w-3.5" />
  <span>{{ t('sidebar.version') }}</span>
  <UpdateRedDot v-if="updaterState?.has_update" />
</button>
```

If `ShieldCheck` is not yet imported in this file, add it to the lucide
import line at the top of `<script setup>`.

The exact existing markup may vary; preserve any surrounding container
classes and only change the inner element to a `<button>` with the click
handler + the optional `<UpdateRedDot>` slot.

- [ ] **Step 3: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add src/components/UpdateRedDot.vue src/components/Sidebar.vue
git commit -m "feat(updater): clickable sidebar version chip with red-dot indicator"
```

---

## Task 20: Settings "更新检查" section

**Files:**
- Modify: `src/pages/SettingsPage.vue`

- [ ] **Step 1: Locate the section list**

Open `src/pages/SettingsPage.vue`. The page renders a list of grouped
settings (paths, scheduling, deploy servers, etc.). Find a stable
insertion point — append at the end of the form, **after** the last
existing section but before the form's submit/save controls.

- [ ] **Step 2: Bind the new fields**

In `<script setup>`, where `appConfig` (or the local form state) is read
from `get_config`, ensure the four new fields are read and writable. The
existing pattern likely uses something like:

```ts
const form = reactive<AppConfig>(await invoke<AppConfig>('get_config'));
```

If so, the new fields automatically appear because they're already in the
`AppConfig` type (Task 13 added them to `tauri.ts`'s shared interface — make
sure the interface is updated). Open `src/lib/tauri.ts` and locate the
`AppConfig` interface. Append the four fields:

```ts
  update_server_url: string;
  notify_on_new_version: boolean;
  last_update_check_at: string | null;
  pending_update: PendingUpdate | null;
```

- [ ] **Step 3: Add the section markup**

Append this block to the settings form (adapt class names to match the
file's existing patterns — the example below uses neutral ones):

```vue
<section class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm">
  <h2 class="text-base font-semibold text-slate-900">
    {{ t('settings.update.section') }}
  </h2>

  <label class="mt-4 flex items-start gap-3">
    <input
      v-model="form.notify_on_new_version"
      type="checkbox"
      class="mt-1"
    />
    <span class="text-sm text-slate-700">
      {{ t('settings.update.notifyToggle') }}
      <span class="block text-xs text-slate-500">
        {{ t('settings.update.notifyHelp') }}
      </span>
    </span>
  </label>

  <label class="mt-4 block">
    <span class="text-sm font-medium text-slate-700">
      {{ t('settings.update.serverLabel') }}
    </span>
    <input
      v-model.trim="form.update_server_url"
      type="text"
      :placeholder="t('settings.update.serverPlaceholder')"
      class="mt-1 w-full rounded-lg border border-slate-200 px-3 py-2 text-sm shadow-sm focus:border-indigo-300 focus:outline-none focus:ring-2 focus:ring-indigo-200"
    />
    <p class="mt-1 text-xs text-slate-500">
      {{ t('settings.update.serverHint') }}
    </p>
  </label>
</section>
```

If the settings page uses a different form state name (e.g., `state` or
`config`), use that name instead of `form`.

- [ ] **Step 4: Type-check**

```bash
pnpm check
```

Expected: passes.

- [ ] **Step 5: Commit**

```bash
git add src/pages/SettingsPage.vue src/lib/tauri.ts
git commit -m "feat(updater): settings UI for notify toggle and server URL"
```

---

## Task 21: Router `/about` + global `useUpdater` mount + UpdateDialog injection

**Files:**
- Modify: `src/router/index.ts`
- Modify: `src/App.vue`

- [ ] **Step 1: Register the route**

Open `src/router/index.ts`. After the existing `/settings` route entry, add:

```ts
  {
    path: '/about',
    component: () => import('../pages/AboutPage.vue'),
  },
```

- [ ] **Step 2: Mount `useUpdater` globally + render dialog**

Open `src/App.vue`. In the `<script setup>` block, add:

```ts
import UpdateDialog from '@/components/UpdateDialog.vue';
import { useUpdater } from '@/composables/useUpdater';

useUpdater(); // initialize listeners eagerly
```

In the `<template>`, add `<UpdateDialog />` at the top level (a sibling of
the existing layout root) so it overlays anything:

```vue
<template>
  <!-- existing top-level layout … -->
  <UpdateDialog />
</template>
```

The exact placement varies depending on the existing layout structure; place
it as the **last** child inside the root template element so the modal sits
above all routed content.

- [ ] **Step 3: Type-check + smoke launch**

```bash
pnpm check
pnpm dev
```

Click around: the sidebar version chip should now navigate to `/about`. In
dev mode, the about page shows the "开发模式" badge and the "立即检查"
button is disabled (because `debug_build` is true). Stop the dev server.

- [ ] **Step 4: Commit**

```bash
git add src/router/index.ts src/App.vue
git commit -m "feat(updater): /about route, global UpdateDialog, eager listener init"
```

---

## Task 22: Bundled `serve.py` (optional but spec'd)

**Files:**
- Create: `scripts/release-server/serve.py`
- Create: `scripts/release-server/README.md`

- [ ] **Step 1: Create the server script**

Create `scripts/release-server/serve.py`:

```python
#!/usr/bin/env python3
"""Minimal release server for File Sync Tool.

Serves the working directory over HTTP on a configurable port. Equivalent to
`python3 -m http.server PORT`, but pins the bind address to 0.0.0.0 and
prints a friendlier startup banner.

Usage:
    python3 serve.py            # serves cwd on 8080
    python3 serve.py 8000       # custom port
    python3 serve.py --port 80  # named arg form
"""

from __future__ import annotations

import argparse
import http.server
import os
import socketserver
import sys


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("port", nargs="?", type=int, default=8080)
    parser.add_argument("--port", dest="port_kw", type=int)
    parser.add_argument("--bind", default="0.0.0.0")
    args = parser.parse_args()
    port = args.port_kw if args.port_kw is not None else args.port

    handler = http.server.SimpleHTTPRequestHandler
    handler.extensions_map.setdefault(".json", "application/json")

    with socketserver.TCPServer((args.bind, port), handler) as httpd:
        cwd = os.getcwd()
        print(f"[file-sync-tool-release] serving {cwd} at http://{args.bind}:{port}")
        print("[file-sync-tool-release] Ctrl+C to stop.")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\n[file-sync-tool-release] stopping.")
            return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 2: Create the README**

Create `scripts/release-server/README.md`:

```markdown
# Release Server

Serves File Sync Tool releases (the `manifest.json` plus `*.exe` files) over
plain HTTP on the LAN. Intentionally tiny.

## Quick start

```bash
cd /opt/file-sync-tool-releases  # directory containing manifest.json
python3 serve.py 8080
```

That's it. The client app polls `http://<host>:8080/manifest.json`.

## systemd unit (recommended)

Save as `/etc/systemd/system/file-sync-tool-releases.service`:

```ini
[Unit]
Description=File Sync Tool Release Server
After=network.target

[Service]
WorkingDirectory=/opt/file-sync-tool-releases
ExecStart=/usr/bin/python3 /opt/file-sync-tool-releases/serve.py 8080
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

```bash
systemctl enable --now file-sync-tool-releases
```

## Publishing a new release

1. Build on Windows: `pnpm tauri:build:versioned-exe`
2. `scp src-tauri/target/release/file-sync-tool-X.Y.Z-*.exe server:/opt/file-sync-tool-releases/`
3. Edit `manifest.json`: prepend a new entry to `versions[]` and bump
   `latest`. Compute the SHA-256 with:

   ```bash
   sha256sum file-sync-tool-X.Y.Z-*.exe
   ```

No service restart is needed — the server reads the directory on every
request.

## Manifest schema

See `docs/superpowers/specs/2026-04-25-update-checker-design.md` §2.3.
```

- [ ] **Step 3: Commit**

```bash
git add scripts/release-server
git commit -m "docs(updater): bundled release-server serve.py and README"
```

---

## Task 23: Final verification

**Files:**
- None (verification only)

- [ ] **Step 1: Run full backend test suite**

```bash
cargo test --manifest-path src-tauri/Cargo.toml -p app
```

Expected: all tests pass (config + updater = ~30 tests new + pre-existing).

- [ ] **Step 2: Run all frontend node tests**

```bash
node --test src/lib/sidebarNavigation.test.mjs src/pages/about/version.test.mjs
```

Expected: all pass.

- [ ] **Step 3: Format / clippy**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Expected: no diff from `cargo fmt`; clippy clean inside `src/updater/`.
Pre-existing warnings outside this scope may remain.

- [ ] **Step 4: Type-check + lint**

```bash
pnpm check
pnpm lint
```

Expected: both clean.

- [ ] **Step 5: Versioned production build**

```bash
cmd /c pnpm tauri:build:versioned-exe
```

Expected: build succeeds; produces `file-sync-tool-1.0.7-YYYYMMDDHHmm.exe`.

- [ ] **Step 6: End-to-end manual QA against a real `python -m http.server`**

Run the bundled server in a sibling directory:

```bash
mkdir -p /tmp/fst-test
cd /tmp/fst-test
# create manifest.json describing version 99.0.0
cat > manifest.json <<'EOF'
{
  "latest": "99.0.0",
  "versions": [
    {
      "version": "99.0.0",
      "url": "fake.exe",
      "sha256": "<fill-in-after-creating-fake.exe>",
      "released_at": "2026-04-26",
      "changelog": ["smoke test entry"]
    }
  ]
}
EOF
# create a fake exe and update sha256
head -c 1000000 </dev/urandom > fake.exe
SHA=$(sha256sum fake.exe | awk '{print $1}')
sed -i "s|<fill-in-after-creating-fake.exe>|$SHA|" manifest.json
python3 -m http.server 8080
```

Then in the running app:

```
[ ] Open Settings → 更新检查 section appears.
[ ] Set "更新服务器地址" to http://localhost:8080  → save.
[ ] Click sidebar version chip → /about opens. Server URL is shown.
[ ] Click "测试连接" → success toast.
[ ] Click "立即检查" → dialog opens (because manifest claims 99.0.0 > current).
[ ] Click "立即升级" → progress bar moves to 100%.
[ ] Verify completes (because we used the real sha256).
[ ] Click "稍后" → dialog closes; restart app → resume dialog appears.
[ ] Tamper sha256 in manifest → 立即检查 → 立即升级 → state 3b shows.
[ ] Stop python server → 立即检查 → toast shows networkFail.
[ ] Toggle "弹窗提示" off; restart app → only red dot, no auto popup.
[ ] Toggle "弹窗提示" on; restart app → dialog auto-opens 5s after launch.
[ ] Run debug build (pnpm tauri dev) → about page shows debug badge; no checks fire.
```

> **Important**: Do NOT click "立即重启升级" with the fake.exe — it will
> replace the app's exe with a 1MB random blob. Use a real built exe if you
> want to exercise the full restart path.

- [ ] **Step 7: Commit any final formatting fixes (if any)**

```bash
git status
```

If any files changed during QA (formatting, missed lint fixes), commit
them:

```bash
git add -A
git commit -m "chore(updater): final formatting and qa fixes"
```

If nothing changed, skip this step.

---

## Out of Scope (do NOT implement here)

Per spec §11, these are explicitly deferred:

- Differential / patch updates.
- Mandatory blocking updates.
- Automatic background downloads (download is always user-initiated).
- HTTPS certificate pinning / proxy auto-detection.
- macOS / Linux update support.
- Tray notifications for new versions.
- Per-machine install history page.
- "Skip this version" persisting state.

Do not add them in this plan even if they look quick.
