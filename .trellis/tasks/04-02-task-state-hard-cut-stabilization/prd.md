# brainstorm: task state hard cut stabilization

## Goal

Stabilize task state persistence and summary logic by fixing Windows-safe overwrite behavior and correcting deploy summary status for mixed success and interrupted attempts, with strict TDD.

## What I already know

* User requires changes limited to `src-tauri/src/task_persist.rs` and `src-tauri/src/task_domain.rs` unless absolutely required.
* `save_task_state` writes to a temp file then calls `fs::rename`, which fails on Windows when the destination exists.
* `TaskRun::refresh_deploy_phase` marks mixed success + interrupted attempts as `DeployState::Completed`, which is incorrect.
* Worktree: `C:/WorkSpace/File-Sync-Tool/.worktrees/task-state-hard-cut` (branch `refactor/task-state-hard-cut`).
* Baseline `cargo test` in `src-tauri` passes (7 tests), with existing warnings in `main.rs`.

## Assumptions (temporary)

* Overwrite fix should be minimal and localized to `task_persist.rs`.
* Mixed success + interrupted deploy attempts should summarize as a non-completed state (likely `PartialFailed`).
* New tests will live in the same modules as the code under test.

## Open Questions

* For mixed Success + Interrupted deploy attempts, should summary be `PartialFailed` or `Interrupted`?
* Is it acceptable to create workflow docs (design/plan) outside the owned files if required by internal process?

## Requirements (evolving)

* Add failing tests before each production change (TDD).
* Ensure `save_task_state` can overwrite an existing task state file on Windows.
* Ensure mixed Success + Interrupted deploy attempts do not summarize as `Completed`.
* Keep scope tight and avoid changes to `deploy.rs` or `task_manager.rs`.

## Acceptance Criteria (evolving)

* [ ] A new test demonstrates overwriting an existing task state file succeeds.
* [ ] A new test demonstrates mixed success + interrupted deploy attempts do not yield `Completed`.
* [ ] All affected tests pass after fixes.

## Definition of Done (team quality bar)

* Tests added/updated (unit tests) and run locally
* Rust tests pass for the affected modules
* No changes outside the owned files unless explicitly approved

## Out of Scope (explicit)

* Any changes in `deploy.rs` or `task_manager.rs`
* Cancel-last-deploy-target issue (explicitly deferred)

## Technical Notes

* `TaskRun::refresh_deploy_phase` logic currently falls through to `Completed` when only `Success` and `Interrupted` statuses exist.
* `task_persist.rs` uses `fs::rename` without handling existing destination on Windows.
* `.trellis/spec/backend/index.md` is missing locally; backend guidelines could not be read.
