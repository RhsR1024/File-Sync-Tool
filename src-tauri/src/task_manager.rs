use crate::task_domain::{
    CopyState, DeployAttempt, DeployStage, DeployState, LocalExecState, TaskGroup, TaskMergeKey,
    TaskRun, TaskRunType, TaskSourceType, TaskState, TaskSummaryStatus, TaskTriggerSource,
};
use crate::task_events::{
    TaskGroupDetailSnapshot, TaskGroupListItem, TaskGroupsSnapshot, TaskLogEntry,
    TASK_GROUPS_SNAPSHOT_EVENT, TASK_GROUP_DETAIL_SNAPSHOT_EVENT, TASK_LOG_EVENT,
};
use crate::task_persist::{load_task_state, save_task_state};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunHandle {
    pub task_group_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartManualCopyRequest {
    pub display_name: String,
    pub folder_name: String,
    pub source_path: String,
    pub local_target_path: String,
    pub trigger_source: TaskTriggerSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartManualDeployRequest {
    pub task_group_id: Option<String>,
    pub display_name: String,
    pub folder_name: String,
    pub local_target_path: String,
    pub source_path: String,
    pub trigger_source: TaskTriggerSource,
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
        manager.emit_group_list_snapshot();
        manager
    }

    #[cfg_attr(not(test), allow(dead_code))]
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
                        local_exec_status: LocalExecState::NotStarted,
                        deploy_status: DeployState::NotStarted,
                        summary_status: TaskSummaryStatus::Queued,
                        started_at: started_at.clone(),
                        finished_at: None,
                        elapsed_seconds: 0,
                        latest_run_id: None,
                        had_failures: false,
                        server_rollups: vec![],
                        runs: vec![],
                        paused: false,
                        cancel_requested: false,
                        paused_at: None,
                        accumulated_paused_seconds: 0,
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
            // Reset pause/cancel state so elapsed_seconds is computed cleanly for the new run
            group.paused = false;
            group.cancel_requested = false;
            group.paused_at = None;
            group.accumulated_paused_seconds = 0;

            group.runs.push(TaskRun {
                run_id: run_id.clone(),
                task_group_id: group.task_group_id.clone(),
                run_type: TaskRunType::CopyAndDeploy,
                trigger_source: request.trigger_source,
                started_at: started_at.clone(),
                finished_at: None,
                copy_phase: CopyState::Pending,
                local_exec_phase: LocalExecState::NotStarted,
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

    pub fn mark_copy_started(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        let started_at = current_timestamp();
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            let run = &mut group.runs[run_index];
            if !matches!(run.copy_phase, CopyState::Pending) {
                return Ok(());
            }
            run.copy_phase = CopyState::Running;
            run.started_at = started_at.clone();
            group.started_at = started_at;
            group.refresh_from_runs();
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn begin_manual_copy_run(
        &self,
        request: StartManualCopyRequest,
    ) -> Result<TaskRunHandle, String> {
        Ok(self.begin_scheduled_copy(TaskStartRequest {
            task_config_id: None,
            display_name: request.display_name,
            folder_name: request.folder_name,
            source_path: request.source_path,
            local_target_path: request.local_target_path,
            source_type: TaskSourceType::Manual,
            trigger_source: request.trigger_source,
        }))
    }

    pub fn begin_manual_deploy_run(
        &self,
        request: StartManualDeployRequest,
    ) -> Result<TaskRunHandle, String> {
        let started_at = current_timestamp();
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let actual_group_id;

        {
            let mut state = self.inner.state.lock().unwrap();
            let group_index = if let Some(existing_id) = request.task_group_id.clone() {
                state
                    .groups
                    .iter()
                    .position(|group| group.task_group_id == existing_id)
                    .ok_or_else(|| format!("Task group not found: {existing_id}"))?
            } else {
                let merge_key = TaskMergeKey::new(
                    None,
                    request.local_target_path.clone(),
                    request.folder_name.clone(),
                );
                let proposed_group_id = format!("group-{}", uuid::Uuid::new_v4());
                state.groups.push(TaskGroup {
                    task_group_id: proposed_group_id,
                    merge_key,
                    task_config_id: None,
                    source_type: TaskSourceType::Manual,
                    display_name: request.display_name.clone(),
                    folder_name: request.folder_name.clone(),
                    source_path: request.source_path.clone(),
                    local_target_path: request.local_target_path.clone(),
                    copy_status: CopyState::Completed,
                    local_exec_status: LocalExecState::NotStarted,
                    deploy_status: DeployState::Pending,
                    summary_status: TaskSummaryStatus::CopyCompleted,
                    started_at: started_at.clone(),
                    finished_at: None,
                    elapsed_seconds: 0,
                    latest_run_id: None,
                    had_failures: false,
                    server_rollups: vec![],
                    runs: vec![],
                    paused: false,
                    cancel_requested: false,
                    paused_at: None,
                    accumulated_paused_seconds: 0,
                });
                state.groups.len() - 1
            };

            let group = &mut state.groups[group_index];
            actual_group_id = group.task_group_id.clone();

            group.runs.push(TaskRun {
                run_id: run_id.clone(),
                task_group_id: group.task_group_id.clone(),
                run_type: TaskRunType::ManualDeploy,
                trigger_source: request.trigger_source,
                started_at: started_at.clone(),
                finished_at: None,
                copy_phase: CopyState::Completed,
                local_exec_phase: LocalExecState::NotStarted,
                deploy_phase: DeployState::Pending,
                deploy_attempts: vec![],
                attempt_ids: vec![],
            });
            group.refresh_from_runs();
        }

        self.after_change(Some(actual_group_id.as_str()));
        Ok(TaskRunHandle {
            task_group_id: actual_group_id,
            run_id,
        })
    }

    pub fn set_run_paused(
        &self,
        task_group_id: &str,
        run_id: &str,
        paused: bool,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            if group.latest_run_id.as_deref() != Some(run_id) {
                return Ok(());
            }
            if matches!(group.copy_status, CopyState::Pending | CopyState::Running) {
                let was_paused = group.paused;
                group.paused = paused;
                if paused && !was_paused {
                    // Starting pause: record the pause start time
                    group.paused_at = Some(current_timestamp());
                    group.cancel_requested = false;
                } else if !paused && was_paused {
                    // Resuming: calculate pause duration and accumulate it
                    if let Some(paused_at_str) = &group.paused_at {
                        let paused_duration =
                            compute_elapsed_seconds(paused_at_str, None);
                        group.accumulated_paused_seconds += paused_duration;
                    }
                    group.paused_at = None;
                }
                group.refresh_from_runs();
            }
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn request_run_cancel(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            if group.latest_run_id.as_deref() != Some(run_id) {
                return Ok(());
            }
            if matches!(group.copy_status, CopyState::Pending | CopyState::Running) {
                group.cancel_requested = true;
                group.paused = false;
                group.refresh_from_runs();
            }
        }
        self.after_change(Some(task_group_id));
        Ok(())
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

    // Drop a scheduled run that produced no real work (0 files matched the copy rules).
    // Scheduled ticks without actual copy/deploy activity would otherwise accumulate as
    // noise rows in the task-detail run history. Removing them keeps the history focused
    // on runs that either copied files or were interrupted/cancelled by the user.
    pub fn discard_noop_run(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        let group_removed;
        {
            let mut state = self.inner.state.lock().unwrap();
            let group_index = state
                .groups
                .iter()
                .position(|group| group.task_group_id == task_group_id)
                .ok_or_else(|| format!("Task group not found: {task_group_id}"))?;

            let group = &mut state.groups[group_index];
            let Some(run_index) = group.runs.iter().position(|run| run.run_id == run_id) else {
                return Ok(());
            };
            group.runs.remove(run_index);

            if group.runs.is_empty() {
                state.groups.remove(group_index);
                group_removed = true;
            } else {
                group.latest_run_id = group.runs.last().map(|run| run.run_id.clone());
                group.refresh_from_runs();
                group_removed = false;
            }
        }

        if group_removed {
            self.after_change(None);
        } else {
            self.after_change(Some(task_group_id));
        }
        Ok(())
    }

    pub fn begin_local_exec(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.local_exec_phase = LocalExecState::Running;
            }
            group.refresh_from_runs();
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn mark_local_exec_completed(
        &self,
        task_group_id: &str,
        run_id: &str,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.local_exec_phase = LocalExecState::Completed;
            }
            group.refresh_from_runs();
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn mark_local_exec_failed(
        &self,
        task_group_id: &str,
        run_id: &str,
        _message: String,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.local_exec_phase = LocalExecState::Failed;
            }
            group.refresh_from_runs();
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn mark_local_exec_partial_failed(
        &self,
        task_group_id: &str,
        run_id: &str,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            {
                let run = &mut group.runs[run_index];
                run.local_exec_phase = LocalExecState::PartialFailed;
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
                attempt.elapsed_seconds =
                    compute_elapsed_seconds(&attempt.started_at, attempt.finished_at.as_deref());
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
                attempt.elapsed_seconds =
                    compute_elapsed_seconds(&attempt.started_at, attempt.finished_at.as_deref());
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

    pub fn record_task_log(
        &self,
        task_group_id: &str,
        run_id: &str,
        server_id: Option<&str>,
        server_name: Option<&str>,
        level: &str,
        message: &str,
    ) -> Result<(), String> {
        let timestamp = current_timestamp();
        let resolved_server_name = {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            let run = &mut group.runs[run_index];

            if let Some(server_id) = server_id {
                let attempt = find_latest_attempt_mut(run, server_id)?;
                attempt.last_log_excerpt = Some(message.to_string());
                Some(attempt.server_name.clone())
            } else {
                None
            }
        };

        if let Some(app_handle) = self.inner.app_handle.as_ref() {
            let _ = app_handle.emit(
                TASK_LOG_EVENT,
                TaskLogEntry {
                    task_group_id: Some(task_group_id.to_string()),
                    run_id: Some(run_id.to_string()),
                    server_id: server_id.map(str::to_string),
                    server_name: server_name.map(str::to_string).or(resolved_server_name),
                    level: level.to_string(),
                    message: message.to_string(),
                    timestamp,
                },
            );
        }

        self.after_log_change(task_group_id);
        Ok(())
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
        self.emit_group_list_snapshot();
        if let Some(group_id) = task_group_id {
            self.emit_detail_snapshot(group_id);
        }
        self.schedule_persist();
    }

    fn after_log_change(&self, task_group_id: &str) {
        self.emit_detail_snapshot(task_group_id);
        self.schedule_persist();
    }

    fn emit_group_list_snapshot(&self) {
        let Some(app_handle) = self.inner.app_handle.as_ref() else {
            return;
        };

        let _ = app_handle.emit(
            TASK_GROUPS_SNAPSHOT_EVENT,
            TaskGroupsSnapshot {
                groups: self.list_groups(),
            },
        );
    }

    fn emit_detail_snapshot(&self, task_group_id: &str) {
        let Some(app_handle) = self.inner.app_handle.as_ref() else {
            return;
        };

        if let Some(group) = self.get_group_detail(task_group_id) {
            let _ = app_handle.emit(
                TASK_GROUP_DETAIL_SNAPSHOT_EVENT,
                TaskGroupDetailSnapshot {
                    task_group_id: task_group_id.to_string(),
                    group,
                },
            );
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
        self.task_manager
            .fail_attempt(&self.task_group_id, &self.run_id, server_id, stage, message)
    }

    pub fn mark_success(&self, server_id: &str) -> Result<(), String> {
        self.task_manager
            .complete_attempt_success(&self.task_group_id, &self.run_id, server_id)
    }

    pub fn cancel_pending(&self) -> Result<(), String> {
        self.task_manager
            .cancel_pending_attempts(&self.task_group_id, &self.run_id)
    }

    pub fn record_log(
        &self,
        server_id: Option<&str>,
        server_name: Option<&str>,
        level: &str,
        message: &str,
    ) -> Result<(), String> {
        self.task_manager.record_task_log(
            &self.task_group_id,
            &self.run_id,
            server_id,
            server_name,
            level,
            message,
        )
    }
}

fn find_group_mut<'a>(
    state: &'a mut TaskState,
    task_group_id: &str,
) -> Result<&'a mut TaskGroup, String> {
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
        assert_eq!(groups[0].summary_status, TaskSummaryStatus::Queued);

        manager
            .mark_copy_started(&handle.task_group_id, &handle.run_id)
            .expect("mark_copy_started should succeed");
        let groups = manager.list_groups();
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
            .register_deploy_targets(
                &handle.task_group_id,
                &handle.run_id,
                &[DeployTarget::sample()],
            )
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
                &[
                    DeployTarget::named("server-a"),
                    DeployTarget::named("server-b"),
                ],
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

    #[test]
    fn begin_manual_copy_run_creates_manual_group() {
        let manager = TaskManager::new_in_memory();
        let handle = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "hotfix-build".to_string(),
                folder_name: "hotfix-build".to_string(),
                source_path: "C:\\drop\\hotfix-build".to_string(),
                local_target_path: "D:\\deploy\\hotfix-build".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        assert_eq!(detail.source_type, TaskSourceType::Manual);
        assert_eq!(detail.runs.len(), 1);
        assert_eq!(detail.runs[0].run_type, TaskRunType::CopyAndDeploy);
    }

    #[test]
    fn manual_copy_completion_marks_group_completed() {
        let manager = TaskManager::new_in_memory();
        let handle = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "hotfix-build".to_string(),
                folder_name: "hotfix-build".to_string(),
                source_path: "C:\\drop\\hotfix-build".to_string(),
                local_target_path: "D:\\deploy\\hotfix-build".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        manager
            .mark_copy_completed(&handle.task_group_id, &handle.run_id, false)
            .unwrap();
        manager
            .record_task_log(
                &handle.task_group_id,
                &handle.run_id,
                None,
                None,
                "success",
                "Manual copy completed",
            )
            .unwrap();

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        assert_eq!(detail.summary_status, TaskSummaryStatus::Completed);
        assert_eq!(detail.copy_status, CopyState::Completed);
        assert_eq!(detail.deploy_status, DeployState::NotStarted);
        assert_eq!(
            detail.latest_run_id.as_deref(),
            Some(handle.run_id.as_str())
        );
        assert!(detail.finished_at.is_some());
        assert!(detail.runs[0].finished_at.is_some());
    }

    #[test]
    fn begin_manual_deploy_run_reuses_existing_group_when_requested() {
        let manager = TaskManager::new_in_memory();
        let seed = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                source_path: "C:\\src\\pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        let deploy = manager
            .begin_manual_deploy_run(StartManualDeployRequest {
                task_group_id: Some(seed.task_group_id.clone()),
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                source_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        let detail = manager.get_group_detail(&seed.task_group_id).unwrap();
        assert_eq!(deploy.task_group_id, seed.task_group_id);
        assert_eq!(detail.runs.len(), 2);
        assert_eq!(detail.runs[1].run_type, TaskRunType::ManualDeploy);
    }

    #[test]
    fn manual_deploy_failure_is_recorded_under_manual_deploy_run() {
        let manager = TaskManager::new_in_memory();
        let seed = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                source_path: "C:\\src\\pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();
        manager
            .mark_copy_completed(&seed.task_group_id, &seed.run_id, false)
            .unwrap();

        let deploy = manager
            .begin_manual_deploy_run(StartManualDeployRequest {
                task_group_id: Some(seed.task_group_id.clone()),
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                source_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();
        manager
            .register_deploy_targets(
                &deploy.task_group_id,
                &deploy.run_id,
                &[DeployTarget {
                    server_id: "server-manual".to_string(),
                    server_name: "Manual Server".to_string(),
                    remote_target: "/srv/pkg".to_string(),
                    trigger_source: TaskTriggerSource::Manual,
                }],
            )
            .unwrap();
        manager
            .fail_attempt(
                &deploy.task_group_id,
                &deploy.run_id,
                "server-manual",
                DeployStage::ExecutingCommands,
                "manual deploy failed".to_string(),
            )
            .unwrap();
        manager
            .record_task_log(
                &deploy.task_group_id,
                &deploy.run_id,
                Some("server-manual"),
                Some("Manual Server"),
                "error",
                "manual deploy failed",
            )
            .unwrap();

        let detail = manager.get_group_detail(&deploy.task_group_id).unwrap();
        assert_eq!(detail.summary_status, TaskSummaryStatus::Failed);
        assert_eq!(
            detail.latest_run_id.as_deref(),
            Some(deploy.run_id.as_str())
        );
        assert_eq!(detail.runs.len(), 2);
        assert!(detail.runs[0].deploy_attempts.is_empty());
        assert_eq!(detail.runs[1].run_type, TaskRunType::ManualDeploy);
        assert_eq!(detail.runs[1].run_id, deploy.run_id);
        assert_eq!(detail.runs[1].deploy_attempts.len(), 1);
        assert_eq!(
            detail.runs[1].deploy_attempts[0].error_message.as_deref(),
            Some("manual deploy failed")
        );
        assert_eq!(
            detail.runs[1].deploy_attempts[0]
                .last_log_excerpt
                .as_deref(),
            Some("manual deploy failed")
        );
        assert_eq!(detail.server_rollups[0].failure_count, 1);
    }

    #[test]
    fn begin_manual_deploy_run_preserves_group_identity_when_reusing() {
        let manager = TaskManager::new_in_memory();
        let seed = manager.begin_scheduled_copy(TaskStartRequest::sample());

        let before = manager.get_group_detail(&seed.task_group_id).unwrap();
        let before_merge_key = before.merge_key.clone();
        let before_task_config_id = before.task_config_id.clone();
        let before_source_type = before.source_type.clone();
        let before_run_count = before.runs.len();

        let deploy = manager
            .begin_manual_deploy_run(StartManualDeployRequest {
                task_group_id: Some(seed.task_group_id.clone()),
                display_name: "manual".to_string(),
                folder_name: "Different".to_string(),
                local_target_path: "Z:\\deploy\\Different".to_string(),
                source_path: "Z:\\deploy\\Different".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        let after = manager.get_group_detail(&seed.task_group_id).unwrap();
        assert_eq!(deploy.task_group_id, seed.task_group_id);
        assert_eq!(after.merge_key, before_merge_key);
        assert_eq!(after.task_config_id, before_task_config_id);
        assert_eq!(after.source_type, before_source_type);
        assert_eq!(after.runs.len(), before_run_count + 1);
        assert_eq!(
            after.runs.last().unwrap().run_type,
            TaskRunType::ManualDeploy
        );
    }

    #[test]
    fn discard_noop_run_removes_empty_group() {
        let manager = TaskManager::new_in_memory();
        let handle = manager.begin_scheduled_copy(TaskStartRequest::sample());

        manager
            .discard_noop_run(&handle.task_group_id, &handle.run_id)
            .unwrap();

        assert!(manager.list_groups().is_empty());
        assert!(manager.get_group_detail(&handle.task_group_id).is_none());
    }

    #[test]
    fn discard_noop_run_preserves_group_when_earlier_run_remains() {
        let manager = TaskManager::new_in_memory();
        let first = manager.begin_scheduled_copy(TaskStartRequest::sample());
        manager
            .mark_copy_completed(&first.task_group_id, &first.run_id, false)
            .unwrap();

        let second = manager.begin_scheduled_copy(TaskStartRequest::sample());
        assert_eq!(second.task_group_id, first.task_group_id);

        manager
            .discard_noop_run(&second.task_group_id, &second.run_id)
            .unwrap();

        let detail = manager.get_group_detail(&first.task_group_id).unwrap();
        assert_eq!(detail.runs.len(), 1);
        assert_eq!(detail.runs[0].run_id, first.run_id);
        assert_eq!(detail.latest_run_id.as_deref(), Some(first.run_id.as_str()));
        assert_eq!(detail.copy_status, CopyState::Completed);
    }

    #[test]
    fn begin_manual_deploy_run_without_group_id_creates_new_group() {
        let manager = TaskManager::new_in_memory();
        let seed = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                source_path: "C:\\src\\pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        let deploy = manager
            .begin_manual_deploy_run(StartManualDeployRequest {
                task_group_id: None,
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                source_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();

        let groups = manager.list_groups();
        assert_eq!(groups.len(), 2);
        assert_ne!(deploy.task_group_id, seed.task_group_id);
    }
}
