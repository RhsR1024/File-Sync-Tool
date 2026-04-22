# Clipboard Gap Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the clipboard manager from its current early implementation to the `M6`-`M9` target defined in `docs/superpowers/specs/2026-04-19-clipboard-gap-migration-spec.md`, matching or exceeding `ElegantClipboard` in capability and performance.

**Architecture:** Keep the current Tauri + Rust + Vue architecture, but replace the clipboard feature's current "single-path" implementation with a richer contract-driven design: expanded data models, stronger DB layer, Windows-native clipboard/system integrations, and a modular Vue UI around groups, context actions, preview windows, and advanced settings. Unspecified details should follow `ElegantClipboard-main/` closely, except for items the spec explicitly excludes.

**Tech Stack:** Rust (`rusqlite`, `arboard`, `clipboard-master`, `windows`, `rayon`, `image`, `enigo`), Vue 3 + TypeScript + Tauri 2, existing i18n/router/store infrastructure, and `ElegantClipboard-main/` as the local reference implementation.

---

## Working Rules

- Implement only inside `C:\WorkSpace\File-Sync-Tool\.worktree\feature-clipboard-manager`.
- Keep `ElegantClipboard-main/` out of commits. `.gitignore` must keep excluding it.
- The spec already defines the approved scope. Do not re-open scope unless a blocker makes the plan impossible.
- Follow TDD for every backend behavior change: write or extend failing Rust/TS tests first, verify the failure, then implement.
- Before changing any constant/field/command signature, search for every usage across Rust, TypeScript, i18n, and routing.
- Backend spec docs are missing in `.trellis/spec/backend/`; infer repository conventions from nearby code and keep changes consistent with current style.

## Baseline Findings

- Current clipboard contracts only expose `text | html | image | file`, one favorite flag, and minimal settings.
- `watcher.rs` only captures images and plain text; files are downgraded to newline text on paste; there is no RTF support.
- `db.rs` still uses a single SQLite connection, fixed schema version `1`, basic search predicates, and no group/pin/import/export infrastructure.
- `ClipboardPanelPage.vue`, `ClipboardManagerPage.vue`, `ClipboardList.vue`, and `useClipboardStore.ts` are still first-generation UI structures.
- Existing `docs/superpowers/plans/2026-04-19-clipboard-manager.md` is a historical build-from-zero plan; this document supersedes it for the gap-migration phase.
- Verification blockers today:
  - `cmd /c pnpm check` fails because worktree-local dependencies are not installed
  - `cargo test` needs network access to download crates in this sandbox

## Milestone Map

### M6
- Content capture parity: html/files/rtf/source-app icons
- Search field parity for file paths and better metadata
- Dedup strategy options
- DB read/write separation and new indexes

### M7
- Context menu and file/image actions
- Merge paste
- Shift-range selection and Alt+1..9 quick paste
- Batch interaction parity

### M8
- Display preferences and richer settings model
- Search highlight and custom search box
- Preview windows moved from in-panel overlay to dedicated windows
- Panel/window persistence and appearance options

### M9
- Tray/system integration
- Non-activating panel display
- Task Scheduler based elevation flow
- Import/export, data path migration, app filters
- Groups and pinned/favorite split
- Cleanup/maintenance tools and final perf pass

---

## Task 0: Repository Hygiene And Execution Context

**Files:**
- Modify: `.gitignore`
- Create: `.trellis/tasks/04-20-clipboard-gap-migration/task.json`
- Create: `.trellis/tasks/04-20-clipboard-gap-migration/prd.md`
- Create: `docs/superpowers/plans/2026-04-20-clipboard-gap-migration-implementation-plan.md`

- [x] **Step 0.1: Confirm ignore rules for temporary assets and worktree metadata**

Run:

```powershell
git diff -- .gitignore
```

Expected: `.gitignore` contains both `/ElegantClipboard-main/` and `.worktree/` / `.worktrees/` protection.

- [x] **Step 0.2: Write the Trellis task card and PRD**

Record the branch, worktree path, approved spec, current blockers, and acceptance criteria in:

```text
.trellis/tasks/04-20-clipboard-gap-migration/task.json
.trellis/tasks/04-20-clipboard-gap-migration/prd.md
```

Expected: a future agent can understand the task without reading chat history.

- [x] **Step 0.3: Save this implementation plan before code implementation starts**

Expected: plan file exists at:

```text
docs/superpowers/plans/2026-04-20-clipboard-gap-migration-implementation-plan.md
```

---

## M6: Data Contracts, Capture, Persistence, And Search Foundation

### Task 1: Expand Shared Clipboard Contracts

**Files:**
- Modify: `src-tauri/src/clipboard/models.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src/lib/clipboardTypes.ts`
- Modify: `src/lib/tauri.ts`
- Test: `src/lib/clipboardSearchParser.test.mjs` (if parser filters need new coverage)

**Depends on:** Task 0

- [x] **Step 1.1: Write failing tests or assertions for new contract fields**

Add or extend tests to prove the frontend parser/types can handle upcoming query and filter fields where applicable. For backend contracts, add assertions in Rust tests once DB/schema task lands. If no existing TS test is the right fit, create a focused test file beside `clipboardTypes.ts` or parser tests.

Run:

```powershell
cmd /c pnpm check
```

Expected: fail before implementation if new fields are referenced but not yet defined.

- [x] **Step 1.2: Extend Rust models to match the spec**

Update `models.rs` to add:
- `ContentKind::Rtf`
- richer item fields such as `rtf_content`, `char_count`, `source_app_icon`, `group_id`, `is_pinned`
- new filters and list query options for group/pin and future search payloads
- expanded `ClipboardSettings` sections for display, preview, shortcuts, toolbar, data, audio, and app-filter settings
- dedicated structs for groups and file-path status responses

Expected: Rust becomes the single source of truth for the M6-M9 clipboard domain.

- [x] **Step 1.3: Extend TypeScript contracts and command wrappers in lockstep**

Mirror every new Rust-facing contract in:
- `src/lib/clipboardTypes.ts`
- `src/lib/tauri.ts`

Expected: no `any`, no untyped invoke payloads, and no stale frontend assumptions.

- [x] **Step 1.4: Update config defaults and backward-compatible deserialization**

Use `#[serde(default)]` and nested default structs so old config files still load cleanly.

Run:

```powershell
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
```

Expected: failures or compile errors identify any missing model/config updates.

- [ ] **Step 1.5: Commit**

```powershell
git add src-tauri/src/clipboard/models.rs src-tauri/src/config.rs src/lib/clipboardTypes.ts src/lib/tauri.ts
git commit -m "feat(clipboard): expand shared contracts for gap migration"
```

Status 2026-04-20:
- Completed in worktree with subagent implementation and review approval.
- Verified `node --test src/lib/clipboardSearchParser.test.mjs` passes after sandbox escalation.
- Full `vue-tsc` remains blocked by existing missing modules (`vue-virtual-scroller`, `vue-draggable-plus`) unrelated to Task 1.
- Rust test execution remains environment-blocked because the build script cannot run `vite` in the current worktree toolchain state.

### Task 2: Rebuild The Clipboard DB Layer For M6 Foundation

**Files:**
- Modify: `src-tauri/src/clipboard/db.rs`
- Modify: `src-tauri/src/clipboard/mod.rs`
- Modify: `src-tauri/src/clipboard/retention.rs`
- Test: `src-tauri/src/clipboard/db.rs` (existing test module)

**Depends on:** Task 1

- [x] **Step 2.1: Write failing Rust tests for migration, dedup, search, and read/write behavior**

Add tests for:
- schema migration from v1 to new fields
- dedup strategies: `move_to_top`, `ignore`, `always_new`
- search covering `content_preview`, `content_full`, `html`, `rtf_content`, `file_paths_json`
- pin/favorite/group filters
- char-count persistence

Run:

```powershell
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
```

Expected: failures show missing schema/logic.

- [x] **Step 2.2: Replace the single-connection model with explicit read/write handles**

Refactor `ClipboardState` and `db.rs` so reads and writes go through separate SQLite connections (or a struct wrapping them), with WAL, cache, mmap, and query-only settings aligned with the spec.

- [x] **Step 2.3: Add schema migration v2 and all new tables/indexes**

Implement:
- new item columns (`rtf_content`, `char_count`, `source_app_icon`, `is_pinned`, `group_id`, etc.)
- `clipboard_groups`
- indexes for kind, dates, pins/favorites, group filters, and search-heavy columns
- data migration for legacy rows

- [x] **Step 2.4: Implement new CRUD primitives**

Add DB functions for:
- pin toggle
- group CRUD and item reassignment
- text update
- manual cleanup / optimize / vacuum
- import/export helpers and merge semantics (stubs can be introduced if Task 13 owns full implementation, but signatures must not drift)

- [ ] **Step 2.5: Run tests green**

```powershell
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
```

Expected: all new DB tests pass.

- [ ] **Step 2.6: Commit**

```powershell
git add src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/mod.rs src-tauri/src/clipboard/retention.rs
git commit -m "feat(clipboard): upgrade db layer for m6 foundation"
```

Status 2026-04-20:
- Task 2 implementation is complete and passed both spec review and code-quality review.
- Added read/write DB foundation, `clipboard_groups` migration support, dedup strategy DB helpers/tests, search/filter coverage, pin/group/text-update/maintenance primitives, and pinned-item retention protection.
- `cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml` is still blocked by the workspace build script invoking `pnpm build:file-share-web`, which fails because `vite` is not available in the current environment.
- `cmd /c pnpm build:file-share-web` reproduces the blocker with: `vite is not recognized as an internal or external command`.

### Task 3: Implement HTML / RTF / Files Capture And Source-App Icons

**Files:**
- Modify: `src-tauri/src/clipboard/watcher.rs`
- Modify: `src-tauri/src/clipboard/source.rs`
- Create: `src-tauri/src/clipboard/icon_store.rs`
- Modify: `src-tauri/src/clipboard/image_store.rs`
- Test: `src-tauri/src/clipboard/source.rs` or new focused tests

**Depends on:** Task 2

- [x] **Step 3.1: Write failing tests for icon extraction and any pure helper logic**

At minimum, add Rust tests for:
- icon cache key stability
- source-app display-name fallback logic
- helper-level html/rtf/file capture prioritization if it can be isolated

Run:

```powershell
cargo test clipboard::source --manifest-path src-tauri\Cargo.toml
```

Expected: fail before helper implementation.

- [x] **Step 3.2: Refactor watcher capture priority to match the spec**

The capture priority should become:
- RTF
- HTML
- files
- image
- text

Requirements:
- populate `content_preview`, `content_full`, `html`, `rtf_content`, `file_paths_json`, `char_count`, `byte_size`
- respect dedup strategy from settings
- include source app name + icon path
- reference `ElegantClipboard-main/src-tauri/src/clipboard/handler.rs` whenever spec details are implicit

- [x] **Step 3.3: Introduce icon extraction and caching**

Use `ElegantClipboard-main/src-tauri/src/clipboard/source_app.rs` as the behavioral reference for:
- exe path hashing
- cache directory layout
- `SHGetFileInfoW` / GDI extraction flow
- fallback behavior on extraction failure

- [x] **Step 3.4: Enable orphan image/icon cleanup hooks**

Ensure orphan image cleanup is no longer dead code and can be reused later by M9 cleanup tasks.

- [x] **Step 3.5: Run targeted tests**

```powershell
cargo test clipboard::source --manifest-path src-tauri\Cargo.toml
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
```

Expected: capture helpers and DB round-trips remain green.

- [x] **Step 3.6: Commit**

```powershell
git add src-tauri/src/clipboard/watcher.rs src-tauri/src/clipboard/source.rs src-tauri/src/clipboard/icon_store.rs src-tauri/src/clipboard/image_store.rs
git commit -m "feat(clipboard): capture html rtf files and source app icons"
```

Status 2026-04-21:
- Task 3 is complete, including the follow-up fixes discovered during spec/code-quality review.
- Worktree verification now reuses the main workspace dependency/build artifacts: worktree `node_modules` is linked to the repo-root dependency tree, and Rust verification uses `CARGO_TARGET_DIR=C:\WorkSpace\File-Sync-Tool\src-tauri\target` with reduced debug info to avoid duplicating large target output.
- Implemented HTML/RTF/files capture priority, source-app/UWP handoff resolution, icon extraction/caching, orphan asset cleanup hooks, payload-fidelity fixes, duplicate source-metadata refresh, and delete/clear asset cleanup triggers.
- Added regression coverage for icon cache keys, display-name fallback, RTF/file/html helper behavior, whitespace preservation, duplicate source metadata refresh, non-UTF-8 RTF decoding, and orphan cleanup on duplicate replacement plus delete/clear mutations.
- Fresh verification passed with:

```powershell
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_INCREMENTAL='0'
cargo test clipboard:: --manifest-path src-tauri\Cargo.toml
```

Expected/Observed: `47` clipboard tests passed, `0` failed.
- Spec review: approved after review-loop fixes.
- Code-quality review: approved after review-loop fixes; remaining notes are minor maintainability hotspots in `watcher.rs` and `db.rs`.
- The branch checkpoint commit for this milestone is recorded as a consolidated Tasks 0-3 snapshot because Tasks 1-2 had been completed earlier in the worktree but not yet committed.

### Task 4: Align Paste/Search/Command Surface For M6

**Files:**
- Modify: `src-tauri/src/clipboard/paste.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/src/clipboard/db.rs`

**Depends on:** Task 3

- [x] **Step 4.1: Write failing tests for new DB search paths and command payload expectations**

Cover:
- file-path search
- kind-specific retrieval for html/rtf/files
- dedup-mode interactions that affect command outputs

- [x] **Step 4.2: Implement kind-aware paste behaviors**

Requirements:
- html items paste html + text unless plain-text override is requested
- rtf items paste rtf + text unless plain-text override is requested
- files support both "paste as actual files" and later "paste as path"
- text/image behavior remains compatible

- [x] **Step 4.3: Add M6 commands**

Add or extend command signatures for:
- richer `cb_list`
- file-path checking payloads
- new setting fields consumed by watcher/db

- [x] **Step 4.4: Register commands in `main.rs` and sync wrappers**

Expected: frontend can invoke the expanded backend without stringly-typed drift.

- [x] **Step 4.5: Run tests**

```powershell
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
```

Expected: green.

- [x] **Step 4.6: Commit**

```powershell
git add src-tauri/src/clipboard/paste.rs src-tauri/src/clipboard/commands.rs src-tauri/src/main.rs src/lib/tauri.ts
git commit -m "feat(clipboard): align paste and commands for m6"
```

Status 2026-04-21:
- Task 4 is complete.
- Added RED→GREEN coverage for file-path payload generation, file-selection validation, kind-specific `cb_list` retrieval, and search payload filter application.
- `paste.rs` now performs kind-aware write-back for HTML/RTF/files: HTML writes rich HTML + plain text, RTF writes real `Rich Text Format` data only when a genuine RTF payload exists, and file items write actual file lists through the Windows clipboard file-list path while still supporting newline-text fallback for plain paste.
- Added Task 4 command-surface support in `commands.rs`/`main.rs`/`tauri.ts`: explicit `cb_paste_as_files`, `cb_check_file_paths`, and synced TypeScript wrappers.
- `db.rs` now applies `search_payload.filters` to SQL, including kind/app/favorite/group/pinned/size filters and local-calendar date bounds for `from` / `to`.
- Fresh verification passed with:

```powershell
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_INCREMENTAL='0'
cargo test clipboard:: --manifest-path src-tauri\Cargo.toml
```

Observed: `52` clipboard tests passed, `0` failed.
- Fresh TypeScript verification still hits only the pre-existing dependency/type-resolution blocker:

```powershell
cmd /c C:\WorkSpace\File-Sync-Tool\node_modules\.bin\vue-tsc.cmd --noEmit -p C:\WorkSpace\File-Sync-Tool\.worktree\feature-clipboard-manager\tsconfig.json
```

Observed blockers:
  - `vue-virtual-scroller`
  - `vue-draggable-plus`
- Spec review: approved.
- Code-quality review: approved after fixing the RTF-slot fallback, mixed/stale file-selection ambiguity, and local-date boundary handling.

---

## M7: Interaction Parity

### Task 5: Implement Backend Action Commands For Context Menu Operations

**Files:**
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/clipboard/paste.rs`
- Create: `src-tauri/src/clipboard/preview.rs` (only if needed as backend support for later M8)
- Test: focused Rust tests where possible

**Depends on:** Task 4

- [x] **Step 5.1: Write failing tests for merge/file actions where logic is testable**

Add tests for:
- merge separator handling
- file-path existence payload formatting
- path-only paste transformation

- [x] **Step 5.2: Add M7 backend commands**

Implement:
- `cb_paste_as_path`
- `cb_paste_as_files`
- `cb_save_image_as`
- `cb_check_file_paths`
- `cb_open_in_explorer`
- `cb_merge_paste`

Reference `ElegantClipboard` behavior for path formatting, Explorer selection, and invalid-file signaling.

- [x] **Step 5.3: Run targeted tests**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

Expected: new helper tests pass.

- [x] **Step 5.4: Commit**

```powershell
git add src-tauri/src/clipboard/commands.rs src-tauri/src/clipboard/paste.rs
git commit -m "feat(clipboard): add backend actions for m7 interactions"
```

Status 2026-04-21:
- Task 5 is complete.
- Added the remaining M7 backend action commands in `commands.rs` and registered them in `main.rs`: `cb_paste_as_path`, `cb_save_image_as`, `cb_open_in_explorer`, and `cb_merge_paste`; Task 4's `cb_paste_as_files` and `cb_check_file_paths` remain part of the delivered Task 5 command surface.
- `paste.rs` now exposes focused helpers for path-only paste, merge-paste text construction, generic plain-text paste, and image save-as copying, with merge semantics tightened so only text-like rows with real `content_full` participate.
- Added/extended focused Rust coverage for path-only transformation, stale selection handling, merge separator behavior, preview-only merge rejection, explorer-path validation, and image save-as success/error paths.
- Fresh verification passed with:

```powershell
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_INCREMENTAL='0'
cargo test clipboard:: --manifest-path src-tauri\Cargo.toml
```

Observed: `62` clipboard tests passed, `0` failed.
- Verification required two environment recoveries in the shared worktree setup: rerunning `cmd /c pnpm build:file-share-web` with elevated sandbox access for the Rust build script, and a full `cargo clean` on the shared target after the drive filled during archive creation.
- Spec review: approved after fixing stale-selection command-path handling and empty-separator fallback coverage.
- Code-quality review: approved after preserving single-space separators and rejecting preview-only merge payloads.

### Task 6: Build The M7 Frontend Interaction Layer

**Files:**
- Create: `src/components/clipboard/ClipboardCardMenu.vue`
- Create: `src/components/clipboard/ClipboardFileDetailsDialog.vue`
- Create: `src/components/clipboard/ClipboardMergePasteDialog.vue`
- Create: `src/composables/useClipboardContextMenu.ts`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/pages/ClipboardManagerPage.vue`
- Modify: `src/locales/messages.ts`

**Depends on:** Task 5

- [x] **Step 6.1: Write failing TS/UI tests if the repo has an appropriate pattern**

If no meaningful component test harness exists, document the manual interaction checklist directly in the task notes and keep logic extracted into small composables/helpers that can still be unit tested.

- [x] **Step 6.2: Add the context menu and dialogs**

Requirements:
- per-kind actions
- file details with invalid-path state
- image save-as
- merge-paste prompt
- clean i18n keys in both languages

- [x] **Step 6.3: Refactor list row events**

Add explicit row action hooks without breaking:
- hover selection
- existing favorite/delete buttons
- virtual scrolling
- manager-page copy behavior

- [x] **Step 6.4: Run checks**

```powershell
cmd /c pnpm check
```

Expected: green type-check.

- [x] **Step 6.5: Commit**

```powershell
git add src/components/clipboard src/composables/useClipboardContextMenu.ts src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue src/locales/messages.ts
git commit -m "feat(clipboard): add m7 context actions and dialogs"
```

Status 2026-04-21:
- Task 6 is complete and committed as `c8b6ca3` (`feat(clipboard): add m7 context actions and dialogs`).
- Added a tested context-menu helper/composable split (`src/composables/clipboardContextMenuHelpers.ts`, `src/composables/useClipboardContextMenu.ts`, `src/composables/useClipboardContextMenu.test.mjs`) plus new UI shells for the row menu, file-details dialog, and merge-paste dialog.
- `ClipboardList.vue`, `ClipboardPanelPage.vue`, and `ClipboardManagerPage.vue` now support per-row menu actions, file-details invalid-path feedback, image save-to-directory flow, and shared batch merge-paste entry points without changing the manager page's primary click-to-copy behavior.
- Synced missing frontend wrappers in `src/lib/tauri.ts` for Task 5/6 clipboard actions and added the required English/Chinese i18n keys.
- Manual interaction checklist for Task 6 was recorded in `.trellis/tasks/04-20-clipboard-gap-migration/prd.md` because the repo still lacks a meaningful component-test harness.
- Fresh RED→GREEN verification passed with:

```powershell
node --test --test-isolation=none src/composables/useClipboardContextMenu.test.mjs
cmd /c pnpm check
```

Observed: `4` tests passed, `0` failed, and `cmd /c pnpm check` completed successfully.
- Step 6.4 was unblocked by reinstalling the shared worktree/frontend dependencies from the branch lockfile with `cmd /c pnpm install --frozen-lockfile --config.confirmModulesPurge=false`, which restored the missing `vue-draggable-plus` and `vue-virtual-scroller` packages in the shared `node_modules`.

### Task 7: Range Selection, Quick-Paste, And Batch Interaction Parity

**Files:**
- Modify: `src/composables/useClipboardHotkey.ts`
- Modify: `src/composables/useClipboardStore.ts`
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/pages/ClipboardManagerPage.vue`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src-tauri/src/clipboard/hotkey.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`

**Depends on:** Task 6

- [x] **Step 7.1: Write failing tests for parser/hotkey/range-selection helpers where possible**

At minimum, isolate selection math and quick-paste mapping into testable helper functions.

- [x] **Step 7.2: Implement Shift-range selection and Alt+1..9 quick paste**

Requirements:
- panel and manager stay in sync with selected ids
- range selection remembers last toggled index
- Alt+1..9 maps to visible row order

- [x] **Step 7.3: Improve batch action wiring**

Ensure merge paste, delete, favorite, and future export actions all use the same selected-id source of truth.

- [x] **Step 7.4: Run checks**

```powershell
cmd /c pnpm check
cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

Expected: green.

Status 2026-04-21:
- Task 7 is complete and committed as `5600680` (`feat(clipboard): add quick paste and range selection`).
- Added a pure interaction helper/test pair (`src/composables/clipboardInteractionHelpers.ts`, `src/composables/clipboardInteractionHelpers.test.mjs`) to lock down range-selection math and Alt+1..9 quick-paste targeting.
- `useClipboardStore.ts` now owns batch-mode state, ordered selected ids, shift-range anchor tracking, and visible-list pruning so panel and manager batch actions read from one source of truth.
- `useClipboardHotkey.ts` now maps Alt+1..9 to the current visible panel row order, while `ClipboardList.vue` emits shift-aware toggle payloads so range selection works inside the shared list component.
- `ClipboardPanelPage.vue`, `ClipboardManagerPage.vue`, and `useClipboardContextMenu.ts` now route merge-paste, delete, favorite, and future batch actions through the ordered selected-id list for consistent visible-order behavior.
- No new Rust command or global-hotkey changes were required for this step because Task 7's quick-paste behavior stayed window-local and existing clipboard commands already covered the needed mutations.
- Verification passed with:

```powershell
node --test --test-isolation=none src/composables/clipboardInteractionHelpers.test.mjs
node --test --test-isolation=none src/composables/useClipboardContextMenu.test.mjs
cmd /c pnpm check
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard:: --manifest-path src-tauri\Cargo.toml
```

Observed: `8` Node tests passed across the Task 6/7 helper suites, `cmd /c pnpm check` completed successfully, and `62` clipboard Rust tests passed with `0` failures.

- [x] **Step 7.5: Commit**

```powershell
git add src/composables/useClipboardHotkey.ts src/composables/useClipboardStore.ts src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue src/components/clipboard/ClipboardList.vue src-tauri/src/clipboard/hotkey.rs src-tauri/src/clipboard/commands.rs
git commit -m "feat(clipboard): add quick paste and range selection"
```

---

## M8: Display Personalization And Preview Architecture

### Task 8: Expand Settings Model For Display, Preview, Toolbar, Audio, And Filters

**Files:**
- Modify: `src-tauri/src/clipboard/models.rs`
- Modify: `src-tauri/src/config.rs`
- Modify: `src/lib/clipboardTypes.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src-tauri/src/clipboard/commands.rs`

**Depends on:** Task 7

- [x] **Step 8.1: Write failing tests for settings round-trip behavior**

Add Rust tests for default serialization/deserialization and any helper logic introduced for new nested settings.

- [x] **Step 8.2: Add M8 settings fields**

Include:
- display density, preview lines, time format, metadata toggles
- source app display mode
- image preview sizing
- panel follow-cursor / remember position / animation / mica option
- keyboard navigation master switch
- toolbar visibility and ordering
- audio flags

- [x] **Step 8.3: Keep old configs loading**

`#[serde(default)]` must protect every new field path.

- [x] **Step 8.4: Run tests**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

Status 2026-04-21:
- Task 8 is complete and committed as `147635e` (`feat(clipboard): expand settings model for m8`).
- Added new nested clipboard settings coverage for panel behavior and navigation (`panel.follow_cursor`, `panel.remember_position`, `panel.animate`, `panel.use_mica`, `navigation.enabled`) plus `toolbar.visible` defaults on both the Rust and TypeScript contracts.
- `src-tauri/src/clipboard/models.rs` now includes explicit serde-defaulted structs/tests for the new settings paths, while `src-tauri/src/config.rs` verifies legacy `AppConfig` JSON still deserializes with the new nested defaults.
- `src/lib/clipboardTypes.ts` and `src/lib/clipboardTypes.contract.test.ts` now mirror the expanded settings shape so frontend normalization keeps old payloads compatible.
- No `src/lib/tauri.ts` or `src-tauri/src/clipboard/commands.rs` edits were required in this step because the existing `cb_get_settings` / `cb_save_settings` and `clipboardApi.getSettings` / `saveSettings` paths already pass the entire `ClipboardSettings` object through unchanged.
- Verification passed with:

```powershell
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard::models::tests --manifest-path src-tauri\Cargo.toml
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test app_config_deserializes_legacy_clipboard_settings_with_new_nested_defaults --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

Observed: `2` new clipboard-settings model tests passed, `1` legacy-config round-trip test passed, `cmd /c pnpm check` completed successfully, and `cargo test clipboard --manifest-path src-tauri\Cargo.toml` finished green with `65` clipboard/config tests passing.

- [x] **Step 8.5: Commit**

```powershell
git add src-tauri/src/clipboard/models.rs src-tauri/src/config.rs src/lib/clipboardTypes.ts src/lib/tauri.ts src-tauri/src/clipboard/commands.rs
git commit -m "feat(clipboard): expand settings model for m8"
```

### Task 9: Replace In-Panel Hover Preview With Dedicated Preview Windows

**Files:**
- Create: `src-tauri/src/clipboard/preview.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/main.rs`
- Create: `src/pages/ClipboardImagePreview.vue`
- Create: `src/pages/ClipboardTextPreview.vue`
- Modify: `src/router/index.ts`
- Modify: `src/composables/useHoverPreview.ts`
- Modify: `src/pages/ClipboardPanelPage.vue`

**Depends on:** Task 8

- [x] **Step 9.1: Write failing tests for backend preview-window helpers when possible**

If window creation code is not easily testable, isolate geometry/anchor calculations into helper functions and test those.

- [x] **Step 9.2: Implement dedicated preview window management**

Requirements:
- show/hide commands for image/text preview
- reuse/prewarm preview windows
- anchor to panel side according to preference
- avoid blocking the main panel

- [x] **Step 9.3: Build preview routes and pages**

Pages must support:
- image zoom percentage
- text scrolling
- content updates from backend events/commands

- [x] **Step 9.4: Remove old overlay-only assumptions**

`useHoverPreview.ts` and panel page should become command-driven rather than local-overlay-driven.

- [x] **Step 9.5: Run checks**

```powershell
cmd /c pnpm check
cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

Status 2026-04-21:
- Task 9 implementation and verification are complete; only the commit step remains.
- Added a new frontend preview helper/test pair (`src/lib/clipboardPreviewHelpers.ts`, `src/lib/clipboardPreviewHelpers.test.mjs`) to lock down hover-target routing and image zoom clamping before wiring the new windows.
- `src-tauri/src/clipboard/preview.rs` now owns dedicated image/text preview window creation, prewarming, placement calculation, show/hide commands, and panel-side focus coordination so preview interaction does not immediately collapse the main panel.
- Added dedicated preview routes/pages in `src/pages/ClipboardImagePreview.vue` and `src/pages/ClipboardTextPreview.vue`, plus the router and Tauri wrapper plumbing needed for backend-driven preview updates.
- `src/composables/useHoverPreview.ts` and `src/pages/ClipboardPanelPage.vue` now use backend preview commands instead of the old in-panel overlay, including cleanup on close/delete/clear flows and delay refresh from saved settings.
- Fresh verification passed with:

```powershell
node --test --test-isolation=none src/lib/clipboardPreviewHelpers.test.mjs
cmd /c pnpm check
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

Observed: `5` new Node tests passed, `cmd /c pnpm check` completed successfully, and `cargo test clipboard --manifest-path src-tauri\Cargo.toml` finished green with `69` clipboard/config tests passing.
- Verification note: the Rust test run needed elevated sandbox access because the build script hit `EPERM` while reading the shared worktree `vite.js` dependency tree; rerunning without sandbox restrictions resolved it.

- [x] **Step 9.6: Commit**

```powershell
git add src-tauri/src/clipboard/preview.rs src-tauri/tauri.conf.json src-tauri/src/main.rs src/pages/ClipboardImagePreview.vue src/pages/ClipboardTextPreview.vue src/router/index.ts src/composables/useHoverPreview.ts src/pages/ClipboardPanelPage.vue
git commit -m "feat(clipboard): move hover preview to dedicated windows"
```

Status 2026-04-21 update:
- Task 9 commit checkpoint is recorded as `463d0a4` (`feat(clipboard): move hover preview to dedicated windows`).

### Task 10: Refactor Clipboard List Rendering For Display Preferences And Highlighting

**Files:**
- Create: `src/components/clipboard/ClipboardHighlightText.vue`
- Create: `src/components/clipboard/ClipboardAppIcon.vue`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src/composables/useClipboardStore.ts`
- Modify: `src/locales/messages.ts`

**Depends on:** Task 9

- [x] **Step 10.1: Write failing tests for highlight helpers and any extracted formatting helpers**

Keep text highlighting logic outside the template if possible so it can be tested.

- [x] **Step 10.2: Add display preference-driven rendering**

The list must react to settings for:
- density
- preview lines
- time format
- char-count / byte-size visibility
- source app icon/name/both
- image height behavior

- [x] **Step 10.3: Add keyword highlighting**

Highlight only the actual search keywords, not the entire DSL string.

- [x] **Step 10.4: Run checks**

```powershell
cmd /c pnpm check
```

Status 2026-04-21:
- Task 10 implementation and verification are complete; the commit step is being recorded with this checkpoint.
- Added a new pure helper/test pair (`src/lib/clipboardListPresentation.ts`, `src/lib/clipboardListPresentation.test.mjs`) to keep DSL keyword extraction, highlight splitting, source-app display policy, and time formatting out of the Vue template.
- `ClipboardList.vue` now reacts to saved display preferences for density, preview-line count, relative/absolute time labels, char-count and byte-size visibility, source-app icon/name presentation, and image preview height.
- Added `ClipboardHighlightText.vue` and `ClipboardAppIcon.vue` so both panel and manager list views share the same highlight and source-app rendering behavior.
- `useClipboardStore.ts` now keeps clipboard settings in sync with the list views and listens for a frontend-emitted `clipboard-settings-updated` event, so saving settings in the manager window updates list presentation immediately without reopening the page.
- `src/locales/messages.ts` did not require changes for this step because the new app-icon helper uses a local fallback label and all other list copy reuses existing clipboard i18n strings.
- Fresh verification passed with:

```powershell
node --test --test-isolation=none src/lib/clipboardListPresentation.test.mjs
cmd /c pnpm check
```

Observed: `3` new Node tests passed and `cmd /c pnpm check` completed successfully.

- [x] **Step 10.5: Commit**

```powershell
git add src/components/clipboard/ClipboardHighlightText.vue src/components/clipboard/ClipboardAppIcon.vue src/components/clipboard/ClipboardList.vue src/composables/useClipboardStore.ts src/locales/messages.ts
git commit -m "feat(clipboard): add display personalization and highlighting"
```

### Task 11: Rebuild Clipboard Settings UI Into M8 Tabs

**Files:**
- Create: `src/components/clipboard-settings/GeneralTab.vue`
- Create: `src/components/clipboard-settings/DisplayTab.vue`
- Create: `src/components/clipboard-settings/ShortcutsTab.vue`
- Create: `src/components/clipboard-settings/DataTab.vue`
- Create: `src/components/clipboard-settings/PreviewTab.vue`
- Create: `src/components/clipboard-settings/AppFilterTab.vue`
- Create: `src/components/clipboard-settings/AudioTab.vue`
- Create: `src/components/clipboard-settings/AboutTab.vue`
- Create: `src/components/clipboard/ClipboardSearchBox.vue`
- Create: `src/components/clipboard/ClipboardToolbar.vue`
- Create: `src/lib/clipboardSettingsUi.ts`
- Modify: `src/components/clipboard/ClipboardSettingsPanel.vue`
- Modify: `src/composables/useClipboardHotkey.ts`
- Modify: `src/locales/messages.ts`
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/pages/ClipboardManagerPage.vue`
- Test: `src/lib/clipboardSettingsUi.test.mjs`

**Depends on:** Task 10

- [x] **Step 11.1: Write failing tests for any new helper-level UI state code**

Prefer testing helper/composable logic over template snapshots.

- [x] **Step 11.2: Split settings into tabs**

Requirements:
- settings information architecture matches the spec and `ElegantClipboard`
- no single oversized settings component
- each tab owns one responsibility

- [x] **Step 11.3: Introduce custom search box and toolbar config**

Add clear button, toolbar visibility/ordering, and keyboard-nav toggle.

- [x] **Step 11.4: Run checks**

```powershell
cmd /c pnpm check
```

Status 2026-04-21:
- Task 11 implementation and verification are complete; the commit step is being recorded with this checkpoint.
- Added a new pure helper/test pair (`src/lib/clipboardSettingsUi.ts`, `src/lib/clipboardSettingsUi.test.mjs`) to lock down toolbar-item normalization, reordering, and page-level toolbar layout resolution before wiring the shared UI.
- Rebuilt `ClipboardSettingsPanel.vue` into an 8-tab settings surface (`General`, `Display`, `Shortcuts`, `Data`, `Preview`, `App Filter`, `Audio`, `About`) so the richer M8 settings model is no longer trapped in one oversized component.
- Added reusable `ClipboardSearchBox.vue` and `ClipboardToolbar.vue`, then updated both `ClipboardPanelPage.vue` and `ClipboardManagerPage.vue` to respect saved toolbar visibility/order for search, filter chips, batch mode, settings access, and panel lock controls.
- `useClipboardHotkey.ts` now respects `navigation.enabled` and uses a caller-provided search updater so the new keyboard-navigation toggle and search clear flows take effect immediately.
- `src/locales/messages.ts` now includes the new tab, toolbar, and settings copy needed for the tabbed settings UI and custom search box.
- Fresh verification passed with:

```powershell
node --test --test-isolation=none src/lib/clipboardSettingsUi.test.mjs
cmd /c pnpm check
```

Observed: `3` new Node tests passed and `cmd /c pnpm check` completed successfully.

- [x] **Step 11.5: Commit**

```powershell
git add src/components/clipboard-settings src/components/clipboard/ClipboardSearchBox.vue src/components/clipboard/ClipboardToolbar.vue src/components/clipboard/ClipboardSettingsPanel.vue src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue
git commit -m "feat(clipboard): rebuild settings ui for m8"
```

---

## M9: System Integration, Data Management, And Final Delivery

### Task 12: Non-Activating Panel Display, Tray Wiring, And Scheduled Elevation

**Files:**
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/clipboard/admin.rs`
- Create: `src-tauri/src/clipboard/task_scheduler.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/components/clipboard-settings/GeneralTab.vue`
- Modify: `src/lib/tauri.ts`
- Modify: `src/locales/messages.ts`

**Depends on:** Task 11

- [x] **Step 12.1: Write failing tests for task-scheduler helper functions where possible**

Isolate command-string generation and task-state parsing if direct Windows calls are hard to test.

- [x] **Step 12.2: Replace focus-stealing panel show logic**

Implement `SWP_NOACTIVATE`-style non-activating display while keeping panel keyboard behavior usable.

- [x] **Step 12.3: Add Task Scheduler based admin flow**

Use the spec and `ElegantClipboard` behavior as reference for:
- task creation
- installed/status checks
- removal
- fallback to current PowerShell elevation if scheduler flow fails

- [x] **Step 12.4: Wire tray menu integration**

Add clipboard panel entry to the existing tray/menu flow and expose settings for tray visibility if required by the spec.

- [x] **Step 12.5: Run checks**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [x] **Step 12.6: Commit**

```powershell
git add src-tauri/src/clipboard/commands.rs src-tauri/src/clipboard/admin.rs src-tauri/src/clipboard/task_scheduler.rs src-tauri/src/main.rs src/components/clipboard-settings/GeneralTab.vue src/lib/tauri.ts
git commit -m "feat(clipboard): add non-activating panel and scheduler elevation"
```

Status 2026-04-21:
- Added task-scheduler helper coverage for command construction and task-query path matching.
- Replaced the clipboard panel's focus-stealing show flow with a Windows `SetWindowPos(..., SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE)` helper while keeping the existing refresh event flow.
- Switched admin auto-start to prefer Task Scheduler registration, keep a PowerShell `Start-Process -Verb RunAs` fallback when scheduler setup fails, and exposed task status/create/remove commands to the frontend.
- Added a tray menu entry for toggling the clipboard panel and surfaced scheduler status plus repair/remove actions in `GeneralTab`.
- Verified:
  - `$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard --manifest-path src-tauri\Cargo.toml`
  - `cmd /c pnpm check`

### Task 13: Import/Export, Data-Dir Migration, And Maintenance Actions

**Files:**
- Create: `src-tauri/src/clipboard/data_transfer.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/config.rs`
- Create: `src/components/clipboard/ClipboardImportExportDialog.vue`
- Modify: `src/components/clipboard-settings/DataTab.vue`
- Modify: `src/lib/tauri.ts`

**Depends on:** Task 12

- [x] **Step 13.1: Write failing tests for export/import round-trip helpers**

Cover:
- replace import
- merge import
- backup naming / rollback paths if feasible

- [x] **Step 13.2: Implement ZIP export/import**

Include:
- DB file
- image/icon assets
- schema validation
- replace vs merge behavior

- [x] **Step 13.3: Align custom data-dir migration with clipboard assets**

`config.rs` already has generic custom data-dir migration support; extend it so clipboard DB, images, and icons migrate correctly.

- [x] **Step 13.4: Add DB optimize/vacuum and cleanup actions**

Backend command surface and settings UI should expose:
- optimize
- vacuum
- clear history
- reset config
- reset all

- [x] **Step 13.5: Run tests**

```powershell
cargo test clipboard::data_transfer --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [ ] **Step 13.6: Commit**

```powershell
git add src-tauri/src/clipboard/data_transfer.rs src-tauri/src/clipboard/commands.rs src-tauri/src/config.rs src/components/clipboard/ClipboardImportExportDialog.vue src/components/clipboard-settings/DataTab.vue src/lib/tauri.ts
git commit -m "feat(clipboard): add import export and maintenance tooling"
```

Status 2026-04-22:
- Task 13 implementation and verification are complete; checkpoint commit is pending.
- Added `src-tauri/src/clipboard/data_transfer.rs` with TDD coverage for backup naming, replace import, and merge import, then implemented ZIP-based export/import for the clipboard DB plus image/icon assets.
- Export now checkpoints the live SQLite database before bundling so WAL-backed data is not dropped, and import supports both `replace` and duplicate-hash-aware `merge` modes with optional backup creation.
- `src-tauri/src/clipboard/commands.rs`, `src-tauri/src/main.rs`, and `src/lib/tauri.ts` now expose `cb_export`, `cb_import`, `cb_db_optimize`, `cb_db_vacuum`, `cb_reset_config`, and `cb_reset_all`, while the existing `cb_clear` surface is reused for clear-history maintenance.
- `src-tauri/src/config.rs` and the generic custom data-dir migration path now carry `clipboard.db`, `clipboard_images/`, and `clipboard_icons/`, including a clipboard DB checkpoint before migration.
- `src/components/clipboard-settings/DataTab.vue` and `src/components/clipboard/ClipboardImportExportDialog.vue` now provide stats, import/export UI, data-directory migration controls, dedup strategy selection, and maintenance/reset actions in settings.
- Verification passed with:

```powershell
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard::data_transfer --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

Observed: `3` `clipboard::data_transfer` tests passed, `cmd /c pnpm check` completed successfully, and the crate compiled cleanly for the new backend command/config wiring aside from pre-existing unrelated warnings elsewhere in the app.

### Task 14: Groups, Pinned/Favorite Split, And Grouped UI

**Files:**
- Create: `src-tauri/src/clipboard/groups.rs`
- Modify: `src-tauri/src/clipboard/db.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Create: `src/components/clipboard/ClipboardGroupSidebar.vue`
- Create: `src/components/clipboard/ClipboardPinnedSection.vue`
- Modify: `src/composables/useClipboardStore.ts`
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/pages/ClipboardManagerPage.vue`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src/locales/messages.ts`

**Depends on:** Task 13

- [x] **Step 14.1: Write failing tests for group CRUD and pin/favorite semantics**

Add DB tests for:
- create/rename/delete group
- `ON DELETE SET NULL`
- pinned retention exemption
- favorite vs pinned visibility rules

- [x] **Step 14.2: Implement backend group and pin APIs**

Add:
- `cb_groups_list/create/rename/delete`
- `cb_move_to_group`
- `cb_toggle_pin`
- list filters for group and pinned section

- [x] **Step 14.3: Rebuild frontend store and pages around grouped navigation**

Requirements:
- left group sidebar
- top pinned section
- favorite tab remains separate
- manager and panel both stay coherent

- [x] **Step 14.4: Run checks**

```powershell
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [x] **Step 14.5: Commit**

```powershell
git add src-tauri/src/clipboard/groups.rs src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/commands.rs src/components/clipboard/ClipboardGroupSidebar.vue src/components/clipboard/ClipboardPinnedSection.vue src/composables/useClipboardStore.ts src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue src/components/clipboard/ClipboardList.vue src/locales/messages.ts
git commit -m "feat(clipboard): add groups and pinned favorite split"
```

Status 2026-04-22:
- Task 14 implementation and verification are complete.
- Added backend clipboard group/pin command coverage with a new `groups.rs` helper module, Tauri command surface for group CRUD plus `cb_toggle_pin` / `cb_move_to_group`, and `clipboard-groups-changed` event emission so the UI stays in sync.
- Rebuilt the clipboard store around `groups`, `selectedGroupId`, `pinnedItems`, and a combined visible-item model, then added `ClipboardGroupSidebar.vue` and `ClipboardPinnedSection.vue` so both the quick panel and manager page now support grouped navigation with a dedicated pinned section above the regular list.
- Updated `ClipboardList.vue` and the clipboard context menu so items can be pinned/unpinned inline, moved between custom groups, and rendered with stable list numbering even when a pinned section is present.
- Added focused RED->GREEN coverage for group-name normalization, pinned-section partitioning, and the new menu action set in:
  - `src-tauri/src/clipboard/groups.rs`
  - `src/composables/useClipboardContextMenu.test.mjs`
  - `src/lib/clipboardGroupsView.test.mjs`
- Fresh verification passed with:

```powershell
node --test --test-isolation=none src/composables/useClipboardContextMenu.test.mjs src/lib/clipboardGroupsView.test.mjs
cmd /c pnpm check
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'; $env:RUSTFLAGS='-C debuginfo=0'; cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
```

Observed:
- `7` focused Node tests passed.
- `cmd /c pnpm check` completed successfully.
- `cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml` passed with `27` database tests green; only pre-existing unrelated Rust warnings remain elsewhere in the app.

### Task 15: App Filters, Final Cleanup Flows, And Performance Hardening

**Files:**
- Modify: `src-tauri/src/clipboard/watcher.rs`
- Modify: `src-tauri/src/clipboard/source.rs`
- Modify: `src-tauri/src/clipboard/image_store.rs`
- Modify: `src-tauri/src/clipboard/retention.rs`
- Modify: `src/components/clipboard-settings/AppFilterTab.vue`
- Modify: `src/components/clipboard-settings/AboutTab.vue`
- Modify: `src/components/clipboard-settings/PreviewTab.vue`
- Modify: `src/components/clipboard-settings/AudioTab.vue`

**Depends on:** Task 14

- [x] **Step 15.1: Write failing tests for app-filter matching and cleanup behavior**

Use helper-level tests to cover:
- wildcard matching
- blacklist/whitelist rules
- cleanup exclusions for pinned/favorites

- [x] **Step 15.2: Implement app filtering and final cleanup hooks**

The watcher should skip capture for excluded apps and cleanup flows should preserve the intended records/assets.

- [x] **Step 15.3: Tune for the spec's performance targets**

Check:
- large-list search path
- panel first paint assumptions
- preview open latency
- cleanup cost

- [x] **Step 15.4: Run checks**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [x] **Step 15.5: Commit**

```powershell
git add src-tauri/src/clipboard/watcher.rs src-tauri/src/clipboard/source.rs src-tauri/src/clipboard/image_store.rs src-tauri/src/clipboard/retention.rs src/components/clipboard-settings/AppFilterTab.vue src/components/clipboard-settings/AboutTab.vue src/components/clipboard-settings/PreviewTab.vue src/components/clipboard-settings/AudioTab.vue
git commit -m "feat(clipboard): finish filters cleanup and performance tuning"
```

Status 2026-04-22:
- Task 15 is complete and closes the remaining M9 filter/cleanup gap before the final verification sweep.
- Added RED/GREEN coverage for wildcard app matching, blacklist/whitelist decisions, and cleanup protection for pinned/favorite records.
- `watcher.rs` now short-circuits excluded source apps before opening heavier clipboard payload APIs, and `source.rs` matches rules against display name, exe name, file stem, and full path with case-insensitive `*` / `?` support.
- `image_store.rs` now avoids rayon overhead on tiny cleanup batches and skips extra path-string allocations during orphan-image scans.
- The settings tabs now explain filter semantics, expose quick pattern / delay / volume presets, disable audio sub-controls when audio is off, and summarize cleanup/filter state in the About tab.
- Fresh verification passed with:

```powershell
$env:CARGO_TARGET_DIR='C:\WorkSpace\File-Sync-Tool\src-tauri\target'
$env:RUSTFLAGS='-C debuginfo=0'
cargo test clipboard --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

Observed: `81` clipboard-related Rust tests passed, `0` failed; `cmd /c pnpm check` completed successfully.

### Task 16: Final Verification, Regression Sweep, And Release Readiness

**Files:**
- Modify as needed based on findings only
- Reference: `docs/superpowers/specs/2026-04-19-clipboard-gap-migration-spec.md`

**Depends on:** Task 15

- [ ] **Step 16.1: Run Rust verification**

```powershell
cargo fmt --manifest-path src-tauri\Cargo.toml --all
cargo clippy --manifest-path src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri\Cargo.toml
```

Expected: all green.

- [ ] **Step 16.2: Run frontend verification**

```powershell
cmd /c pnpm check
cmd /c pnpm lint
cmd /c pnpm tauri:build:versioned-exe
```

Expected: all green.

- [ ] **Step 16.3: Execute manual milestone regression checklist**

Run through the spec's `M6`, `M7`, `M8`, and `M9` manual acceptance checklists, plus performance checks:
- search 10k rows <= 80ms
- first paint <= 150ms
- preview popup <= 150ms
- 10k-row scroll >= 50fps

- [ ] **Step 16.4: Fix any failures, rerun affected checks, and update plan status**

No open spec or quality review issues should remain.

- [ ] **Step 16.5: Prepare branch completion**

Use `superpowers:requesting-code-review` and then `superpowers:finishing-a-development-branch` before merge/cleanup.

---

## Spec Coverage Cross-Check

- M6 content types, dedup, icon extraction, smarter search, and DB performance are covered by Tasks 1-4.
- M7 context actions, merge paste, range selection, and quick-paste are covered by Tasks 5-7.
- M8 personalization, preview windows, highlight/search UX, and tabbed settings are covered by Tasks 8-11.
- M9 system integration, import/export, groups, cleanup, and app filtering are covered by Tasks 12-16.
- Explicit non-goals from the spec remain excluded from this plan: theme presets, dark mode, updater, portable mode, and cross-platform support.

## Verification Notes

- If sandboxed cargo cannot fetch crates, request escalated network access before claiming any Rust verification passed.
- If PowerShell execution policy blocks `pnpm`, use `cmd /c pnpm ...` consistently.
- If worktree-local `node_modules` is missing, either install dependencies in the worktree or confirm resolution from ancestor directories before relying on frontend checks.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-20-clipboard-gap-migration-implementation-plan.md`. The user already requested the recommended execution mode, so execution should proceed with `superpowers:subagent-driven-development`, one bounded task at a time, starting from Task 1 or Task 2 depending on whether contract expansion is already partially complete at implementation start.
