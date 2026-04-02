use crate::task_domain::{
    CopyState, DeployAttempt, DeployStage, DeployState, TaskGroup, TaskMergeKey, TaskRun,
    TaskRunType, TaskSourceType, TaskState, TaskSummaryStatus, TaskTriggerSource,
};
use crate::task_events::{
    TaskGroupDetailSnapshot, TaskGroupListItem, TaskGroupsSnapshot,
    TASK_GROUP_DETAIL_SNAPSHOT_EVENT, TASK_GROUPS_SNAPSHOT_EVENT,
};
use crate::task_persist::{load_task_state, save_task_state};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[derive(Debug, Clone)]
pub struct TaskStartRequest {
    pub task_config_id: Option<String>,
    pub display_name: String,
    pub folder_name: String,
    pub source_path: String,
    pub local_target_path: String,
    pub source_type: TaskSourceType,
    pub trigger_source: TaskTriggerSource,
}

#[derive(Debug, Clone)]
pub struct TaskRunHandle {
    pub task_group_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone)]
pub struct DeployTarget {
    pub server_id: String,
    pub server_name: String,
    pub remote_target: String,
    pub trigger_source: TaskTriggerSource,
}

#[derive(Clone)]
pub struct DeployTrackingContext {
    task_manager: TaskManager,
    task_group_id: String,
    run_id: String,
}

struct TaskManagerInner {
    app_handle: Option<tauri::AppHandle>,
    state: Mutex<TaskState>,
    persist_scheduled: AtomicBool,
}

#[derive(Clone)]
pub struct TaskManager {
    inner: Arc<TaskManagerInner>,
}

impl TaskManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        let mut state = load_task_state(&app_handle);
        state.mark_in_progress_as_interrupted();

        let manager = Self {
            inner: Arc::new(TaskManagerInner {
                app_handle: Some(app_handle.clone()),
                state: Mutex::new(state),
                persist_scheduled: AtomicBool::new(false),
            }),
        };

        let snapshot = manager.snapshot_state();
        let _ = save_task_state(&app_handle, &snapshot);
        manager.emit_snapshots(None);
        manager
    }

    pub fn new_in_memory() -> Self {
        Self {
            inner: Arc::new(TaskManagerInner {
                app_handle: None,
                state: Mutex::new(TaskState {
                    version: 1,
                    groups: vec![],
                }),
                persist_scheduled: AtomicBool::new(false),
            }),
        }
    }

    pub fn begin_scheduled_copy(&self, request: TaskStartRequest) -> TaskRunHandle {
        let started_at = current_timestamp();
        let merge_key = TaskMergeKey::new(
            request.task_config_id.clone(),
            request.local_target_path.clone(),
            request.folder_name.clone(),
        );
        let proposed_group_id = format!("group-{}", uuid::Uuid::new_v4());
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let actual_group_id;

        {
            let mut state = self.inner.state.lock().unwrap();
            let group_index = state
                .groups
                .iter()
                .position(|group| group.merge_key == merge_key)
                .unwrap_or_else(|| {
                    state.groups.push(TaskGroup {
                        task_group_id: proposed_group_id.clone(),
                        merge_key: merge_key.clone(),
                        task_config_id: request.task_config_id.clone(),
                        source_type: request.source_type.clone(),
                        display_name: request.display_name.clone(),
                        folder_name: request.folder_name.clone(),
                        source_path: request.source_path.clone(),
                        local_target_path: request.local_target_path.clone(),
                        copy_status: CopyState::Pending,
                        deploy_status: DeployState::NotStarted,
                        summary_status: TaskSummaryStatus::Queued,
                        started_at: started_at.clone(),
                        finished_at: None,
                        elapsed_seconds: 0,
                        latest_run_id: None,
                        had_failures: false,
                        server_rollups: vec![],
                        runs: vec![],
                    });
                    state.groups.len() - 1
                });

            let group = &mut state.groups[group_index];
            actual_group_id = group.task_group_id.clone();
            group.task_group_id = group.task_group_id.clone();
            group.merge_key = merge_key;
            group.task_config_id = request.task_config_id;
            group.source_type = request.source_type;
            group.display_name = request.display_name;
            group.folder_name = request.folder_name;
            group.source_path = request.source_path;
            group.local_target_path = request.local_target_path;
            group.finished_at = None;

            group.runs.push(TaskRun {
                run_id: run_id.clone(),
                task_group_id: group.task_group_id.clone(),
                run_type: TaskRunType::CopyAndDeploy,
                trigger_source: request.trigger_source,
                started_at: started_at.clone(),
                finished_at: None,
                copy_phase: CopyState::Running,
                deploy_phase: DeployState::NotStarted,
                deploy_attempts: vec![],
                attempt_ids: vec![],
            });
            group.refresh_from_runs();
        }

        self.after_change(Some(actual_group_id.as_str()));
        TaskRunHandle {
            task_group_id: actual_group_id,
            run_id,
        }
    }

    pub fn mark_copy_completed(
        &self,
        task_group_id: &str,
        run_id: &str,
        has_deploy_targets: bool,
    ) -> Result<(), String> {
        let finished_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.copy_phase = CopyState::Completed;
                run.deploy_phase = if has_deploy_targets {
                    DeployState::Pending
                } else {
                    DeployState::NotStarted
                };
                if !has_deploy_targets {
                    run.finished_at = Some(finished_at.clone());
                }
            }
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn mark_copy_failed(
        &self,
        task_group_id: &str,
        run_id: &str,
        _message: String,
    ) -> Result<(), String> {
        let finished_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.copy_phase = CopyState::Failed;
                run.finished_at = Some(finished_at.clone());
            }
            group.refresh_from_runs();
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn mark_copy_cancelled(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        let finished_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.copy_phase = CopyState::Cancelled;
                run.finished_at = Some(finished_at.clone());
            }
            group.refresh_from_runs();
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn register_deploy_targets(
        &self,
        task_group_id: &str,
        run_id: &str,
        targets: &[DeployTarget],
    ) -> Result<(), String> {
        let started_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            let mut prepared_attempts = Vec::<(DeployTarget, u32)>::new();

            if !targets.is_empty() {
                let mut attempt_counters = std::collections::BTreeMap::<String, u32>::new();
                for target in targets {
                    let next_attempt_no = attempt_counters
                        .entry(target.server_id.clone())
                        .or_insert_with(|| group.next_attempt_no_for_server(&target.server_id));
                    prepared_attempts.push((target.clone(), *next_attempt_no));
                    *next_attempt_no += 1;
                }
            }

            {
                let run = &mut group.runs[run_index];
                if targets.is_empty() {
                    run.deploy_phase = DeployState::NotStarted;
                    if run.finished_at.is_none() {
                        run.finished_at = Some(started_at.clone());
                    }
                } else {
                    for (target, attempt_no) in prepared_attempts {
                        run.deploy_attempts.push(DeployAttempt {
                            attempt_id: format!("attempt-{}", uuid::Uuid::new_v4()),
                            task_group_id: task_group_id.to_string(),
                            run_id: run_id.to_string(),
                            server_id: target.server_id,
                            server_name: target.server_name,
                            attempt_no,
                            trigger_source: target.trigger_source,
                            stage: DeployStage::Pending,
                            status: crate::task_domain::AttemptStatus::Running,
                            remote_target: Some(target.remote_target),
                            started_at: started_at.clone(),
                            finished_at: None,
                            elapsed_seconds: 0,
                            progress_percentage: Some(0.0),
                            error_phase: None,
                            error_message: None,
                            last_log_excerpt: None,
                        });
                    }
                    run.deploy_phase = DeployState::Running;
                    run.finished_at = None;
                }
                run.sync_attempt_ids();
            }
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn mark_attempt_stage(
        &self,
        task_group_id: &str,
        run_id: &str,
        server_id: &str,
        stage: DeployStage,
        progress_percentage: Option<f64>,
        remote_target: Option<String>,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                let attempt = find_latest_attempt_mut(run, server_id)?;
                attempt.stage = stage;
                attempt.status = crate::task_domain::AttemptStatus::Running;
                if let Some(progress) = progress_percentage {
                    attempt.progress_percentage = Some(progress);
                }
                if remote_target.is_some() {
                    attempt.remote_target = remote_target;
                }
                run.deploy_phase = DeployState::Running;
                run.finished_at = None;
            }
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn fail_attempt(
        &self,
        task_group_id: &str,
        run_id: &str,
        server_id: &str,
        stage: DeployStage,
        message: String,
    ) -> Result<(), String> {
        let finished_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                let attempt = find_latest_attempt_mut(run, server_id)?;
                attempt.stage = stage.clone();
                attempt.status = crate::task_domain::AttemptStatus::Failed;
                attempt.error_phase = Some(stage);
                attempt.error_message = Some(message);
                attempt.finished_at = Some(finished_at.clone());
                attempt.elapsed_seconds = compute_elapsed_seconds(
                    &attempt.started_at,
                    attempt.finished_at.as_deref(),
                );
                run.refresh_deploy_phase();
                if is_terminal_deploy_phase(&run.deploy_phase) {
                    run.finished_at = Some(finished_at.clone());
                }
            }
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn complete_attempt_success(
        &self,
        task_group_id: &str,
        run_id: &str,
        server_id: &str,
    ) -> Result<(), String> {
        let finished_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                let attempt = find_latest_attempt_mut(run, server_id)?;
                attempt.stage = DeployStage::Done;
                attempt.status = crate::task_domain::AttemptStatus::Success;
                attempt.progress_percentage = Some(100.0);
                attempt.finished_at = Some(finished_at.clone());
                attempt.elapsed_seconds = compute_elapsed_seconds(
                    &attempt.started_at,
                    attempt.finished_at.as_deref(),
                );
                run.refresh_deploy_phase();
                if is_terminal_deploy_phase(&run.deploy_phase) {
                    run.finished_at = Some(finished_at.clone());
                }
            }
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn tracking_context(&self, task_group_id: String, run_id: String) -> DeployTrackingContext {
        DeployTrackingContext {
            task_manager: self.clone(),
            task_group_id,
            run_id,
        }
    }

    pub fn cancel_pending_attempts(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        let finished_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                for attempt in &mut run.deploy_attempts {
                    if attempt.status == crate::task_domain::AttemptStatus::Running {
                        attempt.status = crate::task_domain::AttemptStatus::Cancelled;
                        attempt.finished_at = Some(finished_at.clone());
                        attempt.elapsed_seconds = compute_elapsed_seconds(
                            &attempt.started_at,
                            attempt.finished_at.as_deref(),
                        );
                    }
                }
                run.refresh_deploy_phase();
                if is_terminal_deploy_phase(&run.deploy_phase) {
                    run.finished_at = Some(finished_at.clone());
                }
            }
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn list_groups(&self) -> Vec<TaskGroupListItem> {
        let mut groups = self.snapshot_state().groups;
        groups.sort_by(|left, right| right.started_at.cmp(&left.started_at));
        groups.iter().map(TaskGroupListItem::from).collect()
    }

    pub fn get_group_detail(&self, task_group_id: &str) -> Option<TaskGroup> {
        self.inner
            .state
            .lock()
            .unwrap()
            .groups
            .iter()
            .find(|group| group.task_group_id == task_group_id)
            .cloned()
    }

    pub fn clear_task_group(&self, task_group_id: &str) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            state
                .groups
                .retain(|group| group.task_group_id != task_group_id);
        }
        self.after_change(None);
        Ok(())
    }

    pub fn clear_task_groups(&self) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            state.groups.clear();
        }
        self.after_change(None);
        Ok(())
    }

    pub fn snapshot_state(&self) -> TaskState {
        self.inner.state.lock().unwrap().clone()
    }

    fn after_change(&self, task_group_id: Option<&str>) {
        self.emit_snapshots(task_group_id);
        self.schedule_persist();
    }

    fn emit_snapshots(&self, task_group_id: Option<&str>) {
        let Some(app_handle) = self.inner.app_handle.as_ref() else {
            return;
        };

        let _ = app_handle.emit(
            TASK_GROUPS_SNAPSHOT_EVENT,
            TaskGroupsSnapshot {
                groups: self.list_groups(),
            },
        );

        if let Some(group_id) = task_group_id {
            if let Some(group) = self.get_group_detail(group_id) {
                let _ = app_handle.emit(
                    TASK_GROUP_DETAIL_SNAPSHOT_EVENT,
                    TaskGroupDetailSnapshot {
                        task_group_id: group_id.to_string(),
                        group,
                    },
                );
            }
        }
    }

    fn schedule_persist(&self) {
        let Some(app_handle) = self.inner.app_handle.clone() else {
            return;
        };

        if self
            .inner
            .persist_scheduled
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let snapshot = manager.snapshot_state();
            let _ = save_task_state(&app_handle, &snapshot);
            manager
                .inner
                .persist_scheduled
                .store(false, Ordering::SeqCst);
        });
    }
}

impl DeployTrackingContext {
    pub fn register_targets(&self, targets: &[DeployTarget]) -> Result<(), String> {
        self.task_manager
            .register_deploy_targets(&self.task_group_id, &self.run_id, targets)
    }

    pub fn mark_stage(
        &self,
        server_id: &str,
        stage: DeployStage,
        progress_percentage: Option<f64>,
        remote_target: Option<String>,
    ) -> Result<(), String> {
        self.task_manager.mark_attempt_stage(
            &self.task_group_id,
            &self.run_id,
            server_id,
            stage,
            progress_percentage,
            remote_target,
        )
    }

    pub fn mark_failure(
        &self,
        server_id: &str,
        stage: DeployStage,
        message: String,
    ) -> Result<(), String> {
        self.task_manager.fail_attempt(
            &self.task_group_id,
            &self.run_id,
            server_id,
            stage,
            message,
        )
    }

    pub fn mark_success(&self, server_id: &str) -> Result<(), String> {
        self.task_manager
            .complete_attempt_success(&self.task_group_id, &self.run_id, server_id)
    }

    pub fn cancel_pending(&self) -> Result<(), String> {
        self.task_manager
            .cancel_pending_attempts(&self.task_group_id, &self.run_id)
    }
}

fn find_group_mut<'a>(state: &'a mut TaskState, task_group_id: &str) -> Result<&'a mut TaskGroup, String> {
    state
        .groups
        .iter_mut()
        .find(|group| group.task_group_id == task_group_id)
        .ok_or_else(|| format!("Task group not found: {task_group_id}"))
}

fn find_run_index(group: &TaskGroup, run_id: &str) -> Result<usize, String> {
    group
        .runs
        .iter()
        .position(|run| run.run_id == run_id)
        .ok_or_else(|| format!("Task run not found: {run_id}"))
}

fn find_latest_attempt_mut<'a>(
    run: &'a mut TaskRun,
    server_id: &str,
) -> Result<&'a mut DeployAttempt, String> {
    run.deploy_attempts
        .iter_mut()
        .rev()
        .find(|attempt| attempt.server_id == server_id)
        .ok_or_else(|| format!("Deploy attempt not found for server: {server_id}"))
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
    end.signed_duration_since(start).num_seconds().max(0) as u64
}

fn is_terminal_deploy_phase(phase: &DeployState) -> bool {
    matches!(
        phase,
        DeployState::Completed
            | DeployState::PartialFailed
            | DeployState::Failed
            | DeployState::Cancelled
            | DeployState::Interrupted
    )
}

#[cfg(test)]
impl TaskStartRequest {
    pub fn sample() -> Self {
        Self {
            task_config_id: Some("task-a".to_string()),
            display_name: "Release_01".to_string(),
            folder_name: "Release_01".to_string(),
            source_path: "C:\\source\\Release_01".to_string(),
            local_target_path: "E:\\target\\Release_01".to_string(),
            source_type: TaskSourceType::Scheduled,
            trigger_source: TaskTriggerSource::Scheduled,
        }
    }
}

#[cfg(test)]
impl DeployTarget {
    pub fn sample() -> Self {
        Self {
            server_id: "server-a".to_string(),
            server_name: "Server A".to_string(),
            remote_target: "/srv/release".to_string(),
            trigger_source: TaskTriggerSource::Scheduled,
        }
    }

    pub fn named(name: &str) -> Self {
        Self {
            server_id: name.to_string(),
            server_name: name.to_string(),
            remote_target: format!("/srv/{name}"),
            trigger_source: TaskTriggerSource::Scheduled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_copy_run_creates_group_and_active_run() {
        let manager = TaskManager::new_in_memory();
        let handle = manager.begin_scheduled_copy(TaskStartRequest::sample());

        let groups = manager.list_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].task_group_id, handle.task_group_id);
        assert_eq!(groups[0].summary_status, TaskSummaryStatus::Copying);
    }

    #[test]
    fn failing_connection_attempt_is_recorded_before_upload() {
        let manager = TaskManager::new_in_memory();
        let handle = manager.begin_scheduled_copy(TaskStartRequest::sample());
        manager
            .mark_copy_completed(&handle.task_group_id, &handle.run_id, true)
            .unwrap();
        manager
            .register_deploy_targets(&handle.task_group_id, &handle.run_id, &[DeployTarget::sample()])
            .unwrap();
        manager
            .fail_attempt(
                &handle.task_group_id,
                &handle.run_id,
                "server-a",
                DeployStage::Connecting,
                "timeout".into(),
            )
            .unwrap();

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        assert_eq!(detail.server_rollups[0].failure_count, 1);
        assert_eq!(
            detail.runs[0].deploy_attempts[0].error_phase,
            Some(DeployStage::Connecting)
        );
    }

    #[test]
    fn successful_copy_then_partial_deploy_failure_sets_partial_failed_summary() {
        let manager = TaskManager::new_in_memory();
        let handle = manager.begin_scheduled_copy(TaskStartRequest::sample());
        manager
            .mark_copy_completed(&handle.task_group_id, &handle.run_id, true)
            .unwrap();
        manager
            .register_deploy_targets(
                &handle.task_group_id,
                &handle.run_id,
                &[DeployTarget::named("server-a"), DeployTarget::named("server-b")],
            )
            .unwrap();
        manager
            .complete_attempt_success(&handle.task_group_id, &handle.run_id, "server-a")
            .unwrap();
        manager
            .fail_attempt(
                &handle.task_group_id,
                &handle.run_id,
                "server-b",
                DeployStage::ExecutingCommands,
                "post command failed".into(),
            )
            .unwrap();

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        assert_eq!(detail.summary_status, TaskSummaryStatus::PartialFailed);
        assert!(detail.had_failures);
    }
}
