use crate::task_domain::{ServerRollup, TaskGroup};
use serde::Serialize;

pub const TASK_GROUPS_SNAPSHOT_EVENT: &str = "task-groups-snapshot";
pub const TASK_GROUP_DETAIL_SNAPSHOT_EVENT: &str = "task-group-detail-snapshot";

#[derive(Debug, Clone, Serialize)]
pub struct TaskGroupListItem {
    pub task_group_id: String,
    pub merge_key: String,
    pub task_config_id: Option<String>,
    pub display_name: String,
    pub folder_name: String,
    pub source_path: String,
    pub local_target_path: String,
    pub copy_status: crate::task_domain::CopyState,
    pub deploy_status: crate::task_domain::DeployState,
    pub summary_status: crate::task_domain::TaskSummaryStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_seconds: u64,
    pub latest_run_id: Option<String>,
    pub had_failures: bool,
    pub server_rollups: Vec<ServerRollup>,
}

impl From<&TaskGroup> for TaskGroupListItem {
    fn from(group: &TaskGroup) -> Self {
        Self {
            task_group_id: group.task_group_id.clone(),
            merge_key: group.merge_key.as_str().to_string(),
            task_config_id: group.task_config_id.clone(),
            display_name: group.display_name.clone(),
            folder_name: group.folder_name.clone(),
            source_path: group.source_path.clone(),
            local_target_path: group.local_target_path.clone(),
            copy_status: group.copy_status.clone(),
            deploy_status: group.deploy_status.clone(),
            summary_status: group.summary_status.clone(),
            started_at: group.started_at.clone(),
            finished_at: group.finished_at.clone(),
            elapsed_seconds: group.elapsed_seconds,
            latest_run_id: group.latest_run_id.clone(),
            had_failures: group.had_failures,
            server_rollups: group.server_rollups.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskGroupsSnapshot {
    pub groups: Vec<TaskGroupListItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskGroupDetailSnapshot {
    pub task_group_id: String,
    pub group: TaskGroup,
}
