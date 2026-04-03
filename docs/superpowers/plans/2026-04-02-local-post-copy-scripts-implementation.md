# Local Post-Copy Scripts Implementation Plan

> **For agentic workers:** Use subagent-driven development — dispatch tasks sequentially, review each before proceeding.

**Goal:** Add local Windows script execution (py/ps1/bat) as a post-copy action, with configurable execution order relative to remote deploy.

**Spec:** `docs/superpowers/specs/2026-04-02-local-post-copy-scripts-design.md`

---

## Phase 1: Backend Data Model and Execution Engine

### Task 1: Add Data Model Types to config.rs and tauri.ts

**Files:**
- Modify: `src-tauri/src/config.rs`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1:** Add Rust types: `OnFailure`, `LocalCommandGroup`, `LocalScriptBinding`, `PostCopyExecutionOrder`
- [ ] **Step 2:** Add fields to `AppConfig` (`local_command_groups`) and `ScanTask` (`local_script_binding`, `post_copy_execution_order`)
- [ ] **Step 3:** Add TypeScript interfaces to `tauri.ts`
- [ ] **Step 4:** Run `cargo build && cargo test`
- [ ] **Step 5:** Commit: `feat(config): 新增本地脚本组数据模型`

### Task 2: Create local_exec.rs Execution Engine

**Files:**
- Create: `src-tauri/src/local_exec.rs`
- Modify: `src-tauri/src/main.rs` (add `mod local_exec;`)

- [ ] **Step 1:** Implement `resolve_command()` — interpreter auto-detection (.py/.ps1/.bat/.cmd)
- [ ] **Step 2:** Implement `substitute_variables()` — `${folder_name}`, `${local_target}`, `${source_path}`, `${filename}`
- [ ] **Step 3:** Implement `run_single_command()` — subprocess execution with stdout/stderr capture and log emission
- [ ] **Step 4:** Implement `execute_local_scripts()` — group iteration with `on_failure` handling
- [ ] **Step 5:** Implement `find_tar_gz_filename()` — find first .tar.gz in dir
- [ ] **Step 6:** Add unit tests for `resolve_command` and `substitute_variables`
- [ ] **Step 7:** Run `cargo test local_exec && cargo fmt && cargo clippy`
- [ ] **Step 8:** Commit: `feat(local_exec): 新增本地脚本执行引擎`

### Task 3: Extend Task State Machine for local_exec Phase

**Files:**
- Modify: `src-tauri/src/task_domain.rs`
- Modify: `src-tauri/src/task_events.rs`
- Modify: `src-tauri/src/task_manager.rs`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1:** Add `LocalExecState` enum to `task_domain.rs`
- [ ] **Step 2:** Add `local_exec_phase` to `TaskRun`, add `LocalExecuting` to `TaskSummaryStatus`
- [ ] **Step 3:** Update `summarize_group` to consider copy -> local_exec -> deploy pipeline
- [ ] **Step 4:** Add `begin_local_exec`, `mark_local_exec_completed`, `mark_local_exec_failed` to `TaskManager`
- [ ] **Step 5:** Update `TaskGroupListItem` in `task_events.rs` to include `local_exec_status`
- [ ] **Step 6:** Update TS types in `tauri.ts`
- [ ] **Step 7:** Add unit tests
- [ ] **Step 8:** Run `cargo test && cargo fmt`
- [ ] **Step 9:** Commit: `feat(task_domain): 扩展任务状态机支持本地脚本执行阶段`

### Task 4: Integrate local_exec into scanner.rs Post-Copy Orchestration

**Files:**
- Modify: `src-tauri/src/scanner.rs`

- [ ] **Step 1:** Extract post-copy deploy block into `orchestrate_post_copy()` function
- [ ] **Step 2:** Add dispatch logic based on `(has_local, has_remote, execution_order)`
- [ ] **Step 3:** Implement `local_first` path: local scripts then remote deploy, abort blocks remote
- [ ] **Step 4:** Implement `remote_first` path: remote deploy then local scripts
- [ ] **Step 5:** Implement `parallel` path: both via `std::thread::scope`
- [ ] **Step 6:** Wire TaskManager calls (`begin_local_exec`, `mark_local_exec_completed/failed`)
- [ ] **Step 7:** Run `cargo test && cargo build`
- [ ] **Step 8:** Commit: `feat(scanner): 集成本地脚本执行编排逻辑`
- [ ] **Step 9:** Push (>200 lines)

### Task 5: Wire Local Exec Log Events

**Files:**
- Modify: `src-tauri/src/local_exec.rs`

- [ ] **Step 1:** Ensure `run_single_command` emits `log-message` events for stdout/stderr
- [ ] **Step 2:** Ensure structured `task-log` events are emitted with group/run context
- [ ] **Step 3:** Run `cargo build && cargo test`
- [ ] **Step 4:** Commit: `feat(local_exec): 本地脚本执行日志接入事件流`

---

## Phase 2: Frontend Configuration UI

### Task 6: Settings Page — Local Script Groups Management

**Files:**
- Modify: `src/pages/SettingsPage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1:** Add Local Script Groups CRUD (add/edit/delete groups)
- [ ] **Step 2:** Group form: name, commands list, on_failure dropdown
- [ ] **Step 3:** Show variable reference hint
- [ ] **Step 4:** Add all i18n keys (en + zh)
- [ ] **Step 5:** Run `pnpm check`
- [ ] **Step 6:** Commit: `feat(settings): 新增本地脚本组管理界面`

### Task 7: Task Editor — Local Script Binding and Execution Order

**Files:**
- Modify: `src/pages/SettingsPage.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1:** Add execution order segmented control (local_first / remote_first / parallel)
- [ ] **Step 2:** Add local script group binding list (toggle/reorder)
- [ ] **Step 3:** Wire save/load for new task fields
- [ ] **Step 4:** Add i18n keys
- [ ] **Step 5:** Run `pnpm check`
- [ ] **Step 6:** Commit: `feat(settings): 任务编辑器新增本地脚本绑定和执行顺序配置`
- [ ] **Step 7:** Push (>200 lines across Tasks 6+7)

---

## Phase 3: Frontend Status Display

### Task 8: Task Detail — Local Script Execution Progress

**Files:**
- Modify: `src/components/tasks/TaskGroupDetailPanel.vue`
- Modify: `src/components/tasks/TaskGroupsTable.vue`
- Modify: `src/locales/messages.ts`

- [ ] **Step 1:** Add local_exec phase display between copy and deploy in detail panel
- [ ] **Step 2:** Add `localExecStatusLabel` and `localExecStatusClass` helpers
- [ ] **Step 3:** Update groups table to show local_exec status column if applicable
- [ ] **Step 4:** Add i18n keys
- [ ] **Step 5:** Run `pnpm check`
- [ ] **Step 6:** Commit: `feat(task-detail): 任务详情展示本地脚本执行阶段状态`

### Task 9: Full Verification and Build

**Files:** None (verification only)

- [ ] **Step 1:** `cargo fmt && cargo clippy`
- [ ] **Step 2:** `cargo test`
- [ ] **Step 3:** `pnpm check`
- [ ] **Step 4:** `cmd /c pnpm tauri:build:versioned-exe`
- [ ] **Step 5:** Manual smoke test with a .bat script
- [ ] **Step 6:** Commit fixes if needed, push

---

## Dependency Graph

```
Task 1 (config types) ─┬─> Task 2 (local_exec.rs)
                        ├─> Task 3 (task state machine)
                        │         │
                        │         v
                        └─> Task 4 (scanner orchestration) ─> Task 5 (log wiring)
                        │
                        ├─> Task 6 (Settings: group mgmt) ─> Task 7 (task binding)
                        │
                        └─> Task 8 (detail display) depends on Tasks 3+7
                                    │
                                    v
                              Task 9 (verify + build)
```
