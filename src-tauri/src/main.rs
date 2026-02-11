// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod scanner;

use config::AppConfig;
use scanner::ScanResult;
use std::sync::Mutex;
use tauri::{State, Manager};

struct AppState {
    config: Mutex<AppConfig>,
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
async fn scan_now(state: State<'_, AppState>) -> Result<ScanResult, String> {
    let config = state.config.lock().unwrap().clone();
    Ok(scanner::scan_and_copy(&config).await)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            let config = config::load_config(app.handle());
            app.manage(AppState {
                config: Mutex::new(config),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_config, save_config_cmd, scan_now])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
