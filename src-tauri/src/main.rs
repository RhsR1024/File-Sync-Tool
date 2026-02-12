// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod scanner;
mod history;
mod deploy;

use config::{AppConfig, DeployServer};
use scanner::ScanResult;
use history::HistoryStore;
use std::sync::{Mutex, Arc};
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{State, Manager, Emitter};

struct AppState {
    config: Mutex<AppConfig>,
    is_scanning: Arc<AtomicBool>,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
}

#[tauri::command]
fn get_config(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config_cmd(app_handle: tauri::AppHandle, state: State<AppState>, config: AppConfig) -> Result<(), String> {
    *state.config.lock().unwrap() = config.clone();
    config::save_config(&app_handle, &config)
}

#[tauri::command]
async fn scan_now(app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<ScanResult, String> {
    if state.is_scanning.load(Ordering::SeqCst) {
        return Err("Scan already in progress".to_string());
    }
    
    state.is_scanning.store(true, Ordering::SeqCst);
    state.should_cancel.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);
    
    let config = state.config.lock().unwrap().clone();
    let result = scanner::scan_and_copy(&app_handle, &config, state.should_cancel.clone(), state.is_paused.clone()).await;
    
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
async fn manual_deploy(app_handle: tauri::AppHandle, state: State<'_, AppState>, server: DeployServer, postCommands: Vec<String>, localPath: String, remotePath: String) -> Result<(), String> {
    if state.is_scanning.load(Ordering::SeqCst) {
        return Err("Operation already in progress".to_string());
    }
    
    state.is_scanning.store(true, Ordering::SeqCst);
    state.should_cancel.store(false, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);

    let should_cancel = state.should_cancel.clone();
    let is_paused = state.is_paused.clone();
    let is_scanning = state.is_scanning.clone();

    // This runs in async context, but deploy_manual uses blocking SSH.
    // We should spawn blocking.
    let result = tauri::async_runtime::spawn_blocking(move || {
        deploy::deploy_manual(&app_handle, &server, &postCommands, &localPath, &remotePath, should_cancel, is_paused)
    }).await.map_err(|e| e.to_string())?;
    
    is_scanning.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
fn get_app_paths(app_handle: tauri::AppHandle) -> (String, String) {
    let config = config::get_config_path(&app_handle).to_string_lossy().to_string();
    let log = config::get_log_path(&app_handle).to_string_lossy().to_string();
    (config, log)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app.emit("single-instance", ());
        }))
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            let config = config::load_config(app.handle());
            app.manage(AppState {
                config: Mutex::new(config),
                is_scanning: Arc::new(AtomicBool::new(false)),
                should_cancel: Arc::new(AtomicBool::new(false)),
                is_paused: Arc::new(AtomicBool::new(false)),
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
            get_app_paths
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
