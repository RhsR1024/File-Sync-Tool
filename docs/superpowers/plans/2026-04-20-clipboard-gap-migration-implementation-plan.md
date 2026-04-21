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
- Task 7 implementation and verification are complete; only the commit step remains.
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

- [ ] **Step 7.5: Commit**

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

- [ ] **Step 8.1: Write failing tests for settings round-trip behavior**

Add Rust tests for default serialization/deserialization and any helper logic introduced for new nested settings.

- [ ] **Step 8.2: Add M8 settings fields**

Include:
- display density, preview lines, time format, metadata toggles
- source app display mode
- image preview sizing
- panel follow-cursor / remember position / animation / mica option
- keyboard navigation master switch
- toolbar visibility and ordering
- audio flags

- [ ] **Step 8.3: Keep old configs loading**

`#[serde(default)]` must protect every new field path.

- [ ] **Step 8.4: Run tests**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

- [ ] **Step 8.5: Commit**

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

- [ ] **Step 9.1: Write failing tests for backend preview-window helpers when possible**

If window creation code is not easily testable, isolate geometry/anchor calculations into helper functions and test those.

- [ ] **Step 9.2: Implement dedicated preview window management**

Requirements:
- show/hide commands for image/text preview
- reuse/prewarm preview windows
- anchor to panel side according to preference
- avoid blocking the main panel

- [ ] **Step 9.3: Build preview routes and pages**

Pages must support:
- image zoom percentage
- text scrolling
- content updates from backend events/commands

- [ ] **Step 9.4: Remove old overlay-only assumptions**

`useHoverPreview.ts` and panel page should become command-driven rather than local-overlay-driven.

- [ ] **Step 9.5: Run checks**

```powershell
cmd /c pnpm check
cargo test clipboard --manifest-path src-tauri\Cargo.toml
```

- [ ] **Step 9.6: Commit**

```powershell
git add src-tauri/src/clipboard/preview.rs src-tauri/tauri.conf.json src-tauri/src/main.rs src/pages/ClipboardImagePreview.vue src/pages/ClipboardTextPreview.vue src/router/index.ts src/composables/useHoverPreview.ts src/pages/ClipboardPanelPage.vue
git commit -m "feat(clipboard): move hover preview to dedicated windows"
```

### Task 10: Refactor Clipboard List Rendering For Display Preferences And Highlighting

**Files:**
- Create: `src/components/clipboard/ClipboardHighlightText.vue`
- Create: `src/components/clipboard/ClipboardAppIcon.vue`
- Modify: `src/components/clipboard/ClipboardList.vue`
- Modify: `src/composables/useClipboardStore.ts`
- Modify: `src/locales/messages.ts`

**Depends on:** Task 9

- [ ] **Step 10.1: Write failing tests for highlight helpers and any extracted formatting helpers**

Keep text highlighting logic outside the template if possible so it can be tested.

- [ ] **Step 10.2: Add display preference-driven rendering**

The list must react to settings for:
- density
- preview lines
- time format
- char-count / byte-size visibility
- source app icon/name/both
- image height behavior

- [ ] **Step 10.3: Add keyword highlighting**

Highlight only the actual search keywords, not the entire DSL string.

- [ ] **Step 10.4: Run checks**

```powershell
cmd /c pnpm check
```

- [ ] **Step 10.5: Commit**

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
- Modify: `src/components/clipboard/ClipboardSettingsPanel.vue`
- Modify: `src/pages/ClipboardPanelPage.vue`
- Modify: `src/pages/ClipboardManagerPage.vue`

**Depends on:** Task 10

- [ ] **Step 11.1: Write failing tests for any new helper-level UI state code**

Prefer testing helper/composable logic over template snapshots.

- [ ] **Step 11.2: Split settings into tabs**

Requirements:
- settings information architecture matches the spec and `ElegantClipboard`
- no single oversized settings component
- each tab owns one responsibility

- [ ] **Step 11.3: Introduce custom search box and toolbar config**

Add clear button, toolbar visibility/ordering, and keyboard-nav toggle.

- [ ] **Step 11.4: Run checks**

```powershell
cmd /c pnpm check
```

- [ ] **Step 11.5: Commit**

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

**Depends on:** Task 11

- [ ] **Step 12.1: Write failing tests for task-scheduler helper functions where possible**

Isolate command-string generation and task-state parsing if direct Windows calls are hard to test.

- [ ] **Step 12.2: Replace focus-stealing panel show logic**

Implement `SWP_NOACTIVATE`-style non-activating display while keeping panel keyboard behavior usable.

- [ ] **Step 12.3: Add Task Scheduler based admin flow**

Use the spec and `ElegantClipboard` behavior as reference for:
- task creation
- installed/status checks
- removal
- fallback to current PowerShell elevation if scheduler flow fails

- [ ] **Step 12.4: Wire tray menu integration**

Add clipboard panel entry to the existing tray/menu flow and expose settings for tray visibility if required by the spec.

- [ ] **Step 12.5: Run checks**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [ ] **Step 12.6: Commit**

```powershell
git add src-tauri/src/clipboard/commands.rs src-tauri/src/clipboard/admin.rs src-tauri/src/clipboard/task_scheduler.rs src-tauri/src/main.rs src/components/clipboard-settings/GeneralTab.vue src/lib/tauri.ts
git commit -m "feat(clipboard): add non-activating panel and scheduler elevation"
```

### Task 13: Import/Export, Data-Dir Migration, And Maintenance Actions

**Files:**
- Create: `src-tauri/src/clipboard/data_transfer.rs`
- Modify: `src-tauri/src/clipboard/commands.rs`
- Modify: `src-tauri/src/config.rs`
- Create: `src/components/clipboard/ClipboardImportExportDialog.vue`
- Modify: `src/components/clipboard-settings/DataTab.vue`
- Modify: `src/lib/tauri.ts`

**Depends on:** Task 12

- [ ] **Step 13.1: Write failing tests for export/import round-trip helpers**

Cover:
- replace import
- merge import
- backup naming / rollback paths if feasible

- [ ] **Step 13.2: Implement ZIP export/import**

Include:
- DB file
- image/icon assets
- schema validation
- replace vs merge behavior

- [ ] **Step 13.3: Align custom data-dir migration with clipboard assets**

`config.rs` already has generic custom data-dir migration support; extend it so clipboard DB, images, and icons migrate correctly.

- [ ] **Step 13.4: Add DB optimize/vacuum and cleanup actions**

Backend command surface and settings UI should expose:
- optimize
- vacuum
- clear history
- reset config
- reset all

- [ ] **Step 13.5: Run tests**

```powershell
cargo test clipboard::data_transfer --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [ ] **Step 13.6: Commit**

```powershell
git add src-tauri/src/clipboard/data_transfer.rs src-tauri/src/clipboard/commands.rs src-tauri/src/config.rs src/components/clipboard/ClipboardImportExportDialog.vue src/components/clipboard-settings/DataTab.vue src/lib/tauri.ts
git commit -m "feat(clipboard): add import export and maintenance tooling"
```

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

- [ ] **Step 14.1: Write failing tests for group CRUD and pin/favorite semantics**

Add DB tests for:
- create/rename/delete group
- `ON DELETE SET NULL`
- pinned retention exemption
- favorite vs pinned visibility rules

- [ ] **Step 14.2: Implement backend group and pin APIs**

Add:
- `cb_groups_list/create/rename/delete`
- `cb_move_to_group`
- `cb_toggle_pin`
- list filters for group and pinned section

- [ ] **Step 14.3: Rebuild frontend store and pages around grouped navigation**

Requirements:
- left group sidebar
- top pinned section
- favorite tab remains separate
- manager and panel both stay coherent

- [ ] **Step 14.4: Run checks**

```powershell
cargo test clipboard::db::tests --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [ ] **Step 14.5: Commit**

```powershell
git add src-tauri/src/clipboard/groups.rs src-tauri/src/clipboard/db.rs src-tauri/src/clipboard/commands.rs src/components/clipboard/ClipboardGroupSidebar.vue src/components/clipboard/ClipboardPinnedSection.vue src/composables/useClipboardStore.ts src/pages/ClipboardPanelPage.vue src/pages/ClipboardManagerPage.vue src/components/clipboard/ClipboardList.vue src/locales/messages.ts
git commit -m "feat(clipboard): add groups and pinned favorite split"
```

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

- [ ] **Step 15.1: Write failing tests for app-filter matching and cleanup behavior**

Use helper-level tests to cover:
- wildcard matching
- blacklist/whitelist rules
- cleanup exclusions for pinned/favorites

- [ ] **Step 15.2: Implement app filtering and final cleanup hooks**

The watcher should skip capture for excluded apps and cleanup flows should preserve the intended records/assets.

- [ ] **Step 15.3: Tune for the spec's performance targets**

Check:
- large-list search path
- panel first paint assumptions
- preview open latency
- cleanup cost

- [ ] **Step 15.4: Run checks**

```powershell
cargo test clipboard --manifest-path src-tauri\Cargo.toml
cmd /c pnpm check
```

- [ ] **Step 15.5: Commit**

```powershell
git add src-tauri/src/clipboard/watcher.rs src-tauri/src/clipboard/source.rs src-tauri/src/clipboard/image_store.rs src-tauri/src/clipboard/retention.rs src/components/clipboard-settings/AppFilterTab.vue src/components/clipboard-settings/AboutTab.vue src/components/clipboard-settings/PreviewTab.vue src/components/clipboard-settings/AudioTab.vue
git commit -m "feat(clipboard): finish filters cleanup and performance tuning"
```

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
