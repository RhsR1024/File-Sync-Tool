use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

pub const MIN_SCAN_INTERVAL_MINS: u64 = 5;
pub const MIN_STABILITY_CHECK_SECS: u64 = 60;
pub const MIN_RECENT_FILE_GUARD_MINS: u64 = 3;

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
    /// Value: Date Format (e.g. "YYMMDD")
    DateMatch(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScanTask {
    pub id: String,
    pub enabled: bool,
    pub name: String,
    pub remote_path: String,
    pub local_path: Option<String>, // Optional override
    pub rule: MatchRule,
    /// Server IDs to deploy to after copying. Empty = do not deploy.
    #[serde(default)]
    pub deploy_server_ids: Vec<String>,
    /// Task-specific post commands. If non-empty, overrides global post_commands for this task.
    #[serde(default)]
    pub post_commands: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub tasks: Vec<ScanTask>,

    // Legacy fields (kept for migration, but logic will use tasks)
    #[serde(default)]
    pub remote_paths: Vec<String>,
    #[serde(default)]
    pub target_versions: Vec<String>,
    
    pub local_path: String,
    pub interval_minutes: u64,
    pub time_ranges: Vec<String>, // "HH:mm-HH:mm"
    // New fields for filtering
    pub file_extensions: Vec<String>, // e.g. ["exe", "tar.gz"]
    pub filename_includes: Vec<String>, // e.g. ["UMS", "VMS"] - OR logic
    
    // Deploy Config
    pub deploy_enabled: bool,
    #[serde(default)]
    pub servers: Vec<DeployServer>, // New: Multiple servers
    
    // Legacy single server config (kept for migration/fallback)
    #[serde(default)]
    pub ssh_host: String,
    #[serde(default)]
    pub ssh_port: u16,
    #[serde(default)]
    pub ssh_user: String,
    #[serde(default)]
    pub ssh_password: String,
    #[serde(default)]
    pub remote_linux_path: String,
    
    pub post_commands: Vec<String>,

    /// Seconds to wait after discovering files before copying, to verify they are fully written.
    /// Default: 30.
    #[serde(default = "default_stability_secs")]
    pub stability_check_secs: u64,

    /// If a file was modified within the last N minutes, it must pass the stability wait.
    /// Older files are copied immediately. Minimum: 3 minutes.
    #[serde(default = "default_recent_file_guard_mins")]
    pub recent_file_guard_mins: u64,

    /// One switch for:
    /// 1) launch app on OS startup
    /// 2) auto start scheduler scan when app starts
    #[serde(default)]
    pub launch_and_auto_scan: bool,

    /// When enabled, clicking the window close button hides the app to the tray
    /// instead of exiting the process.
    #[serde(default)]
    pub close_to_tray: bool,

    /// Maximum number of log lines to display in the console. Default: 200.
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
            remote_paths: vec![],
            target_versions: vec![],
            local_path: "E:\\UMS_TEMP".to_string(),
            interval_minutes: 10,
            time_ranges: vec![],
            file_extensions: vec![],
            filename_includes: vec![],
            deploy_enabled: false,
            servers: vec![],
            ssh_host: "".to_string(),
            ssh_port: 22,
            ssh_user: "".to_string(),
            ssh_password: "".to_string(),
            remote_linux_path: "/tmp/upload".to_string(),
            post_commands: vec![],
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
            if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                // Migration 1: If servers empty but legacy host exists, migrate it
                if config.servers.is_empty() && !config.ssh_host.is_empty() {
                    config.servers.push(DeployServer {
                        id: uuid::Uuid::new_v4().to_string(),
                        enabled: true,
                        name: "Default Server".to_string(),
                        host: config.ssh_host.clone(),
                        port: config.ssh_port,
                        user: config.ssh_user.clone(),
                        password: config.ssh_password.clone(),
                        remote_path: config.remote_linux_path.clone(),
                    });
                }
                
                // Migration 2: Convert remote_paths/target_versions to tasks
                if config.tasks.is_empty() && !config.remote_paths.is_empty() {
                    for (i, path) in config.remote_paths.iter().enumerate() {
                        let version = config.target_versions.get(i).cloned().unwrap_or_default();
                        if !path.trim().is_empty() {
                            config.tasks.push(ScanTask {
                                id: uuid::Uuid::new_v4().to_string(),
                                enabled: true,
                                name: format!("Auto Task {}", i + 1),
                                remote_path: path.clone(),
                                local_path: None,
                                rule: MatchRule::VersionMatch(version),
                                deploy_server_ids: vec![],
                                post_commands: vec![],
                            });
                        }
                    }
                }
                
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
