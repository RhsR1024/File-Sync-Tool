# Settings Improvements (Apr 11)

## Goal
Three UX improvements to the Settings page.

## Requirements

### 1. Custom config/log data directory
- Let users configure a custom base directory for config.json, history.json, and app.log (e.g. D:\MyData)
- Use a pivot file at the default AppData location (`{app_config_dir}/pivot.json`) to store the custom path
- Modify `get_config_path`, `get_log_path`, and `get_history_path` to honour the pivot
- New Tauri commands: `get_custom_data_dir`, `set_custom_data_dir(path: String)` (empty string = reset to default)
- UI: in the "About / Paths" section of SettingsPage, show current paths + text input + Save + Reset buttons
- On save, offer to migrate existing files to the new location (copy config.json, history.json, app.log)
- i18n: zh + en

### 2. "Add Server" button in Server Manager modal
- In the server manager modal header (alongside "Test All" and "Close"), add an "Add Server" button
- Clicking it calls the existing `addServer()` function (which already handles the UI correctly)

### 3. SSH connect timeout per server
- Add `ssh_timeout_secs: u64` to `DeployServer` in config.rs (serde default = 30)
- Add to TypeScript `DeployServer` interface
- Use `TcpStream::connect_timeout` instead of `TcpStream::connect` in all 3 connect sites in deploy.rs
- Add a dropdown in the server edit modal: options 1, 3, 5, 10, 30, 60 seconds
- i18n: zh + en

## Acceptance Criteria
- [ ] Users can set a custom data directory; config/log/history save there
- [ ] After restart, app reads from custom dir automatically
- [ ] Resetting removes the pivot and reverts to AppData
- [ ] Server Manager modal has "Add Server" button that opens the add form
- [ ] Each server has a configurable SSH timeout (default 30s)
- [ ] SSH test connection uses the per-server timeout
- [ ] Actual deploy also uses the per-server timeout

## Technical Notes
- Pivot file: `{app_config_dir}/pivot.json` with schema `{"custom_data_dir": "D:\\..."}`
- `get_config_path`, `get_log_path`, `get_history_path` must all call `read_pivot` 
- deploy.rs has 3 TcpStream::connect calls (check_connection, main deploy fn, file-share fn at ~line 772) - all need updating
- Migration: copy files if they exist at old path and not yet at new path; don't overwrite
