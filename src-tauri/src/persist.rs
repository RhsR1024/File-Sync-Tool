use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct UiState {
    pub logs: Vec<Value>,
    pub task_records: Vec<Value>,
}

#[tauri::command]
pub fn save_ui_state(
    app_handle: tauri::AppHandle,
    logs: Vec<Value>,
    task_records: Vec<Value>,
) -> Result<(), String> {
    let state = UiState { logs, task_records };
    let path = get_ui_state_path(&app_handle);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_ui_state(app_handle: tauri::AppHandle) -> UiState {
    let path = get_ui_state_path(&app_handle);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<UiState>(&content) {
                return state;
            }
        }
    }
    UiState::default()
}

fn get_ui_state_path(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_data_dir()
        .unwrap()
        .join("ui_state.json")
}

#[tauri::command]
pub fn save_kv(app_handle: tauri::AppHandle, key: String, value: Value) -> Result<(), String> {
    let path = get_kv_path(&app_handle, &key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, &json).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_kv(app_handle: tauri::AppHandle, key: String) -> Option<Value> {
    let path = get_kv_path(&app_handle, &key);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            return serde_json::from_str(&content).ok();
        }
    }
    None
}

fn get_kv_path(app_handle: &tauri::AppHandle, key: &str) -> PathBuf {
    let safe_key = key.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    app_handle
        .path()
        .app_data_dir()
        .unwrap()
        .join("kv")
        .join(format!("{}.json", safe_key))
}
