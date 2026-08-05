use crate::task_domain::TaskState;
use std::fs;
use std::path::PathBuf;

const TASK_STATE_FILE_NAME: &str = "task_state.json";

pub fn load_task_state(app_handle: &tauri::AppHandle) -> TaskState {
    let path = task_state_path(app_handle);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(mut state) = serde_json::from_str::<TaskState>(&content) {
                // State written by older versions has no checkpoint field. The
                // file modification time is the closest safe interruption
                // boundary available for that one-time migration.
                if state.last_checkpoint_at.is_none() {
                    state.last_checkpoint_at = fs::metadata(&path)
                        .ok()
                        .and_then(|metadata| metadata.modified().ok())
                        .map(|timestamp| {
                            chrono::DateTime::<chrono::Local>::from(timestamp).to_rfc3339()
                        });
                }
                return state;
            }
        }
    }

    TaskState {
        version: 1,
        last_checkpoint_at: None,
        groups: vec![],
    }
}

pub fn save_task_state(app_handle: &tauri::AppHandle, state: &TaskState) -> Result<(), String> {
    let path = task_state_path(app_handle);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let mut checkpoint = state.clone();
    checkpoint.last_checkpoint_at = Some(chrono::Local::now().to_rfc3339());
    let payload = serde_json::to_string_pretty(&checkpoint).map_err(|error| error.to_string())?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, payload).map_err(|error| error.to_string())?;
    fs::rename(tmp_path, path).map_err(|error| error.to_string())
}

fn task_state_path(app_handle: &tauri::AppHandle) -> PathBuf {
    crate::config::get_data_dir(app_handle).join(TASK_STATE_FILE_NAME)
}
