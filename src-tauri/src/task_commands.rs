use crate::task_domain::TaskGroup;
use crate::task_events::TaskGroupListItem;
use tauri::State;

#[tauri::command]
pub fn list_task_groups(state: State<'_, crate::AppState>) -> Vec<TaskGroupListItem> {
    state.task_manager.list_groups()
}

#[tauri::command]
pub fn get_task_group_detail(
    state: State<'_, crate::AppState>,
    task_group_id: String,
) -> Result<TaskGroup, String> {
    state
        .task_manager
        .get_group_detail(&task_group_id)
        .ok_or_else(|| format!("Task group not found: {task_group_id}"))
}

#[tauri::command]
pub fn clear_task_group(
    state: State<'_, crate::AppState>,
    task_group_id: String,
) -> Result<(), String> {
    state.task_manager.clear_task_group(&task_group_id)
}

#[tauri::command]
pub fn clear_task_groups(state: State<'_, crate::AppState>) -> Result<(), String> {
    state.task_manager.clear_task_groups()
}
