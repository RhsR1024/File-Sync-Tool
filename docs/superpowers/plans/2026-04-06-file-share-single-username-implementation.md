# File Share Single Username Implementation Plan

> For agentic workers: execute in small checkpoints, keep the dirty worktree intact, and do not revert unrelated changes.

**Goal:** Convert file-share accounts from the current `display name + login id` model into a real single-`username` model across persistence, Tauri IPC, desktop settings UI, and share-web login/session flows.

**Architecture:** Introduce a v3 file-share settings file with `guest_account` separated from normal `accounts`, update Rust session/auth storage to carry explicit username-based subjects, then align the Vue desktop page and share-web browser with the new `username` contract.

**Tech Stack:** Rust, Tauri, Vue 3, TypeScript, vue-i18n

---

## Working Rules

- Do not migrate `file_share_v2.json`; read and write only the new `file_share_v3.json`.
- Keep unrelated worktree changes untouched.
- Follow TDD for the behavior change: write failing tests first, then implement.
- Preserve current non-account file-share behavior, including roots, preview flags, IP filters, and startup controls.

## File Map

- `src-tauri/src/fileshare/model.rs` - persisted/view/save account models and defaults
- `src-tauri/src/fileshare/persist.rs` - config file path, normalization, save/load validation
- `src-tauri/src/fileshare/auth.rs` - session records and resolved principals
- `src-tauri/src/fileshare/mod.rs` - authentication flow, guest resolution, session building
- `src-tauri/src/fileshare/http.rs` - login/session API contracts and HTTP tests
- `src/lib/tauri.ts` - Tauri IPC request/view types
- `src/pages/FileSharePage.vue` - desktop draft state, validation, and account editor UI
- `src/locales/messages.ts` - desktop account copy
- `src/share-web/api.ts` - browser login request
- `src/share-web/types.ts` - session type
- `src/share-web/types.test.mjs` - lightweight share-web contract coverage
- `src/share-web/components/LoginDialog.vue` - username/password login dialog
- `src/share-web/App.vue` - session display wiring
- `src/share-web/messages.ts` - browser login/session copy

## Tasks

### Task 1: Lock New Contracts With Tests

- [ ] Update `src/share-web/types.test.mjs` to use `username`-based session fixtures.
- [ ] Add/adjust Rust persistence tests for `guest_account`, single `username`, and duplicate-username rejection.
- [ ] Add/adjust Rust auth/HTTP tests for `username + password` login and `username`-only session responses.
- [ ] Run the targeted tests and confirm they fail for the old dual-field implementation.

### Task 2: Implement Rust Single-Username Model

- [ ] Replace account structs with single-username user structs and split `guest_account` out of `accounts`.
- [ ] Bump config constants to v3 and switch persistence to `file_share_v3.json`.
- [ ] Update save/load normalization, uniqueness checks, default config generation, and password retention.
- [ ] Update auth/session storage to stop depending on hidden account ids.
- [ ] Update HTTP login/session request and response payloads to `username`.

### Task 3: Implement Desktop Settings Changes

- [ ] Update Tauri TypeScript bindings to the new settings shape.
- [ ] Replace desktop account drafts and validation so guest and custom accounts each use one `username` field.
- [ ] Add a local-only draft key for editable custom account rows.
- [ ] Remove `display name` and `login id` UI and copy, replacing them with `username`.

### Task 4: Implement Share-Web Changes

- [ ] Update session types and login API calls to `username`.
- [ ] Rename login dialog labels/placeholders and emitted payloads.
- [ ] Update session display to read from `session.username`.
- [ ] Keep guest/session behavior intact aside from the contract rename.

### Task 5: Verify

- [ ] Run `node src/share-web/types.test.mjs`.
- [ ] Run targeted `cargo test` coverage for fileshare persistence and HTTP/auth flows.
- [ ] Run project-level frontend checks if the changed surfaces compile under existing tooling.
- [ ] Review the final diff for accidental changes outside the feature scope.
