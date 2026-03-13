// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod deploy;
mod history;
mod scanner;

use config::{AppConfig, DeployServer};
use scanner::ScanResult;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use serde_json::json;
use tauri::{Emitter, Manager, State, WebviewWindow, WebviewWindowBuilder, WindowEvent};

const TRAY_SHOW_ID: &str = "tray_show_main";
const TRAY_QUIT_ID: &str = "tray_quit";

struct AppState {
    config: Arc<Mutex<AppConfig>>,
    is_scanning: Arc<AtomicBool>,
    is_manually_deploying: Arc<AtomicBool>,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    is_quitting: Arc<AtomicBool>,
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

    let app_handle = app.clone();

    std::thread::spawn(move || {
        let app_for_ui = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            if let Some(window) = app_for_ui.get_webview_window("main") {
                restore_main_window(&window);
                return;
            }

            match WebviewWindowBuilder::from_config(&app_for_ui, &window_config)
                .and_then(|builder| builder.build())
            {
                Ok(window) => {
                    log::warn!("Main window was missing and has been recreated");
                    restore_main_window(&window);
                }
                Err(err) => {
                    log::error!("Failed to recreate main window: {err}");
                }
            }
        });
    });
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        restore_main_window(&window);
    } else {
        recreate_main_window(app);
    }
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
    state.is_paused.store(false, Ordering::SeqCst);

    let config = state.config.lock().unwrap().clone();
    let live_config = state.config.clone();
    let result = scanner::scan_and_copy(
        &app_handle,
        &config,
        live_config,
        state.should_cancel.clone(),
        state.is_paused.clone(),
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
    sourcePath: String,
    targetRootPath: String,
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
        sourcePath,
        targetRootPath,
        should_cancel,
        is_paused,
    )
    .await;

    is_scanning.store(false, Ordering::SeqCst);
    result
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
fn open_directory() -> Result<Option<String>, String> {
    // Open system directory selection dialog
    // Returns Ok(Some(path)) when user selects a directory
    // Returns Ok(None) when user cancels
    let selected_dir = rfd::FileDialog::new().pick_folder();

    Ok(selected_dir.map(|path| path.to_string_lossy().to_string()))
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PasswordChangeResult {
    pub ip: String,
    pub success: bool,
    pub message: String,
    pub failedAt: Option<String>,
}

// Helper function to validate IP address format
fn validate_ip(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| part.parse::<u8>().is_ok())
}

#[tauri::command]
async fn change_framework_password(ips: Vec<String>) -> Result<Vec<PasswordChangeResult>, String> {
    const OLD_PASSWORD_HASH: &str =
        "8d969eef6ecad3c29a3a629280e686cf0c3f5d5a86aff3ca12020c923adc6c92";
    const NEW_PASSWORD_HASH: &str =
        "4d5c5f61bb3d2c299d3211c2992a28a7849b6ce933919c399ce24903c1715d45";

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

        // Step 1: Login
        let login_url = format!("http://{}:21900/openAPI/userMgr/v1/login", ip);
        let login_body = json!({
            "userName": "admin",
            "userPasswd": OLD_PASSWORD_HASH,
            "isUnlockLogin": false
        });

        let token = match client
            .post(&login_url)
            .header("Authorization", "ab94186a-165b-4a18-9337-a9e33809d592")
            .header("content-type", "application/json")
            .json(&login_body)
            .send()
            .await
        {
            Ok(response) => {
                match response.json::<serde_json::Value>().await {
                    Ok(json) => {
                        // Validate response structure
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

        // Step 2: Change Password
        let change_passwd_url = format!("http://{}:21900/openAPI/userMgr/v1/changePasswd", ip);
        let change_passwd_body = json!({
            "userName": "admin",
            "oldUserPasswd": OLD_PASSWORD_HASH,
            "newUserPasswd": NEW_PASSWORD_HASH
        });

        match client
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
                        // Validate response structure
                        if json.get("code").and_then(|v| v.as_i64()) == Some(0) {
                            results.push(PasswordChangeResult {
                                ip: ip.to_string(),
                                success: true,
                                message: "Success".to_string(),
                                failedAt: None,
                            });
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
                        }
                    }
                    Err(e) => {
                        results.push(PasswordChangeResult {
                            ip: ip.to_string(),
                            success: false,
                            message: format!("Change password response parse error: {}", e),
                            failedAt: Some("changePasswd".to_string()),
                        });
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
            }
        }
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

                if !is_quitting && should_close_to_tray(&window.app_handle()) {
                    api.prevent_close();
                    hide_main_window(&window.app_handle());
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
                        app.exit(0);
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
            let _ = sync_launch_on_startup(config.launch_and_auto_scan);
            app.manage(AppState {
                config: Arc::new(Mutex::new(config)),
                is_scanning: Arc::new(AtomicBool::new(false)),
                is_manually_deploying: Arc::new(AtomicBool::new(false)),
                should_cancel: Arc::new(AtomicBool::new(false)),
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
            history::get_history,
            history::clear_history,
            history::add_system_event,
            test_ssh_connection,
            manual_deploy,
            temporary_copy,
            get_app_paths,
            open_path_parent,
            open_directory,
            change_framework_password
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
