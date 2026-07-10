# Config Domain Persistence

> Cross-layer contract for editing the shared `AppConfig` without one UI surface overwriting fields owned by another domain.

## 1. Scope and data flow

This contract applies when code changes configuration from the sync console or application settings:

```text
Vue editor → configStore → configDomains patch builder → tauri.ts invoke
  → Rust update_*_config command → apply_*_patch → validate/normalize → config.json
```

`src/lib/configStore.ts` is the single frontend source for these two surfaces. Components must not keep a second long-lived `AppConfig` copy.

## 2. Signatures

Frontend:

```ts
buildSyncPatch(config: AppConfig): SyncConfigPatch
buildAppPatch(config: AppConfig): AppDomainConfigPatch
configStore.ensureLoaded(): Promise<void>
configStore.refresh(): Promise<void>
configStore.saveSync(): Promise<void>
configStore.saveApp(): Promise<void>
updateSyncConfig(patch: SyncConfigPatch): Promise<void>
updateAppConfig(patch: AppDomainConfigPatch): Promise<void>
```

Rust:

```rust
pub fn apply_sync_patch(config: &mut AppConfig, patch: SyncConfigPatch);
pub fn apply_app_patch(config: &mut AppConfig, patch: AppDomainConfigPatch);

async fn update_sync_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    patch: SyncConfigPatch,
) -> Result<(), String>;

async fn update_app_config(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    patch: AppDomainConfigPatch,
) -> Result<(), String>;
```

Both commands must remain registered in `tauri::generate_handler!`.

## 3. Field ownership matrix

| Domain | Exact writable fields |
| --- | --- |
| Sync | `tasks`, `local_path`, `interval_minutes`, `time_ranges`, `file_extensions`, `filename_includes`, `deploy_enabled`, `servers`, `command_groups`, `local_command_groups`, `stability_check_secs`, `recent_file_guard_mins`, `copy_buffer_size_kb` |
| App | `launch_and_auto_scan`, `launch_and_auto_start_file_share`, `close_to_tray`, `max_log_lines`, `max_task_records`, `appliance_ssh_api_timeout_secs`, `framework_password_api_timeout_secs`, `disk_cleanup_http_timeout_secs`, `disk_cleanup_linux_mode`, `update_server_url`, `notify_on_new_version`, `clipboard` |
| Backend-only | `last_update_check_at`, `pending_update` |

Required invariant:

```text
sync fields ∩ app fields = ∅
sync fields ∪ app fields ∪ backend-only fields = every AppConfig field
```

The backend-only fields must never be accepted from either UI patch. Adding an `AppConfig` field requires assigning it to exactly one row and updating Rust/TypeScript patch types plus the domain completeness test.

## 4. Merge and side-effect contracts

- A command clones the current in-memory config, applies only its typed patch, validates and normalizes the merged value, writes it back to `AppState`, then persists it.
- A sync save must preserve every app and backend-only field. An app save must preserve every sync and backend-only field.
- `configStore.saveSync()` must refresh with `get_config` after success, then call `restartSchedulerInterval()` and emit one `CONFIG_CHANGE` system event.
- `configStore.saveApp()` must refresh after success, update `appStore.maxLogLines` from the refreshed config when positive, and emit one `CONFIG_CHANGE` event.
- Concurrent initial `ensureLoaded()` calls must share one in-flight request.
- Changing `launch_and_auto_scan` or `launch_and_auto_start_file_share` through `update_app_config` must keep the startup-launch side effect from the legacy full-save command.
- Changing `update_server_url` through `update_app_config` must clear `last_update_check_at` and call `updater::commands::handle_config_changed` after the config lock is released.
- The legacy `save_config_cmd` remains available for tool pages outside this contract. Do not migrate those pages implicitly while changing sync/app configuration.

## 5. Validation and error matrix

| Case | Required behavior |
| --- | --- |
| Sync patch contains invalid interval/stability/guard value | `validate_config` rejects; in-memory and persisted config are not replaced |
| App patch contains invalid update URL | `validate_config` rejects; updater side effects do not run |
| Persistence fails after merge | command returns an error; caller shows an error toast and must not report success |
| A save succeeds | frontend refreshes from backend; it does not assume the submitted object is canonical |
| Two sync tabs edit the config | both bind the same `configStore.config`; no page-local full-config cache is created |

## 6. Required tests

- Rust `config::tests::apply_sync_patch_updates_only_sync_domain`.
- Rust `config::tests::apply_app_patch_updates_only_app_domain`.
- `node --test src/lib/configDomains.test.mjs src/lib/configStore.test.mjs`.
- `pnpm check` and `pnpm lint` after changing any patch field, command wrapper, or store action.
- A production Tauri build after cross-layer command registration changes.

## 7. Forbidden patterns

- Calling `saveConfig(config)` from `SettingsPage` or sync-console configuration components.
- Spreading `AppConfig` into a patch; patch builders must enumerate owned fields explicitly.
- Including `last_update_check_at` or `pending_update` in a frontend writable patch.
- Updating the config while holding the lock across updater callbacks.
