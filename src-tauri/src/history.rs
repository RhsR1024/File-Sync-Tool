use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;
use chrono::Local;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: String,
    
    // New fields for generic events
    #[serde(default)]
    pub action_type: String, // "COPY", "PAUSE", "RESUME", "START_TASK", "STOP_TASK", "CONFIG", "CANCEL"
    #[serde(default)]
    pub description: String,

    // Copy specific (Optional or empty)
    pub folder_name: String,
    pub source_path: String,
    pub target_path: String,
    pub copied_files_count: usize,
    pub total_size: u64,
    pub files: Vec<String>, 
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct HistoryStore {
    pub entries: Vec<HistoryEntry>,
}

#[tauri::command]
pub fn add_system_event(app_handle: tauri::AppHandle, action: String, desc: String) {
    let entry = HistoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Local::now().to_rfc3339(),
        action_type: action,
        description: desc,
        folder_name: "".to_string(),
        source_path: "".to_string(),
        target_path: "".to_string(),
        copied_files_count: 0,
        total_size: 0,
        files: vec![],
    };
    add_history_entry(&app_handle, entry);
}

pub fn add_history_entry<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, entry: HistoryEntry) {
    let mut store = load_history(app_handle);
    // Prepend
    store.entries.insert(0, entry);
    // Keep max 100 entries
    if store.entries.len() > 100 {
        store.entries.truncate(100);
    }
    save_history(app_handle, &store);
}

#[tauri::command]
pub fn get_history(app_handle: tauri::AppHandle) -> HistoryStore {
    load_history(&app_handle)
}

#[tauri::command]
pub fn clear_history(app_handle: tauri::AppHandle) -> Result<(), String> {
    let path = get_history_path(&app_handle);
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn get_history_path<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> PathBuf {
    app_handle.path().app_data_dir().unwrap().join("history.json")
}

pub fn load_history<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> HistoryStore {
    let path = get_history_path(app_handle);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(store) = serde_json::from_str(&content) {
                return store;
            }
        }
    }
    HistoryStore::default()
}

pub fn save_history<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, store: &HistoryStore) {
    let path = get_history_path(app_handle);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, serde_json::to_string_pretty(store).unwrap_or_default());
}
