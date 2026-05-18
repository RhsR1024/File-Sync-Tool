# Update Checker

> Contracts for the Windows in-app updater: manifest fetch, verified download, pending update restore, and self-replace via helper batch script.

## Scenario: Manifest / Download / Apply flow

### 1. Scope / Trigger
- Trigger: code under `src-tauri/src/updater/`, `src-tauri/src/config.rs`, or frontend wrappers that call updater commands/events.
- Goal: keep Rust commands, config fields, manifest schema, event payloads, and pending-update lifecycle stable across Tauri and Vue.

### 2. Signatures
- Rust modules:
  - `src-tauri/src/updater/mod.rs`
  - `src-tauri/src/updater/manifest.rs`
  - `src-tauri/src/updater/download.rs`
  - `src-tauri/src/updater/installer.rs`
  - `src-tauri/src/updater/pending.rs`
  - `src-tauri/src/updater/self_heal.rs`
  - `src-tauri/src/updater/commands.rs`
- Tauri commands:
  - `check_update(...) -> Result<UpdateCheckResult, String>`
  - `start_update_download(...) -> Result<(), String>`
  - `cancel_update_download(...) -> Result<(), String>`
  - `apply_update_now(...) -> Result<(), String>`
  - `test_update_server(...) -> Result<TestServerResult, String>`
  - `get_update_state(...) -> UpdateState`
- Events:
  - `update-state-changed`
  - `update-download-progress`
  - `update-download-complete`
  - `open-update-dialog`

### 3. Contracts
- `AppConfig` fields:
  - `update_server_url: String`
  - `notify_on_new_version: bool`
  - `last_update_check_at: Option<String>`
  - `pending_update: Option<PendingUpdate>`
- Config rules:
  - `update_server_url` is trimmed and trailing `/` is stripped during `normalize_config`
  - non-empty update URLs must parse as `http://` or `https://`
- Manifest rules:
  - endpoint is `${server_url}/manifest.json`
  - malformed `versions[]` entries are dropped; the whole manifest only fails when all non-empty entries are invalid
  - relative asset URLs are resolved against `server_url`
  - `manifest.latest` is normalized to `versions[0].version` when they disagree
- Query rules:
  - release builds may auto-check on startup after a 5-second delay, throttled to 24 hours by `last_update_check_at`
  - startup throttle only applies after `UpdaterState.manifest` is already loaded in the current process; a cold start with no in-memory manifest must fetch even when `last_update_check_at` is within 24 hours so `UpdateState.has_update` can drive the sidebar red dot
  - debug builds return `debug_build: true`, never auto-check, and never report `has_update = true`
  - manual `check_update` always re-fetches and updates `last_update_check_at`
- Download / apply rules:
  - `start_update_download` only downloads the latest manifest entry when it is newer than `CURRENT_VERSION`
  - download destination is `<current_exe_parent>/<manifest_file_name>.part` whenever that directory is writable; otherwise a `.part` file under `%TEMP%` is used
  - bytes are hashed while streaming; the `.part` file is renamed to its final name only after SHA-256 verification succeeds, then `pending_update.temp_path` is persisted
  - `cancel_update_download` flips the watch-channel flag and the partial `.part` file is deleted
  - `apply_update_now` validates `pending_update`, writes/spawns the helper batch file, clears `pending_update`, saves config, and exits the app; the helper bat skips the `src → target` move when the verified download already sits at the target path
- Self-heal rules:
  - on startup, after the manifest is loaded, the running binary checks whether its file name matches the canonical filename declared by the manifest entry for `CURRENT_VERSION`
  - if the file name parses as `file-sync-tool-<semver>-<12 digit timestamp>.exe` but the embedded version does not match `CURRENT_VERSION`, and the manifest provides a canonical name, a rename helper bat is spawned and the app exits so it restarts under the correct name
  - self-heal is a no-op when the file name already matches, when the file name is user-renamed (does not parse), when the manifest has no entry for `CURRENT_VERSION`, or when the target file already exists

### 4. Validation & Error Matrix
| Case | Required behavior |
| --- | --- |
| update server URL empty | manual check returns `server_not_configured`; startup auto-check is skipped silently |
| invalid update server URL in Settings | `save_config_cmd` rejects the save |
| manifest network / HTTP error | existing manifest state stays intact; startup logs a warning; manual actions surface the error |
| restart within 24h after a previous check | startup still fetches when no in-memory manifest is loaded; otherwise the sidebar cannot show `has_update` |
| duplicate `start_update_download` | return `already_in_progress` |
| SHA-256 mismatch | delete the temp file and emit `update-download-complete` with `sha256_ok = false` |
| stale `pending_update` file on startup | clear `pending_update` silently and persist the cleanup |
| debug build | `get_update_state` reports `debug_build = true`, no auto-check runs, and update actions are no-ops or rejected |

### 5. Good / Base / Bad Cases
- Good: startup auto-check fetches the manifest, updates `last_update_check_at`, emits `update-state-changed`, and emits `open-update-dialog` only when `notify_on_new_version` is true.
- Good: closing and reopening the app within the throttle window still fetches once on startup because the process-local manifest starts empty; the sidebar version chip red dot is based on the resulting `UpdateState.has_update`.
- Good: manual check with no update still stores the manifest so `/about` can render release history.
- Base: `test_update_server` validates connectivity and manifest JSON without mutating updater state.
- Bad: frontend assumes a pending update exists before `update-download-complete` / `update-state-changed` persist it.
- Bad: `apply_update_now` skips `pending::validate` and trusts a stale temp path.

### 6. Tests Required
- Rust:
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::commands::tests::startup_auto_check`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::commands::tests::resolve_download_paths`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::commands::tests::finalize_part_file`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::manifest`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::download`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::pending`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::installer`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app updater::self_heal`
  - `cargo test --manifest-path src-tauri/Cargo.toml -p app config::tests::`
- Frontend:
  - `node --test src/lib/sidebarNavigation.test.mjs src/pages/about/version.test.mjs`
  - `pnpm check`

### 7. Wrong vs Correct
#### Wrong
```rust
config.pending_update = Some(PendingUpdate {
    target_version,
    temp_path,
    sha256,
    downloaded_at,
});
```

#### Correct
```rust
download::download_to_file(...).await?;
config.pending_update = Some(PendingUpdate {
    target_version,
    temp_path,
    sha256,
    downloaded_at,
});
```
