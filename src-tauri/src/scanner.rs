#![allow(clippy::too_many_arguments)]

use crate::config::{
    AppConfig, CopyMode, LocalScriptBinding, MatchRule, PostCopyExecutionOrder, TaskServerBinding,
};
use crate::deploy::deploy_to_remote;
use crate::local_exec::{self, LocalExecContext, LocalExecResult};
use crate::task_domain::{TaskSourceType, TaskTriggerSource};
use crate::task_manager::{TaskManager, TaskRunHandle, TaskStartRequest};
use crate::task_runtime::{ActiveRunExecution, TaskRuntimeRegistry};
use crate::windows_copy::{copy_files_with_dialog, WindowsCopyError, WindowsCopyRequest};
use chrono::{Duration, Local, NaiveDateTime, NaiveTime, Timelike};
use flate2::write::GzEncoder;
use flate2::Compression;
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};
use tokio::fs;

const COPY_RETRY_WINDOW: StdDuration = StdDuration::from_secs(10 * 60);
const COPY_RETRY_INITIAL_DELAY: StdDuration = StdDuration::from_millis(500);
const COPY_RETRY_MAX_DELAY: StdDuration = StdDuration::from_secs(5);

#[derive(Debug, serde::Serialize, Clone)]
pub struct ScanResult {
    pub scanned_paths: usize,
    pub found_folders: Vec<String>,
    pub copied_folders: Vec<String>,
    pub errors: Vec<String>,
    /// True when the cycle stopped early to let already-queued copies run first.
    /// The remaining candidates were not skipped, only postponed to a later cycle.
    #[serde(default)]
    pub deferred_for_copy_queue: bool,
}

/// Whether copies the user has already queued are still waiting to run.
///
/// A scan cycle copies its candidates one after another while holding the single copy
/// executor, so without this check a candidate the scan discovers on its own would start
/// ahead of a copy the user explicitly queued minutes earlier — and it would do so
/// without ever appearing in the queue. The scan therefore hands the executor back at
/// the next candidate boundary instead of jumping the queue.
pub type CopyQueuePendingProbe = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Debug, serde::Serialize, Clone)]
struct LogEvent {
    msg: String,
    level: String,
}

#[derive(Debug, serde::Serialize, Clone)]
struct ProgressEvent {
    folder: String,
    total_bytes: u64,
    copied_bytes: u64,
    percentage: f64,
    speed: u64, // bytes per second
    eta_seconds: u64,
    elapsed_seconds: u64,
    local_path: String,
    remote_path: String,
    source: String, // "manual" or "scheduled"
}

#[derive(Debug, serde::Serialize, Clone)]
struct ScanQueuedEvent {
    folder: String,
    local_path: String,
    remote_path: String,
    task_group_id: String,
    run_id: String,
}

fn emit_scan_queued<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    folder: &str,
    local_path: &str,
    remote_path: &str,
    task_group_id: &str,
    run_id: &str,
) {
    let _ = app_handle.emit(
        "scan-queued",
        ScanQueuedEvent {
            folder: folder.to_string(),
            local_path: local_path.to_string(),
            remote_path: remote_path.to_string(),
            task_group_id: task_group_id.to_string(),
            run_id: run_id.to_string(),
        },
    );
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    name: String,
    version: String,
    datetime: NaiveDateTime,
}

// Global mutex to serialize log rotation + write, preventing concurrent rotation races
static LOG_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
fn get_log_mutex() -> &'static Mutex<()> {
    LOG_MUTEX.get_or_init(|| Mutex::new(()))
}

fn normalize_path_for_match(value: &str) -> String {
    value
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Minimal typed representation of a persisted UI task record.
/// Using a concrete struct instead of raw `serde_json::Value` ensures that
/// field renames in the frontend will surface as deserialization misses rather
/// than silent match failures.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedTaskRecord {
    #[serde(default)]
    folder: String,
    #[serde(default)]
    source_path: String,
    #[serde(default)]
    local_path: String,
    #[serde(default)]
    ignored: bool,
}

/// One-shot load of all persisted task records from `ui_state.json`.
/// Call once at the start of a scan and pass the result into `perform_copy`
/// to avoid repeated file IO + JSON parsing per candidate directory.
fn load_persisted_task_records<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> Vec<PersistedTaskRecord> {
    let app_dir = if let Some(d) = crate::config::get_custom_data_dir(app_handle) {
        d
    } else {
        match app_handle.path().app_data_dir() {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        }
    };
    let ui_state_path = app_dir.join("ui_state.json");
    let Ok(content) = std::fs::read_to_string(ui_state_path) else {
        return Vec::new();
    };
    let Ok(state) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let Some(arr) = state.get("task_records").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| serde_json::from_value::<PersistedTaskRecord>(v.clone()).ok())
        .collect()
}

/// Check whether a matching task record exists in the pre-loaded list.
/// Uses AND logic: both folder AND localPath must match to be considered the
/// same task, preventing unrelated tasks with the same folder name from
/// blocking each other's re-copy.
fn task_record_exists_in(records: &[PersistedTaskRecord], folder: &str, local_path: &Path) -> bool {
    let normalized_folder = folder.trim().to_lowercase();
    let normalized_local_path = normalize_path_for_match(&local_path.to_string_lossy());

    records.iter().any(|record| {
        let rf = record.folder.trim().to_lowercase();
        let rlp = normalize_path_for_match(&record.local_path);
        rf == normalized_folder && rlp == normalized_local_path
    })
}

fn task_record_ignored_in(
    records: &[PersistedTaskRecord],
    source_path: &Path,
    local_path: &Path,
) -> bool {
    let normalized_source_path = normalize_path_for_match(&source_path.to_string_lossy());
    let normalized_local_path = normalize_path_for_match(&local_path.to_string_lossy());

    records.iter().any(|record| {
        record.ignored
            && normalize_path_for_match(&record.source_path) == normalized_source_path
            && normalize_path_for_match(&record.local_path) == normalized_local_path
    })
}

/// Whether a scheduled scan should leave this candidate alone because its last copy was
/// cancelled. Re-copying a folder the user just stopped only re-opens the dialog they
/// closed, so a cancel holds until they retry the run manually or clear the task record.
fn copy_was_cancelled(
    task_manager: &TaskManager,
    source_path: &Path,
    local_target_path: &Path,
) -> bool {
    task_manager.last_copy_was_cancelled(
        &source_path.to_string_lossy(),
        &local_target_path.to_string_lossy(),
    )
}

/// Stop the scan cycle when the user already has copies waiting in the queue.
///
/// Candidates are copied one at a time while the scan owns the single copy executor, so
/// a candidate that has not started yet must never take the executor ahead of a copy the
/// user queued earlier. Handing the cycle back here lets the copy queue drain first; the
/// remaining candidates are rediscovered by the next cycle.
fn defer_for_copy_queue<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    copy_queue_pending: &CopyQueuePendingProbe,
    result: &mut ScanResult,
) -> bool {
    if !copy_queue_pending() {
        return false;
    }

    result.deferred_for_copy_queue = true;
    emit_log(
        app_handle,
        "Copies queued earlier are still waiting, so the rest of this scan cycle is postponed until the copy queue is empty."
            .to_string(),
        "info",
    );
    true
}

fn cancelled_skip_message(folder_name: &str) -> String {
    format!(
        "Skipping '{}' because its last copy was cancelled. Retry the run manually or clear the task record to copy it again.",
        folder_name
    )
}

fn should_recopy_size_mismatch(
    _records: &[PersistedTaskRecord],
    _folder: &str,
    _local_path: &Path,
) -> bool {
    true
}

/// Compress `src` into a gzip file at `dst`.
fn compress_log(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(src)?;
    let output = std::fs::File::create(dst)?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    std::io::copy(&mut input, &mut encoder)?;
    encoder.finish()?;
    Ok(())
}

/// Rotate log files if `log_path` has reached 5 MB.
/// Keeps up to MAX_ROTATED compressed files; deletes the oldest when exceeded.
fn rotate_log_if_needed(log_path: &Path) {
    const MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB
    const MAX_ROTATED: u32 = 5;

    let size = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
    if size < MAX_SIZE {
        return;
    }

    let dir = match log_path.parent() {
        Some(d) => d,
        None => return,
    };
    let base = log_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Delete oldest rotated file if it exists
    let oldest = dir.join(format!("{}.{}.gz", base, MAX_ROTATED));
    let _ = std::fs::remove_file(&oldest);

    // Shift: app.log.4.gz -> app.log.5.gz, ..., app.log.1.gz -> app.log.2.gz
    for n in (1..MAX_ROTATED).rev() {
        let from = dir.join(format!("{}.{}.gz", base, n));
        let to = dir.join(format!("{}.{}.gz", base, n + 1));
        if from.exists() {
            let _ = std::fs::rename(&from, &to);
        }
    }

    // Compress current log -> app.log.1.gz
    let gz_path = dir.join(format!("{}.1.gz", base));
    if compress_log(log_path, &gz_path).is_ok() {
        // Remove the original log so a fresh one is created on next write
        let _ = std::fs::remove_file(log_path);
    }
}

/// Emit a tool log to the main console (log-message event) and the log file.
/// The message is prefixed with `【{tool_name}】` for easy identification.
pub fn emit_tool_log<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    tool_name: &str,
    msg: &str,
    level: &str,
) {
    let prefixed = format!("【{}】{}", tool_name, msg);
    let _ = app_handle.emit(
        "log-message",
        serde_json::json!({
            "msg": prefixed,
            "level": level,
        }),
    );
    write_log_to_file(app_handle, &prefixed, level);
}

/// Write a log entry to the app log file. Thread-safe. Used by both scanner and deploy modules.
pub fn write_log_to_file<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    msg: &str,
    level: &str,
) {
    let app_dir = crate::config::get_custom_data_dir(app_handle)
        .or_else(|| app_handle.path().app_data_dir().ok());
    if let Some(app_dir) = app_dir {
        write_log_to_dir(&app_dir, msg, level);
    }
}

/// Write a log entry into `app.log` under the given data directory without an AppHandle.
/// Poison-tolerant so it stays usable inside the panic hook.
pub fn write_log_to_dir(app_dir: &Path, msg: &str, level: &str) {
    if std::fs::create_dir_all(app_dir).is_err() {
        return;
    }
    let log_path = app_dir.join("app.log");
    let _guard = match get_log_mutex().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    rotate_log_if_needed(&log_path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let time = Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] [{}] {}", time, level.to_uppercase(), msg);
    }
}

// Helper to emit logs to frontend in real-time
fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit(
        "log-message",
        LogEvent {
            msg: msg.clone(),
            level: level.to_string(),
        },
    );
    write_log_to_file(app_handle, &msg, level);
}

// Owner window for the Windows native copy dialog. Giving the shell an owner keeps it in
// charge of the dialog's lifetime, so closing it cancels cleanly instead of wedging the
// copy engine.
#[cfg(target_os = "windows")]
fn main_window_hwnd<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> Option<isize> {
    app_handle
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize)
}

#[cfg(not(target_os = "windows"))]
fn main_window_hwnd<R: tauri::Runtime>(_app_handle: &tauri::AppHandle<R>) -> Option<isize> {
    None
}

fn mark_copy_completed_for_handle(
    task_manager: &TaskManager,
    task_handle: Option<&TaskRunHandle>,
    has_deploy_targets: bool,
    message: &str,
) {
    let Some(task_handle) = task_handle else {
        return;
    };

    let _ = task_manager.record_task_log(
        &task_handle.task_group_id,
        &task_handle.run_id,
        None,
        None,
        "success",
        message,
    );
    let _ = task_manager.mark_copy_completed(
        &task_handle.task_group_id,
        &task_handle.run_id,
        has_deploy_targets,
    );
}

fn mark_copy_failed_for_handle(
    task_manager: &TaskManager,
    task_handle: Option<&TaskRunHandle>,
    message: &str,
) {
    let Some(task_handle) = task_handle else {
        return;
    };

    let _ = task_manager.mark_copy_failed(
        &task_handle.task_group_id,
        &task_handle.run_id,
        message.to_string(),
    );
    let _ = task_manager.record_task_log(
        &task_handle.task_group_id,
        &task_handle.run_id,
        None,
        None,
        "error",
        message,
    );
}

fn mark_copy_cancelled_for_handle(
    task_manager: &TaskManager,
    task_handle: Option<&TaskRunHandle>,
    message: &str,
) {
    let Some(task_handle) = task_handle else {
        return;
    };

    let _ = task_manager.mark_copy_cancelled(&task_handle.task_group_id, &task_handle.run_id);
    let _ = task_manager.record_task_log(
        &task_handle.task_group_id,
        &task_handle.run_id,
        None,
        None,
        "warn",
        message,
    );
}

fn copy_run_is_paused(task_manager: &TaskManager, task_handle: Option<&TaskRunHandle>) -> bool {
    task_handle
        .is_some_and(|handle| task_manager.is_run_paused(&handle.task_group_id, &handle.run_id))
}

fn clear_owned_runtime(
    task_runtime: &TaskRuntimeRegistry,
    active_execution: Option<&ActiveRunExecution>,
    run_control_target: &Arc<Mutex<Option<ActiveRunExecution>>>,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
) {
    let Some(active_execution) = active_execution else {
        return;
    };

    let _ = task_runtime.clear(&active_execution.task_group_id, &active_execution.run_id);
    let mut target = run_control_target.lock().unwrap();
    if matches!(target.as_ref(), Some(current) if current == active_execution) {
        should_cancel.store(false, Ordering::SeqCst);
        should_skip.store(false, Ordering::SeqCst);
        is_paused.store(false, Ordering::SeqCst);
        *target = None;
    }
}

fn clear_stale_targeted_run_controls(
    active_execution: &ActiveRunExecution,
    run_control_target: &Arc<Mutex<Option<ActiveRunExecution>>>,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
) {
    let mut target = run_control_target.lock().unwrap();
    if matches!(target.as_ref(), Some(current) if current != active_execution) {
        should_cancel.store(false, Ordering::SeqCst);
        should_skip.store(false, Ordering::SeqCst);
        is_paused.store(false, Ordering::SeqCst);
        *target = None;
    }
}

fn run_needs_copy_completion(task_manager: &TaskManager, task_handle: &TaskRunHandle) -> bool {
    task_manager
        .get_group_detail(&task_handle.task_group_id)
        .and_then(|group| {
            group
                .runs
                .into_iter()
                .find(|run| run.run_id == task_handle.run_id)
        })
        .map(|run| run.copy_phase != crate::task_domain::CopyState::Completed)
        .unwrap_or(true)
}

fn run_local_scripts<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    current_config: &AppConfig,
    binding: &LocalScriptBinding,
    folder_name: &str,
    target_path: &std::path::Path,
    source_path: &str,
    should_cancel: Arc<AtomicBool>,
    task_manager: &TaskManager,
    task_handle: Option<&TaskRunHandle>,
) -> LocalExecResult {
    // Notify TaskManager that local exec is starting
    if let Some(th) = task_handle {
        let _ = task_manager.begin_local_exec(&th.task_group_id, &th.run_id);
    }

    let ctx = LocalExecContext {
        folder_name: folder_name.to_string(),
        local_target: target_path.to_string_lossy().to_string(),
        source_path: source_path.to_string(),
        filename: local_exec::find_tar_gz_filename(target_path),
    };

    let result = local_exec::execute_local_scripts(
        handle,
        binding,
        &current_config.local_command_groups,
        &ctx,
        should_cancel,
    );

    // Update TaskManager with result
    if let Some(th) = task_handle {
        if result.success {
            let _ = task_manager.mark_local_exec_completed(&th.task_group_id, &th.run_id);
        } else if result.aborted {
            let _ = task_manager.mark_local_exec_failed(
                &th.task_group_id,
                &th.run_id,
                "Aborted".to_string(),
            );
        } else {
            let _ = task_manager.mark_local_exec_partial_failed(&th.task_group_id, &th.run_id);
        }
    }

    result
}

fn emit_progress<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    folder: &str,
    copied: u64,
    total: u64,
    speed: u64,
    eta_seconds: u64,
    elapsed_seconds: u64,
    local_path: &str,
    remote_path: &str,
    source: &str,
) {
    let percentage = if total > 0 {
        (copied as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let _ = app_handle.emit(
        "copy-progress",
        ProgressEvent {
            folder: folder.to_string(),
            total_bytes: total,
            copied_bytes: copied,
            percentage,
            speed,
            eta_seconds,
            elapsed_seconds,
            local_path: local_path.to_string(),
            remote_path: remote_path.to_string(),
            source: source.to_string(),
        },
    );
}

#[derive(Debug, Clone, Copy)]
struct CopyRetryPolicy {
    retry_for: StdDuration,
    initial_delay: StdDuration,
    max_delay: StdDuration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopySourceSnapshot {
    source_path: String,
    size: u64,
    modified_millis: Option<u64>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PartialCopyMetadata {
    source_path: String,
    source_size: u64,
    source_modified_millis: Option<u64>,
    target_path: String,
}

impl CopyRetryPolicy {
    fn production() -> Self {
        Self {
            retry_for: COPY_RETRY_WINDOW,
            initial_delay: COPY_RETRY_INITIAL_DELAY,
            max_delay: COPY_RETRY_MAX_DELAY,
        }
    }
}

fn is_controlled_copy_stop(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("skipped by user") || lower.contains("cancelled by user")
}

fn is_retryable_copy_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("os error 64")
        || lower.contains("specified network name is no longer available")
        || lower.contains("指定的网络名不再可用")
        || lower.contains("network")
        || lower.contains("broken pipe")
        || lower.contains("timed out")
        || lower.contains("unexpected eof")
        || lower.contains("temporarily unavailable")
        || lower.contains("source file changed during copy")
        || lower.contains("copied file size mismatch")
}

fn next_retry_delay(current: StdDuration, max_delay: StdDuration) -> StdDuration {
    std::cmp::min(current.saturating_mul(2), max_delay)
}

fn system_time_millis(value: SystemTime) -> Option<u64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn read_copy_source_snapshot(source: &Path) -> Result<CopySourceSnapshot, String> {
    let metadata = std::fs::metadata(source).map_err(|error| error.to_string())?;
    Ok(CopySourceSnapshot {
        source_path: normalize_path_for_match(&source.to_string_lossy()),
        size: metadata.len(),
        modified_millis: metadata.modified().ok().and_then(system_time_millis),
    })
}

fn copy_source_snapshots_match(left: &CopySourceSnapshot, right: &CopySourceSnapshot) -> bool {
    left.source_path == right.source_path
        && left.size == right.size
        && match (left.modified_millis, right.modified_millis) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
}

fn partial_copy_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "copy".to_string());
    target.with_file_name(format!("{}.part", file_name))
}

fn partial_copy_metadata_path(partial: &Path) -> PathBuf {
    let file_name = partial
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "copy.part".to_string());
    partial.with_file_name(format!("{}.meta", file_name))
}

fn replacement_backup_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "copy".to_string());
    target.with_file_name(format!("{}.replace-backup", file_name))
}

fn partial_metadata_matches(
    metadata: &PartialCopyMetadata,
    source: &CopySourceSnapshot,
    target: &Path,
) -> bool {
    metadata.source_path == source.source_path
        && metadata.source_size == source.size
        && metadata.source_modified_millis == source.modified_millis
        && metadata.target_path == normalize_path_for_match(&target.to_string_lossy())
}

fn write_partial_copy_metadata(
    partial: &Path,
    target: &Path,
    source: &CopySourceSnapshot,
) -> Result<(), String> {
    let metadata = PartialCopyMetadata {
        source_path: source.source_path.clone(),
        source_size: source.size,
        source_modified_millis: source.modified_millis,
        target_path: normalize_path_for_match(&target.to_string_lossy()),
    };
    let encoded = serde_json::to_vec(&metadata).map_err(|error| error.to_string())?;
    std::fs::write(partial_copy_metadata_path(partial), encoded).map_err(|error| error.to_string())
}

fn remove_partial_copy_state(partial: &Path) -> Result<(), String> {
    match std::fs::remove_file(partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    let metadata_path = partial_copy_metadata_path(partial);
    match std::fs::remove_file(metadata_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    Ok(())
}

fn prepare_partial_copy(
    source: &CopySourceSnapshot,
    target: &Path,
    partial: &Path,
) -> Result<u64, String> {
    if partial.exists() && !partial.is_file() {
        return Err(format!(
            "Partial copy path is not a file and cannot be resumed: {}",
            partial.display()
        ));
    }

    let mut resume_offset = 0;
    if partial.exists() {
        let metadata_path = partial_copy_metadata_path(partial);
        let metadata = std::fs::read(&metadata_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PartialCopyMetadata>(&bytes).ok());
        let partial_size = std::fs::metadata(partial)
            .map_err(|error| error.to_string())?
            .len();

        if metadata
            .as_ref()
            .is_some_and(|metadata| partial_metadata_matches(metadata, source, target))
            && partial_size <= source.size
        {
            resume_offset = partial_size;
        } else {
            remove_partial_copy_state(partial)?;
        }
    }

    write_partial_copy_metadata(partial, target, source)?;
    Ok(resume_offset)
}

fn restore_or_remove_replacement_backup(target: &Path, backup: &Path) -> Result<(), String> {
    if !backup.exists() {
        return Ok(());
    }

    if !backup.is_file() {
        return Err(format!(
            "Replacement backup path is not a file: {}",
            backup.display()
        ));
    }

    if target.exists() {
        std::fs::remove_file(backup).map_err(|error| error.to_string())
    } else {
        std::fs::rename(backup, target).map_err(|error| error.to_string())
    }
}

fn replace_target_with_completed_partial(partial: &Path, target: &Path) -> Result<(), String> {
    let backup = replacement_backup_path(target);
    restore_or_remove_replacement_backup(target, &backup)?;

    if target.exists() {
        if !target.is_file() {
            return Err(format!(
                "Target path is not a file and cannot be overwritten: {}",
                target.display()
            ));
        }

        std::fs::rename(target, &backup).map_err(|error| error.to_string())?;
        if let Err(error) = std::fs::rename(partial, target) {
            let restore_result = std::fs::rename(&backup, target);
            return Err(match restore_result {
                Ok(()) => error.to_string(),
                Err(restore_error) => format!(
                    "{}; failed to restore previous target from backup: {}",
                    error, restore_error
                ),
            });
        }
        let _ = std::fs::remove_file(backup);
    } else {
        std::fs::rename(partial, target).map_err(|error| error.to_string())?;
    }

    let _ = std::fs::remove_file(partial_copy_metadata_path(partial));
    Ok(())
}

// Helper function to copy file with chunking and interruption support.
// The destination is a managed partial file; the final target is replaced only
// after the partial reaches the expected size and the source metadata is stable.
fn copy_file_chunked<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    resume_offset: u64,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64), // bytes copied delta
) -> Result<u64, String> {
    let mut file_in = std::fs::File::open(from).map_err(|e| e.to_string())?;
    if resume_offset > 0 {
        file_in
            .seek(SeekFrom::Start(resume_offset))
            .map_err(|e| e.to_string())?;
    }

    let mut file_out = if resume_offset > 0 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(to)
            .map_err(|e| e.to_string())?
    } else {
        std::fs::File::create(to).map_err(|e| e.to_string())?
    };

    let mut buffer = vec![0u8; buffer_size];
    let mut total_copied = 0;

    loop {
        // Check skip
        if should_skip.load(Ordering::SeqCst) {
            return Err("Skipped by user".to_string());
        }

        // Check cancel
        if should_cancel.load(Ordering::SeqCst) {
            return Err("Cancelled by user".to_string());
        }

        // Check pause
        while is_paused.load(Ordering::SeqCst) {
            if should_skip.load(Ordering::SeqCst) {
                return Err("Skipped by user".to_string());
            }
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Cancelled by user".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let n = file_in.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break; // EOF
        }

        file_out
            .write_all(&buffer[..n])
            .map_err(|e| e.to_string())?;
        total_copied += n as u64;
        on_progress(n as u64);
    }

    file_out.flush().map_err(|e| e.to_string())?;
    Ok(total_copied)
}

fn copy_file_with_overwrite_mode<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    overwrite_existing: bool,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64),
) -> Result<u64, String> {
    copy_file_with_overwrite_mode_and_policy(
        from,
        to,
        overwrite_existing,
        should_cancel,
        should_skip,
        is_paused,
        buffer_size,
        on_progress,
        CopyRetryPolicy::production(),
    )
}

fn copy_file_with_overwrite_mode_and_policy<P: AsRef<Path>, Q: AsRef<Path>>(
    from: P,
    to: Q,
    overwrite_existing: bool,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64),
    retry_policy: CopyRetryPolicy,
) -> Result<u64, String> {
    let started = Instant::now();
    let mut delay = retry_policy.initial_delay;
    let last_error = loop {
        match copy_file_with_overwrite_mode_once(
            from.as_ref(),
            to.as_ref(),
            overwrite_existing,
            should_cancel,
            should_skip,
            is_paused,
            buffer_size,
            on_progress,
        ) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if is_controlled_copy_stop(&error) => return Err(error),
            Err(error) if !is_retryable_copy_error(&error) => return Err(error),
            Err(error) => {
                if started.elapsed() >= retry_policy.retry_for {
                    break error;
                }
                std::thread::sleep(delay);
                delay = next_retry_delay(delay, retry_policy.max_delay);
            }
        }
    };

    Err(format!(
        "{}; retry window exhausted after {}s",
        last_error,
        retry_policy.retry_for.as_secs()
    ))
}

fn copy_file_with_overwrite_mode_once(
    from: &Path,
    to: &Path,
    overwrite_existing: bool,
    should_cancel: &Arc<AtomicBool>,
    should_skip: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    buffer_size: usize,
    on_progress: &mut dyn FnMut(u64),
) -> Result<u64, String> {
    let target: &Path = to;
    let source_snapshot = read_copy_source_snapshot(from)?;
    let partial = partial_copy_path(target);

    if target.exists() {
        if !target.is_file() {
            return Err(format!(
                "Target path is not a file and cannot be overwritten: {}",
                target.display()
            ));
        }

        let target_size = std::fs::metadata(target)
            .map_err(|error| error.to_string())?
            .len();
        if target_size == source_snapshot.size && !overwrite_existing {
            remove_partial_copy_state(&partial)?;
            return Ok(0);
        }
    }

    let resume_offset = prepare_partial_copy(&source_snapshot, target, &partial)?;

    let bytes_copied = copy_file_chunked(
        from,
        &partial,
        resume_offset,
        should_cancel,
        should_skip,
        is_paused,
        buffer_size,
        on_progress,
    )?;

    let after_copy_source = read_copy_source_snapshot(from)?;
    if !copy_source_snapshots_match(&source_snapshot, &after_copy_source) {
        return Err("Source file changed during copy; will retry".to_string());
    }

    let partial_size = std::fs::metadata(&partial)
        .map_err(|error| error.to_string())?
        .len();
    if partial_size != source_snapshot.size {
        return Err(format!(
            "Copied file size mismatch: partial {} bytes, source {} bytes",
            partial_size, source_snapshot.size
        ));
    }

    replace_target_with_completed_partial(&partial, target)?;
    let target_size = std::fs::metadata(target)
        .map_err(|error| error.to_string())?
        .len();
    if target_size != source_snapshot.size {
        return Err(format!(
            "Copied file size mismatch: target {} bytes, source {} bytes",
            target_size, source_snapshot.size
        ));
    }

    Ok(bytes_copied)
}

// Extracted copy logic to reuse across different matching rules
async fn perform_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    source_path: PathBuf,
    folder_name: String,
    target_parent_path: &Path,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    task_manager: TaskManager,
    task_runtime: TaskRuntimeRegistry,
    run_control_target: Arc<Mutex<Option<ActiveRunExecution>>>,
    should_cancel: Arc<AtomicBool>,
    should_skip: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    overwrite_existing: bool,
    result: &mut ScanResult,
    task_id: Option<String>,
    task_handle: Option<TaskRunHandle>,
    allow_deploy: bool,
    source: &str,
    filter_extensions: &[String],
    filter_includes: &[String],
    cached_task_records: &[PersistedTaskRecord],
) {
    let target_full_path = target_parent_path.join(&folder_name);
    let task_handle = task_handle.or_else(|| {
        if source == "scheduled" {
            Some(task_manager.begin_scheduled_copy(TaskStartRequest {
                task_config_id: task_id.clone(),
                display_name: folder_name.clone(),
                folder_name: folder_name.clone(),
                source_path: source_path.to_string_lossy().to_string(),
                local_target_path: target_full_path.to_string_lossy().to_string(),
                source_type: TaskSourceType::Scheduled,
                trigger_source: TaskTriggerSource::Scheduled,
            }))
        } else {
            None
        }
    });
    let owned_runtime_execution = if source == "scheduled" {
        if let Some(task_handle) = task_handle.as_ref() {
            match task_runtime.activate(
                task_handle.task_group_id.clone(),
                task_handle.run_id.clone(),
            ) {
                Ok(execution) => {
                    clear_stale_targeted_run_controls(
                        &execution,
                        &run_control_target,
                        &should_cancel,
                        &should_skip,
                        &is_paused,
                    );
                    let _ = task_manager
                        .mark_copy_started(&task_handle.task_group_id, &task_handle.run_id);
                    Some(execution)
                }
                Err(error) => {
                    emit_log(app_handle, error.clone(), "error");
                    result.errors.push(error.clone());
                    mark_copy_failed_for_handle(&task_manager, Some(task_handle), &error);
                    return;
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    if source != "scheduled" {
        if let Some(handle) = task_handle.as_ref() {
            let _ = task_manager.mark_copy_started(&handle.task_group_id, &handle.run_id);
        }
    }

    if target_full_path.exists() && !target_full_path.is_dir() {
        let err_msg = format!(
            "Target local path already exists as a file and cannot be used as a directory: {}",
            target_full_path.display()
        );
        emit_log(app_handle, err_msg.clone(), "error");
        result.errors.push(err_msg);
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &result.errors[0]);
        clear_owned_runtime(
            &task_runtime,
            owned_runtime_execution.as_ref(),
            &run_control_target,
            &should_cancel,
            &should_skip,
            &is_paused,
        );
        return;
    }

    emit_log(
        app_handle,
        format!("Target local directory: {}", target_full_path.display()),
        "info",
    );

    // Check if target directory exists, but don't skip entire copy - check for new files
    if target_full_path.exists() {
        emit_log(
            app_handle,
            if overwrite_existing {
                format!(
                    "Target directory {} exists. Matching files may be overwritten and missing files will still be copied.",
                    target_full_path.display()
                )
            } else {
                format!(
                    "Target directory {} exists. Checking for new files...",
                    target_full_path.display()
                )
            },
            "info",
        );
    } else {
        emit_log(
            app_handle,
            format!(
                "Starting copy: {} -> {}",
                source_path.display(),
                target_parent_path.display()
            ),
            "info",
        );
    }

    // Ensure parent dir exists
    if let Err(e) = fs::create_dir_all(target_parent_path).await {
        let err_msg = format!(
            "Failed to create local directory {}: {}",
            target_parent_path.display(),
            e
        );
        emit_log(app_handle, err_msg.clone(), "error");
        result.errors.push(err_msg);
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &result.errors[0]);
        clear_owned_runtime(
            &task_runtime,
            owned_runtime_execution.as_ref(),
            &run_control_target,
            &should_cancel,
            &should_skip,
            &is_paused,
        );
        return;
    }

    let app_handle_clone = app_handle.clone();
    let folder_name_clone = folder_name.clone();
    let source_path_clone = source_path.clone();
    let target_full_path_clone = target_full_path.clone();
    let recopy_size_mismatches =
        should_recopy_size_mismatch(cached_task_records, &folder_name, &target_full_path);
    // Clone filter parameters for closure
    let extensions = filter_extensions.to_vec();
    let includes = filter_includes.to_vec();
    let stability_check_secs = config.stability_check_secs;
    let recent_file_guard_mins = config.recent_file_guard_mins;
    let copy_buffer_size = (config.copy_buffer_size_kb as usize).max(64) * 1024;
    let copy_mode = config.copy_mode.clone();
    if let Some(handle) = task_handle.as_ref() {
        let mode_label = match &copy_mode {
            CopyMode::BuiltIn => "Built-in copy engine",
            CopyMode::WindowsShell => "Windows native copy dialog",
        };
        let _ = task_manager.record_task_log(
            &handle.task_group_id,
            &handle.run_id,
            None,
            None,
            "info",
            &format!("Copy mode: {mode_label}"),
        );
    }
    let should_cancel_clone = should_cancel.clone();
    let should_skip_clone = should_skip.clone();
    let is_paused_clone = is_paused.clone();
    let live_config_clone = live_config.clone();
    let task_id_clone = task_id.clone();
    let source_clone = source.to_string();
    let task_manager_clone = task_manager.clone();
    let task_handle_clone = task_handle.clone();

    let copy_task = tauri::async_runtime::spawn_blocking(move || {
        let handle = app_handle_clone;

        // Prepare paths for display (needed later for progress events)
        let local_path_display = target_full_path_clone.to_string_lossy().to_string();
        let remote_path_display = source_path_clone.to_string_lossy().to_string();

        // Just test access to source dir
        if let Err(e) = std::fs::read_dir(&source_path_clone) {
            let e = e.to_string();
            emit_log(
                &handle,
                format!("Failed to access source dir: {}", e),
                "error",
            );
            return Err(fs_extra::error::Error::new(
                fs_extra::error::ErrorKind::Other,
                &e,
            ));
        }

        // Log active filter rules
        let has_ext_filter = !extensions.is_empty();
        let has_inc_filter = !includes.is_empty();
        if has_ext_filter || has_inc_filter {
            let mut parts = Vec::new();
            if has_ext_filter {
                parts.push(format!("extensions=[{}]", extensions.join(", ")));
            }
            if has_inc_filter {
                parts.push(format!("keywords=[{}]", includes.join(", ")));
            }
            emit_log(
                &handle,
                format!(
                    "Active filter rules for '{}': {}",
                    folder_name_clone,
                    parts.join("; ")
                ),
                "info",
            );
        } else {
            emit_log(
                &handle,
                format!(
                    "No filter rules active for '{}' — all files will be considered.",
                    folder_name_clone
                ),
                "info",
            );
        }

        // Collect files with filtering (Iterative)
        let mut filtered_files: Vec<(PathBuf, u64, bool, bool)> = Vec::new();
        let mut total_files_scanned: u64 = 0;
        let mut skipped_by_ext: Vec<String> = Vec::new();
        let mut skipped_by_keyword: Vec<String> = Vec::new();
        let mut skipped_existing: u64 = 0;
        let mut size_mismatch_recopy: u64 = 0;
        let recent_file_guard_secs = recent_file_guard_mins * 60;
        let now_system = SystemTime::now();

        let mut dirs_to_visit = vec![source_path_clone.clone()];
        while let Some(current_dir) = dirs_to_visit.pop() {
            if let Ok(entries) = std::fs::read_dir(&current_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        dirs_to_visit.push(path);
                    } else {
                        total_files_scanned += 1;
                        // File Check
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        let mut ext_match = true;
                        if !extensions.is_empty() {
                            let name_lower = file_name.to_lowercase();
                            let mut any_match = false;
                            for configured_ext in &extensions {
                                let conf_lower = configured_ext.to_lowercase();
                                let suffix = if conf_lower.starts_with('.') {
                                    conf_lower.clone()
                                } else {
                                    format!(".{}", conf_lower)
                                };

                                if name_lower.ends_with(&suffix) {
                                    any_match = true;
                                    break;
                                }
                            }

                            if !any_match {
                                ext_match = false;
                            }
                        }

                        let mut inc_match = true;
                        if !includes.is_empty() {
                            inc_match = false;
                            for inc in &includes {
                                if file_name.contains(inc) {
                                    inc_match = true;
                                    break;
                                }
                            }
                        }

                        if !ext_match {
                            if skipped_by_ext.len() < 20 {
                                skipped_by_ext.push(file_name.clone());
                            }
                            continue;
                        }
                        if !inc_match {
                            if skipped_by_keyword.len() < 20 {
                                skipped_by_keyword.push(file_name.clone());
                            }
                            continue;
                        }

                        let rel_path = path.strip_prefix(&source_path_clone).unwrap_or(&path);
                        let dst = target_full_path_clone.join(rel_path);

                        if dst.exists() && !dst.is_file() {
                            emit_log(
                                &handle,
                                format!(
                                    "Skipping '{}' because target path exists as a directory: {}",
                                    file_name,
                                    dst.display()
                                ),
                                "warn",
                            );
                            continue;
                        }

                        if let Ok(meta) = entry.metadata() {
                            let file_size = meta.len();
                            let is_recent = meta
                                .modified()
                                .ok()
                                .and_then(|modified| now_system.duration_since(modified).ok())
                                .map(|age| age < StdDuration::from_secs(recent_file_guard_secs))
                                .unwrap_or(true);

                            let mut force_overwrite_due_to_size_mismatch = false;
                            if dst.exists() && !overwrite_existing {
                                match std::fs::metadata(&dst) {
                                    Ok(dst_meta) if dst_meta.is_file() => {
                                        if dst_meta.len() != file_size {
                                            if recopy_size_mismatches {
                                                force_overwrite_due_to_size_mismatch = true;
                                                size_mismatch_recopy += 1;
                                                emit_log(
                                                    &handle,
                                                    format!(
                                                        "Detected incomplete local file, will re-copy: {} (local {} bytes, remote {} bytes)",
                                                        dst.display(),
                                                        dst_meta.len(),
                                                        file_size
                                                    ),
                                                    "warn",
                                                );
                                            } else {
                                                skipped_existing += 1;
                                                emit_log(
                                                    &handle,
                                                    format!(
                                                        "Detected size mismatch but skipped auto re-copy because task record still exists: {} (local {} bytes, remote {} bytes)",
                                                        dst.display(),
                                                        dst_meta.len(),
                                                        file_size
                                                    ),
                                                    "warn",
                                                );
                                                continue;
                                            }
                                        } else {
                                            skipped_existing += 1;
                                            continue;
                                        }
                                    }
                                    Ok(_) => {
                                        skipped_existing += 1;
                                        continue;
                                    }
                                    Err(error) => {
                                        emit_log(
                                            &handle,
                                            format!(
                                                "Failed to read target metadata for '{}': {}. Will re-copy it.",
                                                dst.display(),
                                                error
                                            ),
                                            "warn",
                                        );
                                        force_overwrite_due_to_size_mismatch = true;
                                    }
                                }
                            }

                            filtered_files.push((
                                path,
                                file_size,
                                is_recent,
                                overwrite_existing || force_overwrite_due_to_size_mismatch,
                            ));
                        }
                    }
                }
            }
        }

        // Log filtering summary
        let matched_count = filtered_files.len() as u64;
        let ext_skipped = skipped_by_ext.len() as u64;
        let kw_skipped = skipped_by_keyword.len() as u64;
        emit_log(
            &handle,
            format!(
                "Scan summary for '{}': {} file(s) found, {} matched filters, {} skipped by extension, {} skipped by keyword, {} already exist locally, {} scheduled for re-copy due to size mismatch.",
                folder_name_clone, total_files_scanned, matched_count, ext_skipped, kw_skipped, skipped_existing, size_mismatch_recopy
            ),
            "info",
        );
        if !skipped_by_ext.is_empty() {
            emit_log(
                &handle,
                format!("Skipped by extension filter: {}", skipped_by_ext.join(", ")),
                "info",
            );
        }
        if !skipped_by_keyword.is_empty() {
            emit_log(
                &handle,
                format!(
                    "Skipped by keyword filter: {}",
                    skipped_by_keyword.join(", ")
                ),
                "info",
            );
        }

        if filtered_files.is_empty() {
            // Build a concise rule summary so users can tell why nothing matched.
            let rules_summary = {
                let mut parts = Vec::new();
                if !extensions.is_empty() {
                    parts.push(format!("extensions=[{}]", extensions.join(", ")));
                }
                if !includes.is_empty() {
                    parts.push(format!("keywords=[{}]", includes.join(", ")));
                }
                if parts.is_empty() {
                    "no filter rules".to_string()
                } else {
                    parts.join("; ")
                }
            };
            emit_log(
                &handle,
                format!(
                    "Matched 0 file(s) to copy for '{}' (rules: {}). Scanned {} file(s); {} skipped by extension, {} skipped by keyword, {} already exist locally. Skipping copy.",
                    folder_name_clone,
                    rules_summary,
                    total_files_scanned,
                    skipped_by_ext.len(),
                    skipped_by_keyword.len(),
                    skipped_existing
                ),
                "warn",
            );
            return Ok(0u64);
        }

        // Only announce a scheduled task once this scan has found files that
        // actually need copying. A candidate directory can be present on every
        // scan while all of its files are already up to date.
        if source_clone == "scheduled" {
            if let Some(task_handle) = task_handle_clone.as_ref() {
                emit_scan_queued(
                    &handle,
                    &folder_name_clone,
                    &target_full_path_clone.to_string_lossy(),
                    &source_path_clone.to_string_lossy(),
                    &task_handle.task_group_id,
                    &task_handle.run_id,
                );
            }
        }

        // --- Stability check ---
        // Only files modified within the configured recent-file window enter the waiting flow.
        // Older files are copied directly; recent files wait `stability_check_secs` then re-check size.
        let mut files_ready_now: Vec<(PathBuf, u64, bool)> = filtered_files
            .iter()
            .filter(|(_, _, is_recent, _)| !*is_recent)
            .map(|(path, size, _, overwrite)| (path.clone(), *size, *overwrite))
            .collect();
        let recent_files: Vec<(PathBuf, u64, bool)> = filtered_files
            .into_iter()
            .filter(|(_, _, is_recent, _)| *is_recent)
            .map(|(path, size, _, overwrite)| (path, size, overwrite))
            .collect();

        if !files_ready_now.is_empty() {
            emit_log(
                &handle,
                format!(
                    "{} file(s) were last modified over {} minute(s) ago and will be copied immediately.",
                    files_ready_now.len(),
                    recent_file_guard_mins
                ),
                "info",
            );
        }

        if stability_check_secs > 0 && !recent_files.is_empty() {
            emit_log(
                &handle,
                format!(
                    "Waiting {}s to verify {} recently modified file(s) are fully written...",
                    stability_check_secs,
                    recent_files.len()
                ),
                "info",
            );
            let intervals = stability_check_secs * 5;
            for _ in 0..intervals {
                if should_skip_clone.load(Ordering::SeqCst) {
                    return Err(fs_extra::error::Error::new(
                        fs_extra::error::ErrorKind::Interrupted,
                        "Skipped by user",
                    ));
                }
                if should_cancel_clone.load(Ordering::SeqCst) {
                    return Err(fs_extra::error::Error::new(
                        fs_extra::error::ErrorKind::Interrupted,
                        "Cancelled by user",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }

            for (path, original_size, overwrite_this_file) in recent_files {
                match std::fs::metadata(&path) {
                    Ok(meta) => {
                        let current_size = meta.len();
                        if current_size == original_size {
                            files_ready_now.push((path, original_size, overwrite_this_file));
                        } else {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            emit_log(
                                &handle,
                                format!(
                                    "Skipping '{}' — size changed ({} -> {} bytes), will retry next scan",
                                    name, original_size, current_size
                                ),
                                "warn",
                            );
                        }
                    }
                    Err(e) => {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        emit_log(&handle, format!("Cannot stat '{}': {}", name, e), "warn");
                    }
                }
            }
        } else if !recent_files.is_empty() {
            files_ready_now.extend(recent_files);
        }

        let filtered_files: Vec<(PathBuf, u64, bool)> = files_ready_now;
        let total_filtered_bytes: u64 = filtered_files.iter().map(|(_, s, _)| *s).sum();

        if filtered_files.is_empty() {
            emit_log(
                &handle,
                "All recent candidate files are still being written. Will retry next scan."
                    .to_string(),
                "warn",
            );
            return Ok(0u64);
        }
        emit_log(
            &handle,
            format!(
                "{} file(s) confirmed ready, proceeding with copy.",
                filtered_files.len()
            ),
            "info",
        );

        let start_time = Instant::now();
        let mut last_emit_time = Instant::now();

        // Helper for speed/eta progress events
        let mut update_stats = |copied: u64, total: u64| {
            let now = Instant::now();
            if now.duration_since(last_emit_time).as_millis() > 500 || copied == total {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    (copied as f64 / elapsed) as u64
                } else {
                    0
                };
                let eta = if speed > 0 && total > copied {
                    (total - copied) / speed
                } else {
                    0
                };
                emit_progress(
                    &handle,
                    &folder_name_clone,
                    copied,
                    total,
                    speed,
                    eta,
                    elapsed as u64,
                    &local_path_display,
                    &remote_path_display,
                    &source_clone,
                );
                last_emit_time = now;
            }
        };

        emit_log(
            &handle,
            format!(
                "Copying {} file(s) ({} bytes) from '{}'...",
                filtered_files.len(),
                total_filtered_bytes,
                folder_name_clone
            ),
            "info",
        );

        // Keep filtering, stability checks and task orchestration identical between modes;
        // only the final file-transfer engine changes here.
        let copied_bytes_total = match copy_mode {
            CopyMode::BuiltIn => {
                let mut copied_bytes_total = 0;
                let mut copy_failures = Vec::new();

                for (src, _size, overwrite_this_file) in filtered_files {
                    if should_skip_clone.load(Ordering::SeqCst) {
                        return Err(fs_extra::error::Error::new(
                            fs_extra::error::ErrorKind::Interrupted,
                            "Skipped by user",
                        ));
                    }
                    if should_cancel_clone.load(Ordering::SeqCst) {
                        return Err(fs_extra::error::Error::new(
                            fs_extra::error::ErrorKind::Interrupted,
                            "Cancelled by user",
                        ));
                    }

                    let rel_path = src.strip_prefix(&source_path_clone).unwrap_or(&src);
                    let dst = target_full_path_clone.join(rel_path);
                    if let Some(parent) = dst.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    let file_name_display = src
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let copy_res = copy_file_with_overwrite_mode(
                        &src,
                        &dst,
                        overwrite_this_file,
                        &should_cancel_clone,
                        &should_skip_clone,
                        &is_paused_clone,
                        copy_buffer_size,
                        &mut |delta| {
                            copied_bytes_total += delta;
                            update_stats(copied_bytes_total, total_filtered_bytes);
                        },
                    );

                    if let Err(error) = copy_res {
                        if error.contains("Skipped") {
                            return Err(fs_extra::error::Error::new(
                                fs_extra::error::ErrorKind::Interrupted,
                                "Skipped by user",
                            ));
                        }
                        if error.contains("Cancelled") {
                            return Err(fs_extra::error::Error::new(
                                fs_extra::error::ErrorKind::Interrupted,
                                "Cancelled by user",
                            ));
                        }
                        let failure = format!("Failed to copy {}: {}", file_name_display, error);
                        emit_log(&handle, failure.clone(), "error");
                        copy_failures.push(failure);
                    }
                }

                if !copy_failures.is_empty() {
                    return Err(fs_extra::error::Error::new(
                        fs_extra::error::ErrorKind::Other,
                        &copy_failures.join("; "),
                    ));
                }
                copied_bytes_total
            }
            CopyMode::WindowsShell => {
                if should_skip_clone.load(Ordering::SeqCst) {
                    return Err(fs_extra::error::Error::new(
                        fs_extra::error::ErrorKind::Interrupted,
                        "Skipped by user",
                    ));
                }
                if should_cancel_clone.load(Ordering::SeqCst) {
                    return Err(fs_extra::error::Error::new(
                        fs_extra::error::ErrorKind::Interrupted,
                        "Cancelled by user",
                    ));
                }

                emit_log(
                    &handle,
                    "Using the Windows native copy dialog. Pause or cancel from the Windows copy window."
                        .to_string(),
                    "info",
                );
                let requests = filtered_files
                    .into_iter()
                    .map(|(source, expected_size, _overwrite)| {
                        let relative = source.strip_prefix(&source_path_clone).unwrap_or(&source);
                        WindowsCopyRequest {
                            target: target_full_path_clone.join(relative),
                            source,
                            expected_size,
                        }
                    })
                    .collect();

                let owner_hwnd = main_window_hwnd(&handle);
                match copy_files_with_dialog(
                    requests,
                    owner_hwnd,
                    &should_cancel_clone,
                    &mut |copied_so_far| update_stats(copied_so_far, total_filtered_bytes),
                ) {
                    Ok(copied_bytes) => {
                        update_stats(copied_bytes, total_filtered_bytes);
                        copied_bytes
                    }
                    Err(WindowsCopyError::Cancelled) => {
                        return Err(fs_extra::error::Error::new(
                            fs_extra::error::ErrorKind::Interrupted,
                            "Cancelled in Windows copy dialog",
                        ));
                    }
                    Err(WindowsCopyError::Failed(message)) => {
                        return Err(fs_extra::error::Error::new(
                            fs_extra::error::ErrorKind::Other,
                            &message,
                        ));
                    }
                }
            }
        };

        // Post-copy orchestration: local scripts + remote deploy
        // Re-read the latest config so that enabling deploy after scheduler
        // start is detected without needing to restart the scheduler.
        let current_config = live_config_clone.lock().unwrap().clone();

        // Determine local script binding for this task
        let local_binding: Option<LocalScriptBinding> = task_id_clone.as_ref().and_then(|tid| {
            current_config
                .tasks
                .iter()
                .find(|t| &t.id == tid)
                .and_then(|t| t.local_script_binding.clone())
                .filter(|b| !b.command_group_ids.is_empty())
        });

        let execution_order: PostCopyExecutionOrder = task_id_clone
            .as_ref()
            .and_then(|tid| {
                current_config
                    .tasks
                    .iter()
                    .find(|t| &t.id == tid)
                    .map(|t| t.post_copy_execution_order.clone())
            })
            .unwrap_or_default();

        let has_local = local_binding.is_some();

        // Determine remote deploy targets
        let live_server_bindings: Vec<TaskServerBinding> =
            if allow_deploy && current_config.deploy_enabled {
                if let Some(ref tid) = task_id_clone {
                    current_config
                        .tasks
                        .iter()
                        .find(|t| &t.id == tid)
                        .map(|t| t.server_bindings.clone())
                        .unwrap_or_default()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

        let has_enabled_deploy_targets = live_server_bindings
            .iter()
            .filter_map(|binding| {
                current_config
                    .servers
                    .iter()
                    .find(|server| server.id == binding.server_id)
            })
            .any(|server| server.enabled);

        let has_remote = !live_server_bindings.is_empty() && has_enabled_deploy_targets;
        let has_post_actions = has_local || has_remote;

        mark_copy_completed_for_handle(
            &task_manager_clone,
            task_handle_clone.as_ref(),
            has_post_actions,
            "Copy completed",
        );

        // Execute post-copy actions based on configuration
        match (has_local, has_remote) {
            (false, false) => {
                // No post-copy actions needed
            }
            (true, false) => {
                // Local scripts only
                run_local_scripts(
                    &handle,
                    &current_config,
                    local_binding.as_ref().unwrap(),
                    &folder_name_clone,
                    &target_full_path_clone,
                    &source_path_clone.to_string_lossy(),
                    should_cancel_clone,
                    &task_manager_clone,
                    task_handle_clone.as_ref(),
                );
            }
            (false, true) => {
                // Remote deploy only (existing behavior)
                let deploy_tracking = task_handle_clone.as_ref().map(|th| {
                    task_manager_clone.tracking_context(th.task_group_id.clone(), th.run_id.clone())
                });
                if let Err(e) = deploy_to_remote(
                    &handle,
                    &live_server_bindings,
                    &current_config.servers,
                    &current_config.command_groups,
                    &target_full_path_clone,
                    &folder_name_clone,
                    should_cancel_clone,
                    is_paused_clone,
                    deploy_tracking,
                ) {
                    emit_log(&handle, format!("Deployment failed: {}", e), "error");
                }
            }
            (true, true) => match execution_order {
                PostCopyExecutionOrder::LocalFirst => {
                    let local_result = run_local_scripts(
                        &handle,
                        &current_config,
                        local_binding.as_ref().unwrap(),
                        &folder_name_clone,
                        &target_full_path_clone,
                        &source_path_clone.to_string_lossy(),
                        should_cancel_clone.clone(),
                        &task_manager_clone,
                        task_handle_clone.as_ref(),
                    );

                    if local_result.aborted {
                        emit_log(
                            &handle,
                            "Local script execution aborted — skipping remote deploy".to_string(),
                            "warn",
                        );
                    } else {
                        let deploy_tracking = task_handle_clone.as_ref().map(|th| {
                            task_manager_clone
                                .tracking_context(th.task_group_id.clone(), th.run_id.clone())
                        });
                        if let Err(e) = deploy_to_remote(
                            &handle,
                            &live_server_bindings,
                            &current_config.servers,
                            &current_config.command_groups,
                            &target_full_path_clone,
                            &folder_name_clone,
                            should_cancel_clone,
                            is_paused_clone,
                            deploy_tracking,
                        ) {
                            emit_log(&handle, format!("Deployment failed: {}", e), "error");
                        }
                    }
                }
                PostCopyExecutionOrder::RemoteFirst => {
                    let deploy_tracking = task_handle_clone.as_ref().map(|th| {
                        task_manager_clone
                            .tracking_context(th.task_group_id.clone(), th.run_id.clone())
                    });
                    if let Err(e) = deploy_to_remote(
                        &handle,
                        &live_server_bindings,
                        &current_config.servers,
                        &current_config.command_groups,
                        &target_full_path_clone,
                        &folder_name_clone,
                        should_cancel_clone.clone(),
                        is_paused_clone,
                        deploy_tracking,
                    ) {
                        emit_log(&handle, format!("Deployment failed: {}", e), "error");
                    }
                    run_local_scripts(
                        &handle,
                        &current_config,
                        local_binding.as_ref().unwrap(),
                        &folder_name_clone,
                        &target_full_path_clone,
                        &source_path_clone.to_string_lossy(),
                        should_cancel_clone,
                        &task_manager_clone,
                        task_handle_clone.as_ref(),
                    );
                }
                PostCopyExecutionOrder::Parallel => {
                    let handle_local = handle.clone();
                    let config_local = current_config.clone();
                    let binding_local = local_binding.clone().unwrap();
                    let folder_local = folder_name_clone.clone();
                    let target_local = target_full_path_clone.clone();
                    let source_local = source_path_clone.to_string_lossy().to_string();
                    let cancel_local = should_cancel_clone.clone();
                    let tm_local = task_manager_clone.clone();
                    let th_local = task_handle_clone.clone();

                    std::thread::scope(|s| {
                        let local_thread = s.spawn(|| {
                            run_local_scripts(
                                &handle_local,
                                &config_local,
                                &binding_local,
                                &folder_local,
                                &target_local,
                                &source_local,
                                cancel_local,
                                &tm_local,
                                th_local.as_ref(),
                            )
                        });

                        let deploy_tracking = task_handle_clone.as_ref().map(|th| {
                            task_manager_clone
                                .tracking_context(th.task_group_id.clone(), th.run_id.clone())
                        });
                        if let Err(e) = deploy_to_remote(
                            &handle,
                            &live_server_bindings,
                            &current_config.servers,
                            &current_config.command_groups,
                            &target_full_path_clone,
                            &folder_name_clone,
                            should_cancel_clone,
                            is_paused_clone,
                            deploy_tracking,
                        ) {
                            emit_log(&handle, format!("Deployment failed: {}", e), "error");
                        }

                        let _ = local_thread.join();
                    });
                }
            },
        }

        Ok(copied_bytes_total)
    });

    match copy_task.await {
        Ok(Ok(0)) => {
            // Nothing was copied (either no files matched the rules or all files already up to date).
            // For scheduled ticks we discard the run so the task-detail history only keeps rows
            // that actually copied files or were interrupted/cancelled. Manual copies still get a
            // "completed" row so the user sees their explicit action was received.
            if let Some(task_handle) = task_handle.as_ref() {
                if source == "scheduled" {
                    let _ = task_manager
                        .discard_noop_run(&task_handle.task_group_id, &task_handle.run_id);
                } else if run_needs_copy_completion(&task_manager, task_handle) {
                    mark_copy_completed_for_handle(
                        &task_manager,
                        Some(task_handle),
                        false,
                        "Copy completed — 0 files matched the copy rules",
                    );
                }
            }
        }
        Ok(Ok(_)) => {
            emit_log(
                app_handle,
                format!("Successfully copied: {}", folder_name),
                "success",
            );
            result.copied_folders.push(folder_name);
        }
        Ok(Err(e)) => {
            if let fs_extra::error::ErrorKind::Interrupted = e.kind {
                let is_paused = copy_run_is_paused(&task_manager, task_handle.as_ref());
                let is_skip = e.to_string().contains("Skipped");
                let msg = if is_paused {
                    format!("Copy paused: {}", folder_name)
                } else if is_skip {
                    format!("Copy skipped: {}", folder_name)
                } else {
                    format!("Copy cancelled: {}", folder_name)
                };
                // Reaching here means the copy loop already exited: pause blocks inside the
                // loop, so an Interrupted result always means cancel/skip won the race. The
                // run cannot be resumed, so it must reach a terminal state even when the
                // group still carries a pause flag — otherwise it stays "copying" forever.
                mark_copy_cancelled_for_handle(&task_manager, task_handle.as_ref(), &msg);
                emit_log(app_handle, msg.clone(), "warn");
                if source == "manual" {
                    result.errors.push(msg);
                }
            } else {
                let err_msg = format!("Failed to copy {}: {}", folder_name, e);
                mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &err_msg);
                emit_log(app_handle, err_msg.clone(), "error");
                result.errors.push(err_msg);
            }
        }
        Err(e) => {
            let err_msg = format!("Copy task panic: {}", e);
            mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &err_msg);
            emit_log(app_handle, err_msg.clone(), "error");
            result.errors.push(err_msg);
        }
    }
    clear_owned_runtime(
        &task_runtime,
        owned_runtime_execution.as_ref(),
        &run_control_target,
        &should_cancel,
        &should_skip,
        &is_paused,
    );
}

pub async fn temporary_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    task_manager: TaskManager,
    task_runtime: TaskRuntimeRegistry,
    task_handle: Option<TaskRunHandle>,
    source_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    should_cancel: Arc<AtomicBool>,
    should_skip: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
    skip_stability_check: bool,
    task_id: Option<String>,
    allow_deploy: bool,
) -> Result<(), String> {
    let source_path = PathBuf::from(source_path.trim());
    let target_root_path = PathBuf::from(target_root_path.trim());

    if source_path.as_os_str().is_empty() {
        let message = "Source path is required".to_string();
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }
    if target_root_path.as_os_str().is_empty() {
        let message = "Target root path is required".to_string();
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }
    if !source_path.exists() {
        let message = format!("Source path does not exist: {}", source_path.display());
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }
    if !source_path.is_dir() && !source_path.is_file() {
        let message = format!(
            "Source path must be a file or directory: {}",
            source_path.display()
        );
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }
    if !target_root_path.exists() {
        let message = format!(
            "Target root directory does not exist: {}",
            target_root_path.display()
        );
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }
    if !target_root_path.is_dir() {
        let message = format!(
            "Target root path is not a directory: {}",
            target_root_path.display()
        );
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }

    // Handle single file copy
    if source_path.is_file() {
        return temporary_copy_file(
            app_handle,
            config,
            task_manager,
            task_handle,
            source_path,
            target_root_path,
            overwrite_existing,
            should_cancel,
            should_skip,
            is_paused,
            skip_stability_check,
        )
        .await;
    }

    // Directory copy (original logic)
    let folder_name = source_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "temporary-copy".to_string());

    let target_full_path = target_root_path.join(&folder_name);
    if target_full_path == source_path || target_full_path.starts_with(&source_path) {
        let message = format!(
            "Target path would be created inside source path, which may cause recursive copying: {}",
            target_full_path.display()
        );
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }

    emit_log(
        app_handle,
        format!(
            "Starting temporary copy: {} -> {}",
            source_path.display(),
            target_root_path.display()
        ),
        "info",
    );

    let mut result = ScanResult {
        scanned_paths: 1,
        found_folders: vec![folder_name.clone()],
        copied_folders: vec![],
        errors: vec![],
        deferred_for_copy_queue: false,
    };

    // Manual copies always allow size-mismatch re-copy (user explicitly triggered),
    // so pass an empty slice — task_record_exists_in will always return false.
    perform_copy(
        app_handle,
        source_path,
        folder_name,
        &target_root_path,
        config,
        live_config,
        task_manager,
        task_runtime,
        Arc::new(Mutex::new(None)),
        should_cancel,
        should_skip,
        is_paused,
        overwrite_existing,
        &mut result,
        task_id,
        task_handle,
        allow_deploy,
        "manual",
        &file_extensions,
        &filename_includes,
        &[],
    )
    .await;

    if result.errors.is_empty() {
        Ok(())
    } else {
        Err(result.errors.join("; "))
    }
}

/// Copy a single file to the target directory.
async fn temporary_copy_file<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    task_manager: TaskManager,
    task_handle: Option<TaskRunHandle>,
    source_path: PathBuf,
    target_root_path: PathBuf,
    overwrite_existing: bool,
    should_cancel: Arc<AtomicBool>,
    should_skip: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    skip_stability_check: bool,
) -> Result<(), String> {
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "Cannot extract file name from source path".to_string())?;

    let target_file = target_root_path.join(&file_name);

    emit_log(
        app_handle,
        format!(
            "Starting single-file copy: {} -> {}",
            source_path.display(),
            target_root_path.display()
        ),
        "info",
    );

    // Transition the run from Pending to Running so the UI shows "copying"
    // (with progress / stability-wait) instead of staying stuck at "queued".
    if let Some(handle) = task_handle.as_ref() {
        let _ = task_manager.mark_copy_started(&handle.task_group_id, &handle.run_id);
    }

    // Ensure target directory exists
    if let Err(e) = fs::create_dir_all(&target_root_path).await {
        let message = format!(
            "Failed to create target directory {}: {}",
            target_root_path.display(),
            e
        );
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        return Err(message);
    }

    // Get file metadata
    let meta = std::fs::metadata(&source_path).map_err(|e| {
        let message = format!("Cannot read file metadata: {}", e);
        mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
        message
    })?;
    let file_size = meta.len();

    // Check if target file already exists
    if target_file.exists() {
        if !target_file.is_file() {
            let message = format!(
                "Target path already exists as a directory and cannot be used as a file: {}",
                target_file.display()
            );
            mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
            return Err(message);
        }
        let target_size = std::fs::metadata(&target_file)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if target_size != file_size {
            let action = if config.copy_mode == CopyMode::WindowsShell {
                "will re-copy it with Windows"
            } else {
                "will resume copy"
            };
            emit_log(
                app_handle,
                format!(
                    "Detected incomplete local file, {}: {} (local {} bytes, remote {} bytes)",
                    action,
                    target_file.display(),
                    target_size,
                    file_size
                ),
                "warn",
            );
        } else if overwrite_existing {
            emit_log(
                app_handle,
                format!(
                    "File already exists at target and will be overwritten: {}",
                    target_file.display()
                ),
                "warn",
            );
        } else {
            emit_log(
                app_handle,
                format!(
                    "File already exists at target, skipping because overwrite is disabled: {}",
                    target_file.display()
                ),
                "info",
            );
            mark_copy_completed_for_handle(
                &task_manager,
                task_handle.as_ref(),
                false,
                "Copy completed with no file changes",
            );
            return Ok(());
        }
    }

    if target_file.exists() && overwrite_existing {
        emit_log(
            app_handle,
            format!(
                "Preparing overwrite copy for existing target file: {}",
                target_file.display()
            ),
            "warn",
        );
    }

    // Stability check for recently modified files
    let recent_file_guard_secs = config.recent_file_guard_mins * 60;
    let is_recent = meta
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age < StdDuration::from_secs(recent_file_guard_secs))
        .unwrap_or(true);

    if is_recent && skip_stability_check {
        emit_log(
            app_handle,
            "File was recently modified, but user chose to copy immediately; skipping stability wait."
                .to_string(),
            "info",
        );
    }

    if is_recent && config.stability_check_secs > 0 && !skip_stability_check {
        emit_log(
            app_handle,
            format!(
                "File was recently modified, waiting {}s for stability check...",
                config.stability_check_secs
            ),
            "info",
        );
        let intervals = config.stability_check_secs * 5;
        for _ in 0..intervals {
            if should_cancel.load(Ordering::SeqCst) {
                let message = "Cancelled by user".to_string();
                mark_copy_cancelled_for_handle(&task_manager, task_handle.as_ref(), &message);
                return Err(message);
            }
            if should_skip.load(Ordering::SeqCst) {
                let message = "Skipped by user".to_string();
                mark_copy_cancelled_for_handle(&task_manager, task_handle.as_ref(), &message);
                return Err(message);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }

        // Re-check file size
        let new_meta = std::fs::metadata(&source_path).map_err(|e| {
            let message = format!("Cannot re-check file metadata: {}", e);
            mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
            message
        })?;
        if new_meta.len() != file_size {
            let message = format!(
                "File size changed during stability check ({} -> {} bytes), aborting",
                file_size,
                new_meta.len()
            );
            mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
            return Err(message);
        }
    }

    // Copy with progress
    let app_handle_clone = app_handle.clone();
    let file_name_clone = file_name.clone();
    let source_clone = source_path.clone();
    let target_file_clone = target_file.clone();
    let target_file_display = target_file.to_string_lossy().to_string();
    let source_display = source_path.to_string_lossy().to_string();
    let copy_buffer_size = (config.copy_buffer_size_kb as usize).max(64) * 1024;
    let copy_mode = config.copy_mode.clone();
    let owner_hwnd = main_window_hwnd(app_handle);
    if let Some(handle) = task_handle.as_ref() {
        let mode_label = match &copy_mode {
            CopyMode::BuiltIn => "Built-in copy engine",
            CopyMode::WindowsShell => "Windows native copy dialog",
        };
        let _ = task_manager.record_task_log(
            &handle.task_group_id,
            &handle.run_id,
            None,
            None,
            "info",
            &format!("Copy mode: {mode_label}"),
        );
    }

    let copy_result = tauri::async_runtime::spawn_blocking(move || {
        let start_time = Instant::now();
        let mut last_emit_time = Instant::now();
        let mut copied_so_far: u64 = 0;

        let mut on_progress = |delta: u64| {
            copied_so_far += delta;
            let now = Instant::now();
            if now.duration_since(last_emit_time).as_millis() > 500 || copied_so_far == file_size {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    (copied_so_far as f64 / elapsed) as u64
                } else {
                    0
                };
                let eta = if speed > 0 && file_size > copied_so_far {
                    (file_size - copied_so_far) / speed
                } else {
                    0
                };
                emit_progress(
                    &app_handle_clone,
                    &file_name_clone,
                    copied_so_far,
                    file_size,
                    speed,
                    eta,
                    elapsed as u64,
                    &target_file_display,
                    &source_display,
                    "manual",
                );
                last_emit_time = now;
            }
        };

        match copy_mode {
            CopyMode::BuiltIn => copy_file_with_overwrite_mode(
                &source_clone,
                &target_file_clone,
                overwrite_existing,
                &should_cancel,
                &should_skip,
                &is_paused,
                copy_buffer_size,
                &mut on_progress,
            ),
            CopyMode::WindowsShell => {
                if should_cancel.load(Ordering::SeqCst) {
                    return Err("Cancelled by user".to_string());
                }
                if should_skip.load(Ordering::SeqCst) {
                    return Err("Skipped by user".to_string());
                }
                emit_log(
                    &app_handle_clone,
                    "Using the Windows native copy dialog. Pause or cancel from the Windows copy window."
                        .to_string(),
                    "info",
                );
                let mut reported = 0u64;
                match copy_files_with_dialog(
                    vec![WindowsCopyRequest {
                        source: source_clone,
                        target: target_file_clone,
                        expected_size: file_size,
                    }],
                    owner_hwnd,
                    &should_cancel,
                    &mut |copied_so_far| {
                        if copied_so_far > reported {
                            on_progress(copied_so_far - reported);
                            reported = copied_so_far;
                        }
                    },
                ) {
                    Ok(bytes_copied) => {
                        if bytes_copied > reported {
                            on_progress(bytes_copied - reported);
                        }
                        Ok(bytes_copied)
                    }
                    Err(WindowsCopyError::Cancelled) => {
                        Err("Cancelled in Windows copy dialog".to_string())
                    }
                    Err(WindowsCopyError::Failed(message)) => Err(message),
                }
            }
        }
    })
    .await;

    match copy_result {
        Ok(Ok(bytes_copied)) => {
            emit_log(
                app_handle,
                format!(
                    "File copied successfully: {} ({} bytes)",
                    file_name, bytes_copied
                ),
                "success",
            );

            mark_copy_completed_for_handle(
                &task_manager,
                task_handle.as_ref(),
                false,
                "Copy completed",
            );
            Ok(())
        }
        Ok(Err(e)) => {
            if e.to_lowercase().contains("cancelled") || e.to_lowercase().contains("skipped") {
                if copy_run_is_paused(&task_manager, task_handle.as_ref()) {
                    return Err(format!("Copy paused: {file_name}"));
                }
                mark_copy_cancelled_for_handle(&task_manager, task_handle.as_ref(), &e);
                return Err(e);
            } else {
                let message = format!("Failed to copy file: {}", e);
                mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
            }
            Err(format!("Failed to copy file: {}", e))
        }
        Err(e) => {
            let message = format!("Copy task panic: {}", e);
            mark_copy_failed_for_handle(&task_manager, task_handle.as_ref(), &message);
            Err(message)
        }
    }
}

pub async fn scan_and_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    task_manager: TaskManager,
    task_runtime: TaskRuntimeRegistry,
    run_control_target: Arc<Mutex<Option<ActiveRunExecution>>>,
    should_cancel: Arc<AtomicBool>,
    should_skip: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    scan_queue_removals: Arc<Mutex<HashSet<String>>>,
    copy_queue_pending: CopyQueuePendingProbe,
) -> ScanResult {
    let mut result = ScanResult {
        scanned_paths: 0,
        found_folders: vec![],
        copied_folders: vec![],
        errors: vec![],
        deferred_for_copy_queue: false,
    };

    // Load persisted task records once for the entire scan cycle
    let cached_task_records = load_persisted_task_records(app_handle);

    let re_version = Regex::new(r"^(\d{4}_\d{2}_\d{2}_\d{2}_\d{2})\((.+)\)$").unwrap();
    let now_local = Local::now();
    let now = now_local.naive_local();
    let today = now.date();
    let yesterday = today - Duration::days(1);

    // Check Time Ranges
    if !config.time_ranges.is_empty() {
        let current_time = now_local.time();
        let mut in_range = false;
        for range in &config.time_ranges {
            let parts: Vec<&str> = range.split('-').collect();
            if parts.len() == 2 {
                if let (Ok(start), Ok(end)) = (
                    NaiveTime::parse_from_str(parts[0], "%H:%M"),
                    NaiveTime::parse_from_str(parts[1], "%H:%M"),
                ) {
                    if current_time >= start && current_time <= end {
                        in_range = true;
                        break;
                    }
                }
            }
        }

        if !in_range {
            emit_log(
                app_handle,
                format!(
                    "Current time {} is outside of configured time ranges {:?}. Skipping scan.",
                    current_time.format("%H:%M"),
                    config.time_ranges
                ),
                "info",
            );
            return result;
        }
    }

    for task in &config.tasks {
        if !task.enabled {
            continue;
        }

        if should_cancel.load(Ordering::SeqCst) {
            emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
            return result;
        }

        if defer_for_copy_queue(app_handle, &copy_queue_pending, &mut result) {
            return result;
        }

        result.scanned_paths += 1;
        emit_log(
            app_handle,
            format!("Task [{}]: Scanning {}", task.name, task.remote_path),
            "info",
        );

        let path = Path::new(&task.remote_path);
        let local_parent = if let Some(custom_local) = &task.local_path {
            Path::new(custom_local)
        } else {
            Path::new(&config.local_path)
        };

        match &task.rule {
            MatchRule::VersionMatch(target_version) => {
                let mut entries = match fs::read_dir(path).await {
                    Ok(entries) => entries,
                    Err(e) => {
                        let err_msg = format!("Failed to read {}: {}", task.remote_path, e);
                        emit_log(app_handle, err_msg.clone(), "error");
                        result.errors.push(err_msg);
                        continue;
                    }
                };

                // Collect candidates
                let mut candidates: Vec<Candidate> = Vec::new();
                let mut tree_view: Vec<String> = Vec::new();

                while let Ok(Some(entry)) = entries.next_entry().await {
                    if should_cancel.load(Ordering::SeqCst) {
                        emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                        return result;
                    }

                    let file_name = entry.file_name();
                    let name_str = file_name.to_string_lossy().to_string();

                    let mut dt = NaiveDateTime::MIN;
                    if let Some(caps) = re_version.captures(&name_str) {
                        if let Some(date_part) = caps.get(1) {
                            if let Ok(parsed) =
                                NaiveDateTime::parse_from_str(date_part.as_str(), "%Y_%m_%d_%H_%M")
                            {
                                dt = parsed;
                            }
                        }
                    }

                    candidates.push(Candidate {
                        path: entry.path(),
                        name: name_str.clone(),
                        version: if let Some(caps) = re_version.captures(&name_str) {
                            caps.get(2)
                                .map(|m| m.as_str().to_string())
                                .unwrap_or_default()
                        } else {
                            String::new()
                        },
                        datetime: dt,
                    });
                }

                // Sort
                candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datetime));

                // Tree view
                for cand in candidates.iter().take(20) {
                    tree_view.push(format!("├─ {}", cand.name));
                }
                if candidates.len() > 20 {
                    tree_view.push(format!("└─ ... ({} more files)", candidates.len() - 20));
                }
                if !tree_view.is_empty() {
                    emit_log(
                        app_handle,
                        format!("Directory structure (partial):\n{}", tree_view.join("\n")),
                        "info",
                    );
                }

                // Filter by version
                let mut version_matches: Vec<&Candidate> = candidates
                    .iter()
                    .filter(|c| c.version == *target_version)
                    .collect();

                if version_matches.is_empty() {
                    emit_log(
                        app_handle,
                        format!("No candidates found for version {}", target_version),
                        "info",
                    );
                    continue;
                }

                version_matches.sort_by_key(|candidate| std::cmp::Reverse(candidate.datetime));

                if let Some(latest) = version_matches.first() {
                    let folder_date = latest.datetime.date();
                    let local_target_path = local_parent.join(&latest.name);
                    emit_log(
                        app_handle,
                        format!(
                            "Latest candidate for {}: {} ({})",
                            target_version, latest.name, folder_date
                        ),
                        "info",
                    );

                    if folder_date == today || folder_date == yesterday {
                        if task_record_ignored_in(
                            &cached_task_records,
                            &latest.path,
                            &local_target_path,
                        ) {
                            emit_log(
                                app_handle,
                                format!("Ignored previously cancelled task: {}", latest.name),
                                "info",
                            );
                            continue;
                        }

                        if copy_was_cancelled(&task_manager, &latest.path, &local_target_path) {
                            emit_log(app_handle, cancelled_skip_message(&latest.name), "info");
                            continue;
                        }

                        if defer_for_copy_queue(app_handle, &copy_queue_pending, &mut result) {
                            return result;
                        }

                        result.found_folders.push(latest.name.clone());

                        should_skip.store(false, Ordering::SeqCst);
                        perform_copy(
                            app_handle,
                            latest.path.clone(),
                            latest.name.clone(),
                            local_parent,
                            config,
                            live_config.clone(),
                            task_manager.clone(),
                            task_runtime.clone(),
                            run_control_target.clone(),
                            should_cancel.clone(),
                            should_skip.clone(),
                            is_paused.clone(),
                            false,
                            &mut result,
                            Some(task.id.clone()),
                            None,
                            true,
                            "scheduled",
                            &config.file_extensions,
                            &config.filename_includes,
                            &cached_task_records,
                        )
                        .await;
                        should_skip.store(false, Ordering::SeqCst);
                    } else {
                        emit_log(
                            app_handle,
                            format!(
                                "Ignored {} because date {} is not Today ({}) or Yesterday ({})",
                                latest.name, folder_date, today, yesterday
                            ),
                            "info",
                        );
                    }
                }
            }
            MatchRule::DateMatch(format_str) => {
                let fmt = if format_str.trim().is_empty() {
                    "%y%m%d"
                } else {
                    format_str.trim()
                };
                let today_name = now_local.format(fmt).to_string();
                let yesterday_name = (now_local - Duration::days(1)).format(fmt).to_string();

                // Only check yesterday during the first hour of a new day (00:00–01:00),
                // to catch files that were generated near midnight but missed by the last
                // scan of the previous day. perform_copy is incremental so re-scanning
                // yesterday costs nothing once all files have already been copied.
                let is_first_hour = now_local.hour() == 0;
                let dirs_to_check: Vec<String> = if is_first_hour && yesterday_name != today_name {
                    vec![today_name.clone(), yesterday_name]
                } else {
                    vec![today_name.clone()]
                };

                emit_log(
                    app_handle,
                    format!(
                        "Checking date-based folder(s): {}",
                        dirs_to_check.join(", ")
                    ),
                    "info",
                );

                for target_name in dirs_to_check {
                    if should_cancel.load(Ordering::SeqCst) {
                        emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                        return result;
                    }

                    let target_path = path.join(&target_name);

                    if !target_path.exists() || !target_path.is_dir() {
                        emit_log(
                            app_handle,
                            format!(
                                "Folder {} does not exist in {}",
                                target_name, task.remote_path
                            ),
                            "info",
                        );
                        continue;
                    }

                    emit_log(
                        app_handle,
                        format!("Found candidate folder: {}", target_name),
                        "success",
                    );

                    let local_target_base = local_parent.join(&target_name);

                    let mut sub_entries = match fs::read_dir(&target_path).await {
                        Ok(e) => e,
                        Err(e) => {
                            let err = format!(
                                "Failed to list contents of {}: {}",
                                target_path.display(),
                                e
                            );
                            emit_log(app_handle, err.clone(), "error");
                            result.errors.push(err);
                            continue;
                        }
                    };

                    // Pass 1: Collect all subdirectories
                    let mut sub_dirs: Vec<(PathBuf, String)> = Vec::new();
                    while let Ok(Some(entry)) = sub_entries.next_entry().await {
                        let sub_path = entry.path();
                        if sub_path.is_dir() {
                            let sub_name = entry.file_name().to_string_lossy().to_string();
                            sub_dirs.push((sub_path, sub_name));
                        }
                    }

                    if sub_dirs.is_empty() {
                        emit_log(
                            app_handle,
                            format!("No build directories found in {}", target_name),
                            "info",
                        );
                        continue;
                    }

                    // Pass 2: Process each folder
                    for (sub_path, sub_name) in sub_dirs {
                        if should_cancel.load(Ordering::SeqCst) {
                            emit_log(app_handle, "Scan cancelled by user".to_string(), "info");
                            return result;
                        }

                        let local_target_path = local_target_base.join(&sub_name);
                        if task_record_ignored_in(
                            &cached_task_records,
                            &sub_path,
                            &local_target_path,
                        ) {
                            emit_log(
                                app_handle,
                                format!("Ignored previously cancelled task: {}", sub_name),
                                "info",
                            );
                            continue;
                        }

                        if copy_was_cancelled(&task_manager, &sub_path, &local_target_path) {
                            emit_log(app_handle, cancelled_skip_message(&sub_name), "info");
                            continue;
                        }

                        // Check if this folder was removed from queue
                        {
                            let removals = scan_queue_removals.lock().unwrap();
                            if removals.contains(&sub_name) {
                                emit_log(
                                    app_handle,
                                    format!(
                                        "Skipped queued folder (removed by user): {}",
                                        sub_name
                                    ),
                                    "info",
                                );
                                continue;
                            }
                        }

                        if defer_for_copy_queue(app_handle, &copy_queue_pending, &mut result) {
                            return result;
                        }

                        result
                            .found_folders
                            .push(format!("{}/{}", target_name, sub_name));

                        should_skip.store(false, Ordering::SeqCst);
                        perform_copy(
                            app_handle,
                            sub_path,
                            sub_name,
                            &local_target_base,
                            config,
                            live_config.clone(),
                            task_manager.clone(),
                            task_runtime.clone(),
                            run_control_target.clone(),
                            should_cancel.clone(),
                            should_skip.clone(),
                            is_paused.clone(),
                            false,
                            &mut result,
                            Some(task.id.clone()),
                            None,
                            true,
                            "scheduled",
                            &config.file_extensions,
                            &config.filename_includes,
                            &cached_task_records,
                        )
                        .await;
                        should_skip.store(false, Ordering::SeqCst);
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    #[test]
    fn copy_file_with_overwrite_mode_restarts_unknown_short_target() {
        let dir = tempdir().expect("temp dir");
        let source = dir.path().join("source.bin");
        let target = dir.path().join("target.bin");
        fs::write(&source, b"abcdefghij").expect("source file");
        fs::write(&target, b"WXYZ").expect("unknown partial target");

        let should_cancel = Arc::new(AtomicBool::new(false));
        let should_skip = Arc::new(AtomicBool::new(false));
        let is_paused = Arc::new(AtomicBool::new(false));
        let mut progress = 0u64;

        let copied = copy_file_with_overwrite_mode(
            &source,
            &target,
            true,
            &should_cancel,
            &should_skip,
            &is_paused,
            4,
            &mut |delta| progress += delta,
        )
        .expect("unknown partial target should be replaced safely");

        assert_eq!(copied, 10);
        assert_eq!(progress, 10);
        assert_eq!(fs::read(&target).expect("target bytes"), b"abcdefghij");
    }

    #[test]
    fn copy_file_with_overwrite_mode_resumes_managed_partial_file() {
        let dir = tempdir().expect("temp dir");
        let source = dir.path().join("source.bin");
        let target = dir.path().join("target.bin");
        let partial = partial_copy_path(&target);
        fs::write(&source, b"abcdefghij").expect("source file");
        fs::write(&partial, b"abcd").expect("managed partial target");
        let snapshot = read_copy_source_snapshot(&source).expect("source snapshot");
        write_partial_copy_metadata(&partial, &target, &snapshot).expect("partial metadata");

        let should_cancel = Arc::new(AtomicBool::new(false));
        let should_skip = Arc::new(AtomicBool::new(false));
        let is_paused = Arc::new(AtomicBool::new(false));
        let mut progress = 0u64;

        let copied = copy_file_with_overwrite_mode(
            &source,
            &target,
            true,
            &should_cancel,
            &should_skip,
            &is_paused,
            4,
            &mut |delta| progress += delta,
        )
        .expect("managed partial target should resume");

        assert_eq!(copied, 6);
        assert_eq!(progress, 6);
        assert_eq!(fs::read(&target).expect("target bytes"), b"abcdefghij");
        assert!(!partial.exists());
        assert!(!partial_copy_metadata_path(&partial).exists());
    }

    #[test]
    fn existing_task_record_does_not_block_size_mismatch_recopy() {
        let record = PersistedTaskRecord {
            folder: "Release_01".to_string(),
            source_path: "Z:/remote/Release_01".to_string(),
            local_path: "D:/target/Release_01".to_string(),
            ignored: false,
        };

        assert!(task_record_exists_in(
            std::slice::from_ref(&record),
            "Release_01",
            Path::new("D:/target/Release_01")
        ));
        assert!(should_recopy_size_mismatch(
            std::slice::from_ref(&record),
            "Release_01",
            Path::new("D:/target/Release_01")
        ));
    }
}
