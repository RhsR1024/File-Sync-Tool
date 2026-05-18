//! Tauri command handlers and startup orchestration for the updater feature.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;

use crate::config::AppConfig;
use crate::updater::{
    download, installer, manifest, pending, self_heal, DownloadCompletePayload, DownloadProgress,
    Manifest, ManifestVersion, PendingUpdate, SharedUpdaterState, TestServerResult,
    UpdateCheckResult, UpdateState, UpdaterError, CURRENT_VERSION,
};
use crate::{config, AppState};

const AUTO_CHECK_DELAY: Duration = Duration::from_secs(5);
const AUTO_CHECK_THROTTLE_HOURS: i64 = 24;
const PROGRESS_EVENT_INTERVAL: Duration = Duration::from_millis(100);
const EVENT_OPEN_DIALOG: &str = "open-update-dialog";
const EVENT_STATE_CHANGED: &str = "update-state-changed";
const EVENT_DOWNLOAD_PROGRESS: &str = "update-download-progress";
const EVENT_DOWNLOAD_COMPLETE: &str = "update-download-complete";

type SharedConfig = Arc<Mutex<AppConfig>>;

#[tauri::command]
pub async fn check_update(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateCheckResult, String> {
    if crate::updater::is_debug_build() {
        return Ok(debug_check_result());
    }

    perform_check(&app_handle, state.config.clone(), state.updater.clone())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_update_download(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if crate::updater::is_debug_build() {
        return Err(UpdaterError::DebugBuild.to_string());
    }

    let manifest = {
        state
            .updater
            .manifest
            .lock()
            .map_err(|_| "updater_state_poisoned".to_string())?
            .clone()
    }
    .ok_or_else(|| "manifest_invalid: no manifest loaded".to_string())?;

    let latest = latest_entry(&manifest)
        .ok_or_else(|| "manifest_invalid: no versions available".to_string())?;
    if !manifest::is_newer(&latest.version, CURRENT_VERSION) {
        return Err("manifest_invalid: latest version is not newer".to_string());
    }
    let target_file_name = manifest::download_file_name_from_url(&latest.url)
        .ok_or_else(|| "manifest_invalid: latest url has no usable file name".to_string())?;

    let (cancel_tx, cancel_rx) = watch::channel(false);
    {
        let mut cancel_slot = state
            .updater
            .cancel_tx
            .lock()
            .map_err(|_| "updater_state_poisoned".to_string())?;
        *cancel_slot = Some(cancel_tx);
    }
    {
        let mut is_downloading = state
            .updater
            .is_downloading
            .lock()
            .map_err(|_| "updater_state_poisoned".to_string())?;
        if *is_downloading {
            // Roll back cancel_tx slot so a stale sender does not linger.
            if let Ok(mut cancel_slot) = state.updater.cancel_tx.lock() {
                *cancel_slot = None;
            }
            return Err(UpdaterError::AlreadyInProgress.to_string());
        }
        *is_downloading = true;
    }

    let config_state = state.config.clone();
    let updater_state = state.updater.clone();
    let version = latest.version.clone();
    let url = latest.url.clone();
    let sha256 = latest.sha256.clone();
    let (download_part_path, final_path) = resolve_download_paths(&version, &target_file_name);

    tauri::async_runtime::spawn(async move {
        run_download_task(
            app_handle,
            config_state,
            updater_state,
            version,
            url,
            sha256,
            target_file_name,
            download_part_path,
            final_path,
            cancel_rx,
        )
        .await;
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_update_download(state: State<'_, AppState>) -> Result<(), String> {
    let cancel_tx = state
        .updater
        .cancel_tx
        .lock()
        .map_err(|_| "updater_state_poisoned".to_string())?
        .clone();

    if let Some(cancel_tx) = cancel_tx {
        if cancel_tx.send(true).is_err() {
            // Receiver already dropped: download has finished. Treat cancel as a no-op success.
            log::debug!("[updater] cancel_update_download: receiver dropped (download finished)");
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn apply_update_now(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if crate::updater::is_debug_build() {
        return Err(UpdaterError::DebugBuild.to_string());
    }

    let pending = {
        state
            .config
            .lock()
            .map_err(|_| "config_poisoned".to_string())?
            .pending_update
            .clone()
    };
    let pending = pending::validate(pending)
        .ok_or_else(|| "io: pending_update_missing_or_invalid".to_string())?;

    let temp_path = PathBuf::from(&pending.temp_path);
    let current_exe_path = std::env::current_exe().map_err(|error| format!("io: {error}"))?;
    let target_exe_path = resolve_apply_target_path(&current_exe_path, &pending.target_file_name)?;

    let snapshot = {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "config_poisoned".to_string())?;
        config.pending_update = None;
        config.clone()
    };
    config::save_config(&app_handle, &snapshot)?;

    installer::spawn_helper(&temp_path, &current_exe_path, &target_exe_path)
        .map_err(|error| error.to_string())?;

    let _ = emit_state_changed(&app_handle, &state.config, &state.updater);

    for window in app_handle.webview_windows().values() {
        let _ = window.close();
    }
    std::thread::sleep(Duration::from_millis(50));
    app_handle.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn test_update_server(state: State<'_, AppState>) -> Result<TestServerResult, String> {
    let server_url = {
        state
            .config
            .lock()
            .map_err(|_| "config_poisoned".to_string())?
            .update_server_url
            .clone()
    };
    let server_url = server_url.trim();
    if server_url.is_empty() {
        return Ok(TestServerResult {
            ok: false,
            status: None,
            error: Some(UpdaterError::NotConfigured.to_string()),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .map_err(|error| format!("network: {error}"))?;

    let response = match client.get(manifest::manifest_url(server_url)).send().await {
        Ok(response) => response,
        Err(error) => {
            return Ok(TestServerResult {
                ok: false,
                status: None,
                error: Some(format!("network: {error}")),
            });
        }
    };

    let status = response.status().as_u16();
    if !response.status().is_success() {
        return Ok(TestServerResult {
            ok: false,
            status: Some(status),
            error: Some(UpdaterError::Http(status).to_string()),
        });
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("network: {error}"))?;
    match manifest::parse_manifest(&body, server_url) {
        Ok(_) => Ok(TestServerResult {
            ok: true,
            status: Some(status),
            error: None,
        }),
        Err(error) => Ok(TestServerResult {
            ok: false,
            status: Some(status),
            error: Some(error.to_string()),
        }),
    }
}

#[tauri::command]
pub fn get_update_state(state: State<'_, AppState>) -> UpdateState {
    snapshot_state(state.inner())
}

pub fn initialize_on_startup(app_handle: AppHandle, state: &AppState) {
    restore_pending_update(&app_handle, state.config.clone(), state.updater.clone());
    let _ = emit_state_changed(&app_handle, &state.config, &state.updater);

    if crate::updater::is_debug_build() {
        return;
    }

    let config_state = state.config.clone();
    let updater_state = state.updater.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_CHECK_DELAY).await;

        if should_skip_auto_check(&config_state, &updater_state) {
            log::info!("[updater] skip startup auto-check because the throttle is active");
            return;
        }

        match perform_check(&app_handle, config_state.clone(), updater_state.clone()).await {
            Ok(result) => {
                if matches!(
                    self_heal::check_and_repair_filename(&app_handle, &updater_state),
                    self_heal::HealOutcome::Spawned
                ) {
                    return;
                }
                let notify = config_state
                    .lock()
                    .map(|config| config.notify_on_new_version)
                    .unwrap_or(false);
                if result.has_update && notify {
                    let _ = app_handle.emit(EVENT_OPEN_DIALOG, ());
                }
            }
            Err(error) => {
                log::warn!("[updater] startup auto-check failed: {error}");
            }
        }
    });
}

pub fn handle_config_changed(app_handle: &AppHandle, state: &AppState, server_url_changed: bool) {
    if server_url_changed {
        if let Ok(mut manifest) = state.updater.manifest.lock() {
            *manifest = None;
        }
        if let Ok(mut last_checked_at) = state.updater.last_checked_at.lock() {
            *last_checked_at = None;
        }
    } else if let Ok(config) = state.config.lock() {
        if let Ok(mut last_checked_at) = state.updater.last_checked_at.lock() {
            *last_checked_at = config.last_update_check_at.clone();
        }
    }

    let _ = emit_state_changed(app_handle, &state.config, &state.updater);
}

pub fn snapshot_state(state: &AppState) -> UpdateState {
    let config = state
        .config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_else(|_| AppConfig::default());
    snapshot_state_from_parts(&config, &state.updater)
}

fn debug_check_result() -> UpdateCheckResult {
    UpdateCheckResult {
        has_update: false,
        current: CURRENT_VERSION.to_string(),
        latest: None,
        manifest: None,
    }
}

fn snapshot_state_from_parts(config: &AppConfig, updater: &SharedUpdaterState) -> UpdateState {
    if crate::updater::is_debug_build() {
        return UpdateState {
            current: CURRENT_VERSION.to_string(),
            server_url: config.update_server_url.clone(),
            manifest: None,
            has_update: false,
            last_checked_at: None,
            pending_update: None,
            debug_build: true,
        };
    }

    let manifest = updater.manifest.lock().ok().and_then(|value| value.clone());
    let last_checked_at = updater
        .last_checked_at
        .lock()
        .ok()
        .and_then(|value| value.clone())
        .or_else(|| config.last_update_check_at.clone());
    let has_update = manifest
        .as_ref()
        .map(|value| manifest::is_newer(&value.latest, CURRENT_VERSION))
        .unwrap_or(false);

    UpdateState {
        current: CURRENT_VERSION.to_string(),
        server_url: config.update_server_url.clone(),
        manifest,
        has_update,
        last_checked_at,
        pending_update: config.pending_update.clone(),
        debug_build: false,
    }
}

fn restore_pending_update(
    app_handle: &AppHandle,
    config_state: SharedConfig,
    updater_state: SharedUpdaterState,
) {
    let (snapshot, changed) = {
        let mut config = match config_state.lock() {
            Ok(config) => config,
            Err(_) => return,
        };
        let original = config.pending_update.clone();
        let validated = pending::validate(original.clone());
        config.pending_update = validated;
        let changed = config.pending_update != original;
        let snapshot = config.clone();
        (snapshot, changed)
    };

    if let Ok(mut last_checked_at) = updater_state.last_checked_at.lock() {
        *last_checked_at = snapshot.last_update_check_at.clone();
    }

    if changed {
        let _ = config::save_config(app_handle, &snapshot);
    }
}

fn should_skip_auto_check(
    config_state: &SharedConfig,
    updater_state: &SharedUpdaterState,
) -> bool {
    let config = match config_state.lock() {
        Ok(config) => config.clone(),
        Err(_) => return true,
    };
    if config.update_server_url.trim().is_empty() {
        return true;
    }

    let manifest_loaded = updater_state
        .manifest
        .lock()
        .map(|manifest| manifest.is_some())
        .unwrap_or(false);
    if !manifest_loaded {
        return false;
    }

    let Some(last_checked_at) = config.last_update_check_at.as_deref() else {
        return false;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(last_checked_at) else {
        return false;
    };

    Utc::now()
        .signed_duration_since(parsed.with_timezone(&Utc))
        .num_hours()
        < AUTO_CHECK_THROTTLE_HOURS
}

async fn perform_check(
    app_handle: &AppHandle,
    config_state: SharedConfig,
    updater_state: SharedUpdaterState,
) -> Result<UpdateCheckResult, UpdaterError> {
    let server_url = {
        let config = config_state
            .lock()
            .map_err(|_| UpdaterError::Io("config_poisoned".to_string()))?;
        config.update_server_url.clone()
    };
    if server_url.trim().is_empty() {
        return Err(UpdaterError::NotConfigured);
    }

    log::info!(
        "[updater] fetching manifest {}",
        manifest::manifest_url(server_url.trim())
    );
    let manifest = manifest::fetch_manifest(server_url.trim()).await?;
    let checked_at = Utc::now().to_rfc3339();

    {
        let mut manifest_slot = updater_state
            .manifest
            .lock()
            .map_err(|_| UpdaterError::Io("updater_state_poisoned".to_string()))?;
        *manifest_slot = Some(manifest.clone());
    }
    {
        let mut last_checked_at = updater_state
            .last_checked_at
            .lock()
            .map_err(|_| UpdaterError::Io("updater_state_poisoned".to_string()))?;
        *last_checked_at = Some(checked_at.clone());
    }

    let config_snapshot = {
        let mut config = config_state
            .lock()
            .map_err(|_| UpdaterError::Io("config_poisoned".to_string()))?;
        config.last_update_check_at = Some(checked_at);
        config.clone()
    };
    config::save_config(app_handle, &config_snapshot).map_err(UpdaterError::Io)?;
    let _ = emit_state_changed(app_handle, &config_state, &updater_state);

    let latest = Some(manifest.latest.clone());
    let has_update = manifest::is_newer(&manifest.latest, CURRENT_VERSION);
    Ok(UpdateCheckResult {
        has_update,
        current: CURRENT_VERSION.to_string(),
        latest,
        manifest: Some(manifest),
    })
}

async fn run_download_task(
    app_handle: AppHandle,
    config_state: SharedConfig,
    updater_state: SharedUpdaterState,
    version: String,
    url: String,
    sha256: String,
    target_file_name: String,
    download_part_path: PathBuf,
    final_path: PathBuf,
    cancel_rx: watch::Receiver<bool>,
) {
    let started_at = Instant::now();
    let mut last_emit_at = Instant::now()
        .checked_sub(PROGRESS_EVENT_INTERVAL)
        .unwrap_or_else(Instant::now);
    let app_handle_for_progress = app_handle.clone();

    let result = download::download_to_file(
        &url,
        &download_part_path,
        &sha256,
        cancel_rx,
        move |downloaded, total| {
            let now = Instant::now();
            if now.duration_since(last_emit_at) < PROGRESS_EVENT_INTERVAL {
                return;
            }
            last_emit_at = now;

            let elapsed = started_at.elapsed().as_secs_f64();
            let speed_bps = if elapsed > 0.0 {
                (downloaded as f64 / elapsed) as u64
            } else {
                0
            };
            let payload = DownloadProgress {
                downloaded,
                total,
                speed_bps,
            };
            let _ = app_handle_for_progress.emit(EVENT_DOWNLOAD_PROGRESS, payload);
        },
    )
    .await;

    let finalize_result = result.and_then(|()| finalize_part_file(&download_part_path, &final_path));

    finish_download_task(
        &app_handle,
        &config_state,
        &updater_state,
        finalize_result,
        version,
        sha256,
        target_file_name,
        final_path,
    );
}

fn finish_download_task(
    app_handle: &AppHandle,
    config_state: &SharedConfig,
    updater_state: &SharedUpdaterState,
    result: Result<(), UpdaterError>,
    version: String,
    sha256: String,
    target_file_name: String,
    dest: PathBuf,
) {
    if let Ok(mut is_downloading) = updater_state.is_downloading.lock() {
        *is_downloading = false;
    }
    if let Ok(mut cancel_tx) = updater_state.cancel_tx.lock() {
        *cancel_tx = None;
    }

    match result {
        Ok(()) => {
            let pending_update = PendingUpdate {
                target_version: version.clone(),
                temp_path: dest.to_string_lossy().to_string(),
                target_file_name,
                sha256,
                downloaded_at: Utc::now().to_rfc3339(),
            };

            let snapshot = match config_state.lock() {
                Ok(mut config) => {
                    config.pending_update = Some(pending_update.clone());
                    config.clone()
                }
                Err(_) => {
                    let _ = app_handle.emit(
                        EVENT_DOWNLOAD_COMPLETE,
                        DownloadCompletePayload {
                            version,
                            temp_path: dest.to_string_lossy().to_string(),
                            sha256_ok: false,
                            error: Some("config_poisoned".to_string()),
                        },
                    );
                    return;
                }
            };

            if let Err(error) = config::save_config(app_handle, &snapshot) {
                if let Ok(mut config) = config_state.lock() {
                    config.pending_update = None;
                }
                let _ = app_handle.emit(
                    EVENT_DOWNLOAD_COMPLETE,
                    DownloadCompletePayload {
                        version,
                        temp_path: dest.to_string_lossy().to_string(),
                        sha256_ok: false,
                        error: Some(error),
                    },
                );
                return;
            }

            let _ = emit_state_changed(app_handle, config_state, updater_state);
            let _ = app_handle.emit(
                EVENT_DOWNLOAD_COMPLETE,
                DownloadCompletePayload {
                    version,
                    temp_path: pending_update.temp_path,
                    sha256_ok: true,
                    error: None,
                },
            );
        }
        Err(error) => {
            let _ = emit_state_changed(app_handle, config_state, updater_state);
            let _ = app_handle.emit(
                EVENT_DOWNLOAD_COMPLETE,
                DownloadCompletePayload {
                    version,
                    temp_path: dest.to_string_lossy().to_string(),
                    sha256_ok: false,
                    error: Some(error.to_string()),
                },
            );
        }
    }
}

fn emit_state_changed(
    app_handle: &AppHandle,
    config_state: &SharedConfig,
    updater_state: &SharedUpdaterState,
) -> Result<(), String> {
    let config = config_state
        .lock()
        .map_err(|_| "config_poisoned".to_string())?
        .clone();
    let snapshot = snapshot_state_from_parts(&config, updater_state);
    app_handle
        .emit(EVENT_STATE_CHANGED, snapshot)
        .map_err(|error| error.to_string())
}

fn latest_entry(manifest: &Manifest) -> Option<ManifestVersion> {
    manifest
        .versions
        .iter()
        .find(|entry| entry.version == manifest.latest)
        .cloned()
        .or_else(|| manifest.versions.first().cloned())
}

fn resolve_apply_target_path(current_exe: &Path, target_file_name: &str) -> Result<PathBuf, String> {
    let parent = current_exe
        .parent()
        .ok_or_else(|| "io: current_exe_has_no_parent".to_string())?;
    let trimmed = target_file_name.trim();
    if trimmed.is_empty() {
        return Err("io: pending_update_missing_target_file_name".to_string());
    }

    let candidate = Path::new(trimmed);
    let file_name = candidate
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "io: invalid_pending_update_target_file_name".to_string())?;
    if candidate.components().count() != 1 || file_name != trimmed {
        return Err("io: invalid_pending_update_target_file_name".to_string());
    }

    Ok(parent.join(file_name))
}

/// Returns `(part_path, final_path)`. Prefer the running exe's directory so the
/// downloaded binary lands next to the program; if that directory is not
/// writable, fall back to a unique path under %TEMP%.
fn resolve_download_paths(version: &str, target_file_name: &str) -> (PathBuf, PathBuf) {
    let safe_name = target_file_name.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            if is_dir_writable(parent) {
                let final_path = parent.join(&safe_name);
                let part_path = parent.join(format!("{safe_name}.part"));
                return (part_path, final_path);
            }
        }
    }

    let sanitized_version = version.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    let timestamp = Utc::now().format("%Y%m%d%H%M%S");
    let final_name = format!(
        "file-sync-tool-update-{sanitized_version}-{timestamp}-{}-{safe_name}",
        std::process::id(),
    );
    let final_path = std::env::temp_dir().join(&final_name);
    let part_path = std::env::temp_dir().join(format!("{final_name}.part"));
    (part_path, final_path)
}

fn is_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(
        ".fst-write-probe-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Move the verified `.part` file to its final name. If `final_path` already
/// exists (e.g. from a previous download attempt) it is removed first so the
/// rename succeeds on Windows.
fn finalize_part_file(part_path: &Path, final_path: &Path) -> Result<(), UpdaterError> {
    if part_path == final_path {
        return Ok(());
    }
    if final_path.exists() {
        if let Err(error) = std::fs::remove_file(final_path) {
            return Err(UpdaterError::Io(error.to_string()));
        }
    }
    std::fs::rename(part_path, final_path).map_err(|error| UpdaterError::Io(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_apply_target_path_switches_to_new_version_file_name() {
        let resolved = resolve_apply_target_path(
            Path::new(r"C:\Tools\file-sync-tool-1.0.7-202604271553.exe"),
            "file-sync-tool-1.1.0-202604271707.exe",
        )
        .expect("resolve target path");

        assert_eq!(
            resolved,
            PathBuf::from(r"C:\Tools\file-sync-tool-1.1.0-202604271707.exe")
        );
    }

    #[test]
    fn resolve_apply_target_path_rejects_nested_target_names() {
        let error = resolve_apply_target_path(
            Path::new(r"C:\Tools\file-sync-tool-1.0.7-202604271553.exe"),
            r"nested\file-sync-tool-1.1.0.exe",
        )
        .unwrap_err();

        assert!(error.contains("invalid_pending_update_target_file_name"));
    }

    #[test]
    fn resolve_download_paths_returns_part_and_final_paths_together() {
        let (part, final_path) =
            resolve_download_paths("1.1.1", "file-sync-tool-1.1.1-202605181737.exe");

        let part_name = part.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let final_name = final_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        assert!(
            final_name.contains("file-sync-tool-1.1.1-202605181737.exe"),
            "final path keeps the target file name: {final_name}"
        );
        assert_eq!(
            part_name,
            format!("{final_name}.part"),
            "part path is the final name + .part suffix"
        );
        assert_eq!(part.parent(), final_path.parent());
    }

    #[test]
    fn finalize_part_file_renames_into_final_path_and_replaces_existing() {
        let tmp = std::env::temp_dir();
        let part = tmp.join(format!("fst-finalize-part-{}.tmp", std::process::id()));
        let final_path = tmp.join(format!("fst-finalize-final-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&part);
        let _ = std::fs::remove_file(&final_path);

        std::fs::write(&part, b"new payload").expect("write part");
        std::fs::write(&final_path, b"stale payload").expect("write existing final");

        finalize_part_file(&part, &final_path).expect("finalize succeeds");
        let written = std::fs::read(&final_path).expect("read final");
        assert_eq!(written, b"new payload");
        assert!(!part.exists(), "part is consumed");

        let _ = std::fs::remove_file(&final_path);
    }

    #[test]
    fn finalize_part_file_is_noop_when_part_equals_final() {
        let path = std::env::temp_dir().join(format!("fst-finalize-same-{}.tmp", std::process::id()));
        std::fs::write(&path, b"identical").expect("write");
        finalize_part_file(&path, &path).expect("noop");
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn startup_auto_check_runs_when_throttled_but_manifest_is_not_loaded() {
        let mut config = AppConfig::default();
        config.last_update_check_at = Some(Utc::now().to_rfc3339());
        let config_state = Arc::new(Mutex::new(config));
        let updater_state = Arc::new(crate::updater::UpdaterState::new());

        assert!(!should_skip_auto_check(&config_state, &updater_state));
    }

    #[test]
    fn startup_auto_check_skips_when_recent_manifest_is_loaded() {
        let mut config = AppConfig::default();
        config.last_update_check_at = Some(Utc::now().to_rfc3339());
        let config_state = Arc::new(Mutex::new(config));
        let updater_state = Arc::new(crate::updater::UpdaterState::new());
        *updater_state.manifest.lock().expect("manifest lock") = Some(Manifest {
            latest: CURRENT_VERSION.to_string(),
            versions: vec![],
        });

        assert!(should_skip_auto_check(&config_state, &updater_state));
    }
}
