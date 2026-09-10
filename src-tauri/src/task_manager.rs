use crate::task_domain::{
    build_server_rollups, normalize_path_for_merge, ArtifactMetadata, CompositeBatchSummary,
    CopyState, DeployAttempt, DeployStage, DeployState, LocalExecState, ModuleTaskStatus,
    TaskGroup, TaskGroupKind, TaskMergeKey, TaskModuleSummary, TaskRun, TaskRunType,
    TaskSourceType, TaskState, TaskSummaryStatus, TaskTriggerSource,
};
use crate::task_events::{
    TaskGroupDetailSnapshot, TaskGroupListItem, TaskGroupsSnapshot, TaskLogEntry,
    TASK_GROUPS_SNAPSHOT_EVENT, TASK_GROUP_DETAIL_SNAPSHOT_EVENT, TASK_LOG_EVENT,
};
use crate::task_persist::{load_task_state, save_task_state};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

const ACTIVE_TASK_CHECKPOINT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

fn artifact_metadata(source_path: &str, task_config_id: Option<&str>) -> ArtifactMetadata {
    let parts = source_path
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let version_index = parts
        .iter()
        .position(|part| part.starts_with('B') && part.contains('.'));
    let product = version_index
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| parts.get(index))
        .map(|value| (*value).to_string());
    let version = version_index
        .and_then(|index| parts.get(index))
        .map(|value| (*value).to_string());
    let architecture = version_index
        .and_then(|index| parts.get(index + 1))
        .map(|value| (*value).to_string());
    let build_id = parts
        .iter()
        .rev()
        .find(|part| {
            part.strip_prefix('C').is_some_and(|digits| {
                !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
            })
        })
        .map(|value| (*value).to_string());
    let source_modified_at = std::fs::metadata(source_path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339());
    let module_name = product.clone();
    let module_id = task_config_id
        .zip(product.as_deref())
        .map(|(task_id, product)| {
            let normalized = product
                .to_ascii_lowercase()
                .replace(|character: char| !character.is_ascii_alphanumeric(), "-");
            format!("{task_id}:{normalized}")
        });
    ArtifactMetadata {
        product,
        version,
        architecture,
        build_id,
        source_modified_at,
        module_id,
        module_name,
    }
}

#[derive(Debug, Clone)]
pub struct TaskStartRequest {
    pub task_config_id: Option<String>,
    pub display_name: String,
    pub folder_name: String,
    pub source_path: String,
    pub local_target_path: String,
    pub source_type: TaskSourceType,
    pub trigger_source: TaskTriggerSource,
    pub module_id: Option<String>,
    pub module_name: Option<String>,
    pub parent_task_group_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompositeBatchModuleRequest {
    pub module_id: String,
    pub module_name: String,
    pub remote_path: String,
    pub local_path: String,
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
    pub server_host: String,
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
    persist_lock: Mutex<()>,
    persist_scheduled: AtomicBool,
    snapshot_scheduled: AtomicBool,
    snapshot_revision: AtomicU64,
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
                persist_lock: Mutex::new(()),
                persist_scheduled: AtomicBool::new(false),
                snapshot_scheduled: AtomicBool::new(false),
                snapshot_revision: AtomicU64::new(0),
            }),
        };

        manager.persist_now();
        manager.emit_group_list_snapshot();
        manager.start_active_task_checkpoint();
        manager
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new_in_memory() -> Self {
        Self {
            inner: Arc::new(TaskManagerInner {
                app_handle: None,
                state: Mutex::new(TaskState {
                    version: 1,
                    last_checkpoint_at: None,
                    groups: vec![],
                }),
                persist_lock: Mutex::new(()),
                persist_scheduled: AtomicBool::new(false),
                snapshot_scheduled: AtomicBool::new(false),
                snapshot_revision: AtomicU64::new(0),
            }),
        }
    }

    pub fn begin_composite_batch(
        &self,
        task_config_id: &str,
        display_name: &str,
        batch_key: &str,
        modules: Vec<CompositeBatchModuleRequest>,
    ) -> String {
        let started_at = current_timestamp();
        let merge_key = TaskMergeKey::new(
            Some(format!("{task_config_id}:batch")),
            batch_key.to_string(),
            display_name.to_string(),
        );
        let mut state = self.inner.state.lock().unwrap();
        let index = state
            .groups
            .iter()
            .position(|group| {
                group.group_kind == TaskGroupKind::CompositeBatch && group.merge_key == merge_key
            })
            .unwrap_or_else(|| {
                state.groups.push(TaskGroup {
                    task_group_id: format!("group-{}", uuid::Uuid::new_v4()),
                    merge_key: merge_key.clone(),
                    task_config_id: Some(task_config_id.to_string()),
                    source_type: TaskSourceType::Scheduled,
                    display_name: display_name.to_string(),
                    folder_name: batch_key.to_string(),
                    source_path: String::new(),
                    local_target_path: String::new(),
                    artifact: ArtifactMetadata::default(),
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
                    group_kind: TaskGroupKind::CompositeBatch,
                    parent_task_group_id: None,
                    composite_batch: None,
                    paused: false,
                    cancel_requested: false,
                    paused_at: None,
                    accumulated_paused_seconds: 0,
                });
                state.groups.len() - 1
            });
        let group = &mut state.groups[index];
        group.display_name = display_name.to_string();
        group.folder_name = batch_key.to_string();
        group.started_at = started_at;
        group.finished_at = None;
        group.composite_batch = Some(CompositeBatchSummary {
            batch_key: batch_key.to_string(),
            expected_modules: modules.len() as u32,
            found_modules: 0,
            current_module_index: 0,
            current_module_name: None,
            total_bytes: 0,
            copied_bytes: 0,
            modules: modules
                .into_iter()
                .map(|module| TaskModuleSummary {
                    module_id: module.module_id,
                    module_name: module.module_name,
                    remote_path: module.remote_path,
                    local_path: module.local_path,
                    status: ModuleTaskStatus::PendingScan,
                    ..TaskModuleSummary::default()
                })
                .collect(),
        });
        group.refresh_composite_batch();
        let group_id = group.task_group_id.clone();
        drop(state);
        self.after_change(Some(&group_id));
        group_id
    }

    pub fn mark_composite_module_found(
        &self,
        parent_id: &str,
        module_id: &str,
        found_builds: u32,
    ) -> Result<(), String> {
        self.update_composite_module(parent_id, module_id, |module| {
            module.found_builds = found_builds.max(1);
            module.status = ModuleTaskStatus::Queued;
            module.error_message = None;
        })
    }

    pub fn mark_composite_module_no_output(
        &self,
        parent_id: &str,
        module_id: &str,
    ) -> Result<(), String> {
        self.update_composite_module(parent_id, module_id, |module| {
            module.status = ModuleTaskStatus::NoOutput;
            module.error_message = None;
        })
    }

    pub fn mark_composite_module_cancelled(
        &self,
        parent_id: &str,
        module_id: &str,
    ) -> Result<(), String> {
        self.update_composite_module(parent_id, module_id, |module| {
            module.status = ModuleTaskStatus::Cancelled;
            module.error_message = None;
        })
    }

    pub fn mark_composite_module_failed(
        &self,
        parent_id: &str,
        module_id: &str,
        message: String,
    ) -> Result<(), String> {
        self.update_composite_module(parent_id, module_id, |module| {
            module.status = ModuleTaskStatus::Failed;
            module.error_message = Some(message);
        })
    }

    pub fn mark_composite_current(
        &self,
        parent_id: &str,
        module_id: &str,
        current_index: u32,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, parent_id)?;
            let batch = group
                .composite_batch
                .as_mut()
                .ok_or_else(|| "Task group is not a composite batch".to_string())?;
            batch.current_module_index = current_index;
            batch.current_module_name = batch
                .modules
                .iter()
                .find(|module| module.module_id == module_id)
                .map(|module| module.module_name.clone());
            if let Some(module) = batch
                .modules
                .iter_mut()
                .find(|module| module.module_id == module_id)
            {
                module.status = ModuleTaskStatus::Running;
            }
            group.refresh_composite_batch();
        }
        self.after_change(Some(parent_id));
        Ok(())
    }

    fn update_composite_module(
        &self,
        parent_id: &str,
        module_id: &str,
        update: impl FnOnce(&mut TaskModuleSummary),
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, parent_id)?;
            let module = group
                .composite_batch
                .as_mut()
                .and_then(|batch| {
                    batch
                        .modules
                        .iter_mut()
                        .find(|module| module.module_id == module_id)
                })
                .ok_or_else(|| format!("Composite module not found: {module_id}"))?;
            update(module);
            group.refresh_composite_batch();
        }
        self.after_change(Some(parent_id));
        Ok(())
    }

    pub fn update_copy_progress(
        &self,
        task_group_id: &str,
        run_id: &str,
        copied_bytes: u64,
        total_bytes: u64,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            group.runs[run_index].copy_copied_bytes = copied_bytes.min(total_bytes);
            group.runs[run_index].copy_total_bytes = total_bytes;
            group.refresh_from_runs();
            refresh_composite_parents_locked(&mut state);
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn begin_scheduled_copy(&self, request: TaskStartRequest) -> TaskRunHandle {
        let started_at = current_timestamp();
        let parent_task_group_id = request.parent_task_group_id.clone();
        let module_id = request.module_id.clone();
        let merge_key = TaskMergeKey::new_scoped(
            request.task_config_id.clone(),
            request.module_id.as_deref(),
            &request.source_path,
            request.local_target_path.clone(),
            request.folder_name.clone(),
        );
        let proposed_group_id = format!("group-{}", uuid::Uuid::new_v4());
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let mut artifact =
            artifact_metadata(&request.source_path, request.task_config_id.as_deref());
        if request.module_id.is_some() {
            artifact.module_id.clone_from(&request.module_id);
        }
        if request.module_name.is_some() {
            artifact.module_name.clone_from(&request.module_name);
        }
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
                        artifact: artifact.clone(),
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
                        group_kind: TaskGroupKind::Artifact,
                        parent_task_group_id: request.parent_task_group_id.clone(),
                        composite_batch: None,
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
            group.artifact = artifact;
            group.group_kind = TaskGroupKind::Artifact;
            group
                .parent_task_group_id
                .clone_from(&request.parent_task_group_id);
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
                copy_total_bytes: 0,
                copy_copied_bytes: 0,
            });
            group.refresh_from_runs();

            if let (Some(parent_id), Some(module_id)) =
                (parent_task_group_id.as_deref(), module_id.as_deref())
            {
                if let Some(parent) = state
                    .groups
                    .iter_mut()
                    .find(|candidate| candidate.task_group_id == parent_id)
                {
                    if let Some(module) = parent.composite_batch.as_mut().and_then(|batch| {
                        batch
                            .modules
                            .iter_mut()
                            .find(|module| module.module_id == module_id)
                    }) {
                        if !module.child_task_group_ids.contains(&actual_group_id) {
                            module.child_task_group_ids.push(actual_group_id.clone());
                        }
                        module.status = ModuleTaskStatus::Running;
                    }
                    parent.refresh_composite_batch();
                }
            }
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
            module_id: None,
            module_name: None,
            parent_task_group_id: None,
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
                    artifact: artifact_metadata(&request.source_path, None),
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
                    group_kind: TaskGroupKind::Artifact,
                    parent_task_group_id: None,
                    composite_batch: None,
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
                copy_total_bytes: 0,
                copy_copied_bytes: 0,
            });
            group.refresh_from_runs();
        }

        self.after_change(Some(actual_group_id.as_str()));
        Ok(TaskRunHandle {
            task_group_id: actual_group_id,
            run_id,
        })
    }

    pub fn begin_deploy_retry_run(&self, task_group_id: &str) -> Result<TaskRunHandle, String> {
        let started_at = current_timestamp();
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            group.runs.push(TaskRun {
                run_id: run_id.clone(),
                task_group_id: group.task_group_id.clone(),
                run_type: TaskRunType::DeployRetry,
                trigger_source: TaskTriggerSource::Recovery,
                started_at: started_at.clone(),
                finished_at: None,
                copy_phase: CopyState::Completed,
                local_exec_phase: LocalExecState::NotStarted,
                deploy_phase: DeployState::Pending,
                deploy_attempts: vec![],
                attempt_ids: vec![],
                copy_total_bytes: 0,
                copy_copied_bytes: 0,
            });
            group.started_at = started_at;
            group.finished_at = None;
            group.refresh_from_runs();
        }

        self.after_change(Some(task_group_id));
        Ok(TaskRunHandle {
            task_group_id: task_group_id.to_string(),
            run_id,
        })
    }

    pub fn begin_topology_run(
        &self,
        existing_task_group_id: Option<&str>,
        display_name: &str,
        topology_summary: &str,
        primary_ip: &str,
    ) -> Result<TaskRunHandle, String> {
        let started_at = current_timestamp();
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let mut state = self.inner.state.lock().unwrap();
        let group_index = if let Some(group_id) = existing_task_group_id {
            state
                .groups
                .iter()
                .position(|group| group.task_group_id == group_id)
                .ok_or_else(|| format!("Topology task not found: {group_id}"))?
        } else {
            state.groups.push(TaskGroup {
                task_group_id: format!("group-{}", uuid::Uuid::new_v4()),
                merge_key: TaskMergeKey::new(
                    None,
                    primary_ip.to_string(),
                    format!("topology-{}", uuid::Uuid::new_v4()),
                ),
                task_config_id: None,
                source_type: TaskSourceType::Manual,
                display_name: display_name.to_string(),
                folder_name: topology_summary.to_string(),
                source_path: topology_summary.to_string(),
                local_target_path: String::new(),
                artifact: ArtifactMetadata {
                    product: Some(display_name.to_string()),
                    ..ArtifactMetadata::default()
                },
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
                group_kind: TaskGroupKind::Topology,
                parent_task_group_id: None,
                composite_batch: None,
                paused: false,
                cancel_requested: false,
                paused_at: None,
                accumulated_paused_seconds: 0,
            });
            state.groups.len() - 1
        };
        let group = &mut state.groups[group_index];
        group.group_kind = TaskGroupKind::Topology;
        group.display_name = display_name.to_string();
        group.folder_name = topology_summary.to_string();
        group.source_path = topology_summary.to_string();
        group.started_at = started_at.clone();
        group.finished_at = None;
        group.runs.push(TaskRun {
            run_id: run_id.clone(),
            task_group_id: group.task_group_id.clone(),
            run_type: if existing_task_group_id.is_some() {
                TaskRunType::PostInstallRetry
            } else {
                TaskRunType::Topology
            },
            trigger_source: if existing_task_group_id.is_some() {
                TaskTriggerSource::Recovery
            } else {
                TaskTriggerSource::Manual
            },
            started_at,
            finished_at: None,
            copy_phase: CopyState::Completed,
            local_exec_phase: LocalExecState::NotStarted,
            deploy_phase: DeployState::Pending,
            deploy_attempts: vec![],
            attempt_ids: vec![],
            copy_total_bytes: 0,
            copy_copied_bytes: 0,
        });
        group.refresh_from_runs();
        let group_id = group.task_group_id.clone();
        drop(state);
        self.after_change(Some(&group_id));
        Ok(TaskRunHandle {
            task_group_id: group_id,
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
                        let paused_duration = compute_elapsed_seconds(paused_at_str, None);
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

    pub fn requeue_paused_copy(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            let run = &mut group.runs[run_index];
            if matches!(run.copy_phase, CopyState::Pending | CopyState::Running) {
                run.copy_phase = CopyState::Pending;
                run.finished_at = None;
                group.paused = true;
                group.cancel_requested = false;
                group.paused_at.get_or_insert_with(current_timestamp);
                group.refresh_from_runs();
            }
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn is_run_paused(&self, task_group_id: &str, run_id: &str) -> bool {
        self.inner
            .state
            .lock()
            .unwrap()
            .groups
            .iter()
            .find(|group| {
                group.task_group_id == task_group_id
                    && group.latest_run_id.as_deref() == Some(run_id)
            })
            .is_some_and(|group| group.paused)
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

    /// Whether the newest recorded copy for this source/target pair was cancelled.
    ///
    /// Return the latest copy state for an exact source/target pair across scheduled and
    /// manual groups. Matching on paths rather than merge keys lets a successful manual
    /// retry supersede an earlier cancelled scheduled run.
    pub fn latest_copy_state(
        &self,
        source_path: &str,
        local_target_path: &str,
    ) -> Option<CopyState> {
        let source = normalize_path_for_merge(source_path);
        let target = normalize_path_for_merge(local_target_path);
        let state = self.inner.state.lock().unwrap();
        state
            .groups
            .iter()
            .filter(|group| {
                normalize_path_for_merge(&group.source_path) == source
                    && normalize_path_for_merge(&group.local_target_path) == target
            })
            .flat_map(|group| group.runs.iter())
            .max_by_key(|run| run_start_millis(&run.started_at))
            .map(|run| run.copy_phase.clone())
    }

    /// Return the latest state that represents the same fallback package. Scheduled runs
    /// still require the exact source/target pair, while a manual run of the build directory
    /// or any file below it also counts even when the user chose a different local target.
    pub fn latest_fallback_copy_state(
        &self,
        candidate_source_path: &str,
        local_target_path: &str,
    ) -> Option<CopyState> {
        let candidate_source = normalize_path_for_merge(candidate_source_path);
        let target = normalize_path_for_merge(local_target_path);
        let state = self.inner.state.lock().unwrap();
        state
            .groups
            .iter()
            .filter(|group| {
                let group_source = normalize_path_for_merge(&group.source_path);
                let exact_scheduled_pair = group_source == candidate_source
                    && normalize_path_for_merge(&group.local_target_path) == target;
                let manual_source_within_candidate = group.source_type == TaskSourceType::Manual
                    && normalized_path_is_same_or_descendant(&group_source, &candidate_source);
                exact_scheduled_pair || manual_source_within_candidate
            })
            .flat_map(|group| group.runs.iter())
            .max_by_key(|run| run_start_millis(&run.started_at))
            .map(|run| run.copy_phase.clone())
    }

    pub fn last_copy_was_cancelled(&self, source_path: &str, local_target_path: &str) -> bool {
        self.latest_copy_state(source_path, local_target_path)
            .is_some_and(|state| state == CopyState::Cancelled)
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
                            server_host: target.server_host,
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
                            server_role: None,
                            stage_started_at: Some(started_at.clone()),
                            stage_finished_at: None,
                            poll_attempt: 0,
                            next_poll_at: None,
                            resumable: false,
                            topology_dispatched: false,
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
                if attempt.stage != stage {
                    attempt.stage_finished_at = Some(current_timestamp());
                    attempt.stage_started_at = Some(current_timestamp());
                    attempt.poll_attempt = 0;
                    attempt.next_poll_at = None;
                }
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

    pub fn update_attempt_checkpoint(
        &self,
        task_group_id: &str,
        run_id: &str,
        server_id: &str,
        poll_attempt: u32,
        next_poll_at: Option<String>,
        resumable: bool,
        topology_dispatched: bool,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            let run = &mut group.runs[run_index];
            let attempt = find_latest_attempt_mut(run, server_id)?;
            attempt.poll_attempt = poll_attempt;
            attempt.next_poll_at = next_poll_at;
            attempt.resumable = resumable;
            attempt.topology_dispatched = topology_dispatched;
        }
        self.after_change(Some(task_group_id));
        Ok(())
    }

    pub fn set_attempt_server_role(
        &self,
        task_group_id: &str,
        run_id: &str,
        server_id: &str,
        role: String,
    ) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let run_index = find_run_index(group, run_id)?;
            let attempt = find_latest_attempt_mut(&mut group.runs[run_index], server_id)?;
            attempt.server_role = Some(role);
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
        let (resolved_server_name, parent_task_group_id) = {
            let mut state = self.inner.state.lock().unwrap();
            let group = find_group_mut(&mut state, task_group_id)?;
            let parent_task_group_id = group.parent_task_group_id.clone();
            let run_index = find_run_index(group, run_id)?;
            let run = &mut group.runs[run_index];

            let resolved = if let Some(server_id) = server_id {
                let attempt = find_latest_attempt_mut(run, server_id)?;
                attempt.last_log_excerpt = Some(message.to_string());
                Some(attempt.server_name.clone())
            } else {
                None
            };
            (resolved, parent_task_group_id)
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
                    timestamp: timestamp.clone(),
                },
            );
            if let Some(parent_id) = parent_task_group_id.as_deref() {
                let _ = app_handle.emit(
                    TASK_LOG_EVENT,
                    TaskLogEntry {
                        task_group_id: Some(parent_id.to_string()),
                        run_id: Some(run_id.to_string()),
                        server_id: server_id.map(str::to_string),
                        server_name: server_name.map(str::to_string),
                        level: level.to_string(),
                        message: message.to_string(),
                        timestamp,
                    },
                );
            }
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
        let state = self.inner.state.lock().unwrap();
        let mut group = state
            .groups
            .iter()
            .find(|group| group.task_group_id == task_group_id)
            .cloned()?;
        if group.group_kind == TaskGroupKind::CompositeBatch {
            let child_ids = group
                .composite_batch
                .as_ref()
                .map(|batch| {
                    batch
                        .modules
                        .iter()
                        .flat_map(|module| module.child_task_group_ids.iter().cloned())
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default();
            let child_groups = state
                .groups
                .iter()
                .filter(|child| child_ids.contains(&child.task_group_id))
                .collect::<Vec<_>>();
            group.runs = child_groups
                .iter()
                .flat_map(|child| child.runs.iter().cloned())
                .collect();
            group
                .runs
                .sort_by(|left, right| right.started_at.cmp(&left.started_at));
            group.server_rollups = build_server_rollups(&group.runs);
        }
        Some(group)
    }

    pub fn clear_task_group(&self, task_group_id: &str) -> Result<(), String> {
        {
            let mut state = self.inner.state.lock().unwrap();
            state.groups.retain(|group| {
                group.task_group_id != task_group_id
                    && group.parent_task_group_id.as_deref() != Some(task_group_id)
            });
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

    /// Enforce the configured history cap without ever removing active work.
    pub fn prune_terminal_groups(&self, max_records: usize) -> usize {
        let removed = {
            let mut state = self.inner.state.lock().unwrap();
            if state.groups.len() <= max_records {
                return 0;
            }
            let mut terminal = state
                .groups
                .iter()
                .filter(|group| {
                    group.summary_status == TaskSummaryStatus::Completed && !group.had_failures
                })
                .map(|group| (group.task_group_id.clone(), group.started_at.clone()))
                .collect::<Vec<_>>();
            terminal.sort_by(|left, right| right.1.cmp(&left.1));
            let active_count = state.groups.len().saturating_sub(terminal.len());
            let terminal_budget = max_records.saturating_sub(active_count);
            let keep = terminal
                .into_iter()
                .take(terminal_budget)
                .map(|(id, _)| id)
                .collect::<std::collections::HashSet<_>>();
            let before = state.groups.len();
            state.groups.retain(|group| {
                group.summary_status != TaskSummaryStatus::Completed
                    || group.had_failures
                    || keep.contains(&group.task_group_id)
            });
            before - state.groups.len()
        };
        if removed > 0 {
            self.after_change(None);
        }
        removed
    }

    /// Freeze any in-flight task at the current time and persist it before a
    /// graceful application exit.
    pub fn interrupt_in_progress_for_exit(&self) {
        {
            let mut state = self.inner.state.lock().unwrap();
            state.last_checkpoint_at = None;
            state.mark_in_progress_as_interrupted();
        }

        self.emit_group_list_snapshot();
        self.persist_now();
    }

    fn after_change(&self, task_group_id: Option<&str>) {
        {
            let mut state = self.inner.state.lock().unwrap();
            refresh_composite_parents_locked(&mut state);
        }
        self.schedule_group_list_snapshot();
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
                revision: self.inner.snapshot_revision.fetch_add(1, Ordering::SeqCst) + 1,
                groups: self.list_groups(),
            },
        );
    }

    fn schedule_group_list_snapshot(&self) {
        if self.inner.app_handle.is_none()
            || self
                .inner
                .snapshot_scheduled
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return;
        }
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            manager
                .inner
                .snapshot_scheduled
                .store(false, Ordering::SeqCst);
            manager.emit_group_list_snapshot();
        });
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
        if self.inner.app_handle.is_none() {
            return;
        }

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
            manager.persist_now();
            manager
                .inner
                .persist_scheduled
                .store(false, Ordering::SeqCst);
        });
    }

    fn persist_now(&self) {
        let Some(app_handle) = self.inner.app_handle.as_ref() else {
            return;
        };
        let _persist_guard = self.inner.persist_lock.lock().unwrap();
        let snapshot = self.snapshot_state();
        let _ = save_task_state(app_handle, &snapshot);
    }

    fn start_active_task_checkpoint(&self) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(ACTIVE_TASK_CHECKPOINT_INTERVAL).await;
                let has_active_tasks =
                    manager
                        .inner
                        .state
                        .lock()
                        .unwrap()
                        .groups
                        .iter()
                        .any(|group| {
                            matches!(
                                group.summary_status,
                                TaskSummaryStatus::Queued
                                    | TaskSummaryStatus::Copying
                                    | TaskSummaryStatus::Paused
                                    | TaskSummaryStatus::Cancelling
                                    | TaskSummaryStatus::CopyCompleted
                                    | TaskSummaryStatus::LocalExecuting
                                    | TaskSummaryStatus::Deploying
                            )
                        });

                if has_active_tasks {
                    manager.schedule_persist();
                }
            }
        });
    }
}

fn refresh_composite_parents_locked(state: &mut TaskState) {
    let child_state = state
        .groups
        .iter()
        .filter(|group| group.group_kind == TaskGroupKind::Artifact)
        .map(|group| {
            let latest = group.runs.last();
            (
                group.task_group_id.clone(),
                (
                    group.summary_status.clone(),
                    latest.map(|run| run.copy_total_bytes).unwrap_or(0),
                    latest.map(|run| run.copy_copied_bytes).unwrap_or(0),
                    group.runs.last().and_then(|run| {
                        run.deploy_attempts
                            .iter()
                            .rev()
                            .find_map(|attempt| attempt.error_message.clone())
                    }),
                ),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();

    for parent in state
        .groups
        .iter_mut()
        .filter(|group| group.group_kind == TaskGroupKind::CompositeBatch)
    {
        if let Some(batch) = parent.composite_batch.as_mut() {
            for module in &mut batch.modules {
                let children = module
                    .child_task_group_ids
                    .iter()
                    .filter_map(|id| child_state.get(id))
                    .collect::<Vec<_>>();
                if children.is_empty() {
                    continue;
                }
                module.found_builds = module.found_builds.max(children.len() as u32);
                module.completed_builds = children
                    .iter()
                    .filter(|(status, _, _, _)| *status == TaskSummaryStatus::Completed)
                    .count() as u32;
                module.total_bytes = children.iter().map(|(_, total, _, _)| *total).sum();
                module.copied_bytes = children.iter().map(|(_, _, copied, _)| *copied).sum();
                module.status = if children.iter().any(|(status, _, _, _)| {
                    matches!(
                        status,
                        TaskSummaryStatus::Failed | TaskSummaryStatus::PartialFailed
                    )
                }) {
                    ModuleTaskStatus::Failed
                } else if children
                    .iter()
                    .any(|(status, _, _, _)| *status == TaskSummaryStatus::Interrupted)
                {
                    ModuleTaskStatus::Interrupted
                } else if children
                    .iter()
                    .any(|(status, _, _, _)| *status == TaskSummaryStatus::Cancelled)
                {
                    ModuleTaskStatus::Cancelled
                } else if children
                    .iter()
                    .all(|(status, _, _, _)| *status == TaskSummaryStatus::Completed)
                {
                    ModuleTaskStatus::Completed
                } else {
                    ModuleTaskStatus::Running
                };
                module.error_message = children.iter().find_map(|(_, _, _, error)| error.clone());
            }
        }
        parent.refresh_composite_batch();
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

    pub fn mark_checkpoint(
        &self,
        server_id: &str,
        poll_attempt: u32,
        next_poll_at: Option<String>,
        resumable: bool,
        topology_dispatched: bool,
    ) -> Result<(), String> {
        self.task_manager.update_attempt_checkpoint(
            &self.task_group_id,
            &self.run_id,
            server_id,
            poll_attempt,
            next_poll_at,
            resumable,
            topology_dispatched,
        )
    }

    pub fn set_server_role(&self, server_id: &str, role: String) -> Result<(), String> {
        self.task_manager.set_attempt_server_role(
            &self.task_group_id,
            &self.run_id,
            server_id,
            role,
        )
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

/// Sortable start time for a run. Unparsable timestamps sort oldest so they never
/// shadow a run whose time is known. Runs that share a millisecond are broken by
/// iteration order — groups and their runs are both appended chronologically, and
/// `max_by_key` keeps the last of equal keys — so the newest still wins.
fn run_start_millis(started_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(started_at)
        .map(|value| value.timestamp_millis())
        .unwrap_or(i64::MIN)
}

fn normalized_path_is_same_or_descendant(path: &str, parent: &str) -> bool {
    if parent.is_empty() {
        return false;
    }
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('\\'))
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
            module_id: None,
            module_name: None,
            parent_task_group_id: None,
        }
    }
}

#[cfg(test)]
impl DeployTarget {
    pub fn sample() -> Self {
        Self {
            server_id: "server-a".to_string(),
            server_name: "Server A".to_string(),
            server_host: "192.0.2.10".to_string(),
            remote_target: "/srv/release".to_string(),
            trigger_source: TaskTriggerSource::Scheduled,
        }
    }

    pub fn named(name: &str) -> Self {
        Self {
            server_id: name.to_string(),
            server_name: name.to_string(),
            server_host: format!("{name}.example.test"),
            remote_target: format!("/srv/{name}"),
            trigger_source: TaskTriggerSource::Scheduled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_business_identity_from_component_build_path() {
        let metadata = artifact_metadata(
            r"\\t03\每日编译1\组件项目\Jenkins\new\product\VMS_U500_H16\B2101.12.1\x86_64\0903\C1788473722875460505",
            Some("task-components"),
        );
        assert_eq!(metadata.product.as_deref(), Some("VMS_U500_H16"));
        assert_eq!(metadata.version.as_deref(), Some("B2101.12.1"));
        assert_eq!(metadata.architecture.as_deref(), Some("x86_64"));
        assert_eq!(metadata.build_id.as_deref(), Some("C1788473722875460505"));
        assert_eq!(
            metadata.module_id.as_deref(),
            Some("task-components:vms-u500-h16")
        );
    }

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
    fn cancelled_copy_marks_the_source_target_pair_as_cancelled() {
        let manager = TaskManager::new_in_memory();
        let request = TaskStartRequest::sample();
        let source = request.source_path.clone();
        let target = request.local_target_path.clone();
        let handle = manager.begin_scheduled_copy(request);
        assert_eq!(
            manager.latest_copy_state(&source, &target),
            Some(CopyState::Pending)
        );
        assert!(!manager.last_copy_was_cancelled(&source, &target));

        manager
            .mark_copy_cancelled(&handle.task_group_id, &handle.run_id)
            .unwrap();

        assert!(manager.last_copy_was_cancelled(&source, &target));
        assert_eq!(
            manager.latest_copy_state(&source, &target),
            Some(CopyState::Cancelled)
        );
        // Path spelling differences must not let a cancelled candidate slip back in.
        assert!(manager.last_copy_was_cancelled(&source.to_lowercase(), &target.replace('\\', "/")));
        assert!(!manager.last_copy_was_cancelled(&source, "E:\\target\\Other"));
    }

    #[test]
    fn failed_copy_is_not_treated_as_cancelled() {
        let manager = TaskManager::new_in_memory();
        let request = TaskStartRequest::sample();
        let source = request.source_path.clone();
        let target = request.local_target_path.clone();
        let handle = manager.begin_scheduled_copy(request);

        manager
            .mark_copy_failed(
                &handle.task_group_id,
                &handle.run_id,
                "disk full".to_string(),
            )
            .unwrap();

        assert!(!manager.last_copy_was_cancelled(&source, &target));
        assert_eq!(
            manager.latest_copy_state(&source, &target),
            Some(CopyState::Failed)
        );
    }

    #[test]
    fn a_manual_recopy_clears_an_earlier_cancel() {
        let manager = TaskManager::new_in_memory();
        let request = TaskStartRequest::sample();
        let source = request.source_path.clone();
        let target = request.local_target_path.clone();
        let cancelled = manager.begin_scheduled_copy(request);
        manager
            .mark_copy_cancelled(&cancelled.task_group_id, &cancelled.run_id)
            .unwrap();

        // A manual retry lands in its own group (no task_config_id) but copies the same
        // folder to the same place, so it must take the candidate off the skip list.
        let retried = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "Release_01".to_string(),
                folder_name: "Release_01".to_string(),
                source_path: source.clone(),
                local_target_path: target.clone(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();
        manager
            .mark_copy_completed(&retried.task_group_id, &retried.run_id, false)
            .unwrap();

        assert!(!manager.last_copy_was_cancelled(&source, &target));
        assert_eq!(
            manager.latest_copy_state(&source, &target),
            Some(CopyState::Completed)
        );
    }

    #[test]
    fn manual_file_under_build_blocks_that_fallback_candidate_only() {
        let manager = TaskManager::new_in_memory();
        let candidate = r"\\nt03\product\260819\C100";
        let manual_file = format!(r"{candidate}\UNV_Guard-win.exe");
        let manual = manager
            .begin_manual_copy_run(StartManualCopyRequest {
                display_name: "UNV_Guard-win.exe".to_string(),
                folder_name: "UNV_Guard-win.exe".to_string(),
                source_path: manual_file,
                local_target_path: r"E:\UMS_TEMP\UNV_Guard-win.exe".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();
        manager
            .mark_copy_completed(&manual.task_group_id, &manual.run_id, false)
            .unwrap();

        assert_eq!(
            manager.latest_fallback_copy_state(candidate, r"E:\UMS_TEMP\GoProcess\260819\C100"),
            Some(CopyState::Completed)
        );
        assert_eq!(
            manager.latest_fallback_copy_state(
                r"\\nt03\product\260819\C10",
                r"E:\UMS_TEMP\GoProcess\260819\C10"
            ),
            None
        );
    }

    #[test]
    fn descendant_path_matching_respects_component_boundaries() {
        assert!(normalized_path_is_same_or_descendant(
            r"\\nt03\product\c100\package.exe",
            r"\\nt03\product\c100"
        ));
        assert!(!normalized_path_is_same_or_descendant(
            r"\\nt03\product\c1000\package.exe",
            r"\\nt03\product\c100"
        ));
    }

    #[test]
    fn interrupted_copy_is_reported_by_latest_copy_state() {
        let manager = TaskManager::new_in_memory();
        let request = TaskStartRequest::sample();
        let source = request.source_path.clone();
        let target = request.local_target_path.clone();
        let handle = manager.begin_scheduled_copy(request);
        manager
            .mark_copy_started(&handle.task_group_id, &handle.run_id)
            .unwrap();

        assert_eq!(
            manager.latest_copy_state(&source, &target),
            Some(CopyState::Running)
        );

        manager.interrupt_in_progress_for_exit();

        assert_eq!(
            manager.latest_copy_state(&source, &target),
            Some(CopyState::Interrupted)
        );
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
        assert_eq!(detail.server_rollups[0].server_host, "192.0.2.10");
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
    fn paused_copy_can_return_to_pending_without_becoming_cancelled() {
        let manager = TaskManager::new_in_memory();
        let handle = manager.begin_scheduled_copy(TaskStartRequest::sample());
        manager
            .mark_copy_started(&handle.task_group_id, &handle.run_id)
            .unwrap();
        manager
            .requeue_paused_copy(&handle.task_group_id, &handle.run_id)
            .unwrap();

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        assert_eq!(detail.copy_status, CopyState::Pending);
        assert_eq!(detail.summary_status, TaskSummaryStatus::Paused);
        assert!(manager.is_run_paused(&handle.task_group_id, &handle.run_id));

        manager
            .set_run_paused(&handle.task_group_id, &handle.run_id, false)
            .unwrap();
        manager
            .mark_copy_started(&handle.task_group_id, &handle.run_id)
            .unwrap();
        let resumed = manager.get_group_detail(&handle.task_group_id).unwrap();
        assert_eq!(resumed.copy_status, CopyState::Running);
        assert_eq!(resumed.summary_status, TaskSummaryStatus::Copying);
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
                    server_host: "192.0.2.20".to_string(),
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

    #[test]
    fn begin_deploy_retry_run_preserves_group_and_records_recovery_run() {
        let manager = TaskManager::new_in_memory();
        let seed = manager.begin_scheduled_copy(TaskStartRequest::sample());
        manager
            .mark_copy_completed(&seed.task_group_id, &seed.run_id, false)
            .unwrap();

        let retry = manager.begin_deploy_retry_run(&seed.task_group_id).unwrap();
        let detail = manager.get_group_detail(&seed.task_group_id).unwrap();
        let retry_run = detail.runs.last().unwrap();

        assert_eq!(retry.task_group_id, seed.task_group_id);
        assert_eq!(retry_run.run_id, retry.run_id);
        assert_eq!(retry_run.run_type, TaskRunType::DeployRetry);
        assert_eq!(retry_run.trigger_source, TaskTriggerSource::Recovery);
        assert_eq!(retry_run.copy_phase, CopyState::Completed);
        assert_eq!(retry_run.deploy_phase, DeployState::Pending);
    }

    #[test]
    fn composite_batch_keeps_no_output_neutral_and_links_children() {
        let manager = TaskManager::new_in_memory();
        let parent = manager.begin_composite_batch(
            "task-components",
            "Components",
            "2026-09-05",
            vec![
                CompositeBatchModuleRequest {
                    module_id: "module-a".into(),
                    module_name: "A".into(),
                    remote_path: r"\\t03\A".into(),
                    local_path: r"E:\A".into(),
                },
                CompositeBatchModuleRequest {
                    module_id: "module-b".into(),
                    module_name: "B".into(),
                    remote_path: r"\\t03\B".into(),
                    local_path: r"E:\B".into(),
                },
            ],
        );
        manager
            .mark_composite_module_no_output(&parent, "module-b")
            .unwrap();
        manager
            .mark_composite_module_found(&parent, "module-a", 1)
            .unwrap();
        let child = manager.begin_scheduled_copy(TaskStartRequest {
            task_config_id: Some("task-components".into()),
            display_name: "A".into(),
            folder_name: "C1".into(),
            source_path: r"\\t03\A\C1".into(),
            local_target_path: r"E:\A\C1".into(),
            source_type: TaskSourceType::Scheduled,
            trigger_source: TaskTriggerSource::Scheduled,
            module_id: Some("module-a".into()),
            module_name: Some("A".into()),
            parent_task_group_id: Some(parent.clone()),
        });
        manager
            .mark_copy_completed(&child.task_group_id, &child.run_id, false)
            .unwrap();

        let detail = manager.get_group_detail(&parent).unwrap();
        let batch = detail.composite_batch.unwrap();
        assert_eq!(batch.expected_modules, 2);
        assert_eq!(batch.found_modules, 1);
        assert_eq!(batch.modules[1].status, ModuleTaskStatus::NoOutput);
        assert_eq!(detail.summary_status, TaskSummaryStatus::Completed);
        assert!(!detail.had_failures);
    }

    #[test]
    fn composite_batch_surfaces_failed_and_cancelled_modules() {
        let manager = TaskManager::new_in_memory();
        let parent = manager.begin_composite_batch(
            "task-components",
            "Components",
            "2026-09-05",
            vec![
                CompositeBatchModuleRequest {
                    module_id: "module-a".into(),
                    module_name: "A".into(),
                    remote_path: r"\\t03\A".into(),
                    local_path: r"E:\A".into(),
                },
                CompositeBatchModuleRequest {
                    module_id: "module-b".into(),
                    module_name: "B".into(),
                    remote_path: r"\\t03\B".into(),
                    local_path: r"E:\B".into(),
                },
                CompositeBatchModuleRequest {
                    module_id: "module-c".into(),
                    module_name: "C".into(),
                    remote_path: r"\\t03\C".into(),
                    local_path: r"E:\C".into(),
                },
            ],
        );
        manager
            .mark_composite_module_no_output(&parent, "module-b")
            .unwrap();
        for module_id in ["module-a", "module-c"] {
            manager
                .mark_composite_module_found(&parent, module_id, 1)
                .unwrap();
        }
        let child = |module_id: &str| {
            manager.begin_scheduled_copy(TaskStartRequest {
                task_config_id: Some("task-components".into()),
                display_name: module_id.to_string(),
                folder_name: "C1".into(),
                source_path: format!(r"\\t03\{module_id}\C1"),
                local_target_path: format!(r"E:\{module_id}\C1"),
                source_type: TaskSourceType::Scheduled,
                trigger_source: TaskTriggerSource::Scheduled,
                module_id: Some(module_id.to_string()),
                module_name: Some(module_id.to_string()),
                parent_task_group_id: Some(parent.clone()),
            })
        };
        let failed = child("module-a");
        manager
            .mark_copy_failed(&failed.task_group_id, &failed.run_id, "copy failed".into())
            .unwrap();
        let cancelled = child("module-c");
        manager
            .mark_copy_cancelled(&cancelled.task_group_id, &cancelled.run_id)
            .unwrap();

        let detail = manager.get_group_detail(&parent).unwrap();
        let batch = detail.composite_batch.unwrap();
        assert_eq!(batch.modules[0].status, ModuleTaskStatus::Failed);
        assert_eq!(batch.modules[1].status, ModuleTaskStatus::NoOutput);
        assert_eq!(batch.modules[2].status, ModuleTaskStatus::Cancelled);
        assert_eq!(detail.summary_status, TaskSummaryStatus::PartialFailed);
        assert!(detail.had_failures);
    }

    #[test]
    fn composite_batch_can_cancel_module_before_child_run_is_created() {
        let manager = TaskManager::new_in_memory();
        let parent = manager.begin_composite_batch(
            "task-components",
            "Components",
            "2026-09-10",
            vec![
                CompositeBatchModuleRequest {
                    module_id: "module-a".into(),
                    module_name: "A".into(),
                    remote_path: r"\\t03\A".into(),
                    local_path: r"E:\A".into(),
                },
                CompositeBatchModuleRequest {
                    module_id: "module-b".into(),
                    module_name: "B".into(),
                    remote_path: r"\\t03\B".into(),
                    local_path: r"E:\B".into(),
                },
            ],
        );
        manager
            .mark_composite_module_no_output(&parent, "module-a")
            .unwrap();
        manager
            .mark_composite_module_cancelled(&parent, "module-b")
            .unwrap();

        let detail = manager.get_group_detail(&parent).unwrap();
        let batch = detail.composite_batch.unwrap();
        assert_eq!(batch.modules[0].status, ModuleTaskStatus::NoOutput);
        assert_eq!(batch.modules[1].status, ModuleTaskStatus::Cancelled);
        assert_eq!(detail.summary_status, TaskSummaryStatus::Cancelled);
        assert!(!detail.had_failures);
    }

    #[test]
    fn task_list_100_and_1000_record_baseline_is_bounded() {
        for count in [100usize, 1_000] {
            let manager = TaskManager::new_in_memory();
            let started = std::time::Instant::now();
            for index in 0..count {
                let mut request = TaskStartRequest::sample();
                request.folder_name = format!("C{index:019}");
                request.source_path = format!(r"\\t03\product\{index}");
                request.local_target_path = format!(r"E:\sync\{index}");
                let handle = manager.begin_scheduled_copy(request);
                manager
                    .mark_copy_completed(&handle.task_group_id, &handle.run_id, false)
                    .unwrap();
            }
            let build_elapsed = started.elapsed();
            let list_started = std::time::Instant::now();
            let groups = manager.list_groups();
            let list_elapsed = list_started.elapsed();
            eprintln!(
                "task-list-baseline count={count} build_ms={} list_ms={}",
                build_elapsed.as_millis(),
                list_elapsed.as_millis()
            );
            assert_eq!(groups.len(), count);
            assert!(list_elapsed < std::time::Duration::from_secs(2));
        }
    }
}
