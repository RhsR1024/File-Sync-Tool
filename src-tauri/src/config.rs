use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

pub const MIN_SCAN_INTERVAL_MINS: u64 = 5;
pub const MIN_STABILITY_CHECK_SECS: u64 = 60;
pub const MIN_RECENT_FILE_GUARD_MINS: u64 = 3;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandGroup {
    pub id: String,
    pub name: String,
    pub commands: Vec<String>,
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub tasks: Vec<ScanTask>,

    pub local_path: String,
    pub interval_minutes: u64,
    pub time_ranges: Vec<String>, // "HH:mm-HH:mm"

    pub file_extensions: Vec<String>, // e.g. ["exe", "tar.gz"]
    pub filename_includes: Vec<String>, // e.g. ["UMS", "VMS"] - OR logic

    pub deploy_enabled: bool,
    #[serde(default)]
    pub servers: Vec<DeployServer>,

    /// Named command groups, each with an ordered list of shell commands.
    #[serde(default)]
    pub command_groups: Vec<CommandGroup>,

    /// Seconds to wait after discovering files before copying, to verify they are fully written.
    #[serde(default = "default_stability_secs")]
    pub stability_check_secs: u64,

    /// If a file was modified within the last N minutes, it must pass the stability wait.
    #[serde(default = "default_recent_file_guard_mins")]
    pub recent_file_guard_mins: u64,

    #[serde(default)]
    pub launch_and_auto_scan: bool,

    #[serde(default)]
    pub close_to_tray: bool,

    #[serde(default = "default_max_log_lines")]
    pub max_log_lines: u32,
}

fn default_stability_secs() -> u64 { 30 }
fn default_recent_file_guard_mins() -> u64 { MIN_RECENT_FILE_GUARD_MINS }
fn default_max_log_lines() -> u32 { 200 }

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
            stability_check_secs: 120,
            recent_file_guard_mins: MIN_RECENT_FILE_GUARD_MINS,
            launch_and_auto_scan: false,
            close_to_tray: false,
            max_log_lines: 200,
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
        return Err(format!("Scan interval must be at least {} minutes", MIN_SCAN_INTERVAL_MINS));
    }
    if config.stability_check_secs < MIN_STABILITY_CHECK_SECS {
        return Err(format!("File stability wait must be at least {} second", MIN_STABILITY_CHECK_SECS));
    }
    if config.recent_file_guard_mins < MIN_RECENT_FILE_GUARD_MINS {
        return Err(format!("Recent file threshold must be at least {} minutes", MIN_RECENT_FILE_GUARD_MINS));
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

pub fn get_log_path(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle.path().app_data_dir().unwrap().join("app.log")
}

pub fn get_config_path(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle.path().app_config_dir().unwrap().join("config.json")
}
