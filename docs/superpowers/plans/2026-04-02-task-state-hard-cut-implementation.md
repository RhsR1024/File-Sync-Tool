# Task State Hard Cut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all frontend task-status inference with the backend `TaskGroup` / `TaskRun` / `DeployAttempt` state machine, including manual copy and manual deploy, with no long-lived compatibility path.

**Architecture:** Start from the Phase A backend baseline, then extend the backend with manual-task creation, run-control commands, and structured task-log events. Rebuild the frontend around a dedicated task-state store that hydrates from backend snapshots, rewrites the task page around group/detail DTOs, switches manual copy/deploy entry points to the new commands, and finally deletes the old `taskRecords` inference flow.

**Tech Stack:** Rust, Tauri 2, serde, Tokio, Vue 3, TypeScript, node `assert`, existing `pnpm check` / `cargo test` workflows

---

## Planned File Structure

**Backend**
- `src-tauri/src/task_domain.rs`: Extend task enums and request/result types needed for manual runs and retries.
- `src-tauri/src/task_events.rs`: Snapshot DTOs plus structured `task-log` event payloads.
- `src-tauri/src/task_manager.rs`: Single source of truth for creating manual/scheduled runs, retry runs, and mutating task state.
- `src-tauri/src/task_commands.rs`: Tauri commands for list/detail/start/retry/clear/control.
- `src-tauri/src/task_runtime.rs`: New runtime registry mapping the single active executor to `task_group_id` / `run_id`.
- `src-tauri/src/task_persist.rs`: Phase A task-state file persistence.
- `src-tauri/src/main.rs`: AppState wiring, command registration, manual worker integration, runtime command routing.
- `src-tauri/src/scanner.rs`: Scheduled/manual copy lifecycle reporting into `TaskManager`.
- `src-tauri/src/deploy.rs`: Scheduled/manual deploy lifecycle reporting into `TaskManager` plus structured task logs.
- `src-tauri/src/persist.rs`: UI persistence reduced to logs-only payloads.

**Frontend**
- `src/lib/tauri.ts`: TS DTOs and invoke wrappers for task-state commands.
- `src/lib/store.ts`: Keep logs, scheduler flags, manual form state; remove `taskRecords` state machine duties.
- `src/lib/taskStateStore.ts`: New reactive store for groups, details, task logs, hydration, and task actions.
- `src/lib/taskStateStore.test.mjs`: Pure tests for store hydration, snapshot merge, log append, selection, and manual-task actions.
- `src/lib/taskStatusView.ts`: Pure selectors/formatters used by the task page.
- `src/lib/taskStatusView.test.mjs`: Tests for list sorting, detail shaping, failure/retry visibility, and status labels.
- `src/App.vue`: Hydrate task-state store, subscribe to backend snapshots/task logs, persist logs only.
- `src/pages/TaskStatusPage.vue`: Rewrite list/detail/actions around backend DTOs.
- `src/components/tasks/TaskGroupsTable.vue`: Table/list rendering for task groups.
- `src/components/tasks/TaskGroupDetailPanel.vue`: Detail rendering for runs, attempts, rollups, failures, and logs.
- `src/components/ManualCopyModal.vue`: Call `start_manual_copy_task`, then focus the created task group.
- `src/pages/ManualCopyPage.vue`: Reuse the new manual-copy action path.
- `src/pages/SettingsPage.vue`: Replace manual deploy pre-registration/log parsing with `start_manual_deploy_task`.
- `src/locales/messages.ts`: Labels for new task-state UI and control actions.

## Prerequisite

This plan assumes the execution branch carries the Phase A backend baseline. The current `main` branch does not: `src-tauri/src/task_domain.rs`, `src-tauri/src/task_events.rs`, `src-tauri/src/task_manager.rs`, `src-tauri/src/task_commands.rs`, and `src-tauri/src/task_persist.rs` are still absent there today.

---

### Task 0: Sync the Branch onto the Phase A Backend Baseline

**Files:**
- Create: `src-tauri/src/task_domain.rs`
- Create: `src-tauri/src/task_events.rs`
- Create: `src-tauri/src/task_manager.rs`
- Create: `src-tauri/src/task_commands.rs`
- Create: `src-tauri/src/task_persist.rs`
- Modify: `src-tauri/src/main.rs`
- Test: `src-tauri/src/task_manager.rs`

- [ ] **Step 1: Verify whether the current branch already contains the Phase A files**

Run: `Get-ChildItem -Path src-tauri/src/task_*.rs`
Expected: On `main`, the command returns no `task_*` files. On a branch already rebased onto Phase A, the five files above should appear.

- [ ] **Step 2: If Phase A files are missing, restore them from `refactor/task-state-backend-phase-a`**

```bash
git checkout refactor/task-state-backend-phase-a -- \
  src-tauri/src/task_domain.rs \
  src-tauri/src/task_events.rs \
  src-tauri/src/task_manager.rs \
  src-tauri/src/task_commands.rs \
  src-tauri/src/task_persist.rs \
  src-tauri/src/main.rs
```

- [ ] **Step 3: Confirm `main.rs` contains the Phase A task-state wiring**

```rust
mod task_domain;
mod task_commands;
mod task_events;
mod task_manager;
mod task_persist;

struct AppState {
    config: Arc<Mutex<AppConfig>>,
    task_manager: task_manager::TaskManager,
    is_scanning: Arc<AtomicBool>,
    is_manually_deploying: Arc<AtomicBool>,
    // existing runtime flags...
}
```

- [ ] **Step 4: Run the focused Phase A test suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_ -- --nocapture`
Expected: PASS, confirming the current branch now has the same task-state foundation already validated in Phase A.

- [ ] **Step 5: Commit the Phase A sync**

```bash
git add src-tauri/src/task_domain.rs src-tauri/src/task_events.rs src-tauri/src/task_manager.rs src-tauri/src/task_commands.rs src-tauri/src/task_persist.rs src-tauri/src/main.rs
git commit -m "chore: 同步 task-state phase a 基线"
git push
```

This sync is safely above the 200-line threshold, so push immediately after the commit.

---

### Task 1: Extend the Backend Task-State Contract for Manual Runs, Retries, and Task Logs

**Files:**
- Modify: `src-tauri/src/task_domain.rs`
- Modify: `src-tauri/src/task_events.rs`
- Modify: `src-tauri/src/task_manager.rs`
- Modify: `src-tauri/src/task_commands.rs`
- Modify: `src-tauri/src/main.rs`
- Test: `src-tauri/src/task_manager.rs`

- [ ] **Step 1: Add failing tests for manual copy/manual deploy run creation**

Append these tests to `src-tauri/src/task_manager.rs`:

```rust
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
```

- [ ] **Step 2: Run the manager tests and confirm the new cases fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_manager::tests -- --nocapture`
Expected: FAIL because `StartManualCopyRequest`, `StartManualDeployRequest`, and `TaskRunType::ManualDeploy` do not exist yet.

- [ ] **Step 3: Extend the domain and event contracts**

In `src-tauri/src/task_domain.rs`, add the new run type and request DTO support:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskRunType {
    CopyAndDeploy,
    DeployRetry,
    ManualDeploy,
}
```

In `src-tauri/src/task_events.rs`, add the structured task log event and make DTOs round-trip friendly:

```rust
pub const TASK_LOG_EVENT: &str = "task-log";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLogEntry {
    pub task_group_id: Option<String>,
    pub run_id: Option<String>,
    pub server_id: Option<String>,
    pub server_name: Option<String>,
    pub level: String,
    pub message: String,
    pub timestamp: String,
}
```

- [ ] **Step 4: Implement manual run creation and new Tauri commands**

In `src-tauri/src/task_manager.rs`, add concrete request structs and manager methods:

```rust
#[derive(Debug, Clone)]
pub struct StartManualCopyRequest {
    pub display_name: String,
    pub folder_name: String,
    pub source_path: String,
    pub local_target_path: String,
    pub trigger_source: TaskTriggerSource,
}

#[derive(Debug, Clone)]
pub struct StartManualDeployRequest {
    pub task_group_id: Option<String>,
    pub display_name: String,
    pub folder_name: String,
    pub local_target_path: String,
    pub source_path: String,
    pub trigger_source: TaskTriggerSource,
}

pub fn begin_manual_copy_run(&self, request: StartManualCopyRequest) -> Result<TaskRunHandle, String> {
    self.begin_copy_run(TaskStartRequest {
        task_config_id: None,
        display_name: request.display_name,
        folder_name: request.folder_name,
        source_path: request.source_path,
        local_target_path: request.local_target_path,
        source_type: TaskSourceType::Manual,
        trigger_source: request.trigger_source,
    })
}

pub fn begin_manual_deploy_run(&self, request: StartManualDeployRequest) -> Result<TaskRunHandle, String> {
    self.begin_deploy_only_run(request)
}
```

In `src-tauri/src/task_commands.rs`, add command wrappers:

```rust
#[tauri::command]
pub fn start_manual_copy_task(
    state: State<'_, crate::AppState>,
    request: crate::task_manager::StartManualCopyRequest,
) -> Result<crate::task_manager::TaskRunHandle, String> {
    state.task_manager.begin_manual_copy_run(request)
}

#[tauri::command]
pub fn start_manual_deploy_task(
    state: State<'_, crate::AppState>,
    request: crate::task_manager::StartManualDeployRequest,
) -> Result<crate::task_manager::TaskRunHandle, String> {
    state.task_manager.begin_manual_deploy_run(request)
}
```

Register both commands in `src-tauri/src/main.rs` next to the existing task commands.

- [ ] **Step 5: Re-run the manager tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_manager::tests -- --nocapture`
Expected: PASS, including the two new manual-run tests.

- [ ] **Step 6: Commit the contract expansion**

```bash
git add src-tauri/src/task_domain.rs src-tauri/src/task_events.rs src-tauri/src/task_manager.rs src-tauri/src/task_commands.rs src-tauri/src/main.rs
git commit -m "feat: 扩展 task-state 手动任务与 task-log 契约"
```

---

### Task 2: Add a Runtime Registry for `task_group_id` / `run_id` Control Commands

**Files:**
- Create: `src-tauri/src/task_runtime.rs`
- Modify: `src-tauri/src/main.rs`
- Test: `src-tauri/src/task_runtime.rs`

- [ ] **Step 1: Add failing tests for active-run validation**

Create `src-tauri/src/task_runtime.rs` with these tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cancel_for_non_active_run() {
        let runtime = TaskRuntimeRegistry::default();
        runtime.activate("group-a".into(), "run-a".into());

        let err = runtime.require_active("group-b", "run-b").unwrap_err();
        assert!(err.contains("Active run mismatch"));
    }

    #[test]
    fn clears_active_run_after_finish() {
        let runtime = TaskRuntimeRegistry::default();
        runtime.activate("group-a".into(), "run-a".into());
        runtime.clear("group-a", "run-a");

        assert!(runtime.current().is_none());
    }
}
```

- [ ] **Step 2: Run the new runtime tests and confirm they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_runtime::tests -- --nocapture`
Expected: FAIL because `task_runtime.rs` and `TaskRuntimeRegistry` do not exist yet.

- [ ] **Step 3: Implement the runtime registry**

In `src-tauri/src/task_runtime.rs`, add:

```rust
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveRunExecution {
    pub task_group_id: String,
    pub run_id: String,
}

#[derive(Default)]
pub struct TaskRuntimeRegistry {
    active: Mutex<Option<ActiveRunExecution>>,
}

impl TaskRuntimeRegistry {
    pub fn activate(&self, task_group_id: String, run_id: String) {
        *self.active.lock().unwrap() = Some(ActiveRunExecution { task_group_id, run_id });
    }

    pub fn require_active(&self, task_group_id: &str, run_id: &str) -> Result<(), String> {
        let active = self.active.lock().unwrap();
        match active.as_ref() {
            Some(current) if current.task_group_id == task_group_id && current.run_id == run_id => Ok(()),
            Some(current) => Err(format!(
                "Active run mismatch: requested {task_group_id}/{run_id}, active {}/{}",
                current.task_group_id, current.run_id
            )),
            None => Err("No active task run".to_string()),
        }
    }

    pub fn clear(&self, task_group_id: &str, run_id: &str) {
        let mut active = self.active.lock().unwrap();
        if matches!(active.as_ref(), Some(current) if current.task_group_id == task_group_id && current.run_id == run_id) {
            *active = None;
        }
    }

    pub fn current(&self) -> Option<ActiveRunExecution> {
        self.active.lock().unwrap().clone()
    }
}
```

- [ ] **Step 4: Wire the registry and run-control commands into `main.rs`**

Add the new module and state field:

```rust
mod task_runtime;

struct AppState {
    config: Arc<Mutex<AppConfig>>,
    task_manager: task_manager::TaskManager,
    task_runtime: task_runtime::TaskRuntimeRegistry,
    is_scanning: Arc<AtomicBool>,
    // existing fields...
}
```

Add new commands:

```rust
#[tauri::command]
fn cancel_task_run(
    state: State<'_, AppState>,
    task_group_id: String,
    run_id: String,
) -> Result<(), String> {
    state.task_runtime.require_active(&task_group_id, &run_id)?;
    state.should_cancel.store(true, Ordering::SeqCst);
    state.is_paused.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn pause_task_run(
    state: State<'_, AppState>,
    task_group_id: String,
    run_id: String,
) -> Result<(), String> {
    state.task_runtime.require_active(&task_group_id, &run_id)?;
    state.is_paused.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn resume_task_run(
    state: State<'_, AppState>,
    task_group_id: String,
    run_id: String,
) -> Result<(), String> {
    state.task_runtime.require_active(&task_group_id, &run_id)?;
    state.is_paused.store(false, Ordering::SeqCst);
    Ok(())
}
```

Register the new commands in `tauri::generate_handler!`.

- [ ] **Step 5: Re-run the runtime tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_runtime::tests -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Commit the runtime registry**

```bash
git add src-tauri/src/task_runtime.rs src-tauri/src/main.rs
git commit -m "feat: 新增基于 group run 的任务运行时控制"
```

---

### Task 3: Wire Scheduled Copy, Manual Copy, and Manual Deploy into the Same Backend State Machine

**Files:**
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/scanner.rs`
- Modify: `src-tauri/src/deploy.rs`
- Modify: `src-tauri/src/task_manager.rs`
- Modify: `src-tauri/src/task_events.rs`
- Test: `src-tauri/src/task_manager.rs`

- [ ] **Step 1: Add failing backend tests for manual completion and manual deploy failure**

Append these tests to `src-tauri/src/task_manager.rs`:

```rust
#[test]
fn manual_copy_completion_marks_group_completed() {
    let manager = TaskManager::new_in_memory();
    let handle = manager
        .begin_manual_copy_run(StartManualCopyRequest {
            display_name: "pkg".to_string(),
            folder_name: "pkg".to_string(),
            source_path: "C:\\src\\pkg".to_string(),
            local_target_path: "D:\\dst\\pkg".to_string(),
            trigger_source: TaskTriggerSource::Manual,
        })
        .unwrap();

    manager
        .mark_copy_completed(&handle.task_group_id, &handle.run_id, false)
        .unwrap();

    let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
    assert_eq!(detail.summary_status, TaskSummaryStatus::Completed);
}

#[test]
fn manual_deploy_failure_is_recorded_under_manual_deploy_run() {
    let manager = TaskManager::new_in_memory();
    let handle = manager
        .begin_manual_deploy_run(StartManualDeployRequest {
            task_group_id: None,
            display_name: "pkg".to_string(),
            folder_name: "pkg".to_string(),
            local_target_path: "D:\\dst\\pkg".to_string(),
            source_path: "D:\\dst\\pkg".to_string(),
            trigger_source: TaskTriggerSource::Manual,
        })
        .unwrap();

    manager
        .register_deploy_targets(
            &handle.task_group_id,
            &handle.run_id,
            &[DeployTarget {
                server_id: "server-a".to_string(),
                server_name: "Server A".to_string(),
                remote_target: "/srv/pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            }],
        )
        .unwrap();

    manager
        .fail_attempt(
            &handle.task_group_id,
            &handle.run_id,
            "server-a",
            DeployStage::Connecting,
            "ssh timeout".to_string(),
        )
        .unwrap();

    let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
    assert_eq!(detail.runs[0].run_type, TaskRunType::ManualDeploy);
    assert_eq!(detail.summary_status, TaskSummaryStatus::Failed);
}
```

- [ ] **Step 2: Run the manager tests and confirm the new cases fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_manager::tests -- --nocapture`
Expected: FAIL until the runtime code actually creates and mutates manual runs.

- [ ] **Step 3: Wire scheduled and manual copy paths through `TaskManager` and `TaskRuntimeRegistry`**

In `src-tauri/src/main.rs`, update the manual copy worker so each queued task creates a backend run before copy starts:

```rust
let run_handle = task_manager
    .begin_manual_copy_run(StartManualCopyRequest {
        display_name: task.folder_name.clone(),
        folder_name: task.folder_name.clone(),
        source_path: task.source_path.clone(),
        local_target_path: task.local_path.clone(),
        trigger_source: TaskTriggerSource::Manual,
    })
    .unwrap();

task_runtime.activate(run_handle.task_group_id.clone(), run_handle.run_id.clone());

let result = scanner::temporary_copy(
    &app_handle,
    &config_snapshot,
    config.clone(),
    task_manager.clone(),
    Some(run_handle.clone()),
    task.source_path.clone(),
    task.target_root_path.clone(),
    task.overwrite_existing,
    task.file_extensions.clone(),
    task.filename_includes.clone(),
    should_cancel.clone(),
    is_paused.clone(),
)
.await;
```

In `src-tauri/src/scanner.rs`, make `temporary_copy` and `perform_copy` accept an optional `TaskRunHandle` and update copy state through the manager:

```rust
pub async fn temporary_copy<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    live_config: Arc<Mutex<AppConfig>>,
    task_manager: crate::task_manager::TaskManager,
    run_handle: Option<crate::task_manager::TaskRunHandle>,
    source_path: String,
    target_root_path: String,
    overwrite_existing: bool,
    file_extensions: Vec<String>,
    filename_includes: Vec<String>,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> Result<ScanResult, String> {
    // call mark_copy_completed / mark_copy_failed / mark_copy_cancelled using run_handle
}
```

- [ ] **Step 4: Wire manual deploy and deploy logging into the same run history**

In `src-tauri/src/main.rs`, the new `start_manual_deploy_task` command should create the run, activate the runtime, register targets, and then call `deploy::deploy_manual`:

```rust
let run_handle = state.task_manager.begin_manual_deploy_run(request.clone())?;
state.task_runtime.activate(run_handle.task_group_id.clone(), run_handle.run_id.clone());

let tracking = state
    .task_manager
    .tracking_context(run_handle.task_group_id.clone(), run_handle.run_id.clone());

tracking.register_targets(&targets)?;

deploy::deploy_manual(
    &app_handle,
    &server,
    &post_commands,
    &local_path,
    &remote_path,
    should_cancel.clone(),
    Some(tracking.clone()),
)?;
```

In `src-tauri/src/deploy.rs`, add structured task-log emission alongside the existing text log:

```rust
fn emit_task_log<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    tracking: Option<&crate::task_manager::DeployTrackingContext>,
    server: Option<&DeployServer>,
    level: &str,
    message: String,
) {
    let entry = crate::task_events::TaskLogEntry {
        task_group_id: tracking.map(|ctx| ctx.task_group_id().to_string()),
        run_id: tracking.map(|ctx| ctx.run_id().to_string()),
        server_id: server.map(|s| s.id.clone()),
        server_name: server.map(|s| s.name.clone()),
        level: level.to_string(),
        message,
        timestamp: crate::task_domain::current_timestamp(),
    };
    let _ = app_handle.emit(crate::task_events::TASK_LOG_EVENT, entry);
}
```

- [ ] **Step 5: Re-run the manager tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_manager::tests -- --nocapture`
Expected: PASS, including the new manual copy/manual deploy cases.

- [ ] **Step 6: Commit the backend runtime integration**

```bash
git add src-tauri/src/main.rs src-tauri/src/scanner.rs src-tauri/src/deploy.rs src-tauri/src/task_manager.rs src-tauri/src/task_events.rs
git commit -m "feat: 将手动复制部署并入后端任务状态机"
git push
```

This slice will almost certainly exceed 200 changed lines, so push immediately after the commit.

---

### Task 4: Add Frontend Task-State DTOs and a Dedicated `taskStateStore`

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/lib/taskStateStore.ts`
- Create: `src/lib/taskStateStore.test.mjs`

- [ ] **Step 1: Write failing store tests**

Create `src/lib/taskStateStore.test.mjs` with:

```javascript
import assert from 'node:assert/strict';
import { createTaskStateStore } from './taskStateStore.ts';

const sampleGroup = {
  task_group_id: 'group-1',
  display_name: 'pkg',
  folder_name: 'pkg',
  source_path: 'C:\\src\\pkg',
  local_target_path: 'D:\\dst\\pkg',
  summary_status: 'copying',
  copy_status: 'running',
  deploy_status: 'not_started',
  started_at: '2026-04-02T12:00:00+08:00',
  finished_at: null,
  elapsed_seconds: 12,
  latest_run_id: 'run-1',
  had_failures: false,
  merge_key: 'manual||d:\\dst\\pkg||pkg',
  task_config_id: null,
  server_rollups: [],
};

const sampleDetail = {
  task_group_id: 'group-1',
  merge_key: 'manual||d:\\dst\\pkg||pkg',
  task_config_id: null,
  source_type: 'manual',
  display_name: 'pkg',
  folder_name: 'pkg',
  source_path: 'C:\\src\\pkg',
  local_target_path: 'D:\\dst\\pkg',
  copy_status: 'running',
  deploy_status: 'not_started',
  summary_status: 'copying',
  started_at: '2026-04-02T12:00:00+08:00',
  finished_at: null,
  elapsed_seconds: 12,
  latest_run_id: 'run-1',
  had_failures: false,
  server_rollups: [],
  runs: [],
};

const api = {
  listTaskGroups: async () => [sampleGroup],
  getTaskGroupDetail: async () => sampleDetail,
};

const store = createTaskStateStore(api);
await store.hydrateTaskState();
assert.equal(store.groups[0].task_group_id, 'group-1');

await store.selectTaskGroup('group-1');
assert.equal(store.selectedTaskGroupId, 'group-1');
assert.equal(store.selectedGroupDetail.task_group_id, 'group-1');

store.applyGroupsSnapshot({ groups: [] });
assert.equal(store.groups.length, 0);
```

- [ ] **Step 2: Run the store tests and confirm they fail**

Run: `node src/lib/taskStateStore.test.mjs`
Expected: FAIL because `taskStateStore.ts` and the DTO wrappers do not exist yet.

- [ ] **Step 3: Add DTOs and invoke wrappers to `src/lib/tauri.ts`**

Add the backend task DTOs and command wrappers:

```typescript
export interface TaskRunHandle {
  task_group_id: string;
  run_id: string;
}

export interface TaskLogEntry {
  task_group_id: string | null;
  run_id: string | null;
  server_id: string | null;
  server_name: string | null;
  level: 'info' | 'success' | 'warn' | 'error' | 'command';
  message: string;
  timestamp: string;
}

export async function listTaskGroups(): Promise<TaskGroupListItem[]> {
  return await invoke('list_task_groups');
}

export async function getTaskGroupDetail(taskGroupId: string): Promise<TaskGroup> {
  return await invoke('get_task_group_detail', { taskGroupId });
}

export async function startManualCopyTask(request: StartManualCopyTaskRequest): Promise<TaskRunHandle> {
  return await invoke('start_manual_copy_task', { request });
}

export async function startManualDeployTask(request: StartManualDeployTaskRequest): Promise<TaskRunHandle> {
  return await invoke('start_manual_deploy_task', { request });
}
```

- [ ] **Step 4: Implement the dedicated task-state store**

Create `src/lib/taskStateStore.ts`:

```typescript
import { reactive } from 'vue';
import {
  listTaskGroups,
  getTaskGroupDetail,
  startManualCopyTask,
  startManualDeployTask,
  clearTaskGroup,
  clearTaskGroups,
  cancelTaskRun,
  pauseTaskRun,
  resumeTaskRun,
  retryTaskGroupDeploy,
} from './tauri';

export function createTaskStateStore(api = {
  listTaskGroups,
  getTaskGroupDetail,
  startManualCopyTask,
  startManualDeployTask,
  clearTaskGroup,
  clearTaskGroups,
  cancelTaskRun,
  pauseTaskRun,
  resumeTaskRun,
  retryTaskGroupDeploy,
}) {
  const state = reactive({
    groups: [] as TaskGroupListItem[],
    selectedTaskGroupId: null as string | null,
    selectedGroupDetail: null as TaskGroup | null,
    isHydrated: false,
    isLoadingDetail: false,
    taskLogs: [] as TaskLogEntry[],
  });

  async function hydrateTaskState() {
    state.groups = await api.listTaskGroups();
    state.isHydrated = true;
  }

  async function selectTaskGroup(taskGroupId: string) {
    state.selectedTaskGroupId = taskGroupId;
    state.isLoadingDetail = true;
    try {
      state.selectedGroupDetail = await api.getTaskGroupDetail(taskGroupId);
    } finally {
      state.isLoadingDetail = false;
    }
  }

  function applyGroupsSnapshot(payload: { groups: TaskGroupListItem[] }) {
    state.groups = payload.groups;
    if (state.selectedTaskGroupId && !payload.groups.some(group => group.task_group_id === state.selectedTaskGroupId)) {
      state.selectedTaskGroupId = null;
      state.selectedGroupDetail = null;
    }
  }

  function applyDetailSnapshot(payload: { task_group_id: string; group: TaskGroup }) {
    if (payload.task_group_id === state.selectedTaskGroupId) {
      state.selectedGroupDetail = payload.group;
    }
  }

  function appendTaskLog(entry: TaskLogEntry) {
    state.taskLogs.push(entry);
  }

  return { ...state, hydrateTaskState, selectTaskGroup, applyGroupsSnapshot, applyDetailSnapshot, appendTaskLog };
}

export const taskStateStore = createTaskStateStore();
```

- [ ] **Step 5: Re-run the store tests**

Run: `node src/lib/taskStateStore.test.mjs`
Expected: PASS.

- [ ] **Step 6: Commit the frontend task-state foundation**

```bash
git add src/lib/tauri.ts src/lib/taskStateStore.ts src/lib/taskStateStore.test.mjs
git commit -m "feat: 新增前端 task-state store 与调用契约"
```

---

### Task 5: Switch App-Level Hydration and Persistence to Snapshots + Logs Only

**Files:**
- Modify: `src/App.vue`
- Modify: `src/lib/store.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src-tauri/src/persist.rs`
- Test: `src-tauri/src/persist.rs`

- [ ] **Step 1: Add a failing persistence test for logs-only UI state**

Append to `src-tauri/src/persist.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::UiState;

    #[test]
    fn ui_state_ignores_legacy_task_records_field() {
        let parsed: UiState = serde_json::from_str(
            r#"{"logs":[{"msg":"ok"}],"task_records":[{"id":"legacy-row"}]}"#,
        )
        .unwrap();

        assert_eq!(parsed.logs.len(), 1);
    }
}
```

- [ ] **Step 2: Run the persistence test and confirm it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml persist::tests -- --nocapture`
Expected: FAIL until `UiState` no longer requires `task_records`.

- [ ] **Step 3: Make UI persistence logs-only and keep legacy reads harmless**

In `src-tauri/src/persist.rs`, change the shape:

```rust
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct UiState {
    #[serde(default)]
    pub logs: Vec<Value>,
}

#[tauri::command]
pub fn save_ui_state(
    app_handle: tauri::AppHandle,
    logs: Vec<Value>,
) -> Result<(), String> {
    let state = UiState { logs };
    // existing tmp + rename persistence
}
```

In `src/lib/tauri.ts`, update the frontend contract:

```typescript
export interface UiState {
  logs: unknown[];
}

export async function saveUiState(logs: unknown[]): Promise<void> {
  await invoke('save_ui_state', { logs });
}
```

- [ ] **Step 4: Rewire `App.vue` and `src/lib/store.ts`**

In `src/lib/store.ts`, remove `taskRecords` and keep only logs, scheduler flags, manual deploy flags, and manual copy form state:

```typescript
export const appStore = reactive({
  logs: [] as LogEntry[],
  progress: null as ProgressState | null,
  isRunning: false,
  nextRunTime: '-',
  isManualDeploying: false,
  manualDeployMsg: '',
  maxLogLines: 200,
});
```

In `src/App.vue`, replace the old listeners with task-state store hydration and task-log subscriptions:

```typescript
import { taskStateStore } from '@/lib/taskStateStore';

let unlistenTaskGroups: (() => void) | null = null;
let unlistenTaskDetail: (() => void) | null = null;
let unlistenTaskLog: (() => void) | null = null;

watch(() => appStore.logs.length, scheduleSave);

onMounted(async () => {
  const persisted = await loadUiState();
  if (Array.isArray(persisted.logs)) {
    appStore.logs.push(...persisted.logs.slice(-appStore.maxLogLines));
  }

  await taskStateStore.hydrateTaskState();

  unlistenTaskGroups = await listen('task-groups-snapshot', (event) => {
    taskStateStore.applyGroupsSnapshot(event.payload as { groups: TaskGroupListItem[] });
  });
  unlistenTaskDetail = await listen('task-group-detail-snapshot', (event) => {
    taskStateStore.applyDetailSnapshot(event.payload as { task_group_id: string; group: TaskGroup });
  });
  unlistenTaskLog = await listen('task-log', (event) => {
    taskStateStore.appendTaskLog(event.payload as TaskLogEntry);
  });
});
```

- [ ] **Step 5: Re-run persistence tests and frontend typecheck**

Run: `cargo test --manifest-path src-tauri/Cargo.toml persist::tests -- --nocapture`
Expected: PASS.

Run: `pnpm check`
Expected: PASS for the new `saveUiState(logs)` signature and removal of `taskRecords` from `App.vue`.

- [ ] **Step 6: Commit the app-shell cutover**

```bash
git add src/App.vue src/lib/store.ts src/lib/tauri.ts src-tauri/src/persist.rs
git commit -m "refactor: 改为后端快照 hydration 与日志持久化"
```

---

### Task 6: Rewrite `TaskStatusPage` Around Task Groups, Runs, Attempts, and Group/Run Actions

**Files:**
- Create: `src/components/tasks/TaskGroupsTable.vue`
- Create: `src/components/tasks/TaskGroupDetailPanel.vue`
- Create: `src/lib/taskStatusView.ts`
- Create: `src/lib/taskStatusView.test.mjs`
- Modify: `src/pages/TaskStatusPage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1: Write failing selector tests for the new task page view-model**

Create `src/lib/taskStatusView.test.mjs`:

```javascript
import assert from 'node:assert/strict';
import { buildTaskRows, buildTaskDetailSections } from './taskStatusView.ts';

const rows = buildTaskRows([
  {
    task_group_id: 'group-2',
    display_name: 'newer',
    folder_name: 'newer',
    source_path: 'C:\\src\\newer',
    local_target_path: 'D:\\dst\\newer',
    summary_status: 'deploying',
    copy_status: 'completed',
    deploy_status: 'running',
    started_at: '2026-04-02T12:10:00+08:00',
    finished_at: null,
    elapsed_seconds: 40,
    latest_run_id: 'run-2',
    had_failures: false,
    merge_key: 'scheduled||d:\\dst\\newer||newer',
    task_config_id: 'task-a',
    server_rollups: [],
  },
  {
    task_group_id: 'group-1',
    display_name: 'older',
    folder_name: 'older',
    source_path: 'C:\\src\\older',
    local_target_path: 'D:\\dst\\older',
    summary_status: 'completed',
    copy_status: 'completed',
    deploy_status: 'completed',
    started_at: '2026-04-02T11:10:00+08:00',
    finished_at: '2026-04-02T11:20:00+08:00',
    elapsed_seconds: 600,
    latest_run_id: 'run-1',
    had_failures: false,
    merge_key: 'scheduled||d:\\dst\\older||older',
    task_config_id: 'task-a',
    server_rollups: [],
  },
]);

assert.equal(rows[0].task_group_id, 'group-2');

const detail = buildTaskDetailSections({
  task_group_id: 'group-2',
  merge_key: 'scheduled||d:\\dst\\newer||newer',
  task_config_id: 'task-a',
  source_type: 'scheduled',
  display_name: 'newer',
  folder_name: 'newer',
  source_path: 'C:\\src\\newer',
  local_target_path: 'D:\\dst\\newer',
  copy_status: 'completed',
  deploy_status: 'partial_failed',
  summary_status: 'partial_failed',
  started_at: '2026-04-02T12:10:00+08:00',
  finished_at: null,
  elapsed_seconds: 40,
  latest_run_id: 'run-2',
  had_failures: true,
  server_rollups: [
    {
      server_id: 'server-a',
      server_name: 'Server A',
      latest_status: 'failed',
      latest_attempt_id: 'attempt-1',
      success_count: 0,
      failure_count: 1,
      last_error_message: 'ssh timeout',
      attempt_ids: ['attempt-1'],
    },
  ],
  runs: [],
});

assert.equal(detail.serverFailures[0].message, 'ssh timeout');
```

- [ ] **Step 2: Run the selector tests and confirm they fail**

Run: `node src/lib/taskStatusView.test.mjs`
Expected: FAIL because `taskStatusView.ts` does not exist yet.

- [ ] **Step 3: Implement pure selectors and focused UI components**

Create `src/lib/taskStatusView.ts`:

```typescript
export function buildTaskRows(groups: TaskGroupListItem[]) {
  return [...groups].sort((left, right) => right.started_at.localeCompare(left.started_at));
}

export function buildTaskDetailSections(group: TaskGroup) {
  return {
    serverFailures: group.server_rollups
      .filter((rollup) => rollup.last_error_message)
      .map((rollup) => ({
        serverId: rollup.server_id,
        serverName: rollup.server_name,
        message: rollup.last_error_message!,
      })),
    runs: group.runs,
  };
}
```

Create `src/components/tasks/TaskGroupsTable.vue`:

```vue
<script setup lang="ts">
import type { TaskGroupListItem } from '@/lib/tauri';

defineProps<{
  rows: TaskGroupListItem[];
  selectedTaskGroupId: string | null;
  onSelect: (taskGroupId: string) => void;
}>();
</script>
```

Create `src/components/tasks/TaskGroupDetailPanel.vue`:

```vue
<script setup lang="ts">
import type { TaskGroup, TaskLogEntry } from '@/lib/tauri';

defineProps<{
  group: TaskGroup | null;
  taskLogs: TaskLogEntry[];
  onRetryDeploy: (taskGroupId: string) => Promise<void>;
  onPauseRun: (taskGroupId: string, runId: string) => Promise<void>;
  onResumeRun: (taskGroupId: string, runId: string) => Promise<void>;
  onCancelRun: (taskGroupId: string, runId: string) => Promise<void>;
}>();
</script>
```

- [ ] **Step 4: Rewrite `TaskStatusPage.vue` to use the new store and actions**

Replace the old `taskRecords`-driven page logic with store-backed group/run actions:

```typescript
import { computed, onMounted } from 'vue';
import { taskStateStore } from '@/lib/taskStateStore';
import { buildTaskRows } from '@/lib/taskStatusView';

const rows = computed(() => buildTaskRows(taskStateStore.groups));

async function handleSelect(taskGroupId: string) {
  await taskStateStore.selectTaskGroup(taskGroupId);
}

async function handlePause(taskGroupId: string, runId: string) {
  await taskStateStore.pauseTaskRun(taskGroupId, runId);
}

async function handleResume(taskGroupId: string, runId: string) {
  await taskStateStore.resumeTaskRun(taskGroupId, runId);
}

async function handleCancel(taskGroupId: string, runId: string) {
  await taskStateStore.cancelTaskRun(taskGroupId, runId);
}

async function handleRetryDeploy(taskGroupId: string) {
  await taskStateStore.retryTaskGroupDeploy(taskGroupId);
}

onMounted(async () => {
  if (!taskStateStore.isHydrated) {
    await taskStateStore.hydrateTaskState();
  }
});
```

Update `src/locales/messages.ts` with labels for `queued`, `deploying`, `partial_failed`, `retry deploy`, `clear group`, `server failures`, `attempt timeline`, and `task logs`.

- [ ] **Step 5: Re-run selector tests and frontend typecheck**

Run: `node src/lib/taskStatusView.test.mjs`
Expected: PASS.

Run: `pnpm check`
Expected: PASS for the rewritten page and the new task components.

- [ ] **Step 6: Commit the task-page rewrite**

```bash
git add src/components/tasks/TaskGroupsTable.vue src/components/tasks/TaskGroupDetailPanel.vue src/lib/taskStatusView.ts src/lib/taskStatusView.test.mjs src/pages/TaskStatusPage.vue src/locales/messages.ts
git commit -m "refactor: 重写任务页以消费后端状态机快照"
git push
```

This rewrite is expected to exceed 200 changed lines, so push immediately after the commit.

---

### Task 7: Switch Manual Copy, Restore, and Manual Deploy Entry Points to the New Commands

**Files:**
- Modify: `src/components/ManualCopyModal.vue`
- Modify: `src/pages/ManualCopyPage.vue`
- Modify: `src/pages/SettingsPage.vue`
- Modify: `src/lib/taskStateStore.ts`
- Modify: `src/lib/taskStateStore.test.mjs`

- [ ] **Step 1: Add failing store tests for manual task actions**

Extend `src/lib/taskStateStore.test.mjs`:

```javascript
const actionCalls = [];
const actionStore = createTaskStateStore({
  ...api,
  startManualCopyTask: async (request) => {
    actionCalls.push(['copy', request]);
    return { task_group_id: 'group-copy', run_id: 'run-copy' };
  },
  startManualDeployTask: async (request) => {
    actionCalls.push(['deploy', request]);
    return { task_group_id: 'group-deploy', run_id: 'run-deploy' };
  },
});

await actionStore.startManualCopyTask({
  source_path: 'C:\\src\\pkg',
  target_root_path: 'D:\\dst',
  overwrite_existing: false,
  file_extensions: ['.zip'],
  filename_includes: ['pkg'],
});

await actionStore.startManualDeployTask({
  task_group_id: 'group-copy',
  display_name: 'pkg',
  local_path: 'D:\\dst\\pkg',
  remote_path: '/srv/pkg',
  bindings: [],
});

assert.equal(actionCalls.length, 2);
assert.equal(actionCalls[0][0], 'copy');
assert.equal(actionCalls[1][0], 'deploy');
```

- [ ] **Step 2: Run the store tests and confirm they fail**

Run: `node src/lib/taskStateStore.test.mjs`
Expected: FAIL because the store does not yet expose `startManualCopyTask` and `startManualDeployTask`.

- [ ] **Step 3: Add manual task action methods to the store**

In `src/lib/taskStateStore.ts`, add:

```typescript
async function startManualCopyTask(request: StartManualCopyTaskRequest) {
  const handle = await api.startManualCopyTask(request);
  await selectTaskGroup(handle.task_group_id);
  return handle;
}

async function startManualDeployTask(request: StartManualDeployTaskRequest) {
  const handle = await api.startManualDeployTask(request);
  await selectTaskGroup(handle.task_group_id);
  return handle;
}
```

- [ ] **Step 4: Update the manual copy modal, manual copy page, and settings manual deploy form**

In `src/components/ManualCopyModal.vue`, replace `queueTemporaryCopy` with the new store action:

```typescript
import { taskStateStore } from '@/lib/taskStateStore';

async function enqueueCopy(source: string, target: string, overwriteExisting: boolean) {
  const exts = [...selectedExtensions.value];
  const kws = [...selectedKeywords.value];

  await taskStateStore.startManualCopyTask({
    source_path: source,
    target_root_path: target,
    overwrite_existing: overwriteExisting,
    file_extensions: exts,
    filename_includes: kws,
  });

  statusTone.value = 'success';
  statusMsg.value = t('manualCopy.addedToQueue');
}
```

In `src/pages/TaskStatusPage.vue`, update the restore action to use `startManualCopyTask` instead of `queueTemporaryCopy`.

In `src/pages/SettingsPage.vue`, replace `preRegisterManualDeploy` / `setManualDeployCurrentServer` / `manualDeploy(...)` with one store-driven action:

```typescript
import { taskStateStore } from '@/lib/taskStateStore';

await taskStateStore.startManualDeployTask({
  task_group_id: null,
  display_name: folderName,
  folder_name: folderName,
  local_path: manualLocalPath.value,
  remote_path: manualRemotePath.value,
  bindings: manualServerBindings.value.map((binding) => ({
    server_id: binding.server_id,
    command_group_ids: [...binding.command_group_ids],
  })),
});
```

- [ ] **Step 5: Re-run store tests and frontend typecheck**

Run: `node src/lib/taskStateStore.test.mjs`
Expected: PASS.

Run: `pnpm check`
Expected: PASS for manual copy, restore, and manual deploy entry points.

- [ ] **Step 6: Commit the manual entry-point switch**

```bash
git add src/components/ManualCopyModal.vue src/pages/ManualCopyPage.vue src/pages/SettingsPage.vue src/lib/taskStateStore.ts src/lib/taskStateStore.test.mjs src/pages/TaskStatusPage.vue
git commit -m "refactor: 切换手动复制与手动部署入口到 task-state 命令"
git push
```

This task rewires multiple screens and will likely exceed 200 changed lines, so push immediately after the commit.

---

### Task 8: Delete the Legacy `taskRecords` Inference Path and Old State-Driving Commands

**Files:**
- Modify: `src/lib/store.ts`
- Modify: `src/App.vue`
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/TaskStatusPage.vue`
- Modify: `src/pages/SettingsPage.vue`
- Modify: `src/components/ManualCopyModal.vue`
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Remove the old frontend APIs and helpers**

Delete these exports from `src/lib/store.ts`:

```typescript
// remove all of these
upsertTaskRecord
syncTaskRecordByLog
updateManualCopyTaskState
markTaskRecordCancelled
markTaskRecordSkipped
markTaskRecordIgnored
prepareTaskRecordForRetry
removeTaskRecord
preRegisterManualDeploy
setManualDeployCurrentServer
```

Delete these frontend wrappers from `src/lib/tauri.ts`:

```typescript
manualDeploy
temporaryCopy
queueTemporaryCopy
```

- [ ] **Step 2: Remove old backend command exposure and legacy state-driving events**

In `src-tauri/src/main.rs`, remove the old commands from `tauri::generate_handler!`:

```rust
manual_deploy,
temporary_copy,
queue_temporary_copy,
```

Also remove the `emit_manual_copy_task_state(...)` helper and any code that still emits `manual-copy-task-state` purely to drive frontend status.

- [ ] **Step 3: Verify the old inference symbols are gone from app code**

Run:

```bash
rg "taskRecords|syncTaskRecordByLog|upsertTaskRecord|updateManualCopyTaskState|preRegisterManualDeploy|setManualDeployCurrentServer|queueTemporaryCopy\\(|manualDeploy\\(" src src-tauri/src/main.rs
```

Expected: No matches in application code. Matches in docs/spec files are acceptable.

- [ ] **Step 4: Re-run the frontend typecheck and backend tests**

Run: `pnpm check`
Expected: PASS.

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Commit the hard cut**

```bash
git add src/lib/store.ts src/App.vue src/lib/tauri.ts src/pages/TaskStatusPage.vue src/pages/SettingsPage.vue src/components/ManualCopyModal.vue src-tauri/src/main.rs
git commit -m "refactor: 删除旧 taskRecords 状态推断链路"
git push
```

This is the actual compatibility removal step and should be pushed immediately after the commit.

---

### Task 9: Full Verification, Build, and Manual Smoke Testing

**Files:** None (verification only)

- [ ] **Step 1: Run the backend task-state tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_ -- --nocapture`
Expected: PASS for all task-state manager/runtime tests.

- [ ] **Step 2: Run the full backend suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 3: Run the frontend pure tests**

Run: `node src/lib/taskStateStore.test.mjs`
Expected: PASS.

Run: `node src/lib/taskStatusView.test.mjs`
Expected: PASS.

- [ ] **Step 4: Run the frontend typecheck**

Run: `pnpm check`
Expected: PASS.

- [ ] **Step 5: Build the desktop app**

Run: `cmd /c pnpm tauri:build:versioned-exe`
Expected: PASS, producing the renamed versioned executable.

- [ ] **Step 6: Manual smoke test the hard cut**

Verify these flows manually:

```text
1. Open the task page with no new runtime logs arriving and confirm historical groups still load from list_task_groups.
2. Trigger a scheduled scan and confirm the task page updates via task-groups-snapshot/task-group-detail-snapshot, not log parsing.
3. Queue a manual copy from Manual Copy and confirm a manual TaskGroup appears immediately.
4. Trigger manual deploy from Settings and confirm it creates a ManualDeploy run in TaskGroup history.
5. Pause, resume, cancel, clear, and retry deploy from the task page and confirm all actions use task_group_id/run_id successfully.
6. Restart the app during an active run and confirm the persisted state comes back as interrupted.
7. Open task detail logs and confirm task-log entries are filterable by group/run/server.
```

- [ ] **Step 7: Commit any final fixes and push if needed**

```bash
git add -A
git commit -m "fix: 完成 task-state hard cut 验证修正"
git push
```

---

## Spec Coverage Check

- Hard cut to backend snapshots: covered by Tasks 4, 5, 6, and 8.
- Manual copy/deploy merged into the same backend model: covered by Tasks 1, 3, and 7.
- Group/run-based actions: covered by Tasks 2, 6, and 7.
- No compatibility path: covered by Task 8.
- Logs remain complete and clear: covered by Tasks 1, 3, 5, and 9.
- Restart recovery and persisted state: covered by Tasks 0, 5, and 9.

## Placeholder Scan

- No `TODO`, `TBD`, or “implement later” markers were intentionally left in this plan.
- Every task contains exact files, commands, and concrete code snippets.
- Each change slice ends with an explicit commit step, and multi-file large slices include `git push` as required by the repository rule.

## Type Consistency Check

- Backend command naming is consistent across the plan: `start_manual_copy_task`, `start_manual_deploy_task`, `cancel_task_run`, `pause_task_run`, `resume_task_run`, `retry_task_group_deploy`.
- Frontend wrappers mirror the backend names with `listTaskGroups`, `getTaskGroupDetail`, `startManualCopyTask`, `startManualDeployTask`, `cancelTaskRun`, `pauseTaskRun`, `resumeTaskRun`, and `retryTaskGroupDeploy`.
- The store consistently uses `selectedTaskGroupId`, `selectedGroupDetail`, and `taskLogs`.
