use crate::task_domain::TaskGroup;
use crate::task_events::TaskGroupListItem;
use crate::task_manager::{StartManualCopyRequest, StartManualDeployRequest, TaskRunHandle};
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

#[tauri::command]
pub fn start_manual_copy_task(
    state: State<'_, crate::AppState>,
    request: StartManualCopyRequest,
) -> Result<TaskRunHandle, String> {
    state.task_manager.begin_manual_copy_run(request)
}

#[tauri::command]
pub fn start_manual_deploy_task(
    state: State<'_, crate::AppState>,
    request: StartManualDeployRequest,
) -> Result<TaskRunHandle, String> {
    state.task_manager.begin_manual_deploy_run(request)
}
