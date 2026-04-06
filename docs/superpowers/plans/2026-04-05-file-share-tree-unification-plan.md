# File Share Tree Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current `root + path` file-share browsing flow with a unified node-tree model that shows visible share roots as the home directory, supports root-level download/rename/delete, and handles runtime permission changes cleanly.

**Architecture:** Add a backend node model plus `/api/tree` and `/api/tree/search` endpoints, then migrate write/download APIs to `node_id` semantics. Refactor `share-web` to drive navigation by `currentNodeId` instead of `currentRoot/currentPath`, reuse one list view for home and nested directories, and refresh permissions after 403 write denials.

**Tech Stack:** Rust, Axum, Serde, Vue 3, TypeScript, vue-i18n

---

### Task 1: Lock Backend Tree API with Failing Tests

**Files:**
- Modify: `src-tauri/src/fileshare/http.rs`
- Modify: `src-tauri/src/fileshare/search.rs`

- [ ] **Step 1: Write failing HTTP tests for home tree and nested tree responses**

Add tests in `src-tauri/src/fileshare/http.rs` that expect:
- `GET /api/tree` to return a `home` current node and visible `share_root` children
- `GET /api/tree?node_id=<share-root-id>` to return nested directory children

- [ ] **Step 2: Run the new tree tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::http::tests::tree_ -- --nocapture`
Expected: FAIL because `/api/tree` does not exist yet

- [ ] **Step 3: Write failing search tests for share-root hits**

Add tests in `src-tauri/src/fileshare/search.rs` and/or `src-tauri/src/fileshare/http.rs` that expect:
- global tree search to include a matching `share_root`
- subtree search to stay inside the selected node

- [ ] **Step 4: Run the new search tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::search::tests -- --nocapture`
Expected: FAIL because the search layer does not emit unified nodes yet

- [ ] **Step 5: Commit checkpoint**

```bash
git add src-tauri/src/fileshare/http.rs src-tauri/src/fileshare/search.rs
git commit -m "test: cover file share tree api"
```

### Task 2: Implement Backend Node Model and Tree/Search APIs

**Files:**
- Modify: `src-tauri/src/fileshare/http.rs`
- Modify: `src-tauri/src/fileshare/mod.rs`
- Modify: `src-tauri/src/fileshare/ops.rs`
- Modify: `src-tauri/src/fileshare/search.rs`
- Modify: `src-tauri/src/fileshare/model.rs`

- [ ] **Step 1: Add unified node response types and node-id helpers**

Implement backend types for:
- `FileShareNode`
- tree response current/breadcrumb structures
- node-id encode/decode helpers for `share_root`, `directory`, and `file`

- [ ] **Step 2: Add `/api/tree` and `/api/tree/search` handlers**

Implement minimal handlers that:
- return home nodes when `node_id` is absent
- return child nodes for share roots and directories
- reject file nodes for tree browsing
- support global and subtree search

- [ ] **Step 3: Run the targeted tests and make them pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::http::tests::tree_ fileshare::search::tests -- --nocapture`
Expected: PASS

- [ ] **Step 4: Remove old root/list/search HTTP routes and structs once the new handlers are in place**

Delete the old `/api/roots`, `/api/list`, and `/api/search` route wiring plus unused request/response types.

- [ ] **Step 5: Commit checkpoint**

```bash
git add src-tauri/src/fileshare/http.rs src-tauri/src/fileshare/mod.rs src-tauri/src/fileshare/ops.rs src-tauri/src/fileshare/search.rs src-tauri/src/fileshare/model.rs
git commit -m "feat: add unified file share tree api"
```

### Task 3: Add Node-Based Write/Download Operations and Permission Refresh Semantics

**Files:**
- Modify: `src-tauri/src/fileshare/http.rs`
- Modify: `src-tauri/src/fileshare/mod.rs`
- Modify: `src-tauri/src/fileshare/ops.rs`
- Modify: `src-tauri/src/fileshare/persist.rs`

- [ ] **Step 1: Write failing tests for root-level rename/delete/download and runtime permission changes**

Add tests that expect:
- root-level rename updates disk and persisted root path
- root-level delete removes disk content and root config
- write actions return 403 after permissions are revoked post-page-load

- [ ] **Step 2: Run the new write-operation tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::http::tests::node_ fileshare::persist::tests -- --nocapture`
Expected: FAIL because node-based write routes and config sync do not exist yet

- [ ] **Step 3: Implement node-based handlers and config synchronization**

Migrate these flows to `node_id`:
- create directory
- create text
- upload files
- upload directory
- rename
- delete
- file download
- archive download
- preview

For `share_root` rename/delete:
- update filesystem
- persist root config changes
- return explicit permission errors when realtime checks fail

- [ ] **Step 4: Re-run the targeted write tests and make them pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare::http::tests fileshare::persist::tests fileshare::ops::tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit checkpoint**

```bash
git add src-tauri/src/fileshare/http.rs src-tauri/src/fileshare/mod.rs src-tauri/src/fileshare/ops.rs src-tauri/src/fileshare/persist.rs
git commit -m "feat: switch file share operations to node ids"
```

### Task 4: Refactor Share Web to the Node Tree Model

**Files:**
- Modify: `src/share-web/types.ts`
- Modify: `src/share-web/api.ts`
- Modify: `src/share-web/App.vue`
- Modify: `src/share-web/components/ToolbarActions.vue`
- Modify: `src/share-web/components/EntryTable.vue`
- Modify: `src/share-web/components/SearchBar.vue`
- Modify: `src/share-web/components/LoginDialog.vue`
- Modify: `src/share-web/messages.ts`

- [ ] **Step 1: Update TypeScript types and API client for tree/search/node operations**

Replace old root/list/search request types with:
- unified node types
- tree response types
- node-id based write/download/preview helpers

- [ ] **Step 2: Refactor the page state to `currentNodeId`**

Update `App.vue` to:
- load home tree on bootstrap
- navigate by `node_id`
- render home and nested directories with the same list
- hide upload/create actions at home
- default search to global at home and current-directory inside a node

- [ ] **Step 3: Compact action UI and runtime-permission refresh**

Update list and toolbar components so that:
- directory and share-root archive actions render as compact `下载`
- the old shared-root selector disappears
- 403 write failures show the explicit “权限已变更” message and trigger a session/tree refresh

- [ ] **Step 4: Run frontend verification**

Run:
- `pnpm check`
- `pnpm build:file-share-web`

Expected: both PASS

- [ ] **Step 5: Commit checkpoint**

```bash
git add src/share-web/types.ts src/share-web/api.ts src/share-web/App.vue src/share-web/components/ToolbarActions.vue src/share-web/components/EntryTable.vue src/share-web/components/SearchBar.vue src/share-web/components/LoginDialog.vue src/share-web/messages.ts
git commit -m "feat: migrate share web to file share tree"
```

### Task 5: Full Verification and Cleanup

**Files:**
- Modify: any touched implementation files as needed

- [ ] **Step 1: Run backend regression verification**

Run: `cargo test --manifest-path src-tauri/Cargo.toml fileshare -- --nocapture`
Expected: PASS

- [ ] **Step 2: Run frontend verification**

Run:
- `pnpm check`
- `pnpm build`

Expected: PASS

- [ ] **Step 3: Inspect git diff for leftover old-model code**

Confirm the old shared-root selector and old `root/path` API wiring are gone.

- [ ] **Step 4: Commit final implementation**

```bash
git add src-tauri/src/fileshare src/share-web
git commit -m "feat: unify file share tree navigation"
```
