use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskMergeKey(String);

impl TaskMergeKey {
    pub fn new(
        task_config_id: Option<String>,
        local_target_path: String,
        folder_name: String,
    ) -> Self {
        let task_id = task_config_id.unwrap_or_else(|| "manual".to_string());
        let normalized_path = normalize_path_for_merge(&local_target_path);
        let normalized_folder = normalize_token(&folder_name);
        Self(format!("{task_id}||{normalized_path}||{normalized_folder}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskSummaryStatus {
    Queued,
    Copying,
    Paused,
    Cancelling,
    CopyCompleted,
    LocalExecuting,
    Deploying,
    PartialFailed,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    Running,
    Success,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeployStage {
    Pending,
    Connecting,
    Uploading,
    ExecutingCommands,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunType {
    CopyAndDeploy,
    DeployRetry,
    ManualDeploy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskTriggerSource {
    Scheduled,
    Manual,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskSourceType {
    Scheduled,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CopyState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalExecState {
    NotStarted,
    Running,
    Completed,
    PartialFailed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeployState {
    NotStarted,
    Pending,
    Running,
    Completed,
    PartialFailed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployAttempt {
    pub attempt_id: String,
    pub task_group_id: String,
    pub run_id: String,
    pub server_id: String,
    pub server_name: String,
    pub attempt_no: u32,
    pub trigger_source: TaskTriggerSource,
    pub stage: DeployStage,
    pub status: AttemptStatus,
    pub remote_target: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_seconds: u64,
    pub progress_percentage: Option<f64>,
    pub error_phase: Option<DeployStage>,
    pub error_message: Option<String>,
    pub last_log_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRun {
    pub run_id: String,
    pub task_group_id: String,
    pub run_type: TaskRunType,
    pub trigger_source: TaskTriggerSource,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub copy_phase: CopyState,
    #[serde(default = "default_local_exec_state")]
    pub local_exec_phase: LocalExecState,
    pub deploy_phase: DeployState,
    pub deploy_attempts: Vec<DeployAttempt>,
    pub attempt_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerRollup {
    pub server_id: String,
    pub server_name: String,
    pub latest_status: AttemptStatus,
    pub latest_attempt_id: Option<String>,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_error_message: Option<String>,
    pub attempt_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGroup {
    pub task_group_id: String,
    pub merge_key: TaskMergeKey,
    pub task_config_id: Option<String>,
    pub source_type: TaskSourceType,
    pub display_name: String,
    pub folder_name: String,
    pub source_path: String,
    pub local_target_path: String,
    pub copy_status: CopyState,
    pub local_exec_status: LocalExecState,
    pub deploy_status: DeployState,
    pub summary_status: TaskSummaryStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_seconds: u64,
    pub latest_run_id: Option<String>,
    pub had_failures: bool,
    pub server_rollups: Vec<ServerRollup>,
    pub runs: Vec<TaskRun>,
    #[serde(default, skip_serializing)]
    pub paused: bool,
    #[serde(default, skip_serializing)]
    pub cancel_requested: bool,
    #[serde(default, skip_serializing)]
    pub paused_at: Option<String>,
    #[serde(default, skip_serializing)]
    pub accumulated_paused_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub version: u32,
    pub groups: Vec<TaskGroup>,
}

impl TaskState {
    pub fn mark_in_progress_as_interrupted(&mut self) {
        let now = current_timestamp();
        for group in &mut self.groups {
            group.mark_in_progress_as_interrupted(&now);
        }
    }

    #[cfg(test)]
    pub fn sample_running() -> Self {
        Self {
            version: 1,
            groups: vec![TaskGroup {
                task_group_id: "group-1".to_string(),
                merge_key: TaskMergeKey::new(
                    Some("task-a".to_string()),
                    "E:\\target\\builds".to_string(),
                    "Release_01".to_string(),
                ),
                task_config_id: Some("task-a".to_string()),
                source_type: TaskSourceType::Scheduled,
                display_name: "Release_01".to_string(),
                folder_name: "Release_01".to_string(),
                source_path: "C:\\source\\Release_01".to_string(),
                local_target_path: "E:\\target\\builds".to_string(),
                copy_status: CopyState::Completed,
                local_exec_status: LocalExecState::NotStarted,
                deploy_status: DeployState::Running,
                summary_status: TaskSummaryStatus::Deploying,
                started_at: "2026-04-02T12:00:00+08:00".to_string(),
                finished_at: None,
                elapsed_seconds: 30,
                latest_run_id: Some("run-1".to_string()),
                had_failures: false,
                server_rollups: vec![],
                paused: false,
                cancel_requested: false,
                paused_at: None,
                accumulated_paused_seconds: 0,
                runs: vec![TaskRun {
                    run_id: "run-1".to_string(),
                    task_group_id: "group-1".to_string(),
                    run_type: TaskRunType::CopyAndDeploy,
                    trigger_source: TaskTriggerSource::Scheduled,
                    started_at: "2026-04-02T12:00:00+08:00".to_string(),
                    finished_at: None,
                    copy_phase: CopyState::Completed,
                    local_exec_phase: LocalExecState::NotStarted,
                    deploy_phase: DeployState::Running,
                    attempt_ids: vec!["attempt-1".to_string()],
                    deploy_attempts: vec![DeployAttempt {
                        attempt_id: "attempt-1".to_string(),
                        task_group_id: "group-1".to_string(),
                        run_id: "run-1".to_string(),
                        server_id: "server-a".to_string(),
                        server_name: "Server A".to_string(),
                        attempt_no: 1,
                        trigger_source: TaskTriggerSource::Scheduled,
                        stage: DeployStage::Connecting,
                        status: AttemptStatus::Running,
                        remote_target: Some("/srv/release".to_string()),
                        started_at: "2026-04-02T12:00:10+08:00".to_string(),
                        finished_at: None,
                        elapsed_seconds: 20,
                        progress_percentage: None,
                        error_phase: None,
                        error_message: None,
                        last_log_excerpt: None,
                    }],
                }],
            }],
        }
    }
}

impl TaskGroup {
    pub fn mark_in_progress_as_interrupted(&mut self, finished_at: &str) {
        let mut changed = false;

        for run in &mut self.runs {
            changed |= run.mark_in_progress_as_interrupted(finished_at);
        }

        // In-flight pause/cancel intent does not survive an app restart.
        self.paused = false;
        self.cancel_requested = false;

        if changed
            || matches!(
                self.summary_status,
                TaskSummaryStatus::Queued
                    | TaskSummaryStatus::Copying
                    | TaskSummaryStatus::Paused
                    | TaskSummaryStatus::Cancelling
                    | TaskSummaryStatus::CopyCompleted
                    | TaskSummaryStatus::LocalExecuting
                    | TaskSummaryStatus::Deploying
            )
        {
            self.summary_status = TaskSummaryStatus::Interrupted;
        }

        self.refresh_from_runs();

        if self.finished_at.is_none() && self.summary_status == TaskSummaryStatus::Interrupted {
            self.finished_at = Some(finished_at.to_string());
        }
        let total_elapsed = compute_elapsed_seconds(&self.started_at, self.finished_at.as_deref());
        // Subtract any time the task was paused (including current pause if ongoing)
        let mut paused_duration = self.accumulated_paused_seconds;
        if let Some(paused_at_str) = &self.paused_at {
            paused_duration += compute_elapsed_seconds(paused_at_str, None);
        }
        self.elapsed_seconds = total_elapsed.saturating_sub(paused_duration);
    }

    pub fn next_attempt_no_for_server(&self, server_id: &str) -> u32 {
        self.runs
            .iter()
            .flat_map(|run| run.deploy_attempts.iter())
            .filter(|attempt| attempt.server_id == server_id)
            .count() as u32
            + 1
    }

    pub fn refresh_from_runs(&mut self) {
        let Some(latest_run) = self.runs.last_mut() else {
            return;
        };

        latest_run.sync_attempt_ids();
        latest_run.refresh_deploy_phase();

        self.latest_run_id = Some(latest_run.run_id.clone());
        self.started_at = latest_run.started_at.clone();
        self.finished_at = latest_run.finished_at.clone();
        self.copy_status = latest_run.copy_phase.clone();
        self.local_exec_status = latest_run.local_exec_phase.clone();
        self.deploy_status = latest_run.deploy_phase.clone();
        let base_summary = summarize_group(
            &latest_run.copy_phase,
            &latest_run.local_exec_phase,
            &latest_run.deploy_phase,
        );

        // Clear pause/cancel overrides once the copy phase reaches a terminal state,
        // so post-copy phases (e.g. Deploying) surface normally.
        let copy_terminal = matches!(
            latest_run.copy_phase,
            CopyState::Completed
                | CopyState::Failed
                | CopyState::Cancelled
                | CopyState::Interrupted
        );
        if copy_terminal {
            self.paused = false;
            self.cancel_requested = false;
        }

        self.summary_status = if self.cancel_requested && base_summary == TaskSummaryStatus::Copying
        {
            TaskSummaryStatus::Cancelling
        } else if self.paused && base_summary == TaskSummaryStatus::Copying {
            TaskSummaryStatus::Paused
        } else {
            base_summary
        };
        self.server_rollups = build_server_rollups(&self.runs);
        self.had_failures = self.runs.iter().any(|run| {
            run.copy_phase == CopyState::Failed
                || run
                    .deploy_attempts
                    .iter()
                    .any(|attempt| attempt.status == AttemptStatus::Failed)
        });
        let total_elapsed = compute_elapsed_seconds(&self.started_at, self.finished_at.as_deref());
        // Subtract any time the task was paused (including current pause if ongoing)
        let mut paused_duration = self.accumulated_paused_seconds;
        if let Some(paused_at_str) = &self.paused_at {
            paused_duration += compute_elapsed_seconds(paused_at_str, None);
        }
        self.elapsed_seconds = total_elapsed.saturating_sub(paused_duration);
    }
}

impl TaskRun {
    pub fn mark_in_progress_as_interrupted(&mut self, finished_at: &str) -> bool {
        let mut changed = false;

        if matches!(self.copy_phase, CopyState::Pending | CopyState::Running) {
            self.copy_phase = CopyState::Interrupted;
            changed = true;
        }

        if matches!(self.local_exec_phase, LocalExecState::Running) {
            self.local_exec_phase = LocalExecState::Interrupted;
            changed = true;
        }

        if matches!(
            self.deploy_phase,
            DeployState::Pending | DeployState::Running
        ) {
            self.deploy_phase = DeployState::Interrupted;
            changed = true;
        }

        for attempt in &mut self.deploy_attempts {
            if attempt.status == AttemptStatus::Running {
                attempt.status = AttemptStatus::Interrupted;
                attempt
                    .error_phase
                    .get_or_insert_with(|| attempt.stage.clone());
                attempt.finished_at = Some(finished_at.to_string());
                attempt.elapsed_seconds =
                    compute_elapsed_seconds(&attempt.started_at, attempt.finished_at.as_deref());
                changed = true;
            }
        }

        if changed && self.finished_at.is_none() {
            self.finished_at = Some(finished_at.to_string());
        }

        self.sync_attempt_ids();
        self.refresh_deploy_phase();
        changed
    }

    pub fn sync_attempt_ids(&mut self) {
        self.attempt_ids = self
            .deploy_attempts
            .iter()
            .map(|attempt| attempt.attempt_id.clone())
            .collect();
    }

    pub fn refresh_deploy_phase(&mut self) {
        if self.deploy_attempts.is_empty() {
            if self.deploy_phase == DeployState::Running {
                self.deploy_phase = DeployState::NotStarted;
            }
            return;
        }

        let any_running = self
            .deploy_attempts
            .iter()
            .any(|attempt| attempt.status == AttemptStatus::Running);
        if any_running {
            self.deploy_phase = DeployState::Running;
            self.finished_at = None;
            return;
        }

        let success_count = self
            .deploy_attempts
            .iter()
            .filter(|attempt| attempt.status == AttemptStatus::Success)
            .count();
        let failed_count = self
            .deploy_attempts
            .iter()
            .filter(|attempt| attempt.status == AttemptStatus::Failed)
            .count();
        let interrupted_count = self
            .deploy_attempts
            .iter()
            .filter(|attempt| attempt.status == AttemptStatus::Interrupted)
            .count();
        let cancelled_count = self
            .deploy_attempts
            .iter()
            .filter(|attempt| attempt.status == AttemptStatus::Cancelled)
            .count();
        let total = self.deploy_attempts.len();

        self.deploy_phase = if success_count == total {
            DeployState::Completed
        } else if failed_count == total {
            DeployState::Failed
        } else if interrupted_count == total {
            DeployState::Interrupted
        } else if cancelled_count == total {
            DeployState::Cancelled
        } else if interrupted_count > 0 && failed_count == 0 && cancelled_count == 0 {
            DeployState::Interrupted
        } else if failed_count > 0 || cancelled_count > 0 {
            DeployState::PartialFailed
        } else {
            DeployState::Completed
        };
    }
}

fn default_local_exec_state() -> LocalExecState {
    LocalExecState::NotStarted
}

fn normalize_path_for_merge(value: &str) -> String {
    let mut normalized = value.trim().replace('/', "\\");
    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }
    normalized.to_lowercase()
}

fn normalize_token(value: &str) -> String {
    value.trim().to_lowercase()
}

fn current_timestamp() -> String {
    chrono::Local::now().to_rfc3339()
}

fn compute_elapsed_seconds(started_at: &str, finished_at: Option<&str>) -> u64 {
    let Ok(start) = chrono::DateTime::parse_from_rfc3339(started_at) else {
        return 0;
    };
    let end = finished_at
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .unwrap_or_else(|| chrono::Local::now().fixed_offset());
    let seconds = end.signed_duration_since(start).num_seconds();
    seconds.max(0) as u64
}

fn summarize_group(
    copy_phase: &CopyState,
    local_exec_phase: &LocalExecState,
    deploy_phase: &DeployState,
) -> TaskSummaryStatus {
    match copy_phase {
        CopyState::Pending | CopyState::Running => TaskSummaryStatus::Copying,
        CopyState::Failed => TaskSummaryStatus::Failed,
        CopyState::Cancelled => TaskSummaryStatus::Cancelled,
        CopyState::Interrupted => TaskSummaryStatus::Interrupted,
        CopyState::Completed => match local_exec_phase {
            LocalExecState::Running => TaskSummaryStatus::LocalExecuting,
            LocalExecState::Failed => TaskSummaryStatus::Failed,
            LocalExecState::Cancelled => TaskSummaryStatus::Cancelled,
            LocalExecState::Interrupted => TaskSummaryStatus::Interrupted,
            LocalExecState::NotStarted
            | LocalExecState::Completed
            | LocalExecState::PartialFailed => {
                // local exec done (or not needed), check deploy
                match deploy_phase {
                    DeployState::NotStarted => {
                        if *local_exec_phase == LocalExecState::PartialFailed {
                            TaskSummaryStatus::PartialFailed
                        } else {
                            TaskSummaryStatus::Completed
                        }
                    }
                    DeployState::Pending => TaskSummaryStatus::CopyCompleted,
                    DeployState::Running => TaskSummaryStatus::Deploying,
                    DeployState::Completed => {
                        if *local_exec_phase == LocalExecState::PartialFailed {
                            TaskSummaryStatus::PartialFailed
                        } else {
                            TaskSummaryStatus::Completed
                        }
                    }
                    DeployState::PartialFailed => TaskSummaryStatus::PartialFailed,
                    DeployState::Failed => TaskSummaryStatus::Failed,
                    DeployState::Cancelled => TaskSummaryStatus::Cancelled,
                    DeployState::Interrupted => TaskSummaryStatus::Interrupted,
                }
            }
        },
    }
}

fn build_server_rollups(runs: &[TaskRun]) -> Vec<ServerRollup> {
    let mut rollups = BTreeMap::<String, ServerRollup>::new();

    for run in runs {
        for attempt in &run.deploy_attempts {
            let entry = rollups
                .entry(attempt.server_id.clone())
                .or_insert_with(|| ServerRollup {
                    server_id: attempt.server_id.clone(),
                    server_name: attempt.server_name.clone(),
                    latest_status: attempt.status.clone(),
                    latest_attempt_id: Some(attempt.attempt_id.clone()),
                    success_count: 0,
                    failure_count: 0,
                    last_error_message: None,
                    attempt_ids: vec![],
                });

            if attempt.status == AttemptStatus::Success {
                entry.success_count += 1;
            }
            if attempt.status == AttemptStatus::Failed {
                entry.failure_count += 1;
            }

            if attempt.error_message.is_some() {
                entry.last_error_message = attempt.error_message.clone();
            }

            entry.latest_status = attempt.status.clone();
            entry.latest_attempt_id = Some(attempt.attempt_id.clone());
            entry.attempt_ids.push(attempt.attempt_id.clone());
        }
    }

    rollups.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_key_normalizes_windows_paths() {
        let key = TaskMergeKey::new(
            Some("task-a".into()),
            "E:/Target/Builds/".into(),
            "Release_01".into(),
        );

        assert_eq!(key.as_str(), "task-a||e:\\target\\builds||release_01");
    }

    #[test]
    fn interrupted_conversion_marks_running_group_and_attempts() {
        let mut state = TaskState::sample_running();
        state.mark_in_progress_as_interrupted();

        assert_eq!(
            state.groups[0].summary_status,
            TaskSummaryStatus::Interrupted
        );
        assert_eq!(
            state.groups[0].runs[0].deploy_attempts[0].status,
            AttemptStatus::Interrupted
        );
    }

    #[test]
    fn mixed_success_and_interrupted_deploy_is_interrupted() {
        let mut run = TaskRun {
            run_id: "run-1".to_string(),
            task_group_id: "group-1".to_string(),
            run_type: TaskRunType::CopyAndDeploy,
            trigger_source: TaskTriggerSource::Manual,
            started_at: "2026-04-02T12:00:00+08:00".to_string(),
            finished_at: Some("2026-04-02T12:00:30+08:00".to_string()),
            copy_phase: CopyState::Completed,
            local_exec_phase: LocalExecState::NotStarted,
            deploy_phase: DeployState::Running,
            deploy_attempts: vec![
                DeployAttempt {
                    attempt_id: "attempt-1".to_string(),
                    task_group_id: "group-1".to_string(),
                    run_id: "run-1".to_string(),
                    server_id: "server-a".to_string(),
                    server_name: "Server A".to_string(),
                    attempt_no: 1,
                    trigger_source: TaskTriggerSource::Manual,
                    stage: DeployStage::Done,
                    status: AttemptStatus::Success,
                    remote_target: Some("/srv/release".to_string()),
                    started_at: "2026-04-02T12:00:05+08:00".to_string(),
                    finished_at: Some("2026-04-02T12:00:15+08:00".to_string()),
                    elapsed_seconds: 10,
                    progress_percentage: Some(1.0),
                    error_phase: None,
                    error_message: None,
                    last_log_excerpt: None,
                },
                DeployAttempt {
                    attempt_id: "attempt-2".to_string(),
                    task_group_id: "group-1".to_string(),
                    run_id: "run-1".to_string(),
                    server_id: "server-b".to_string(),
                    server_name: "Server B".to_string(),
                    attempt_no: 2,
                    trigger_source: TaskTriggerSource::Manual,
                    stage: DeployStage::Uploading,
                    status: AttemptStatus::Interrupted,
                    remote_target: Some("/srv/release".to_string()),
                    started_at: "2026-04-02T12:00:05+08:00".to_string(),
                    finished_at: Some("2026-04-02T12:00:20+08:00".to_string()),
                    elapsed_seconds: 15,
                    progress_percentage: Some(0.4),
                    error_phase: Some(DeployStage::Uploading),
                    error_message: Some("Interrupted".to_string()),
                    last_log_excerpt: None,
                },
            ],
            attempt_ids: vec![],
        };

        run.refresh_deploy_phase();

        assert_eq!(run.deploy_phase, DeployState::Interrupted);
    }
}
