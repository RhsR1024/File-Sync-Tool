use crate::task_domain::TaskState;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

const TASK_STATE_FILE_NAME: &str = "task_state.json";

pub fn load_task_state(app_handle: &tauri::AppHandle) -> TaskState {
    let path = task_state_path(app_handle);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<TaskState>(&content) {
                return state;
            }
        }
    }

    TaskState {
        version: 1,
        groups: vec![],
    }
}

pub fn save_task_state(app_handle: &tauri::AppHandle, state: &TaskState) -> Result<(), String> {
    let path = task_state_path(app_handle);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let payload = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, payload).map_err(|error| error.to_string())?;
    fs::rename(tmp_path, path).map_err(|error| error.to_string())
}

fn task_state_path(app_handle: &tauri::AppHandle) -> PathBuf {
    crate::config::get_data_dir(app_handle).join(TASK_STATE_FILE_NAME)
}
