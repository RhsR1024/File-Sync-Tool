use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

use crate::clipboard::models::ClipboardSettings;

pub const MIN_SCAN_INTERVAL_MINS: u64 = 5;
pub const MIN_STABILITY_CHECK_SECS: u64 = 60;
pub const MIN_RECENT_FILE_GUARD_MINS: u64 = 3;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandGroup {
    pub id: String,
    pub name: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    #[default]
    Continue,
    Abort,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalCommandGroup {
    pub id: String,
    pub name: String,
    pub commands: Vec<String>,
    #[serde(default)]
    pub on_failure: OnFailure,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LocalScriptBinding {
    #[serde(default)]
    pub command_group_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PostCopyExecutionOrder {
    #[default]
    LocalFirst,
    RemoteFirst,
    Parallel,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskServerBinding {
    pub server_id: String,
    #[serde(default)]
    pub command_group_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeployServer {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub remote_path: String,
    /// SSH TCP connect timeout in seconds. Default: 30.
    #[serde(default = "default_ssh_timeout_secs")]
    pub ssh_timeout_secs: u64,
}

fn default_ssh_timeout_secs() -> u64 {
    5
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "value")]
pub enum MatchRule {
    /// Legacy Version Match: Matches YYYY_MM_DD_HH_MM_(Version)
    /// Value: Target Version (e.g. "1.3.9.P02")
    VersionMatch(String),
    /// Date Directory Match: Matches directory with specific date format
    /// Value: Date Format (e.g. "%y%m%d")
    DateMatch(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScanTask {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub remote_path: String,
    pub local_path: Option<String>,
    pub rule: MatchRule,
    /// Per-server deployment bindings. Each binding specifies which command groups
    /// to run on a given server after the upload completes.
    #[serde(default)]
    pub server_bindings: Vec<TaskServerBinding>,
    #[serde(default)]
    pub local_script_binding: Option<LocalScriptBinding>,
    #[serde(default)]
    pub post_copy_execution_order: PostCopyExecutionOrder,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub tasks: Vec<ScanTask>,

    pub local_path: String,
    pub interval_minutes: u64,
    pub time_ranges: Vec<String>, // "HH:mm-HH:mm"

    pub file_extensions: Vec<String>,   // e.g. ["exe", "tar.gz"]
    pub filename_includes: Vec<String>, // e.g. ["UMS", "VMS"] - OR logic

    pub deploy_enabled: bool,
    #[serde(default)]
    pub servers: Vec<DeployServer>,

    /// Named command groups, each with an ordered list of shell commands.
    #[serde(default)]
    pub command_groups: Vec<CommandGroup>,

    /// Named local command groups for post-copy local script execution.
    #[serde(default)]
    pub local_command_groups: Vec<LocalCommandGroup>,

    /// Seconds to wait after discovering files before copying, to verify they are fully written.
    #[serde(default = "default_stability_secs")]
    pub stability_check_secs: u64,

    /// If a file was modified within the last N minutes, it must pass the stability wait.
    #[serde(default = "default_recent_file_guard_mins")]
    pub recent_file_guard_mins: u64,

    #[serde(default)]
    pub launch_and_auto_scan: bool,

    #[serde(default)]
    pub launch_and_auto_start_file_share: bool,

    #[serde(default)]
    pub close_to_tray: bool,

    #[serde(default = "default_max_log_lines")]
    pub max_log_lines: u32,

    /// Copy buffer size in KB. Controls the read/write chunk size when copying files.
    /// Larger values improve throughput on fast network shares. Default: 4096 (4 MB).
    #[serde(default = "default_copy_buffer_size_kb")]
    pub copy_buffer_size_kb: u32,

    /// Maximum number of task records to persist and display. Default: 100.
    #[serde(default = "default_max_task_records")]
    pub max_task_records: u32,

    /// HTTP request timeout in seconds for the appliance SSH API (/openAPI/system/v1/network/SSH/get).
    /// Default: 5.
    #[serde(default = "default_appliance_ssh_api_timeout_secs")]
    pub appliance_ssh_api_timeout_secs: u64,

    /// HTTP request timeout in seconds for the framework password API.
    /// Default: 5.
    #[serde(default = "default_framework_password_api_timeout_secs")]
    pub framework_password_api_timeout_secs: u64,

    /// Clipboard manager settings (spec §2026-04-19-clipboard-manager §7.1).
    #[serde(default)]
    pub clipboard: ClipboardSettings,
}

fn default_stability_secs() -> u64 {
    30
}
fn default_recent_file_guard_mins() -> u64 {
    MIN_RECENT_FILE_GUARD_MINS
}
fn default_max_log_lines() -> u32 {
    200
}
fn default_copy_buffer_size_kb() -> u32 {
    4096
}
fn default_max_task_records() -> u32 {
    100
}
fn default_appliance_ssh_api_timeout_secs() -> u64 {
    5
}
fn default_framework_password_api_timeout_secs() -> u64 {
    5
}

fn default_clipboard_settings() -> ClipboardSettings {
    ClipboardSettings::default()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tasks: vec![],
            local_path: "E:\\UMS_TEMP".to_string(),
            interval_minutes: 10,
            time_ranges: vec![],
            file_extensions: vec![],
            filename_includes: vec![],
            deploy_enabled: false,
            servers: vec![],
            command_groups: vec![],
            local_command_groups: vec![],
            stability_check_secs: 120,
            recent_file_guard_mins: MIN_RECENT_FILE_GUARD_MINS,
            launch_and_auto_scan: false,
            launch_and_auto_start_file_share: false,
            close_to_tray: false,
            max_log_lines: 200,
            copy_buffer_size_kb: 4096,
            max_task_records: 100,
            appliance_ssh_api_timeout_secs: 5,
            framework_password_api_timeout_secs: 5,
            clipboard: default_clipboard_settings(),
        }
    }
}

pub fn normalize_config(mut config: AppConfig) -> AppConfig {
    if config.interval_minutes < MIN_SCAN_INTERVAL_MINS {
        config.interval_minutes = MIN_SCAN_INTERVAL_MINS;
    }
    if config.stability_check_secs < MIN_STABILITY_CHECK_SECS {
        config.stability_check_secs = MIN_STABILITY_CHECK_SECS;
    }
    if config.recent_file_guard_mins < MIN_RECENT_FILE_GUARD_MINS {
        config.recent_file_guard_mins = MIN_RECENT_FILE_GUARD_MINS;
    }
    config
}

pub fn validate_config(config: &AppConfig) -> Result<(), String> {
    if config.interval_minutes < MIN_SCAN_INTERVAL_MINS {
        return Err(format!(
            "Scan interval must be at least {} minutes",
            MIN_SCAN_INTERVAL_MINS
        ));
    }
    if config.stability_check_secs < MIN_STABILITY_CHECK_SECS {
        return Err(format!(
            "File stability wait must be at least {} second",
            MIN_STABILITY_CHECK_SECS
        ));
    }
    if config.recent_file_guard_mins < MIN_RECENT_FILE_GUARD_MINS {
        return Err(format!(
            "Recent file threshold must be at least {} minutes",
            MIN_RECENT_FILE_GUARD_MINS
        ));
    }
    Ok(())
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let config_path = get_config_path(app_handle);
    if config_path.exists() {
        if let Ok(content) = fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                return normalize_config(config);
            }
        }
    }
    AppConfig::default()
}

pub fn save_config(app_handle: &tauri::AppHandle, config: &AppConfig) -> Result<(), String> {
    let config_path = get_config_path(app_handle);
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(config_path, content).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Custom data directory (pivot file) ──────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
struct Pivot {
    custom_data_dir: Option<String>,
}

fn pivot_path<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> PathBuf {
    app_handle
        .path()
        .app_config_dir()
        .unwrap()
        .join("pivot.json")
}

fn read_pivot<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> Pivot {
    let path = pivot_path(app_handle);
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(p) = serde_json::from_str::<Pivot>(&content) {
            return p;
        }
    }
    Pivot::default()
}

/// Returns the custom data dir if configured and the directory exists.
pub fn get_custom_data_dir<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) -> Option<PathBuf> {
    let pivot = read_pivot(app_handle);
    pivot.custom_data_dir.and_then(|d| {
        let path = PathBuf::from(d);
        if path.is_dir() {
            Some(path)
        } else {
            None
        }
    })
}

/// Saves or clears the custom data dir pivot. Empty string = reset to default.
/// Migrates existing data files from the current directory to the new directory.
pub fn set_custom_data_dir(app_handle: &tauri::AppHandle, path: String) -> Result<(), String> {
    // Collect current paths BEFORE changing the pivot
    let old_data_dir = get_data_dir(app_handle);
    let old_config_path = get_config_path(app_handle);

    // Determine the new target directory
    let new_dir = if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(&path))
    };
    if let Some(target_dir) = &new_dir {
        fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;
    }

    // Write the pivot file
    let pivot_file = pivot_path(app_handle);
    if let Some(parent) = pivot_file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let pivot = if path.is_empty() {
        Pivot {
            custom_data_dir: None,
        }
    } else {
        Pivot {
            custom_data_dir: Some(path),
        }
    };
    let content = serde_json::to_string_pretty(&pivot).map_err(|e| e.to_string())?;
    fs::write(pivot_file, content).map_err(|e| e.to_string())?;

    // Determine the effective new data dir and config path AFTER pivot change
    let new_data_dir = get_data_dir(app_handle);
    let new_config_path = get_config_path(app_handle);

    // Skip migration if directories are the same
    if old_data_dir == new_data_dir {
        return Ok(());
    }

    // Ensure new directory exists
    fs::create_dir_all(&new_data_dir).map_err(|e| e.to_string())?;

    // Migrate config file
    migrate_file(&old_config_path, &new_config_path);

    // Migrate data files
    let data_files = [
        "app.log",
        "history.json",
        "ui_state.json",
        "task_state.json",
        "clipboard.db",
    ];
    for name in &data_files {
        let src = old_data_dir.join(name);
        let dst = new_data_dir.join(name);
        migrate_file(&src, &dst);
    }

    // Migrate kv/ directory
    let old_kv = old_data_dir.join("kv");
    let new_kv = new_data_dir.join("kv");
    migrate_dir_contents(&old_kv, &new_kv);

    migrate_dir_contents(
        &old_data_dir.join("clipboard_images"),
        &new_data_dir.join("clipboard_images"),
    );
    migrate_dir_contents(
        &old_data_dir.join("clipboard_icons"),
        &new_data_dir.join("clipboard_icons"),
    );

    Ok(())
}

/// Copy a file from src to dst if src exists and dst does not (no overwrite).
fn migrate_file(src: &PathBuf, dst: &PathBuf) {
    if src.exists() && !dst.exists() {
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(src, dst);
    }
}

fn migrate_dir_contents(src_dir: &PathBuf, dst_dir: &PathBuf) {
    if !src_dir.is_dir() {
        return;
    }

    let _ = fs::create_dir_all(dst_dir);
    if let Ok(entries) = fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            let dst = dst_dir.join(entry.file_name());
            if src.is_dir() {
                migrate_dir_contents(&src, &dst);
            } else if src.is_file() {
                migrate_file(&src, &dst);
            }
        }
    }
}

/// Returns the effective data directory: custom_data_dir if set, otherwise app_data_dir.
pub fn get_data_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    get_custom_data_dir(app_handle).unwrap_or_else(|| app_handle.path().app_data_dir().unwrap())
}

pub fn get_log_path(app_handle: &tauri::AppHandle) -> PathBuf {
    get_data_dir(app_handle).join("app.log")
}

pub fn get_config_path(app_handle: &tauri::AppHandle) -> PathBuf {
    match get_custom_data_dir(app_handle) {
        Some(d) => d.join("config.json"),
        None => app_handle
            .path()
            .app_config_dir()
            .unwrap()
            .join("config.json"),
    }
}

/// Returns the default (non-custom) data directory.
pub fn get_default_data_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle.path().app_data_dir().unwrap()
}

/// Returns the default (non-custom) config directory.
pub fn get_default_config_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle.path().app_config_dir().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn app_config_deserializes_legacy_clipboard_settings_with_new_nested_defaults() {
        let config: AppConfig = serde_json::from_value(json!({
            "tasks": [],
            "local_path": "E:/UMS_TEMP",
            "interval_minutes": 10,
            "time_ranges": [],
            "file_extensions": [],
            "filename_includes": [],
            "deploy_enabled": false,
            "servers": [],
            "command_groups": [],
            "local_command_groups": [],
            "clipboard": {
                "enabled": true,
                "toolbar": {
                    "items": ["search", "filter"]
                }
            }
        }))
        .unwrap();

        assert!(config.clipboard.toolbar.visible);
        assert_eq!(
            config.clipboard.toolbar.items,
            vec!["search".to_string(), "filter".to_string()]
        );
        assert!(config.clipboard.panel.follow_cursor);
        assert!(!config.clipboard.panel.remember_position);
        assert!(config.clipboard.panel.animate);
        assert!(config.clipboard.panel.use_mica);
        assert!(config.clipboard.navigation.enabled);
    }
}
