// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod code_count;
mod config;
mod deploy;
mod history;
mod network;
mod persist;
mod scanner;
mod task_domain;
mod task_commands;
mod task_events;
mod task_manager;
mod task_persist;

use config::{AppConfig, DeployServer};
use scanner::ScanResult;
use ssh2::{ExtendedData, Session};
use std::collections::{HashSet, VecDeque};
use std::io::Read;
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
const TRAY_QUIT_ID: &str = "tray_quit";

struct AppState {
    config: Arc<Mutex<AppConfig>>,
    task_manager: task_manager::TaskManager,
    is_scanning: Arc<AtomicBool>,
    is_manually_deploying: Arc<AtomicBool>,
    manual_copy_queue: Arc<Mutex<VecDeque<ManualCopyQueueItem>>>,
    manual_copy_keys: Arc<Mutex<HashSet<String>>>,
    manual_copy_worker_running: Arc<AtomicBool>,
    should_cancel: Arc<AtomicBool>,
    should_skip_current: Arc<AtomicBool>,
    scan_queue_removals: Arc<Mutex<HashSet<String>>>,
    is_paused: Arc<AtomicBool>,
    is_quitting: Arc<AtomicBool>,
}

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

#[derive(Clone, serde::Serialize)]
struct ManualCopyTaskStateEvent {
    folder: String,
    source_path: String,
    local_path: String,
    state: String,
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
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("Cannot access path {}: {}", path.display(), e))?;
    let mut normalized = canonical.to_string_lossy().replace('/', "\\");

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

fn emit_manual_copy_task_state<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    task: &ManualCopyQueueItem,
    state: &str,
) {
    let _ = app_handle.emit(
        "manual-copy-task-state",
        ManualCopyTaskStateEvent {
            folder: task.folder_name.clone(),
            source_path: task.source_path.clone(),
            local_path: task.local_path.clone(),
            state: state.to_string(),
        },
    );
}

fn start_manual_copy_worker(app_handle: tauri::AppHandle, state: &AppState) {
    let config = state.config.clone();
    let task_manager = state.task_manager.clone();
    let manual_copy_queue = state.manual_copy_queue.clone();
    let manual_copy_keys = state.manual_copy_keys.clone();
    let manual_copy_worker_running = state.manual_copy_worker_running.clone();
    let is_scanning = state.is_scanning.clone();
    let should_cancel = state.should_cancel.clone();
    let is_paused = state.is_paused.clone();

    tauri::async_runtime::spawn(async move {
        loop {
            if is_scanning
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                let queue_empty = manual_copy_queue.lock().unwrap().is_empty();
                if queue_empty {
                    manual_copy_worker_running.store(false, Ordering::SeqCst);
                    let queue_still_empty = manual_copy_queue.lock().unwrap().is_empty();
                    if queue_still_empty || manual_copy_worker_running.swap(true, Ordering::SeqCst)
                    {
                        break;
                    }
                }
                continue;
            }

            loop {
                let next_task = { manual_copy_queue.lock().unwrap().pop_front() };
                let Some(task) = next_task else {
                    break;
                };

                should_cancel.store(false, Ordering::SeqCst);
                is_paused.store(false, Ordering::SeqCst);
                emit_manual_copy_task_state(&app_handle, &task, "started");

                let config_snapshot = config.lock().unwrap().clone();
                let result = scanner::temporary_copy(
                    &app_handle,
                    &config_snapshot,
                    config.clone(),
                    task_manager.clone(),
                    task.source_path.clone(),
                    task.target_root_path.clone(),
                    task.overwrite_existing,
                    should_cancel.clone(),
                    is_paused.clone(),
                    task.file_extensions.clone(),
                    task.filename_includes.clone(),
                )
                .await;

                manual_copy_keys.lock().unwrap().remove(&task.key);

                if let Err(error) = result {
                    let state = if error.to_lowercase().contains("cancelled") {
                        "cancelled"
                    } else {
                        "failed"
                    };
                    emit_manual_copy_task_state(&app_handle, &task, state);
                    emit_runtime_log(
                        &app_handle,
                        format!("Manual copy task failed: {}", error),
                        "error",
                    );
                } else {
                    emit_manual_copy_task_state(&app_handle, &task, "completed");
                }
            }

            is_scanning.store(false, Ordering::SeqCst);

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

fn sync_launch_on_startup(enabled: bool) -> Result<(), String> {
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
    let config = config::normalize_config(config);
    sync_launch_on_startup(config.launch_and_auto_scan)?;
    *state.config.lock().unwrap() = config.clone();
    config::save_config(&app_handle, &config)
}

#[tauri::command]
async fn scan_now(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ScanResult, String> {
    if state.is_scanning.load(Ordering::SeqCst) {
        return Err("Scan already in progress".to_string());
    }

    state.is_scanning.store(true, Ordering::SeqCst);
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
        state.should_cancel.clone(),
        state.should_skip_current.clone(),
        state.is_paused.clone(),
        state.scan_queue_removals.clone(),
    )
    .await;

    state.is_scanning.store(false, Ordering::SeqCst);
    Ok(result)
}

#[tauri::command]
fn cancel_scan(state: State<AppState>) {
    state.should_cancel.store(true, Ordering::SeqCst);
    // Also unpause if paused, so the loop can proceed to cancel
    state.is_paused.store(false, Ordering::SeqCst);
}

#[tauri::command]
fn pause_scan(state: State<AppState>) {
    state.is_paused.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn resume_scan(state: State<AppState>) {
    state.is_paused.store(false, Ordering::SeqCst);
}

#[tauri::command]
fn skip_current_copy(state: State<AppState>) {
    state.should_skip_current.store(true, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);
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
async fn manual_deploy(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    server: DeployServer,
    postCommands: Vec<String>,
    localPath: String,
    remotePath: String,
) -> Result<(), String> {
    if state.is_manually_deploying.load(Ordering::SeqCst) {
        return Err("Manual deploy already in progress".to_string());
    }

    state.is_manually_deploying.store(true, Ordering::SeqCst);
    state.should_cancel.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);

    let should_cancel = state.should_cancel.clone();
    let is_paused = state.is_paused.clone();
    let is_manually_deploying = state.is_manually_deploying.clone();

    // This runs in async context, but deploy_manual uses blocking SSH.
    // We should spawn blocking.
    let result = tauri::async_runtime::spawn_blocking(move || {
        deploy::deploy_manual(
            &app_handle,
            &server,
            &postCommands,
            &localPath,
            &remotePath,
            should_cancel,
            is_paused,
        )
    })
    .await
    .map_err(|e| e.to_string())?;

    is_manually_deploying.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
async fn temporary_copy(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    source_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
) -> Result<(), String> {
    if state.is_scanning.load(Ordering::SeqCst) {
        return Err("Operation already in progress".to_string());
    }

    state.is_scanning.store(true, Ordering::SeqCst);
    state.should_cancel.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);

    let config = state.config.lock().unwrap().clone();
    let live_config = state.config.clone();
    let should_cancel = state.should_cancel.clone();
    let is_paused = state.is_paused.clone();
    let is_scanning = state.is_scanning.clone();

    let result = scanner::temporary_copy(
        &app_handle,
        &config,
        live_config,
        state.task_manager.clone(),
        source_path,
        target_root_path,
        overwrite_existing,
        should_cancel,
        is_paused,
        file_extensions,
        filename_includes,
    )
    .await;

    is_scanning.store(false, Ordering::SeqCst);
    result
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

#[tauri::command]
async fn open_directory() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let selected_dir = rfd::FileDialog::new().pick_folder();
        Ok(selected_dir.map(|path| path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn save_text_file(
    content: String,
    default_file_name: String,
    filter_name: String,
    extensions: Vec<String>,
) -> Result<Option<String>, String> {
    let extension_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let mut dialog = rfd::FileDialog::new().set_file_name(&default_file_name);

    if !extension_refs.is_empty() {
        dialog = dialog.add_filter(&filter_name, &extension_refs);
    }

    let Some(mut target_path) = dialog.save_file() else {
        return Ok(None);
    };

    if target_path.extension().is_none() {
        if let Some(default_extension) = extensions.first() {
            target_path.set_extension(default_extension);
        }
    }

    std::fs::write(&target_path, content).map_err(|e| e.to_string())?;

    Ok(Some(target_path.to_string_lossy().to_string()))
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PasswordChangeResult {
    pub ip: String,
    pub success: bool,
    pub message: String,
    pub failedAt: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplianceSshRequest {
    pub ips: Vec<String>,
    #[serde(default)]
    pub ssh_username: String,
    #[serde(default)]
    pub ssh_password: String,
    #[serde(default)]
    pub add_whitelist_rule: bool,
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

// Helper function to validate IP address format
fn validate_ip(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| part.parse::<u8>().is_ok())
}

async fn get_appliance_ssh_status(
    client: &reqwest::Client,
    ip: &str,
) -> Result<ApplianceSshStatusData, String> {
    let request_url = format!("http://{}:23006/openAPI/system/v1/network/SSH/get", ip);
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

async fn enable_appliance_ssh_via_api(client: &reqwest::Client, ip: &str) -> Result<(), String> {
    let request_url = format!("http://{}:23006/openAPI/system/v1/network/SSH/set", ip);
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

async fn wait_for_appliance_ssh_enabled(
    client: &reqwest::Client,
    ip: &str,
    attempts: usize,
    delay: Duration,
) -> Result<ApplianceSshStatusData, String> {
    let mut last_error: Option<String> = None;

    for attempt in 0..attempts {
        match get_appliance_ssh_status(client, ip).await {
            Ok(status) => {
                if status.enable == Some(1) {
                    return Ok(status);
                }

                last_error = Some(format!(
                    "current enable state is {}",
                    status
                        .enable
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                ));
            }
            Err(e) => {
                last_error = Some(e);
            }
        }

        if attempt + 1 < attempts {
            tokio::time::sleep(delay).await;
        }
    }

    Err(match last_error {
        Some(error) => format!("SSH did not become enabled in time: {}", error),
        None => "SSH did not become enabled in time".to_string(),
    })
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

fn build_iptables_whitelist_command(source_ip: &str, port: u16) -> String {
    format!(
        "sh -lc 'iptables -C INPUT -p tcp -s {source_ip} --dport {port} -j ACCEPT || iptables -I INPUT 1 -p tcp -s {source_ip} --dport {port} -j ACCEPT'"
    )
}

fn run_remote_command_over_ssh(
    ip: &str,
    port: u16,
    username: &str,
    password: &str,
    command: &str,
) -> Result<String, String> {
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

    let mut channel = sess
        .channel_session()
        .map_err(|e| format!("SSH channel init failed: {}", e))?;
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

/// RSA encrypt plaintext with a PEM public key, return Base64-encoded ciphertext.
fn rsa_encrypt(public_key_pem: &str, plaintext: &str) -> Result<String, String> {
    use base64::{engine::general_purpose, Engine as _};
    use rand::rngs::OsRng;
    use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};

    let public_key = RsaPublicKey::from_public_key_pem(public_key_pem)
        .map_err(|e| format!("Failed to parse public key: {}", e))?;
    let mut rng = OsRng;
    let encrypted = public_key
        .encrypt(&mut rng, Pkcs1v15Encrypt, plaintext.as_bytes())
        .map_err(|e| format!("RSA encryption failed: {}", e))?;
    Ok(general_purpose::STANDARD.encode(&encrypted))
}

#[tauri::command]
async fn change_framework_password(
    ips: Vec<String>,
    old_password: Option<String>,
    new_password: Option<String>,
) -> Result<Vec<PasswordChangeResult>, String> {
    let old_passwd = old_password.unwrap_or_else(|| "123456".to_string());
    let new_passwd = new_password.unwrap_or_else(|| "admin_123".to_string());

    let mut results = Vec::new();
    let client = reqwest::Client::new();

    for ip in ips.iter() {
        let ip = ip.trim();
        if ip.is_empty() {
            continue;
        }

        // Validate IP format (basic check)
        if !validate_ip(ip) {
            results.push(PasswordChangeResult {
                ip: ip.to_string(),
                success: false,
                message: format!("Invalid IP address: {}", ip),
                failedAt: Some("login".to_string()),
            });
            continue;
        }

        // Step 1: Get RSA public key
        let pubkey_url = format!("http://{}:21900/openAPI/auth/v1/publicKey", ip);
        let public_key = match client.get(&pubkey_url).send().await {
            Ok(response) => match response.json::<serde_json::Value>().await {
                Ok(json) => {
                    if let Some(key) = json
                        .get("data")
                        .and_then(|d| d.get("publicKey"))
                        .and_then(|k| k.as_str())
                    {
                        key.to_string()
                    } else {
                        results.push(PasswordChangeResult {
                            ip: ip.to_string(),
                            success: false,
                            message: format!("Failed to get public key: {:?}", json),
                            failedAt: Some("login".to_string()),
                        });
                        continue;
                    }
                }
                Err(e) => {
                    results.push(PasswordChangeResult {
                        ip: ip.to_string(),
                        success: false,
                        message: format!("Public key response parse error: {}", e),
                        failedAt: Some("login".to_string()),
                    });
                    continue;
                }
            },
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("Public key request failed: {}", e),
                    failedAt: Some("login".to_string()),
                });
                continue;
            }
        };

        // Step 2: Login with RSA-encrypted password
        let encrypted_old = match rsa_encrypt(&public_key, &old_passwd) {
            Ok(v) => v,
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("RSA encrypt old password failed: {}", e),
                    failedAt: Some("login".to_string()),
                });
                continue;
            }
        };

        let login_url = format!("http://{}:21900/openAPI/userMgr/v1/login", ip);
        let login_body = json!({
            "userName": "admin",
            "userPasswd": encrypted_old,
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
            Ok(response) => {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                            if let Some(token) = json
                                .get("data")
                                .and_then(|d| d.get("token"))
                                .and_then(|t| t.as_str())
                            {
                                token.to_string()
                            } else {
                                results.push(PasswordChangeResult {
                                    ip: ip.to_string(),
                                    success: false,
                                    message: "Login response missing token".to_string(),
                                    failedAt: Some("login".to_string()),
                                });
                                continue;
                            }
                        } else {
                            let msg = json
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error");
                            results.push(PasswordChangeResult {
                                ip: ip.to_string(),
                                success: false,
                                message: format!("Login failed: {}", msg),
                                failedAt: Some("login".to_string()),
                            });
                            continue;
                        }
                    }
                    Err(e) => {
                        results.push(PasswordChangeResult {
                            ip: ip.to_string(),
                            success: false,
                            message: format!("Login response parse error: {}", e),
                            failedAt: Some("login".to_string()),
                        });
                        continue;
                    }
                }
            }
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("Login request failed: {}", e),
                    failedAt: Some("login".to_string()),
                });
                continue;
            }
        };

        // Step 3: Change Password with RSA-encrypted old & new passwords
        let encrypted_old_for_change = match rsa_encrypt(&public_key, &old_passwd) {
            Ok(v) => v,
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("RSA encrypt failed: {}", e),
                    failedAt: Some("changePasswd".to_string()),
                });
                continue;
            }
        };
        let encrypted_new = match rsa_encrypt(&public_key, &new_passwd) {
            Ok(v) => v,
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("RSA encrypt new password failed: {}", e),
                    failedAt: Some("changePasswd".to_string()),
                });
                continue;
            }
        };

        let change_passwd_url = format!("http://{}:21900/openAPI/userMgr/v1/changePasswd", ip);
        let change_passwd_body = json!({
            "userName": "admin",
            "oldUserPasswd": encrypted_old_for_change,
            "newUserPasswd": encrypted_new
        });

        let change_success = match client
            .post(&change_passwd_url)
            .header("Authorization", &token)
            .header("content-type", "application/json")
            .json(&change_passwd_body)
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                            true
                        } else {
                            let msg = json
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown error");
                            results.push(PasswordChangeResult {
                                ip: ip.to_string(),
                                success: false,
                                message: format!("Change password failed: {}", msg),
                                failedAt: Some("changePasswd".to_string()),
                            });
                            false
                        }
                    }
                    Err(e) => {
                        results.push(PasswordChangeResult {
                            ip: ip.to_string(),
                            success: false,
                            message: format!("Change password response parse error: {}", e),
                            failedAt: Some("changePasswd".to_string()),
                        });
                        false
                    }
                }
            }
            Err(e) => {
                results.push(PasswordChangeResult {
                    ip: ip.to_string(),
                    success: false,
                    message: format!("Change password request failed: {}", e),
                    failedAt: Some("changePasswd".to_string()),
                });
                false
            }
        };

        if !change_success {
            continue;
        }

        // Step 4: Logout
        let logout_url = format!("http://{}:21900/openAPI/userMgr/v1/logout", ip);
        let logout_body = json!({
            "userName": "admin",
            "userPasswd": encrypted_old,
            "token": token
        });

        // Logout is best-effort; don't fail the overall result if it errors
        let _ = client
            .post(&logout_url)
            .header("Authorization", &token)
            .header("content-type", "application/json")
            .json(&logout_body)
            .send()
            .await;

        results.push(PasswordChangeResult {
            ip: ip.to_string(),
            success: true,
            message: "Success".to_string(),
            failedAt: None,
        });
    }

    Ok(results)
}

#[tauri::command]
async fn enable_appliance_ssh(
    request: ApplianceSshRequest,
) -> Result<Vec<ApplianceSshResult>, String> {
    let mut results = Vec::new();
    let client = reqwest::Client::new();
    let ssh_username = request.ssh_username.trim().to_string();
    let ssh_password = request.ssh_password.clone();

    for ip in request.ips.iter() {
        let ip = ip.trim();
        if ip.is_empty() {
            continue;
        }

        let mut result = ApplianceSshResult {
            ip: ip.to_string(),
            success: false,
            message: String::new(),
            previous_enable: None,
            current_enable: None,
            port: None,
            whitelist_source_ip: None,
            whitelist_applied: None,
        };

        if !validate_ip(ip) {
            result.message = format!("Invalid IP address: {}", ip);
            results.push(result);
            continue;
        }

        let initial_status = match get_appliance_ssh_status(&client, ip).await {
            Ok(status) => status,
            Err(e) => {
                result.message = format!("Failed to get SSH status: {}", e);
                results.push(result);
                continue;
            }
        };

        result.previous_enable = initial_status.enable;
        result.port = initial_status.port;

        let current_status = if initial_status.enable == Some(1) {
            initial_status
        } else {
            if let Err(e) = enable_appliance_ssh_via_api(&client, ip).await {
                result.message = format!("Failed to enable SSH: {}", e);
                results.push(result);
                continue;
            }
            match wait_for_appliance_ssh_enabled(&client, ip, 10, Duration::from_secs(1)).await {
                Ok(status) => status,
                Err(e) => {
                    result.message = format!("SSH status verification failed: {}", e);
                    results.push(result);
                    continue;
                }
            }
        };

        result.current_enable = current_status.enable;
        result.port = current_status.port.or(result.port).or(Some(23333));
        let ssh_port = result.port.unwrap_or(23333);

        if request.add_whitelist_rule {
            if ssh_username.is_empty() || ssh_password.is_empty() {
                result.whitelist_applied = Some(false);
                result.message =
                    "SSH username and password are required when adding an iptables whitelist rule"
                        .to_string();
                results.push(result);
                continue;
            }

            let source_ip = match detect_local_source_ip(ip, ssh_port) {
                Ok(source_ip) => source_ip,
                Err(e) => {
                    result.whitelist_applied = Some(false);
                    result.message = format!("Failed to determine local source IP: {}", e);
                    results.push(result);
                    continue;
                }
            };

            result.whitelist_source_ip = Some(source_ip.clone());

            let ip_owned = ip.to_string();
            let user_owned = ssh_username.clone();
            let password_owned = ssh_password.clone();
            let command = build_iptables_whitelist_command(&source_ip, ssh_port);

            let whitelist_result = match tauri::async_runtime::spawn_blocking(move || {
                run_remote_command_over_ssh(
                    &ip_owned,
                    ssh_port,
                    &user_owned,
                    &password_owned,
                    &command,
                )
            })
            .await
            {
                Ok(result) => result,
                Err(e) => {
                    result.whitelist_applied = Some(false);
                    result.message = format!("Failed to run the SSH whitelist task: {}", e);
                    results.push(result);
                    continue;
                }
            };

            match whitelist_result {
                Ok(_) => {
                    result.success = true;
                    result.whitelist_applied = Some(true);
                    result.message = format!(
                        "SSH is enabled. Added an iptables whitelist rule for {}:{}",
                        source_ip, ssh_port
                    );
                }
                Err(e) => {
                    result.whitelist_applied = Some(false);
                    result.message = format!(
                        "SSH is enabled, but failed to add the iptables whitelist rule for {}:{}: {}",
                        source_ip, ssh_port, e
                    );
                }
            }
        } else {
            result.success = true;
            result.message = if result.previous_enable == Some(1) {
                format!("SSH is already enabled. Port: {}", ssh_port)
            } else {
                format!("SSH enabled successfully. Port: {}", ssh_port)
            };
        }

        results.push(result);
    }

    Ok(results)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
            let _ = app.emit("single-instance", ());
        }))
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
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
                } else if should_close_to_tray(&window.app_handle()) {
                    api.prevent_close();
                    hide_main_window(&window.app_handle());
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
            let _ = sync_launch_on_startup(config.launch_and_auto_scan);
            app.manage(network::NetworkState::default());
            app.manage(AppState {
                config: Arc::new(Mutex::new(config)),
                task_manager,
                is_scanning: Arc::new(AtomicBool::new(false)),
                is_manually_deploying: Arc::new(AtomicBool::new(false)),
                manual_copy_queue: Arc::new(Mutex::new(VecDeque::new())),
                manual_copy_keys: Arc::new(Mutex::new(HashSet::new())),
                manual_copy_worker_running: Arc::new(AtomicBool::new(false)),
                should_cancel: Arc::new(AtomicBool::new(false)),
                should_skip_current: Arc::new(AtomicBool::new(false)),
                scan_queue_removals: Arc::new(Mutex::new(HashSet::new())),
                is_paused: Arc::new(AtomicBool::new(false)),
                is_quitting: Arc::new(AtomicBool::new(false)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config_cmd,
            scan_now,
            cancel_scan,
            pause_scan,
            resume_scan,
            skip_current_copy,
            remove_from_scan_queue,
            history::get_history,
            history::clear_history,
            history::add_system_event,
            test_ssh_connection,
            manual_deploy,
            temporary_copy,
            queue_temporary_copy,
            preview_temporary_copy,
            get_app_paths,
            open_path_parent,
            open_directory,
            save_text_file,
            change_framework_password,
            enable_appliance_ssh,
            code_count::code_count_analyze,
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
            network::send_wol,
            confirm_quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
