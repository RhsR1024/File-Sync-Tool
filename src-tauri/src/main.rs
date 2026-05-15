// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod async_utils;
mod clipboard;
mod code_count;
mod config;
mod deploy;
mod disk_cleanup;
mod error_code;
mod fileshare;
mod history;
mod local_exec;
mod network;
mod persist;
mod scanner;
mod screenshare;
mod task_commands;
mod task_domain;
mod task_events;
mod task_manager;
mod task_persist;
mod task_runtime;
mod updater;

use config::{AppConfig, DeployServer};
use scanner::ScanResult;
use ssh2::{ExtendedData, Session};
use std::collections::{HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use serde_json::json;
use tauri::{Emitter, Manager, State, WebviewWindow, WebviewWindowBuilder, WindowEvent};

const TRAY_SHOW_ID: &str = "tray_show_main";
const TRAY_CLIPBOARD_PANEL_ID: &str = "tray_toggle_clipboard_panel";
const TRAY_QUIT_ID: &str = "tray_quit";
const MANUAL_COPY_RECOVERY_DELAY: Duration = Duration::from_secs(60);

struct AppState {
    config: Arc<Mutex<AppConfig>>,
    updater: updater::SharedUpdaterState,
    task_manager: task_manager::TaskManager,
    task_runtime: task_runtime::TaskRuntimeRegistry,
    executor_active: Arc<AtomicBool>,
    run_control_target: Arc<Mutex<Option<task_runtime::ActiveRunExecution>>>,
    is_scanning: Arc<AtomicBool>,
    is_manual_copying: Arc<AtomicBool>,
    is_manually_deploying: Arc<AtomicBool>,
    manual_copy_queue: Arc<Mutex<VecDeque<ManualCopyQueueItem>>>,
    manual_copy_keys: Arc<Mutex<HashSet<String>>>,
    manual_copy_worker_running: Arc<AtomicBool>,
    should_cancel: Arc<AtomicBool>,
    should_skip_current: Arc<AtomicBool>,
    scan_queue_removals: Arc<Mutex<HashSet<String>>>,
    is_paused: Arc<AtomicBool>,
    is_quitting: Arc<AtomicBool>,
    code_count_should_cancel: Arc<AtomicBool>,
    screen_share: Arc<screenshare::ScreenShareHandle>,
    file_share: Arc<fileshare::FileShareHandle>,
    clipboard: Arc<clipboard::ClipboardState>,
    error_code: error_code::ErrorCodeState,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct ManualCopyQueueItem {
    key: String,
    folder_name: String,
    source_path: String,
    local_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
    task_handle: Option<task_manager::TaskRunHandle>,
    trigger_source: task_domain::TaskTriggerSource,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StartManualCopyTaskRequest {
    source_path: String,
    target_root_path: String,
    #[serde(default)]
    overwrite_existing: bool,
    #[serde(default)]
    file_extensions: Vec<String>,
    #[serde(default)]
    filename_includes: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StartManualDeployBindingRequest {
    server_id: String,
    #[serde(default)]
    command_group_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct StartManualDeployTaskRequest {
    task_group_id: Option<String>,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    folder_name: String,
    local_path: String,
    remote_path: String,
    #[serde(default)]
    bindings: Vec<StartManualDeployBindingRequest>,
}

#[derive(serde::Serialize)]
struct ManualCopyQueueAck {
    folder_name: String,
    source_path: String,
    local_path: String,
    queued_ahead: usize,
}

#[derive(serde::Serialize)]
struct ManualCopyPreview {
    folder_name: String,
    source_path: String,
    local_path: String,
    resolved_target_path: String,
    source_kind: String,
    target_exists: bool,
}

struct ExecutorReservation {
    executor_active: Arc<AtomicBool>,
    category_flag: Arc<AtomicBool>,
}

impl Drop for ExecutorReservation {
    fn drop(&mut self) {
        self.category_flag.store(false, Ordering::SeqCst);
        self.executor_active.store(false, Ordering::SeqCst);
    }
}

fn try_reserve_executor(
    executor_active: Arc<AtomicBool>,
    category_flag: Arc<AtomicBool>,
) -> Option<ExecutorReservation> {
    if executor_active
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return None;
    }

    if category_flag
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        executor_active.store(false, Ordering::SeqCst);
        return None;
    }

    Some(ExecutorReservation {
        executor_active,
        category_flag,
    })
}

fn reserve_scan_executor(
    state: &AppState,
    already_running_message: &'static str,
) -> Result<ExecutorReservation, String> {
    try_reserve_executor(state.executor_active.clone(), state.is_scanning.clone()).ok_or_else(
        || {
            if state.is_manually_deploying.load(Ordering::SeqCst) {
                "Manual deploy already in progress".to_string()
            } else {
                already_running_message.to_string()
            }
        },
    )
}

#[allow(dead_code)]
fn reserve_manual_copy_executor(
    state: &AppState,
    already_running_message: &'static str,
) -> Result<ExecutorReservation, String> {
    try_reserve_executor(
        state.executor_active.clone(),
        state.is_manual_copying.clone(),
    )
    .ok_or_else(|| {
        if state.is_manually_deploying.load(Ordering::SeqCst) {
            "Manual deploy already in progress".to_string()
        } else if state.is_scanning.load(Ordering::SeqCst) {
            "Scan already in progress".to_string()
        } else {
            already_running_message.to_string()
        }
    })
}

fn reserve_manual_deploy_executor(state: &AppState) -> Result<ExecutorReservation, String> {
    try_reserve_executor(
        state.executor_active.clone(),
        state.is_manually_deploying.clone(),
    )
    .ok_or_else(|| {
        if state.is_manually_deploying.load(Ordering::SeqCst) {
            "Manual deploy already in progress".to_string()
        } else {
            "Copy or scan already in progress".to_string()
        }
    })
}

fn clear_stale_targeted_run_controls(
    active_execution: &task_runtime::ActiveRunExecution,
    run_control_target: &Arc<Mutex<Option<task_runtime::ActiveRunExecution>>>,
    should_cancel: &Arc<AtomicBool>,
    should_skip_current: Option<&Arc<AtomicBool>>,
    is_paused: &Arc<AtomicBool>,
) {
    let mut target = run_control_target.lock().unwrap();
    if matches!(target.as_ref(), Some(current) if current != active_execution) {
        should_cancel.store(false, Ordering::SeqCst);
        is_paused.store(false, Ordering::SeqCst);
        if let Some(should_skip_current) = should_skip_current {
            should_skip_current.store(false, Ordering::SeqCst);
        }
        *target = None;
    }
}

fn clear_finished_targeted_run_controls(
    finished_execution: &task_runtime::ActiveRunExecution,
    run_control_target: &Arc<Mutex<Option<task_runtime::ActiveRunExecution>>>,
    should_cancel: &Arc<AtomicBool>,
    should_skip_current: Option<&Arc<AtomicBool>>,
    is_paused: &Arc<AtomicBool>,
) {
    let mut target = run_control_target.lock().unwrap();
    if matches!(target.as_ref(), Some(current) if current == finished_execution) {
        should_cancel.store(false, Ordering::SeqCst);
        is_paused.store(false, Ordering::SeqCst);
        if let Some(should_skip_current) = should_skip_current {
            should_skip_current.store(false, Ordering::SeqCst);
        }
        *target = None;
    }
}

fn set_scan_session_controls(
    state: &AppState,
    cancel: Option<bool>,
    paused: Option<bool>,
) -> Result<(), String> {
    if !state.is_scanning.load(Ordering::SeqCst) {
        return Err("No scan or copy in progress".to_string());
    }

    *state.run_control_target.lock().unwrap() = None;
    if let Some(cancel) = cancel {
        state.should_cancel.store(cancel, Ordering::SeqCst);
    }
    if let Some(paused) = paused {
        state.is_paused.store(paused, Ordering::SeqCst);
    }

    Ok(())
}

fn set_targeted_run_controls(
    state: &AppState,
    task_group_id: &str,
    run_id: &str,
    cancel: Option<bool>,
    paused: Option<bool>,
    skip_current: Option<bool>,
) -> Result<(), String> {
    state
        .task_runtime
        .apply_if_active(task_group_id, run_id, |active| {
            *state.run_control_target.lock().unwrap() = Some(active.clone());
            if let Some(cancel) = cancel {
                state.should_cancel.store(cancel, Ordering::SeqCst);
            }
            if let Some(paused) = paused {
                state.is_paused.store(paused, Ordering::SeqCst);
            }
            if let Some(skip_current) = skip_current {
                state
                    .should_skip_current
                    .store(skip_current, Ordering::SeqCst);
            }
        })?;

    Ok(())
}

fn emit_runtime_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit(
        "log-message",
        json!({
            "msg": msg,
            "level": level,
        }),
    );
    scanner::write_log_to_file(app_handle, &msg, level);
}

fn normalize_existing_path(path: &Path) -> Result<String, String> {
    // Prefer canonicalize to resolve symlinks and relative paths. On Windows,
    // some volumes (NAS mounts, external drives with filesystems the OS can't
    // fully introspect) make canonicalize fail with OS error 1005. The purpose
    // here is only to build a stable dedup key for the manual-copy queue, so
    // fall back to the input path — existence is already validated upstream.
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let raw = resolved.to_string_lossy().replace('/', "\\");

    // Strip Windows extended-length prefixes so canonical and fallback forms
    // produce identical dedup keys.
    let stripped = if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        raw
    };

    let mut normalized = stripped;
    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }

    Ok(normalized.to_lowercase())
}

fn manual_copy_queue_key(source_path: &Path, target_root_path: &Path) -> Result<String, String> {
    Ok(format!(
        "{}=>{}",
        normalize_existing_path(source_path)?,
        normalize_existing_path(target_root_path)?
    ))
}

fn manual_copy_folder_name(source_path: &Path) -> String {
    source_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "temporary-copy".to_string())
}

fn resolve_manual_copy_target_path(
    source_path: &Path,
    target_root_path: &Path,
) -> Result<PathBuf, String> {
    if source_path.is_file() {
        let Some(file_name) = source_path.file_name() else {
            return Err("Cannot extract file name from source path".to_string());
        };
        return Ok(target_root_path.join(file_name));
    }

    Ok(target_root_path.join(manual_copy_folder_name(source_path)))
}

fn validate_manual_copy_request(
    source_path: &Path,
    target_root_path: &Path,
) -> Result<(String, String, PathBuf, String), String> {
    if source_path.as_os_str().is_empty() {
        return Err("SOURCE_PATH_REQUIRED".to_string());
    }
    if target_root_path.as_os_str().is_empty() {
        return Err("TARGET_ROOT_REQUIRED".to_string());
    }
    if !source_path.exists() {
        return Err(format!("SOURCE_NOT_FOUND::{}", source_path.display()));
    }
    if !source_path.is_dir() && !source_path.is_file() {
        return Err(format!("INVALID_SOURCE_TYPE::{}", source_path.display()));
    }
    if !target_root_path.exists() {
        return Err(format!(
            "TARGET_ROOT_NOT_FOUND::{}",
            target_root_path.display()
        ));
    }
    if !target_root_path.is_dir() {
        return Err(format!(
            "TARGET_ROOT_NOT_DIRECTORY::{}",
            target_root_path.display()
        ));
    }

    let folder_name = manual_copy_folder_name(source_path);
    let resolved_target_path = resolve_manual_copy_target_path(source_path, target_root_path)?;

    if source_path.is_file() && resolved_target_path == source_path {
        return Err(format!(
            "TARGET_SAME_AS_SOURCE::{}",
            resolved_target_path.display()
        ));
    }
    if source_path.is_file() && resolved_target_path.exists() && !resolved_target_path.is_file() {
        return Err(format!(
            "TARGET_FILE_CONFLICTS_WITH_DIRECTORY::{}",
            resolved_target_path.display()
        ));
    }
    if source_path.is_dir() && resolved_target_path.exists() && !resolved_target_path.is_dir() {
        return Err(format!(
            "TARGET_DIRECTORY_CONFLICTS_WITH_FILE::{}",
            resolved_target_path.display()
        ));
    }
    if source_path.is_dir()
        && (resolved_target_path == source_path || resolved_target_path.starts_with(source_path))
    {
        return Err(format!(
            "TARGET_INSIDE_SOURCE::{}",
            resolved_target_path.display()
        ));
    }

    Ok((
        folder_name,
        resolved_target_path.to_string_lossy().to_string(),
        resolved_target_path,
        if source_path.is_file() {
            "file".to_string()
        } else {
            "directory".to_string()
        },
    ))
}

fn build_manual_copy_start_request(
    source_path: &Path,
    target_root_path: &Path,
) -> task_manager::StartManualCopyRequest {
    build_manual_copy_start_request_with_trigger(
        source_path,
        target_root_path,
        task_domain::TaskTriggerSource::Manual,
    )
}

fn build_manual_copy_start_request_with_trigger(
    source_path: &Path,
    target_root_path: &Path,
    trigger_source: task_domain::TaskTriggerSource,
) -> task_manager::StartManualCopyRequest {
    let folder_name = manual_copy_folder_name(source_path);
    let local_target_path = resolve_manual_copy_target_path(source_path, target_root_path)
        .unwrap_or_else(|_| target_root_path.join(&folder_name));

    task_manager::StartManualCopyRequest {
        display_name: folder_name.clone(),
        folder_name,
        source_path: source_path.to_string_lossy().to_string(),
        local_target_path: local_target_path.to_string_lossy().to_string(),
        trigger_source,
    }
}

fn manual_deploy_folder_name(local_path: &str) -> String {
    Path::new(local_path.trim())
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "manual-deploy".to_string())
}

fn resolve_manual_deploy_remote_target(local_path: &str, remote_path: &str) -> String {
    let mut resolved = remote_path.trim().to_string();
    if resolved.ends_with('/') || resolved.ends_with('\\') {
        if let Some(name) = Path::new(local_path.trim()).file_name() {
            resolved = format!(
                "{}/{}",
                resolved.trim_end_matches(&['/', '\\'][..]),
                name.to_string_lossy()
            );
        }
    }

    resolved.replace('\\', "/")
}

fn resolve_manual_deploy_post_commands(
    command_group_ids: &[String],
    command_groups: &[config::CommandGroup],
) -> Vec<String> {
    let mut commands = Vec::new();
    for group_id in command_group_ids {
        if let Some(group) = command_groups.iter().find(|group| &group.id == group_id) {
            commands.extend(group.commands.iter().cloned());
        }
    }
    commands
}

fn start_manual_copy_worker(app_handle: tauri::AppHandle, state: &AppState) {
    let config = state.config.clone();
    let task_manager = state.task_manager.clone();
    let manual_copy_queue = state.manual_copy_queue.clone();
    let manual_copy_keys = state.manual_copy_keys.clone();
    let manual_copy_worker_running = state.manual_copy_worker_running.clone();
    let executor_active = state.executor_active.clone();
    let run_control_target = state.run_control_target.clone();
    let is_manual_copying = state.is_manual_copying.clone();
    let should_cancel = state.should_cancel.clone();
    let should_skip_current = state.should_skip_current.clone();
    let is_paused = state.is_paused.clone();
    let task_runtime = state.task_runtime.clone();

    tauri::async_runtime::spawn(async move {
        loop {
            let execution_reservation = loop {
                if let Some(reservation) =
                    try_reserve_executor(executor_active.clone(), is_manual_copying.clone())
                {
                    break reservation;
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                let queue_empty = manual_copy_queue.lock().unwrap().is_empty();
                if queue_empty {
                    manual_copy_worker_running.store(false, Ordering::SeqCst);
                    let queue_still_empty = manual_copy_queue.lock().unwrap().is_empty();
                    if queue_still_empty || manual_copy_worker_running.swap(true, Ordering::SeqCst)
                    {
                        return;
                    }
                }
            };

            loop {
                let next_task = { manual_copy_queue.lock().unwrap().pop_front() };
                let Some(task) = next_task else {
                    break;
                };

                should_cancel.store(false, Ordering::SeqCst);
                should_skip_current.store(false, Ordering::SeqCst);
                is_paused.store(false, Ordering::SeqCst);

                let source_path = PathBuf::from(task.source_path.trim());
                let target_root_path = PathBuf::from(task.target_root_path.trim());
                let run_handle = if let Some(handle) = task.task_handle.clone() {
                    handle
                } else {
                    match task_manager.begin_manual_copy_run(
                        build_manual_copy_start_request_with_trigger(
                            &source_path,
                            &target_root_path,
                            task.trigger_source.clone(),
                        ),
                    ) {
                        Ok(handle) => handle,
                        Err(error) => {
                            manual_copy_keys.lock().unwrap().remove(&task.key);
                            emit_runtime_log(
                                &app_handle,
                                format!("Manual copy task failed to start: {}", error),
                                "error",
                            );
                            continue;
                        }
                    }
                };
                let active_execution = match task_runtime
                    .activate(run_handle.task_group_id.clone(), run_handle.run_id.clone())
                {
                    Ok(active_execution) => active_execution,
                    Err(error) => {
                        let _ = task_manager.mark_copy_failed(
                            &run_handle.task_group_id,
                            &run_handle.run_id,
                            error.clone(),
                        );
                        let _ = task_manager.record_task_log(
                            &run_handle.task_group_id,
                            &run_handle.run_id,
                            None,
                            None,
                            "error",
                            &error,
                        );
                        manual_copy_keys.lock().unwrap().remove(&task.key);
                        emit_runtime_log(
                            &app_handle,
                            format!("Manual copy task failed to start: {}", error),
                            "error",
                        );
                        continue;
                    }
                };
                clear_stale_targeted_run_controls(
                    &active_execution,
                    &run_control_target,
                    &should_cancel,
                    Some(&should_skip_current),
                    &is_paused,
                );

                let config_snapshot = config.lock().unwrap().clone();
                let result = scanner::temporary_copy(
                    &app_handle,
                    &config_snapshot,
                    config.clone(),
                    task_manager.clone(),
                    task_runtime.clone(),
                    Some(run_handle.clone()),
                    task.source_path.clone(),
                    task.target_root_path.clone(),
                    task.overwrite_existing,
                    should_cancel.clone(),
                    should_skip_current.clone(),
                    is_paused.clone(),
                    task.file_extensions.clone(),
                    task.filename_includes.clone(),
                )
                .await;
                let _ = task_runtime.clear(&run_handle.task_group_id, &run_handle.run_id);
                clear_finished_targeted_run_controls(
                    &active_execution,
                    &run_control_target,
                    &should_cancel,
                    Some(&should_skip_current),
                    &is_paused,
                );

                if let Err(error) = result {
                    let error_lower = error.to_lowercase();
                    let state =
                        if error_lower.contains("cancelled") || error_lower.contains("skipped") {
                            "cancelled"
                        } else {
                            "failed"
                        };
                    let level = if state == "cancelled" {
                        "warn"
                    } else {
                        "error"
                    };
                    emit_runtime_log(
                        &app_handle,
                        format!("Manual copy task failed: {}", error),
                        level,
                    );
                    if state == "cancelled" {
                        manual_copy_keys.lock().unwrap().remove(&task.key);
                    } else {
                        emit_runtime_log(
                            &app_handle,
                            format!(
                                "Manual copy recovery will retry in {}s: {} -> {}",
                                MANUAL_COPY_RECOVERY_DELAY.as_secs(),
                                task.source_path,
                                task.target_root_path
                            ),
                            "warn",
                        );
                        tokio::time::sleep(MANUAL_COPY_RECOVERY_DELAY).await;
                        let mut recovery_task = task;
                        recovery_task.task_handle = None;
                        recovery_task.trigger_source = task_domain::TaskTriggerSource::Recovery;
                        manual_copy_queue.lock().unwrap().push_back(recovery_task);
                    }
                } else {
                    manual_copy_keys.lock().unwrap().remove(&task.key);
                }
            }

            drop(execution_reservation);

            manual_copy_worker_running.store(false, Ordering::SeqCst);
            let queue_has_items = !manual_copy_queue.lock().unwrap().is_empty();
            if !queue_has_items || manual_copy_worker_running.swap(true, Ordering::SeqCst) {
                break;
            }
        }
    });
}

fn restore_main_window(window: &WebviewWindow) {
    let should_center = window.current_monitor().ok().flatten().is_none();

    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_skip_taskbar(false);
    let _ = window.set_focusable(true);

    if should_center {
        let _ = window.center();
    }

    #[cfg(target_os = "windows")]
    let _ = window.set_always_on_top(true);

    let _ = window.set_focus();

    #[cfg(target_os = "windows")]
    let _ = window.set_always_on_top(false);
}

fn recreate_main_window(app: &tauri::AppHandle) {
    let Some(window_config) = app.config().app.windows.first().cloned() else {
        log::error!("Cannot recreate main window: missing window config");
        return;
    };

    // Called from main thread (via show_main_window → run_on_main_thread)
    if let Some(window) = app.get_webview_window("main") {
        restore_main_window(&window);
        return;
    }

    match WebviewWindowBuilder::from_config(app, &window_config).and_then(|builder| builder.build())
    {
        Ok(window) => {
            log::warn!("Main window was missing and has been recreated");
            restore_main_window(&window);
        }
        Err(err) => {
            log::error!("Failed to recreate main window: {err}");
        }
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    let app_clone = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_clone.get_webview_window("main") {
            restore_main_window(&window);
        } else {
            recreate_main_window(&app_clone);
        }
    });
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn should_close_to_tray(app: &tauri::AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| state.config.lock().unwrap().close_to_tray)
        .unwrap_or(false)
}

pub(crate) fn sync_launch_on_startup(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        const RUN_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        const VALUE_NAME: &str = "FileSyncToolAutoStart";

        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe_quoted = format!("\"{}\"", exe.to_string_lossy());
            let status = Command::new("reg")
                .args([
                    "add",
                    RUN_KEY,
                    "/v",
                    VALUE_NAME,
                    "/t",
                    "REG_SZ",
                    "/d",
                    &exe_quoted,
                    "/f",
                ])
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err("Failed to enable launch on startup".to_string());
            }
        } else {
            // Ignore delete failure if value does not exist.
            let _ = Command::new("reg")
                .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
                .status();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
    }

    Ok(())
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn confirm_quit(app_handle: tauri::AppHandle) {
    app_handle.exit(0);
}

#[tauri::command]
fn save_config_cmd(
    app_handle: tauri::AppHandle,
    state: State<AppState>,
    config: AppConfig,
) -> Result<(), String> {
    config::validate_config(&config)?;
    let previous = state.config.lock().unwrap().clone();
    let mut config = config::normalize_config(config);
    let server_url_changed = previous.update_server_url != config.update_server_url;
    if server_url_changed {
        config.last_update_check_at = None;
    }
    sync_launch_on_startup(config.launch_and_auto_scan || config.launch_and_auto_start_file_share)?;
    *state.config.lock().unwrap() = config.clone();
    config::save_config(&app_handle, &config)?;
    updater::commands::handle_config_changed(&app_handle, state.inner(), server_url_changed);
    Ok(())
}

#[tauri::command]
async fn scan_now(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ScanResult, String> {
    let _execution_reservation = reserve_scan_executor(state.inner(), "Scan already in progress")?;
    state.should_cancel.store(false, Ordering::SeqCst);
    state.should_skip_current.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);
    state.scan_queue_removals.lock().unwrap().clear();

    let config = state.config.lock().unwrap().clone();
    let live_config = state.config.clone();
    let result = scanner::scan_and_copy(
        &app_handle,
        &config,
        live_config,
        state.task_manager.clone(),
        state.task_runtime.clone(),
        state.run_control_target.clone(),
        state.should_cancel.clone(),
        state.should_skip_current.clone(),
        state.is_paused.clone(),
        state.scan_queue_removals.clone(),
    )
    .await;

    Ok(result)
}

#[tauri::command]
fn cancel_scan(state: State<AppState>) -> Result<(), String> {
    set_scan_session_controls(state.inner(), Some(true), Some(false))
}

#[tauri::command]
fn pause_scan(state: State<AppState>) -> Result<(), String> {
    set_scan_session_controls(state.inner(), None, Some(true))
}

#[tauri::command]
fn resume_scan(state: State<AppState>) -> Result<(), String> {
    set_scan_session_controls(state.inner(), None, Some(false))
}

#[tauri::command]
fn cancel_task_run(
    state: State<'_, AppState>,
    task_group_id: String,
    run_id: String,
) -> Result<(), String> {
    let is_active = state
        .task_runtime
        .current()
        .map(|active| active.task_group_id == task_group_id && active.run_id == run_id)
        .unwrap_or(false);

    if is_active {
        set_targeted_run_controls(
            state.inner(),
            &task_group_id,
            &run_id,
            Some(true),
            Some(false),
            None,
        )?;
        let _ = state
            .task_manager
            .request_run_cancel(&task_group_id, &run_id);
        return Ok(());
    }

    let removed_key = {
        let mut queue = state.manual_copy_queue.lock().unwrap();
        let position = queue.iter().position(|item| {
            item.task_handle
                .as_ref()
                .map(|handle| handle.task_group_id == task_group_id && handle.run_id == run_id)
                .unwrap_or(false)
        });
        position.and_then(|idx| queue.remove(idx).map(|item| item.key))
    };
    if let Some(key) = removed_key {
        state.manual_copy_keys.lock().unwrap().remove(&key);
    }

    let _ = state
        .task_manager
        .request_run_cancel(&task_group_id, &run_id);
    let _ = state
        .task_manager
        .mark_copy_cancelled(&task_group_id, &run_id);
    Ok(())
}

#[tauri::command]
fn pause_task_run(
    state: State<'_, AppState>,
    task_group_id: String,
    run_id: String,
) -> Result<(), String> {
    set_targeted_run_controls(
        state.inner(),
        &task_group_id,
        &run_id,
        None,
        Some(true),
        None,
    )?;
    let _ = state
        .task_manager
        .set_run_paused(&task_group_id, &run_id, true);
    Ok(())
}

#[tauri::command]
fn resume_task_run(
    state: State<'_, AppState>,
    task_group_id: String,
    run_id: String,
) -> Result<(), String> {
    set_targeted_run_controls(
        state.inner(),
        &task_group_id,
        &run_id,
        Some(false),
        Some(false),
        Some(false),
    )?;
    let _ = state
        .task_manager
        .set_run_paused(&task_group_id, &run_id, false);
    Ok(())
}

#[tauri::command]
fn skip_current_copy(state: State<AppState>) -> Result<(), String> {
    if !state.is_scanning.load(Ordering::SeqCst) {
        return Err("Skip current copy is only available during scan or copy".to_string());
    }
    let active = state
        .task_runtime
        .current()
        .ok_or_else(|| "No active task run".to_string())?;
    set_targeted_run_controls(
        state.inner(),
        &active.task_group_id,
        &active.run_id,
        None,
        Some(false),
        Some(true),
    )
}

#[tauri::command]
fn remove_from_scan_queue(state: State<AppState>, folder: String) {
    state.scan_queue_removals.lock().unwrap().insert(folder);
}

#[tauri::command]
async fn test_ssh_connection(server: DeployServer) -> Result<String, String> {
    deploy::check_connection(&server)
}

#[tauri::command]
async fn start_manual_deploy_task(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    request: StartManualDeployTaskRequest,
) -> Result<task_manager::TaskRunHandle, String> {
    let execution_reservation = reserve_manual_deploy_executor(state.inner())?;
    if request.bindings.is_empty() {
        return Err("At least one manual deploy binding is required".to_string());
    }

    state.should_cancel.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);

    let config_snapshot = state.config.lock().unwrap().clone();
    let folder_name = if request.folder_name.trim().is_empty() {
        manual_deploy_folder_name(&request.local_path)
    } else {
        request.folder_name.trim().to_string()
    };
    let display_name = if request.display_name.trim().is_empty() {
        folder_name.clone()
    } else {
        request.display_name.trim().to_string()
    };
    let remote_target =
        resolve_manual_deploy_remote_target(&request.local_path, &request.remote_path);

    let mut resolved_bindings = Vec::with_capacity(request.bindings.len());
    let mut targets = Vec::with_capacity(request.bindings.len());
    for binding in &request.bindings {
        let server = config_snapshot
            .servers
            .iter()
            .find(|server| server.id == binding.server_id)
            .cloned()
            .ok_or_else(|| format!("Manual deploy server not found: {}", binding.server_id))?;
        let post_commands = resolve_manual_deploy_post_commands(
            &binding.command_group_ids,
            &config_snapshot.command_groups,
        );
        targets.push(task_manager::DeployTarget {
            server_id: server.id.clone(),
            server_name: server.name.clone(),
            remote_target: remote_target.clone(),
            trigger_source: task_domain::TaskTriggerSource::Manual,
        });
        resolved_bindings.push((server, post_commands));
    }

    let task_manager = state.task_manager.clone();
    let task_runtime = state.task_runtime.clone();
    let run_handle =
        task_manager.begin_manual_deploy_run(task_manager::StartManualDeployRequest {
            task_group_id: request.task_group_id.clone(),
            display_name,
            folder_name,
            local_target_path: request.local_path.clone(),
            source_path: request.local_path.clone(),
            trigger_source: task_domain::TaskTriggerSource::Manual,
        })?;

    let tracking =
        task_manager.tracking_context(run_handle.task_group_id.clone(), run_handle.run_id.clone());
    tracking.register_targets(&targets)?;

    let active_execution =
        match task_runtime.activate(run_handle.task_group_id.clone(), run_handle.run_id.clone()) {
            Ok(active_execution) => active_execution,
            Err(error) => {
                for target in &targets {
                    let _ = task_manager.fail_attempt(
                        &run_handle.task_group_id,
                        &run_handle.run_id,
                        &target.server_id,
                        task_domain::DeployStage::Pending,
                        error.clone(),
                    );
                    let _ = task_manager.record_task_log(
                        &run_handle.task_group_id,
                        &run_handle.run_id,
                        Some(target.server_id.as_str()),
                        Some(target.server_name.as_str()),
                        "error",
                        &error,
                    );
                }
                return Err(error);
            }
        };
    clear_stale_targeted_run_controls(
        &active_execution,
        &state.run_control_target,
        &state.should_cancel,
        Some(&state.should_skip_current),
        &state.is_paused,
    );

    let app_handle_for_task = app_handle.clone();
    let app_handle_for_result = app_handle.clone();
    let local_path = request.local_path.clone();
    let remote_path = request.remote_path.clone();
    let run_handle_for_task = run_handle.clone();
    let active_execution_for_task = active_execution.clone();
    let task_runtime_for_task = task_runtime.clone();
    let task_manager_for_task = task_manager.clone();
    let run_control_target = state.run_control_target.clone();
    let should_cancel = state.should_cancel.clone();
    let should_cancel_for_cleanup = state.should_cancel.clone();
    let should_skip_current = state.should_skip_current.clone();
    let is_paused = state.is_paused.clone();
    let is_paused_for_cleanup = state.is_paused.clone();
    let targets_for_task = targets.clone();

    tauri::async_runtime::spawn(async move {
        let join_result = tauri::async_runtime::spawn_blocking(move || {
            let _execution_reservation = execution_reservation;
            let mut first_error: Option<String> = None;
            for (server, post_commands) in resolved_bindings {
                if let Err(error) = deploy::deploy_manual(
                    &app_handle_for_task,
                    &server,
                    &post_commands,
                    &local_path,
                    &remote_path,
                    should_cancel.clone(),
                    is_paused.clone(),
                    Some(tracking.clone()),
                ) {
                    if error.to_lowercase().contains("cancelled") {
                        let _ = tracking.cancel_pending();
                        return Err(error);
                    }
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
        .await;

        let _ = task_runtime_for_task.clear(
            &run_handle_for_task.task_group_id,
            &run_handle_for_task.run_id,
        );
        clear_finished_targeted_run_controls(
            &active_execution_for_task,
            &run_control_target,
            &should_cancel_for_cleanup,
            Some(&should_skip_current),
            &is_paused_for_cleanup,
        );

        match join_result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let level = if error.to_lowercase().contains("cancelled") {
                    "warn"
                } else {
                    "error"
                };
                emit_runtime_log(
                    &app_handle_for_result,
                    format!("Manual deploy task failed: {}", error),
                    level,
                );
            }
            Err(error) => {
                let message = format!("Manual deploy task panic: {}", error);
                for target in &targets_for_task {
                    let _ = task_manager_for_task.fail_attempt(
                        &run_handle_for_task.task_group_id,
                        &run_handle_for_task.run_id,
                        &target.server_id,
                        task_domain::DeployStage::Pending,
                        message.clone(),
                    );
                    let _ = task_manager_for_task.record_task_log(
                        &run_handle_for_task.task_group_id,
                        &run_handle_for_task.run_id,
                        Some(target.server_id.as_str()),
                        Some(target.server_name.as_str()),
                        "error",
                        &message,
                    );
                }
                emit_runtime_log(&app_handle_for_result, message, "error");
            }
        }
    });

    Ok(run_handle)
}

#[tauri::command]
#[allow(dead_code, non_snake_case)]
async fn manual_deploy(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    server: DeployServer,
    postCommands: Vec<String>,
    localPath: String,
    remotePath: String,
    taskGroupId: Option<String>,
) -> Result<(), String> {
    let _execution_reservation = reserve_manual_deploy_executor(state.inner())?;
    state.should_cancel.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);

    let should_cancel = state.should_cancel.clone();
    let is_paused = state.is_paused.clone();
    let task_manager = state.task_manager.clone();
    let task_runtime = state.task_runtime.clone();
    let folder_name = manual_deploy_folder_name(&localPath);
    let run_handle =
        match task_manager.begin_manual_deploy_run(task_manager::StartManualDeployRequest {
            task_group_id: taskGroupId.clone(),
            display_name: folder_name.clone(),
            folder_name: folder_name.clone(),
            local_target_path: localPath.clone(),
            source_path: localPath.clone(),
            trigger_source: task_domain::TaskTriggerSource::Manual,
        }) {
            Ok(handle) => handle,
            Err(error) => return Err(error),
        };
    let server_id = server.id.clone();
    let server_name = server.name.clone();
    let remote_target = resolve_manual_deploy_remote_target(&localPath, &remotePath);
    task_manager.register_deploy_targets(
        &run_handle.task_group_id,
        &run_handle.run_id,
        &[task_manager::DeployTarget {
            server_id: server_id.clone(),
            server_name: server_name.clone(),
            remote_target,
            trigger_source: task_domain::TaskTriggerSource::Manual,
        }],
    )?;
    let active_execution =
        match task_runtime.activate(run_handle.task_group_id.clone(), run_handle.run_id.clone()) {
            Ok(active_execution) => active_execution,
            Err(error) => {
                let _ = task_manager.fail_attempt(
                    &run_handle.task_group_id,
                    &run_handle.run_id,
                    &server_id,
                    task_domain::DeployStage::Pending,
                    error.clone(),
                );
                let _ = task_manager.record_task_log(
                    &run_handle.task_group_id,
                    &run_handle.run_id,
                    Some(server_id.as_str()),
                    Some(server_name.as_str()),
                    "error",
                    &error,
                );
                return Err(error);
            }
        };
    clear_stale_targeted_run_controls(
        &active_execution,
        &state.run_control_target,
        &state.should_cancel,
        Some(&state.should_skip_current),
        &state.is_paused,
    );
    let tracking =
        task_manager.tracking_context(run_handle.task_group_id.clone(), run_handle.run_id.clone());

    // This runs in async context, but deploy_manual uses blocking SSH.
    // We should spawn blocking.
    let join_result = tauri::async_runtime::spawn_blocking(move || {
        deploy::deploy_manual(
            &app_handle,
            &server,
            &postCommands,
            &localPath,
            &remotePath,
            should_cancel,
            is_paused,
            Some(tracking),
        )
    })
    .await;

    let _ = task_runtime.clear(&run_handle.task_group_id, &run_handle.run_id);
    clear_finished_targeted_run_controls(
        &active_execution,
        &state.run_control_target,
        &state.should_cancel,
        Some(&state.should_skip_current),
        &state.is_paused,
    );

    match join_result {
        Ok(result) => result,
        Err(error) => {
            let message = format!("Manual deploy task panic: {}", error);
            let _ = task_manager.fail_attempt(
                &run_handle.task_group_id,
                &run_handle.run_id,
                &server_id,
                task_domain::DeployStage::Pending,
                message.clone(),
            );
            let _ = task_manager.record_task_log(
                &run_handle.task_group_id,
                &run_handle.run_id,
                Some(server_id.as_str()),
                Some(server_name.as_str()),
                "error",
                &message,
            );
            Err(message)
        }
    }
}

#[tauri::command]
#[allow(dead_code)]
async fn temporary_copy(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
) -> Result<(), String> {
    let _execution_reservation =
        reserve_manual_copy_executor(state.inner(), "Operation already in progress")?;
    state.should_cancel.store(false, Ordering::SeqCst);
    state.should_skip_current.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);

    let config = state.config.lock().unwrap().clone();
    let live_config = state.config.clone();
    let should_cancel = state.should_cancel.clone();
    let is_paused = state.is_paused.clone();
    let task_manager = state.task_manager.clone();
    let task_runtime = state.task_runtime.clone();
    let run_handle = match task_manager.begin_manual_copy_run(build_manual_copy_start_request(
        Path::new(source_path.trim()),
        Path::new(target_root_path.trim()),
    )) {
        Ok(handle) => handle,
        Err(error) => return Err(error),
    };
    let active_execution =
        match task_runtime.activate(run_handle.task_group_id.clone(), run_handle.run_id.clone()) {
            Ok(active_execution) => active_execution,
            Err(error) => {
                let _ = task_manager.mark_copy_failed(
                    &run_handle.task_group_id,
                    &run_handle.run_id,
                    error.clone(),
                );
                let _ = task_manager.record_task_log(
                    &run_handle.task_group_id,
                    &run_handle.run_id,
                    None,
                    None,
                    "error",
                    &error,
                );
                return Err(error);
            }
        };
    clear_stale_targeted_run_controls(
        &active_execution,
        &state.run_control_target,
        &state.should_cancel,
        Some(&state.should_skip_current),
        &state.is_paused,
    );

    let result = scanner::temporary_copy(
        &app_handle,
        &config,
        live_config,
        task_manager,
        task_runtime.clone(),
        Some(run_handle.clone()),
        source_path,
        target_root_path,
        overwrite_existing,
        should_cancel,
        state.should_skip_current.clone(),
        is_paused,
        file_extensions,
        filename_includes,
    )
    .await;

    let _ = task_runtime.clear(&run_handle.task_group_id, &run_handle.run_id);
    clear_finished_targeted_run_controls(
        &active_execution,
        &state.run_control_target,
        &state.should_cancel,
        Some(&state.should_skip_current),
        &state.is_paused,
    );
    result
}

#[tauri::command]
async fn start_manual_copy_task(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    request: StartManualCopyTaskRequest,
) -> Result<task_manager::TaskRunHandle, String> {
    let source_path = PathBuf::from(request.source_path.trim());
    let target_root_path = PathBuf::from(request.target_root_path.trim());

    validate_manual_copy_request(&source_path, &target_root_path)?;
    let task_key = manual_copy_queue_key(&source_path, &target_root_path)?;

    {
        let mut keys = state.manual_copy_keys.lock().unwrap();
        if keys.contains(&task_key) {
            return Err(format!(
                "DUPLICATE_TASK::{} => {}",
                source_path.display(),
                target_root_path.display()
            ));
        }
        keys.insert(task_key.clone());
    }

    let run_handle =
        match state
            .task_manager
            .begin_manual_copy_run(build_manual_copy_start_request(
                &source_path,
                &target_root_path,
            )) {
            Ok(handle) => handle,
            Err(error) => {
                state.manual_copy_keys.lock().unwrap().remove(&task_key);
                return Err(error);
            }
        };

    let (folder_name, local_path, _, _) =
        validate_manual_copy_request(&source_path, &target_root_path)?;

    state
        .manual_copy_queue
        .lock()
        .unwrap()
        .push_back(ManualCopyQueueItem {
            key: task_key,
            folder_name,
            source_path: source_path.to_string_lossy().to_string(),
            local_path,
            target_root_path: target_root_path.to_string_lossy().to_string(),
            overwrite_existing: request.overwrite_existing,
            file_extensions: request.file_extensions,
            filename_includes: request.filename_includes,
            task_handle: Some(run_handle.clone()),
            trigger_source: task_domain::TaskTriggerSource::Manual,
        });

    emit_runtime_log(
        &app_handle,
        format!(
            "Manual copy task queued: {} -> {}",
            source_path.display(),
            target_root_path.display()
        ),
        "info",
    );

    if !state
        .manual_copy_worker_running
        .swap(true, Ordering::SeqCst)
    {
        start_manual_copy_worker(app_handle, state.inner());
    }

    Ok(run_handle)
}

#[tauri::command]
async fn queue_temporary_copy(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
) -> Result<ManualCopyQueueAck, String> {
    let source_path = PathBuf::from(source_path.trim());
    let target_root_path = PathBuf::from(target_root_path.trim());

    let (folder_name, local_path, _, _) =
        validate_manual_copy_request(&source_path, &target_root_path)?;

    let task_key = manual_copy_queue_key(&source_path, &target_root_path)?;

    {
        let mut keys = state.manual_copy_keys.lock().unwrap();
        if keys.contains(&task_key) {
            return Err(format!(
                "DUPLICATE_TASK::{} => {}",
                source_path.display(),
                target_root_path.display()
            ));
        }
        keys.insert(task_key.clone());
    }

    let run_handle =
        match state
            .task_manager
            .begin_manual_copy_run(build_manual_copy_start_request(
                &source_path,
                &target_root_path,
            )) {
            Ok(handle) => handle,
            Err(error) => {
                state.manual_copy_keys.lock().unwrap().remove(&task_key);
                return Err(error);
            }
        };

    let queued_ahead = state.manual_copy_queue.lock().unwrap().len()
        + usize::from(state.manual_copy_worker_running.load(Ordering::SeqCst));

    state
        .manual_copy_queue
        .lock()
        .unwrap()
        .push_back(ManualCopyQueueItem {
            key: task_key,
            folder_name: folder_name.clone(),
            source_path: source_path.to_string_lossy().to_string(),
            local_path: local_path.clone(),
            target_root_path: target_root_path.to_string_lossy().to_string(),
            overwrite_existing,
            file_extensions,
            filename_includes,
            task_handle: Some(run_handle),
            trigger_source: task_domain::TaskTriggerSource::Manual,
        });

    emit_runtime_log(
        &app_handle,
        format!(
            "Manual copy task queued: {} -> {}",
            source_path.display(),
            target_root_path.display()
        ),
        "info",
    );

    if !state
        .manual_copy_worker_running
        .swap(true, Ordering::SeqCst)
    {
        start_manual_copy_worker(app_handle, state.inner());
    }

    Ok(ManualCopyQueueAck {
        folder_name,
        source_path: source_path.to_string_lossy().to_string(),
        local_path,
        queued_ahead,
    })
}

#[tauri::command]
async fn preview_temporary_copy(
    source_path: String,
    target_root_path: String,
) -> Result<ManualCopyPreview, String> {
    let source_path = PathBuf::from(source_path.trim());
    let target_root_path = PathBuf::from(target_root_path.trim());
    let (folder_name, local_path, resolved_target_path, source_kind) =
        validate_manual_copy_request(&source_path, &target_root_path)?;

    Ok(ManualCopyPreview {
        folder_name,
        source_path: source_path.to_string_lossy().to_string(),
        local_path,
        resolved_target_path: resolved_target_path.to_string_lossy().to_string(),
        source_kind,
        target_exists: resolved_target_path.exists(),
    })
}

#[tauri::command]
fn get_app_paths(app_handle: tauri::AppHandle) -> (String, String) {
    let config = config::get_config_path(&app_handle)
        .to_string_lossy()
        .to_string();
    let log = config::get_log_path(&app_handle)
        .to_string_lossy()
        .to_string();
    (config, log)
}

#[tauri::command]
fn get_custom_data_dir(app_handle: tauri::AppHandle) -> String {
    config::get_custom_data_dir(&app_handle)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[tauri::command]
fn set_custom_data_dir(
    app_handle: tauri::AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<(), String> {
    // Validate non-empty path is a valid directory
    if !path.is_empty() {
        let p = std::path::PathBuf::from(&path);
        if !p.is_dir() {
            return Err(format!("Directory does not exist: {}", path));
        }
    }

    {
        let conn = state.clipboard.write_db.lock();
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    config::set_custom_data_dir(&app_handle, path)?;

    // Hot-reload config from the new location into AppState
    let new_config = config::load_config(&app_handle);
    *state.config.lock().unwrap() = new_config;

    Ok(())
}

#[tauri::command]
fn open_path_parent(path: String) -> Result<(), String> {
    let raw = PathBuf::from(path);
    let target_dir = if raw.is_dir() {
        raw
    } else {
        raw.parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Invalid path: no parent directory".to_string())?
    };

    if !target_dir.exists() {
        return Err(format!(
            "Directory does not exist: {}",
            target_dir.display()
        ));
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(target_dir.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(target_dir.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(target_dir.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_url_via_shell_execute(url: &str) -> Result<(), String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide_url: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide_url.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    if result.0 as usize <= 32 {
        return Err(format!(
            "ShellExecuteW failed with code {}",
            result.0 as usize
        ));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn open_url_windows(url: &str) -> Result<(), String> {
    open_url_via_shell_execute(url)
}

#[cfg(all(target_os = "windows", test))]
fn open_url_windows_with<CmdStart, ShellExecute>(
    url: &str,
    _cmd_start: CmdStart,
    shell_execute: ShellExecute,
) -> Result<(), String>
where
    CmdStart: FnOnce(&str) -> Result<(), String>,
    ShellExecute: FnOnce(&str) -> Result<(), String>,
{
    shell_execute(url)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        open_url_windows(&url)?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

type MainThreadDialogTask = Box<dyn FnOnce() + Send + 'static>;

fn schedule_dialog_task<T, Schedule, Run>(
    schedule: Schedule,
    run: Run,
) -> Result<tokio::sync::oneshot::Receiver<Result<T, String>>, String>
where
    T: Send + 'static,
    Schedule: FnOnce(MainThreadDialogTask) -> Result<(), String>,
    Run: FnOnce() -> Result<T, String> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    schedule(Box::new(move || {
        let _ = tx.send(run());
    }))?;
    Ok(rx)
}

pub(crate) async fn run_dialog_task_on_main_thread<T, Run>(
    window: &WebviewWindow,
    run: Run,
) -> Result<T, String>
where
    T: Send + 'static,
    Run: FnOnce() -> Result<T, String> + Send + 'static,
{
    let rx = schedule_dialog_task(
        |task| {
            window
                .run_on_main_thread(task)
                .map_err(|error| format!("MAIN_THREAD_DIALOG_DISPATCH_FAILED::{}", error))
        },
        run,
    )?;

    rx.await
        .map_err(|_| "MAIN_THREAD_DIALOG_RESULT_DROPPED".to_string())?
}

pub(crate) async fn pick_directory_on_main_thread_with<Schedule, Pick>(
    schedule: Schedule,
    pick: Pick,
) -> Result<Option<String>, String>
where
    Schedule: FnOnce(MainThreadDialogTask) -> Result<(), String>,
    Pick: FnOnce() -> Option<PathBuf> + Send + 'static,
{
    let rx = schedule_dialog_task(schedule, move || {
        Ok(pick().map(|path| path.to_string_lossy().to_string()))
    })?;

    rx.await
        .map_err(|_| "MAIN_THREAD_DIALOG_RESULT_DROPPED".to_string())?
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[cfg(target_os = "windows")]
    use super::open_url_windows_with;
    use super::pick_directory_on_main_thread_with;
    use super::schedule_dialog_task;
    use super::{
        appliance_ssh_api_port, build_appliance_ssh_api_url, build_iptables_whitelist_rule,
        ApplianceSshApiVersion, ApplianceSshWhitelistScope,
    };
    #[cfg(target_os = "windows")]
    use std::cell::Cell;

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_open_url_prefers_shell_execute_without_cmd() {
        let cmd_called = Cell::new(false);
        let shell_called = Cell::new(false);

        let result = open_url_windows_with(
            "https://example.com",
            |_| {
                cmd_called.set(true);
                Ok(())
            },
            |_| {
                shell_called.set(true);
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert!(
            !cmd_called.get(),
            "Windows URL open should not go through cmd /C start"
        );
        assert!(
            shell_called.get(),
            "Windows URL open should go through ShellExecute"
        );
    }

    #[test]
    fn schedules_dialog_task_on_main_thread_and_returns_result() {
        let receiver = schedule_dialog_task(
            |task| {
                task();
                Ok(())
            },
            || Ok(Some("C:\\selected".to_string())),
        )
        .expect("dialog task should be scheduled");

        let result = tauri::async_runtime::block_on(receiver)
            .expect("dialog task should send a result")
            .expect("dialog task should succeed");

        assert_eq!(result, Some("C:\\selected".to_string()));
    }

    #[test]
    fn schedule_dialog_task_reports_dispatch_failure() {
        let result = schedule_dialog_task::<Option<String>, _, _>(
            |_task| Err("main thread unavailable".to_string()),
            || Ok(Some("C:\\selected".to_string())),
        );

        assert_eq!(result.unwrap_err(), "main thread unavailable");
    }

    #[test]
    fn pick_directory_on_main_thread_maps_selected_path() {
        let result = tauri::async_runtime::block_on(pick_directory_on_main_thread_with(
            |task| {
                task();
                Ok(())
            },
            || Some(PathBuf::from(r"C:\selected\new-folder")),
        ))
        .expect("directory picker should succeed");

        assert_eq!(result, Some(r"C:\selected\new-folder".to_string()));
    }

    #[test]
    fn pick_directory_on_main_thread_reports_dispatch_failure() {
        let result = tauri::async_runtime::block_on(pick_directory_on_main_thread_with(
            |_task| Err("main thread unavailable".to_string()),
            || Some(PathBuf::from(r"C:\selected\new-folder")),
        ));

        assert_eq!(result.unwrap_err(), "main thread unavailable");
    }

    #[test]
    fn appliance_ssh_api_version_defaults_to_componentized_port() {
        assert_eq!(
            appliance_ssh_api_port(ApplianceSshApiVersion::default()),
            23006
        );
        assert_eq!(
            appliance_ssh_api_port(ApplianceSshApiVersion::Componentized),
            23006
        );
    }

    #[test]
    fn appliance_ssh_api_version_uses_mainline_port() {
        assert_eq!(
            appliance_ssh_api_port(ApplianceSshApiVersion::Mainline),
            9007
        );
    }

    #[test]
    fn appliance_ssh_api_url_uses_selected_version_port() {
        assert_eq!(
            build_appliance_ssh_api_url(
                "192.168.1.10",
                ApplianceSshApiVersion::Componentized,
                "get"
            ),
            "http://192.168.1.10:23006/openAPI/system/v1/network/SSH/get"
        );
        assert_eq!(
            build_appliance_ssh_api_url("192.168.1.10", ApplianceSshApiVersion::Mainline, "set"),
            "http://192.168.1.10:9007/openAPI/system/v1/network/SSH/set"
        );
    }

    #[test]
    fn appliance_ssh_all_tcp_whitelist_rule_omits_destination_port() {
        let rule = build_iptables_whitelist_rule(
            "192.115.1.15",
            23333,
            ApplianceSshWhitelistScope::AllTcp,
        );

        assert!(rule.contains("-p tcp -s 192.115.1.15 -j ACCEPT"));
        assert!(!rule.contains("--dport"));
    }

    #[test]
    fn appliance_ssh_ssh_only_whitelist_rule_targets_reported_ssh_port() {
        let rule = build_iptables_whitelist_rule(
            "192.115.1.15",
            23333,
            ApplianceSshWhitelistScope::SshOnly,
        );

        assert!(rule.contains("-p tcp -s 192.115.1.15 --dport 23333 -j ACCEPT"));
    }
}

#[tauri::command]
async fn open_directory(window: WebviewWindow) -> Result<Option<String>, String> {
    // Windows shell folder dialogs are sensitive to COM apartment ownership
    // when users create a folder inside the dialog. Run the sync dialog on
    // Tauri's main UI thread instead of a transient async worker thread.
    pick_directory_on_main_thread_with(
        |task| {
            window
                .run_on_main_thread(task)
                .map_err(|error| format!("MAIN_THREAD_DIALOG_DISPATCH_FAILED::{}", error))
        },
        || rfd::FileDialog::new().pick_folder(),
    )
    .await
}

#[tauri::command]
async fn save_text_file(
    content: String,
    default_file_name: String,
    filter_name: String,
    extensions: Vec<String>,
) -> Result<Option<String>, String> {
    // Save dialogs do not exercise the folder-picker confirmation path fixed
    // above, so keep the non-blocking async save dialog here.
    let extension_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let mut dialog = rfd::AsyncFileDialog::new().set_file_name(&default_file_name);

    if !extension_refs.is_empty() {
        dialog = dialog.add_filter(&filter_name, &extension_refs);
    }

    let Some(handle) = dialog.save_file().await else {
        return Ok(None);
    };

    let mut target_path = handle.path().to_path_buf();

    if target_path.extension().is_none() {
        if let Some(default_extension) = extensions.first() {
            target_path.set_extension(default_extension);
        }
    }

    std::fs::write(&target_path, content).map_err(|e| e.to_string())?;

    Ok(Some(target_path.to_string_lossy().to_string()))
}

#[allow(non_snake_case)]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PasswordChangeResult {
    pub ip: String,
    pub success: bool,
    pub message: String,
    pub failedAt: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplianceSshTarget {
    pub ip: String,
    #[serde(default)]
    pub jump_host: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ApplianceSshApiVersion {
    #[default]
    Componentized,
    Mainline,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ApplianceSshWhitelistScope {
    #[default]
    SshOnly,
    AllTcp,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplianceSshRequest {
    /// Preferred path: explicit list of targets with optional per-target jump host.
    /// `ips` is accepted for backward compatibility and merged into `targets`.
    #[serde(default)]
    pub targets: Vec<ApplianceSshTarget>,
    #[serde(default)]
    pub ips: Vec<String>,
    #[serde(default)]
    pub appliance_version: ApplianceSshApiVersion,
    #[serde(default)]
    pub ssh_username: String,
    #[serde(default)]
    pub ssh_password: String,
    #[serde(default)]
    pub add_whitelist_rule: bool,
    #[serde(default)]
    pub whitelist_scope: ApplianceSshWhitelistScope,
    /// When Some, use this CIDR (e.g., "10.0.0.0/24") as the whitelist source
    /// instead of auto-detecting the local IP.
    #[serde(default)]
    pub whitelist_cidr: Option<String>,
    /// When true, `jump_host_username` / `jump_host_password` are used for SSH
    /// to the jump host instead of the main `ssh_username` / `ssh_password`.
    #[serde(default)]
    pub jump_host_use_separate_creds: bool,
    #[serde(default)]
    pub jump_host_username: Option<String>,
    #[serde(default)]
    pub jump_host_password: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplianceSshResult {
    pub ip: String,
    pub success: bool,
    pub message: String,
    pub previous_enable: Option<u8>,
    pub current_enable: Option<u8>,
    pub port: Option<u16>,
    pub whitelist_source_ip: Option<String>,
    pub whitelist_applied: Option<bool>,
    #[serde(default)]
    pub jump_host: Option<String>,
}

#[derive(serde::Deserialize, Clone, Debug)]
struct ApplianceSshStatusData {
    enable: Option<u8>,
    port: Option<u16>,
}

#[derive(serde::Deserialize, Debug)]
struct ApplianceSshStatusResponse {
    code: i64,
    message: Option<String>,
    data: Option<ApplianceSshStatusData>,
}

const APPLIANCE_SSH_COMPONENTIZED_API_PORT: u16 = 23006;
const APPLIANCE_SSH_MAINLINE_API_PORT: u16 = 9007;

fn appliance_ssh_api_port(version: ApplianceSshApiVersion) -> u16 {
    match version {
        ApplianceSshApiVersion::Componentized => APPLIANCE_SSH_COMPONENTIZED_API_PORT,
        ApplianceSshApiVersion::Mainline => APPLIANCE_SSH_MAINLINE_API_PORT,
    }
}

fn build_appliance_ssh_api_url(ip: &str, version: ApplianceSshApiVersion, action: &str) -> String {
    format!(
        "http://{}:{}/openAPI/system/v1/network/SSH/{}",
        ip,
        appliance_ssh_api_port(version),
        action
    )
}

// Helper function to validate IP address format
fn validate_ip(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| part.parse::<u8>().is_ok())
}

// Validate an IPv4 CIDR like "192.168.1.0/24". Prefix must be 0-32.
fn validate_cidr(value: &str) -> bool {
    let Some((addr, prefix)) = value.split_once('/') else {
        return false;
    };
    if !validate_ip(addr) {
        return false;
    }
    matches!(prefix.parse::<u8>(), Ok(p) if p <= 32)
}

const DEVICE_BATCH_CONCURRENCY_LIMIT: usize = 4;
const DEVICE_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

fn build_device_http_client_with_timeout(
    request_timeout: Duration,
) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(DEVICE_HTTP_CONNECT_TIMEOUT)
        .timeout(request_timeout)
        .build()
        .map_err(|e| format!("Failed to create device HTTP client: {}", e))
}

async fn get_appliance_ssh_status(
    client: &reqwest::Client,
    ip: &str,
    version: ApplianceSshApiVersion,
) -> Result<ApplianceSshStatusData, String> {
    let request_url = build_appliance_ssh_api_url(ip, version, "get");
    let response = client
        .post(&request_url)
        .header("content-type", "application/json")
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| format!("POST request failed: {}", e))?;
    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    let trimmed_text = response_text.trim();

    if !status.is_success() {
        return Err(if trimmed_text.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {}", status.as_u16(), trimmed_text)
        });
    }

    let parsed = serde_json::from_str::<ApplianceSshStatusResponse>(trimmed_text)
        .map_err(|e| format!("Response parse error: {}", e))?;

    if parsed.code != 0 {
        return Err(parsed
            .message
            .unwrap_or_else(|| format!("API returned code {}", parsed.code)));
    }

    parsed
        .data
        .ok_or_else(|| "Response missing data".to_string())
}

async fn enable_appliance_ssh_via_api(
    client: &reqwest::Client,
    ip: &str,
    version: ApplianceSshApiVersion,
) -> Result<(), String> {
    let request_url = build_appliance_ssh_api_url(ip, version, "set");
    let request_body = json!({
        "ServiceSshdEnable": 1
    });

    let response = client
        .post(&request_url)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("SET request failed: {}", e))?;
    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read SET response body: {}", e))?;
    let trimmed_text = response_text.trim();

    if !status.is_success() {
        return Err(if trimmed_text.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {}", status.as_u16(), trimmed_text)
        });
    }

    if !trimmed_text.is_empty() {
        if let Ok(parsed_json) = serde_json::from_str::<serde_json::Value>(trimmed_text) {
            if parsed_json
                .get("code")
                .and_then(|value| value.as_i64())
                .is_some_and(|code| code != 0)
            {
                let api_message = parsed_json
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or(trimmed_text);
                return Err(api_message.to_string());
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
enum WaitForEnableOutcome {
    Enabled(ApplianceSshStatusData),
    NotEnabled { last_status: ApplianceSshStatusData },
    GetFailed { last_error: String },
}

async fn wait_for_appliance_ssh_enabled(
    client: &reqwest::Client,
    ip: &str,
    version: ApplianceSshApiVersion,
    attempts: usize,
    delay: Duration,
) -> WaitForEnableOutcome {
    let mut last_status: Option<ApplianceSshStatusData> = None;
    let mut last_get_error: Option<String> = None;

    for attempt in 0..attempts {
        match get_appliance_ssh_status(client, ip, version).await {
            Ok(status) => {
                if status.enable == Some(1) {
                    return WaitForEnableOutcome::Enabled(status);
                }
                last_status = Some(status);
            }
            Err(e) => {
                last_get_error = Some(e);
            }
        }

        if attempt + 1 < attempts {
            tokio::time::sleep(delay).await;
        }
    }

    if let Some(status) = last_status {
        WaitForEnableOutcome::NotEnabled {
            last_status: status,
        }
    } else {
        WaitForEnableOutcome::GetFailed {
            last_error: last_get_error
                .unwrap_or_else(|| "no GET attempts were made".to_string()),
        }
    }
}

fn detect_local_source_ip(ip: &str, port: u16) -> Result<String, String> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind local UDP socket: {}", e))?;
    socket
        .connect(format!("{}:{}", ip, port))
        .map_err(|e| format!("Failed to determine local route: {}", e))?;

    match socket
        .local_addr()
        .map_err(|e| format!("Failed to read local socket address: {}", e))?
        .ip()
    {
        std::net::IpAddr::V4(addr) => Ok(addr.to_string()),
        std::net::IpAddr::V6(addr) => Err(format!(
            "Resolved local source IP is IPv6 ({}), but the whitelist rule requires IPv4",
            addr
        )),
    }
}

fn build_iptables_whitelist_rule(
    source_ip: &str,
    port: u16,
    scope: ApplianceSshWhitelistScope,
) -> String {
    match scope {
        ApplianceSshWhitelistScope::AllTcp => {
            format!(
                "iptables -C INPUT -p tcp -s {source_ip} -j ACCEPT || iptables -I INPUT 1 -p tcp -s {source_ip} -j ACCEPT"
            )
        }
        ApplianceSshWhitelistScope::SshOnly => {
            format!(
                "iptables -C INPUT -p tcp -s {source_ip} --dport {port} -j ACCEPT || iptables -I INPUT 1 -p tcp -s {source_ip} --dport {port} -j ACCEPT"
            )
        }
    }
}

fn build_iptables_whitelist_command(
    source_ip: &str,
    port: u16,
    scope: ApplianceSshWhitelistScope,
) -> String {
    let rule = build_iptables_whitelist_rule(source_ip, port, scope);
    format!("sh -lc '{rule}'")
}

fn describe_whitelist_scope(scope: ApplianceSshWhitelistScope, port: u16) -> String {
    match scope {
        ApplianceSshWhitelistScope::AllTcp => "all TCP ports".to_string(),
        ApplianceSshWhitelistScope::SshOnly => format!("SSH port {port}"),
    }
}

/// Default SSH port assumed for targets reached via jump host (the REST API
/// that reports the real port is only available on direct targets).
const JUMP_HOST_DEFAULT_TARGET_SSH_PORT: u16 = 23333;

/// Build a command to be executed on the jump host that (1) opens the
/// idempotent iptables whitelist locally on A so future user→A SSH attempts
/// pass A's firewall, and (2) SSHes into the target B to apply the same rule.
/// Appliance master/backup pairs come pre-provisioned with passwordless SSH
/// (key-based or host-based auth) between each other, so no password is
/// passed here; `BatchMode=yes` makes ssh fail fast if interactive auth would
/// be required instead of hanging. `source` may be a single IPv4 address or
/// a CIDR.
fn build_nested_iptables_whitelist_command(
    target_user: &str,
    target_ip: &str,
    target_port: u16,
    iptables_source: &str,
    iptables_port: u16,
    scope: ApplianceSshWhitelistScope,
) -> String {
    let rule = build_iptables_whitelist_rule(iptables_source, iptables_port, scope);
    format!(
        "({rule}) && ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 \
-p {target_port} {target_user}@{target_ip} '{rule}'"
    )
}

fn run_remote_command_over_ssh(
    ip: &str,
    port: u16,
    username: &str,
    password: &str,
    command: &str,
) -> Result<RemoteCommandResult, String> {
    // Some hardened appliances ship sshd with `ForceCommand` or a restricted
    // login shell that rejects non-interactive `exec` channels with messages
    // like "Remote command execution is not allowed.". Try plain exec first
    // (the standard, lower-overhead path); on a restriction-shaped failure,
    // retry with a PTY and finally with a real interactive shell.
    match exec_over_ssh(ip, port, username, password, command, false) {
        Ok(output) => Ok(RemoteCommandResult {
            output,
            mode: "exec",
        }),
        Err(exec_error) if exec_restriction_hint(&exec_error) => {
            match exec_over_ssh(ip, port, username, password, command, true) {
                Ok(output) => Ok(RemoteCommandResult {
                    output,
                    mode: "exec+pty",
                }),
                Err(pty_error) if exec_restriction_hint(&pty_error) => {
                    match exec_shell_over_ssh(ip, port, username, password, command) {
                        Ok(output) => Ok(RemoteCommandResult {
                            output,
                            mode: "shell",
                        }),
                        Err(shell_error) => Err(format!(
                            "exec failed: {}; exec+pty failed: {}; shell failed: {}",
                            exec_error, pty_error, shell_error
                        )),
                    }
                }
                Err(pty_error) => Err(format!(
                    "exec failed: {}; exec+pty failed: {}",
                    exec_error, pty_error
                )),
            }
        }
        Err(e) => Err(e),
    }
}

struct RemoteCommandResult {
    output: String,
    mode: &'static str,
}

fn exec_restriction_hint(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("not allowed")
        || lower.contains("not permitted")
        || lower.contains("forbidden")
        || lower.contains("restricted")
}

fn connect_ssh_session(
    ip: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<Session, String> {
    let socket_addr: SocketAddr = format!("{}:{}", ip, port)
        .parse()
        .map_err(|e| format!("Invalid SSH address: {}", e))?;
    let tcp = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(10))
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(15)));

    let mut sess = Session::new().map_err(|e| format!("SSH session init failed: {}", e))?;
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;
    sess.userauth_password(username, password)
        .map_err(|e| format!("SSH authentication failed: {}", e))?;

    if !sess.authenticated() {
        return Err("SSH authentication failed".to_string());
    }

    Ok(sess)
}

fn exec_over_ssh(
    ip: &str,
    port: u16,
    username: &str,
    password: &str,
    command: &str,
    request_pty: bool,
) -> Result<String, String> {
    let sess = connect_ssh_session(ip, port, username, password)?;
    let mut channel = sess
        .channel_session()
        .map_err(|e| format!("SSH channel init failed: {}", e))?;
    if request_pty {
        channel
            .request_pty("xterm", None, None)
            .map_err(|e| format!("SSH PTY allocation failed: {}", e))?;
    }
    channel
        .handle_extended_data(ExtendedData::Merge)
        .map_err(|e| format!("SSH channel stderr merge failed: {}", e))?;
    channel
        .exec(command)
        .map_err(|e| format!("Remote command execution failed: {}", e))?;
    channel
        .send_eof()
        .map_err(|e| format!("Failed to close SSH stdin: {}", e))?;

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| format!("Failed to read remote command output: {}", e))?;
    channel
        .wait_close()
        .map_err(|e| format!("Failed to close SSH channel: {}", e))?;

    let exit_code = channel.exit_status().unwrap_or(-1);
    if exit_code != 0 {
        let trimmed_output = output.trim();
        return Err(if trimmed_output.is_empty() {
            format!("Remote command exited with code {}", exit_code)
        } else {
            format!(
                "Remote command exited with code {}: {}",
                exit_code, trimmed_output
            )
        });
    }

    Ok(output.trim().to_string())
}

fn exec_shell_over_ssh(
    ip: &str,
    port: u16,
    username: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
    let sess = connect_ssh_session(ip, port, username, password)?;
    let mut channel = sess
        .channel_session()
        .map_err(|e| format!("SSH channel init failed: {}", e))?;
    channel
        .request_pty("xterm", None, None)
        .map_err(|e| format!("SSH PTY allocation failed: {}", e))?;
    channel
        .handle_extended_data(ExtendedData::Merge)
        .map_err(|e| format!("SSH channel stderr merge failed: {}", e))?;
    channel
        .shell()
        .map_err(|e| format!("SSH shell start failed: {}", e))?;

    let wrapped = format!("{command}\nprintf '\\n__FST_EXIT__%s\\n' \"$?\"\nexit\n");
    channel
        .write_all(wrapped.as_bytes())
        .map_err(|e| format!("Failed to write remote shell command: {}", e))?;
    channel
        .send_eof()
        .map_err(|e| format!("Failed to close SSH shell stdin: {}", e))?;

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| format!("Failed to read remote shell output: {}", e))?;
    channel
        .wait_close()
        .map_err(|e| format!("Failed to close SSH shell channel: {}", e))?;

    let Some(marker_index) = output.rfind("__FST_EXIT__") else {
        return Err(format!(
            "Remote shell did not report command exit status: {}",
            output.trim()
        ));
    };
    let status_text = output[marker_index + "__FST_EXIT__".len()..]
        .lines()
        .next()
        .unwrap_or("")
        .trim();
    let exit_code = status_text
        .parse::<i32>()
        .map_err(|_| format!("Remote shell reported invalid exit status: {}", status_text))?;
    let command_output = output[..marker_index].trim().to_string();

    if exit_code != 0 {
        return Err(if command_output.is_empty() {
            format!("Remote command exited with code {}", exit_code)
        } else {
            format!(
                "Remote command exited with code {}: {}",
                exit_code, command_output
            )
        });
    }

    Ok(command_output)
}

/// SHA-256 hash of the given text, returned as lowercase hex.
fn sha256_hex(plaintext: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn password_change_failure(ip: &str, message: String, failed_at: &str) -> PasswordChangeResult {
    PasswordChangeResult {
        ip: ip.to_string(),
        success: false,
        message,
        failedAt: Some(failed_at.to_string()),
    }
}

async fn change_framework_password_for_ip(
    client: reqwest::Client,
    ip_input: String,
    old_passwd: String,
    new_passwd: String,
) -> Option<PasswordChangeResult> {
    let ip = ip_input.trim().to_string();
    if ip.is_empty() {
        return None;
    }

    if !validate_ip(&ip) {
        return Some(password_change_failure(
            &ip,
            format!("Invalid IP address: {}", ip),
            "login",
        ));
    }

    let hashed_old = sha256_hex(&old_passwd);
    let hashed_new = sha256_hex(&new_passwd);

    let login_url = format!("http://{}:21900/openAPI/userMgr/v1/login", ip);
    let login_body = json!({
        "userName": "admin",
        "userPasswd": hashed_old,
        "isUnlockLogin": false
    });

    let token = match client
        .post(&login_url)
        .header("Authorization", "")
        .header("content-type", "application/json")
        .json(&login_body)
        .send()
        .await
    {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                    match json
                        .get("data")
                        .and_then(|d| d.get("token"))
                        .and_then(|t| t.as_str())
                    {
                        Some(token) => token.to_string(),
                        None => {
                            return Some(password_change_failure(
                                &ip,
                                "Login response missing token".to_string(),
                                "login",
                            ));
                        }
                    }
                } else {
                    let msg = json
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");
                    return Some(password_change_failure(
                        &ip,
                        format!("Login failed: {}", msg),
                        "login",
                    ));
                }
            }
            Err(e) => {
                return Some(password_change_failure(
                    &ip,
                    format!("Login response parse error: {}", e),
                    "login",
                ));
            }
        },
        Err(e) => {
            return Some(password_change_failure(
                &ip,
                format!("Login request failed: {}", e),
                "login",
            ));
        }
    };

    let change_passwd_url = format!("http://{}:21900/openAPI/userMgr/v1/changePasswd", ip);
    let change_passwd_body = json!({
        "userName": "admin",
        "oldUserPasswd": hashed_old,
        "newUserPasswd": hashed_new
    });

    let change_success = match client
        .post(&change_passwd_url)
        .header("Authorization", &token)
        .header("content-type", "application/json")
        .json(&change_passwd_body)
        .send()
        .await
    {
        Ok(response) => match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                    true
                } else {
                    let msg = json
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error");
                    return Some(password_change_failure(
                        &ip,
                        format!("Change password failed: {}", msg),
                        "changePasswd",
                    ));
                }
            }
            Err(e) => {
                return Some(password_change_failure(
                    &ip,
                    format!("Change password response parse error: {}", e),
                    "changePasswd",
                ));
            }
        },
        Err(e) => {
            return Some(password_change_failure(
                &ip,
                format!("Change password request failed: {}", e),
                "changePasswd",
            ));
        }
    };

    if !change_success {
        return None;
    }

    let logout_url = format!("http://{}:21900/openAPI/userMgr/v1/logout", ip);
    let logout_body = json!({
        "userName": "admin",
        "userPasswd": hashed_old,
        "token": token
    });

    let _ = client
        .post(&logout_url)
        .header("Authorization", &token)
        .header("content-type", "application/json")
        .json(&logout_body)
        .send()
        .await;

    Some(PasswordChangeResult {
        ip,
        success: true,
        message: "Success".to_string(),
        failedAt: None,
    })
}

#[tauri::command]
async fn change_framework_password(
    ips: Vec<String>,
    old_password: Option<String>,
    new_password: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PasswordChangeResult>, String> {
    let old_passwd = old_password.unwrap_or_else(|| "123456".to_string());
    let new_passwd = new_password.unwrap_or_else(|| "admin_123".to_string());
    let api_timeout_secs = state
        .config
        .lock()
        .unwrap()
        .framework_password_api_timeout_secs;
    let client = build_device_http_client_with_timeout(Duration::from_secs(api_timeout_secs))?;

    let results = crate::async_utils::run_ordered_with_limit(
        ips,
        DEVICE_BATCH_CONCURRENCY_LIMIT,
        move |ip| {
            let client = client.clone();
            let old_passwd = old_passwd.clone();
            let new_passwd = new_passwd.clone();
            async move { change_framework_password_for_ip(client, ip, old_passwd, new_passwd).await }
        },
    )
    .await?;

    Ok(results.into_iter().flatten().collect())
}

#[allow(clippy::too_many_arguments)]
async fn enable_appliance_ssh_for_target(
    app_handle: tauri::AppHandle,
    client: reqwest::Client,
    target: ApplianceSshTarget,
    api_version: ApplianceSshApiVersion,
    ssh_username: String,
    ssh_password: String,
    add_whitelist_rule: bool,
    whitelist_scope: ApplianceSshWhitelistScope,
    whitelist_cidr: Option<String>,
    jump_host_username: Option<String>,
    jump_host_password: Option<String>,
) -> Option<ApplianceSshResult> {
    let ip = target.ip.trim().to_string();
    let jump_host = target
        .jump_host
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if ip.is_empty() {
        return None;
    }

    let mut result = ApplianceSshResult {
        ip: ip.clone(),
        success: false,
        message: String::new(),
        previous_enable: None,
        current_enable: None,
        port: None,
        whitelist_source_ip: None,
        whitelist_applied: None,
        jump_host: jump_host.clone(),
    };

    if !validate_ip(&ip) {
        result.message = format!("Invalid IP address: {}", ip);
        return Some(result);
    }
    if let Some(jh) = &jump_host {
        if !validate_ip(jh) {
            result.message = format!("Invalid jump host IP: {}", jh);
            return Some(result);
        }
    }

    // API ip and SSH ip differ when a jump host is present: the REST API lives
    // on the jump host (A); the target (B) is reached via SSH through A.
    let api_ip = jump_host.as_deref().unwrap_or(&ip).to_string();
    emit_runtime_log(
        &app_handle,
        format!(
            "[appliance-access] target={} apiHost={} apiPort={} version={:?}",
            ip,
            api_ip,
            appliance_ssh_api_port(api_version),
            api_version
        ),
        "info",
    );

    // Initial GET is best-effort: some appliances (especially under access-control
    // hardening) reject the get endpoint while still accepting set. We log the
    // failure and proceed to SET so the user's enable intent isn't blocked.
    let initial_status = match get_appliance_ssh_status(&client, &api_ip, api_version).await {
        Ok(status) => {
            emit_runtime_log(
                &app_handle,
                format!(
                    "[appliance-access] target={} initialStatus enable={:?} sshPort={:?}",
                    ip, status.enable, status.port
                ),
                "info",
            );
            Some(status)
        }
        Err(e) => {
            emit_runtime_log(
                &app_handle,
                format!(
                    "[appliance-access] target={} initial GET failed: {}; proceeding to SET",
                    ip, e
                ),
                "warn",
            );
            None
        }
    };

    result.previous_enable = initial_status.as_ref().and_then(|s| s.enable);
    result.port = initial_status.as_ref().and_then(|s| s.port);

    let current_status = if initial_status.as_ref().and_then(|s| s.enable) == Some(1) {
        initial_status.expect("checked enable==Some(1) above")
    } else {
        if let Err(e) = enable_appliance_ssh_via_api(&client, &api_ip, api_version).await {
            result.message = format!("Failed to enable SSH: {}", e);
            return Some(result);
        }
        match wait_for_appliance_ssh_enabled(
            &client,
            &api_ip,
            api_version,
            10,
            Duration::from_secs(1),
        )
        .await
        {
            WaitForEnableOutcome::Enabled(status) => status,
            WaitForEnableOutcome::NotEnabled { last_status } => {
                let observed = last_status
                    .enable
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                result.current_enable = last_status.enable;
                result.port = last_status.port.or(result.port);
                result.message = format!(
                    "SSH status verification failed: current enable state is {}",
                    observed
                );
                return Some(result);
            }
            WaitForEnableOutcome::GetFailed { last_error } => {
                // SET succeeded but every GET (initial + verification) failed.
                // Trust the SET success and treat the appliance as enabled.
                emit_runtime_log(
                    &app_handle,
                    format!(
                        "[appliance-access] target={} GET unavailable after SET ({}); treating SET success as enabled",
                        ip, last_error
                    ),
                    "warn",
                );
                ApplianceSshStatusData {
                    enable: Some(1),
                    port: initial_status.as_ref().and_then(|s| s.port),
                }
            }
        }
    };

    result.current_enable = current_status.enable;
    result.port = current_status.port.or(result.port).or(Some(23333));
    let api_ssh_port = result.port.unwrap_or(23333);
    emit_runtime_log(
        &app_handle,
        format!(
            "[appliance-access] target={} currentStatus enable={:?} sshPort={}",
            ip, current_status.enable, api_ssh_port
        ),
        "info",
    );

    if add_whitelist_rule {
        // Resolve credentials for SSH to jump host (or direct target).
        let (ssh_user, ssh_pass) = if jump_host.is_some() {
            let user = jump_host_username
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| ssh_username.clone());
            let pass = jump_host_password
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| ssh_password.clone());
            (user, pass)
        } else {
            (ssh_username.clone(), ssh_password.clone())
        };

        if ssh_user.is_empty() || ssh_pass.is_empty() {
            result.whitelist_applied = Some(false);
            result.message =
                "SSH username and password are required when adding an iptables whitelist rule"
                    .to_string();
            return Some(result);
        }

        // Resolve whitelist source: user-supplied CIDR replaces auto-detected local IP.
        let source = match whitelist_cidr
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(cidr) => {
                if !validate_cidr(cidr) {
                    result.whitelist_applied = Some(false);
                    result.message = format!("Invalid CIDR for whitelist source: {}", cidr);
                    return Some(result);
                }
                cidr.to_string()
            }
            None => match detect_local_source_ip(&api_ip, api_ssh_port) {
                Ok(ip) => ip,
                Err(e) => {
                    result.whitelist_applied = Some(false);
                    result.message = format!("Failed to determine local source IP: {}", e);
                    return Some(result);
                }
            },
        };
        result.whitelist_source_ip = Some(source.clone());
        let whitelist_scope_desc = describe_whitelist_scope(whitelist_scope, api_ssh_port);

        // Build the command that will run via SSH on `api_ip` (jump host or direct).
        let (ssh_host, command) = if jump_host.is_some() {
            // Nested: run on A a command that SSHes to B (default port) and
            // applies the iptables rule. Relies on passwordless SSH between A
            // and B (pre-shared keys), which is the appliance HA convention.
            // B's SSH username defaults to the main SSH username (typically
            // `root` — same across the master/backup pair).
            let target_port = JUMP_HOST_DEFAULT_TARGET_SSH_PORT;
            let cmd = build_nested_iptables_whitelist_command(
                &ssh_username,
                &ip,
                target_port,
                &source,
                target_port,
                whitelist_scope,
            );
            (api_ip.clone(), cmd)
        } else {
            // Direct: run iptables locally on the target.
            let cmd = build_iptables_whitelist_command(&source, api_ssh_port, whitelist_scope);
            (api_ip.clone(), cmd)
        };
        emit_runtime_log(
            &app_handle,
            format!(
                "[appliance-access] target={} whitelist source={} scope={} sshExec={} command={}",
                ip, source, whitelist_scope_desc, ssh_host, command
            ),
            "command",
        );

        let host_owned = ssh_host.clone();
        let user_owned = ssh_user.clone();
        let password_owned = ssh_pass.clone();
        let command_owned = command.clone();
        let whitelist_result = match tauri::async_runtime::spawn_blocking(move || {
            run_remote_command_over_ssh(
                &host_owned,
                api_ssh_port,
                &user_owned,
                &password_owned,
                &command_owned,
            )
        })
        .await
        {
            Ok(r) => r,
            Err(e) => {
                result.whitelist_applied = Some(false);
                result.message = format!("Failed to run the SSH whitelist task: {}", e);
                return Some(result);
            }
        };

        match whitelist_result {
            Ok(remote_result) => {
                result.success = true;
                result.whitelist_applied = Some(true);
                emit_runtime_log(
                    &app_handle,
                    format!(
                        "[appliance-access] target={} whitelist applied via {} output={}",
                        ip, remote_result.mode, remote_result.output
                    ),
                    "success",
                );
                result.message = if jump_host.is_some() {
                    format!(
                        "SSH is enabled on jump host {}. Added an iptables whitelist rule on {} for {} ({})",
                        api_ip, ip, source, whitelist_scope_desc
                    )
                } else {
                    format!(
                        "SSH is enabled. Added an iptables whitelist rule for {} ({})",
                        source, whitelist_scope_desc
                    )
                };
            }
            Err(e) => {
                result.whitelist_applied = Some(false);
                emit_runtime_log(
                    &app_handle,
                    format!("[appliance-access] target={} whitelist failed: {}", ip, e),
                    "error",
                );
                result.message = if jump_host.is_some() {
                    format!(
                        "SSH is enabled on jump host {}, but failed to apply the iptables rule on {}: {}",
                        api_ip, ip, e
                    )
                } else {
                    format!(
                        "SSH is enabled, but failed to add the iptables whitelist rule for {} ({}): {}",
                        source, whitelist_scope_desc, e
                    )
                };
            }
        }
    } else {
        result.success = true;
        result.message = if jump_host.is_some() {
            if result.previous_enable == Some(1) {
                format!("Jump host SSH is already enabled. Port: {}", api_ssh_port)
            } else {
                format!("Jump host SSH enabled successfully. Port: {}", api_ssh_port)
            }
        } else if result.previous_enable == Some(1) {
            format!("SSH is already enabled. Port: {}", api_ssh_port)
        } else {
            format!("SSH enabled successfully. Port: {}", api_ssh_port)
        };
    }

    Some(result)
}

#[tauri::command]
async fn enable_appliance_ssh(
    app_handle: tauri::AppHandle,
    request: ApplianceSshRequest,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ApplianceSshResult>, String> {
    let api_timeout_secs = state.config.lock().unwrap().appliance_ssh_api_timeout_secs;
    let client = build_device_http_client_with_timeout(Duration::from_secs(api_timeout_secs))?;

    // Merge legacy `ips` into `targets` so older callers keep working.
    let mut targets: Vec<ApplianceSshTarget> = request.targets;
    for ip in request.ips {
        targets.push(ApplianceSshTarget {
            ip,
            jump_host: None,
        });
    }

    let ssh_username = request.ssh_username.trim().to_string();
    let ssh_password = request.ssh_password;
    let add_whitelist_rule = request.add_whitelist_rule;
    let whitelist_scope = request.whitelist_scope;
    let whitelist_cidr = request.whitelist_cidr;
    let api_version = request.appliance_version;
    let (jump_user, jump_pass) = if request.jump_host_use_separate_creds {
        (request.jump_host_username, request.jump_host_password)
    } else {
        (None, None)
    };

    let results = crate::async_utils::run_ordered_with_limit(
        targets,
        DEVICE_BATCH_CONCURRENCY_LIMIT,
        move |target| {
            let app_handle = app_handle.clone();
            let client = client.clone();
            let ssh_username = ssh_username.clone();
            let ssh_password = ssh_password.clone();
            let whitelist_cidr = whitelist_cidr.clone();
            let jump_user = jump_user.clone();
            let jump_pass = jump_pass.clone();
            async move {
                enable_appliance_ssh_for_target(
                    app_handle,
                    client,
                    target,
                    api_version,
                    ssh_username,
                    ssh_password,
                    add_whitelist_rule,
                    whitelist_scope,
                    whitelist_cidr,
                    jump_user,
                    jump_pass,
                )
                .await
            }
        },
    )
    .await?;

    Ok(results.into_iter().flatten().collect())
}

fn main() {
    // Register an explicit AppUserModelID so Windows notifications show the app
    // name ("File Sync Tool") rather than the launching shell ("PowerShell").
    // Must run before any notification or WinRT call. Failures are non-fatal.
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let aumid: Vec<u16> = "com.filesync.tool\0".encode_utf16().collect();
        let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(aumid.as_ptr()));
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
            let _ = app.emit("single-instance", ());
        }))
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                let is_quitting = window
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|state| state.is_quitting.load(Ordering::SeqCst))
                    .unwrap_or(false);

                if is_quitting {
                    // Already confirmed quit (via confirm_quit command) — allow close
                } else if should_close_to_tray(window.app_handle()) {
                    api.prevent_close();
                    hide_main_window(window.app_handle());
                } else {
                    // close_to_tray=false, first X click: intercept to let frontend save
                    api.prevent_close();
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        state.is_quitting.store(true, Ordering::SeqCst);
                    }
                    let _ = window.app_handle().emit("before-quit", ());
                    let app_clone = window.app_handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(2));
                        app_clone.exit(0);
                    });
                }
            }
        })
        .setup(|app| {
            let tray_menu = MenuBuilder::new(app)
                .text(TRAY_SHOW_ID, "显示主窗口")
                .text(TRAY_CLIPBOARD_PANEL_ID, "Clipboard Panel")
                .separator()
                .text(TRAY_QUIT_ID, "退出")
                .build()?;

            let app_handle = app.handle().clone();
            let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .tooltip("File Sync Tool")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    TRAY_SHOW_ID => show_main_window(app),
                    TRAY_CLIPBOARD_PANEL_ID => {
                        let _ = clipboard::commands::cb_toggle_panel_internal(app.clone());
                    }
                    TRAY_QUIT_ID => {
                        if let Some(state) = app.try_state::<AppState>() {
                            state.is_quitting.store(true, Ordering::SeqCst);
                        }
                        // Notify frontend to save state before exiting
                        let _ = app.emit("before-quit", ());
                        // Fallback: force exit after 2 seconds if frontend doesn't respond
                        let app_clone = app.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_secs(2));
                            app_clone.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(&app_handle);
                    }
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }

            let _ = tray_builder.build(app)?;

            let config = config::load_config(app.handle());
            let task_manager = task_manager::TaskManager::new(app.handle().clone());
            let _ = sync_launch_on_startup(
                config.launch_and_auto_scan || config.launch_and_auto_start_file_share,
            );
            if config.clipboard.run_as_admin {
                match std::env::current_exe() {
                    Ok(exe) => {
                        let exe_path = exe.to_string_lossy().to_string();
                        if let Err(error) =
                            crate::clipboard::admin::set_autostart_as_admin(&exe_path, true)
                        {
                            log::warn!(
                                "[clipboard] failed to refresh admin autostart path on startup: {}",
                                error
                            );
                        }
                    }
                    Err(error) => {
                        log::warn!(
                            "[clipboard] failed to resolve current exe for admin autostart refresh: {}",
                            error
                        );
                    }
                }
            }

            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("app_data_dir: {e}"))?;
            let clipboard_state =
                clipboard::ClipboardState::init(&app_data_dir, config.clipboard.clone())?;
            let clipboard_state_for_startup = clipboard_state.clone();
            let clipboard_enabled_at_start = config.clipboard.enabled;
            let clipboard_hotkey_at_start = config.clipboard.hotkey.clone();
            let config_show_startup_notification = config.clipboard.show_startup_notification;

            app.manage(network::NetworkState::default());
            app.manage(AppState {
                config: Arc::new(Mutex::new(config)),
                updater: Arc::new(updater::UpdaterState::new()),
                task_manager,
                task_runtime: task_runtime::TaskRuntimeRegistry::new(),
                executor_active: Arc::new(AtomicBool::new(false)),
                run_control_target: Arc::new(Mutex::new(None)),
                is_scanning: Arc::new(AtomicBool::new(false)),
                is_manual_copying: Arc::new(AtomicBool::new(false)),
                is_manually_deploying: Arc::new(AtomicBool::new(false)),
                manual_copy_queue: Arc::new(Mutex::new(VecDeque::new())),
                manual_copy_keys: Arc::new(Mutex::new(HashSet::new())),
                manual_copy_worker_running: Arc::new(AtomicBool::new(false)),
                should_cancel: Arc::new(AtomicBool::new(false)),
                should_skip_current: Arc::new(AtomicBool::new(false)),
                scan_queue_removals: Arc::new(Mutex::new(HashSet::new())),
                is_paused: Arc::new(AtomicBool::new(false)),
                is_quitting: Arc::new(AtomicBool::new(false)),
                code_count_should_cancel: Arc::new(AtomicBool::new(false)),
                screen_share: Arc::new(screenshare::ScreenShareHandle::new()),
                file_share: Arc::new(fileshare::FileShareHandle::new()),
                clipboard: clipboard_state,
                error_code: std::sync::Mutex::new(error_code::ErrorCodeStore::default()),
            });

            if let Some(state) = app.try_state::<AppState>() {
                updater::commands::initialize_on_startup(app.handle().clone(), state.inner());
            }

            // Start the clipboard watcher if the persisted config has it enabled.
            // init() seeds is_enabled from config, but watcher thread only runs after enable().
            if clipboard_enabled_at_start {
                // Flip is_enabled back to false so enable() will actually spawn the watcher
                // (enable() short-circuits when is_enabled is already true).
                clipboard_state_for_startup
                    .is_enabled
                    .store(false, Ordering::SeqCst);
                clipboard_state_for_startup.enable(app.handle().clone());
            }

            // Create clipboard panel window (hidden by default; shown on demand via cb_toggle_panel or hotkey).
            let panel = tauri::WebviewWindowBuilder::new(
                app,
                "clipboard-panel",
                tauri::WebviewUrl::App("index.html#/clipboard-panel".into()),
            )
            .title("Clipboard")
            .inner_size(420.0, 720.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .visible(false)
            .build()?;

            // Auto-hide on focus loss. We debounce by 150ms and re-check
            // `is_focused()` because calling `startDragging()` from the header
            // causes a transient Focused(false) -> Focused(true) flicker
            // during the WM_NCLBUTTONDOWN drag modal loop. Without this guard
            // the panel would hide the instant the user pressed the header.
            // Skip auto-hide entirely when the user pinned the panel.
            //
            // The same orphan-dismissal logic also runs from each preview window's
            // `Focused(false)` handler so the panel still hides when the user clicks
            // outside while the preview held focus (e.g. after clicking a zoom button).
            let panel_clone = panel.clone();
            let preview_app_handle = panel.app_handle().clone();
            let pinned_flag = clipboard_state_for_startup.clone();
            panel.on_window_event(move |ev| {
                if let tauri::WindowEvent::Focused(false) = ev {
                    clipboard::preview::debug_window_snapshot(
                        &preview_app_handle,
                        "panel-focused-false:received",
                    );
                    clipboard::preview::schedule_dismiss_if_orphaned(
                        preview_app_handle.clone(),
                        panel_clone.clone(),
                        pinned_flag.clone(),
                    );
                }
            });

            clipboard::preview::ensure_preview_windows(app)?;
            clipboard::preview::attach_preview_dismiss_handlers(
                app.handle(),
                panel.clone(),
                clipboard_state_for_startup.clone(),
            )?;

            // Register the clipboard global shortcut (default Alt+C) when the feature is on.
            if clipboard_enabled_at_start {
                match clipboard::hotkey::register(app.handle().clone(), &clipboard_hotkey_at_start)
                {
                    Ok(handle) => {
                        *clipboard_state_for_startup.hotkey_handle.lock() = Some(handle);
                    }
                    Err(e) => {
                        eprintln!("[clipboard] hotkey register failed: {e}");
                    }
                }
            }

            // Fire a startup toast so users know the watcher is live and how to open the panel.
            // Delayed 500ms so the notification plugin + tray finish initializing first.
            if clipboard_enabled_at_start && config_show_startup_notification {
                use tauri_plugin_notification::NotificationExt;
                let handle = app.handle().clone();
                let hotkey_display = clipboard_hotkey_at_start.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let _ = handle
                        .notification()
                        .builder()
                        .title("File-Sync-Tool 剪贴板")
                        .body(format!("剪贴板监听已启动，按 {hotkey_display} 呼出面板"))
                        .show();
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config_cmd,
            scan_now,
            cancel_scan,
            pause_scan,
            resume_scan,
            cancel_task_run,
            pause_task_run,
            resume_task_run,
            skip_current_copy,
            remove_from_scan_queue,
            history::get_history,
            history::clear_history,
            history::add_system_event,
            test_ssh_connection,
            start_manual_copy_task,
            start_manual_deploy_task,
            queue_temporary_copy,
            preview_temporary_copy,
            get_app_paths,
            get_custom_data_dir,
            set_custom_data_dir,
            open_path_parent,
            open_url,
            open_directory,
            save_text_file,
            change_framework_password,
            enable_appliance_ssh,
            updater::commands::check_update,
            updater::commands::start_update_download,
            updater::commands::cancel_update_download,
            updater::commands::apply_update_now,
            updater::commands::test_update_server,
            updater::commands::get_update_state,
            disk_cleanup::disk_cleanup_list_servers,
            disk_cleanup::disk_cleanup_list_disks,
            disk_cleanup::disk_cleanup_check_redis,
            disk_cleanup::disk_cleanup_delete_cache,
            disk_cleanup::disk_cleanup_list_linux_servers,
            disk_cleanup::disk_cleanup_list_mainline_servers,
            disk_cleanup::disk_cleanup_list_linux_disks,
            disk_cleanup::disk_cleanup_list_windows_disks,
            disk_cleanup::disk_cleanup_list_ipsans,
            disk_cleanup::disk_cleanup_list_ipsan_resource_groups,
            disk_cleanup::disk_cleanup_check_cache_keys,
            disk_cleanup::disk_cleanup_get_cache_key_contents,
            disk_cleanup::disk_cleanup_delete_cache_keys,
            code_count::code_count_analyze,
            code_count::code_count_cancel,
            code_count::code_count_list_scope_tree,
            task_commands::list_task_groups,
            task_commands::get_task_group_detail,
            task_commands::clear_task_group,
            task_commands::clear_task_groups,
            persist::save_ui_state,
            persist::load_ui_state,
            persist::save_kv,
            persist::load_kv,
            network::ping_scan,
            network::cancel_ping_scan,
            network::get_tcp_connections,
            network::test_ports,
            network::cancel_port_test,
            network::send_wol,
            screenshare::screen_share_list_monitors,
            screenshare::screen_share_list_interfaces,
            screenshare::screen_share_scan_conflicts,
            screenshare::screen_share_force_close_conflicts,
            screenshare::screen_share_start,
            screenshare::screen_share_stop,
            screenshare::screen_share_get_status,
            fileshare::file_share_pick_directory,
            fileshare::persist::file_share_load_settings,
            fileshare::persist::file_share_save_settings,
            fileshare::file_share_start_saved,
            fileshare::file_share_start,
            fileshare::file_share_stop,
            fileshare::file_share_get_status,
            error_code::commands::error_code_sync,
            error_code::commands::error_code_query,
            error_code::commands::error_code_get_meta,
            confirm_quit,
            clipboard::commands::cb_is_enabled,
            clipboard::commands::cb_enable,
            clipboard::commands::cb_disable,
            clipboard::commands::cb_list,
            clipboard::commands::cb_get,
            clipboard::commands::cb_delete,
            clipboard::commands::cb_delete_batch,
            clipboard::commands::cb_clear,
            clipboard::commands::cb_toggle_favorite,
            clipboard::commands::cb_toggle_pin,
            clipboard::commands::cb_groups_list,
            clipboard::commands::cb_groups_create,
            clipboard::commands::cb_groups_rename,
            clipboard::commands::cb_groups_delete,
            clipboard::commands::cb_move_to_group,
            clipboard::commands::cb_toggle_panel,
            clipboard::commands::cb_set_hotkey,
            clipboard::commands::cb_paste,
            clipboard::commands::cb_paste_plain,
            clipboard::commands::cb_copy,
            clipboard::commands::cb_paste_as_files,
            clipboard::commands::cb_paste_as_path,
            clipboard::commands::cb_check_file_paths,
            clipboard::commands::cb_save_image_as,
            clipboard::commands::cb_open_in_explorer,
            clipboard::commands::cb_merge_paste,
            clipboard::commands::cb_show_image_preview,
            clipboard::commands::cb_show_text_preview,
            clipboard::commands::cb_get_image_preview_payload,
            clipboard::commands::cb_get_text_preview_payload,
            clipboard::commands::cb_hide_preview,
            clipboard::commands::cb_debug_window_snapshot,
            clipboard::commands::cb_toggle_preview_fullscreen,
            clipboard::commands::cb_reorder_favorites,
            clipboard::commands::cb_stats,
            clipboard::commands::cb_get_settings,
            clipboard::commands::cb_save_settings,
            clipboard::commands::cb_export,
            clipboard::commands::cb_import,
            clipboard::commands::cb_db_optimize,
            clipboard::commands::cb_db_vacuum,
            clipboard::commands::cb_reset_config,
            clipboard::commands::cb_reset_all,
            clipboard::commands::cb_enable_win_v,
            clipboard::commands::cb_disable_win_v,
            clipboard::commands::cb_is_win_v_enabled,
            clipboard::commands::cb_is_elevated,
            clipboard::commands::cb_is_run_as_admin_enabled,
            clipboard::commands::cb_admin_task_status,
            clipboard::commands::cb_admin_task_create,
            clipboard::commands::cb_admin_task_remove,
            clipboard::commands::cb_set_run_as_admin,
            clipboard::commands::cb_set_panel_pinned,
            clipboard::commands::cb_is_panel_pinned,
            clipboard::commands::cb_open_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
