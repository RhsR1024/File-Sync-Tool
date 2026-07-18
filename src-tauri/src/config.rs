use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

use crate::clipboard::models::ClipboardSettings;
use app_lib::device_simulator::api::{DeviceGroupDraft, RtspPorts};
use app_lib::device_simulator::profiles::identity::MAX_PREVIEW_DEVICES;
use app_lib::device_simulator::profiles::scope::{
    validate_nvr_channel_count, TargetPlatform, DEFAULT_NVR_CHANNEL_COUNT,
};

pub const MIN_SCAN_INTERVAL_MINS: u64 = 5;
pub const MIN_STABILITY_CHECK_SECS: u64 = 60;
pub const MIN_RECENT_FILE_GUARD_MINS: u64 = 3;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct DeviceSimulatorSettings {
    pub asset_server_url_override: Option<String>,
    pub selected_interface_id: Option<String>,
    pub last_platform: Option<TargetPlatform>,
    pub last_start_ip: Option<std::net::Ipv4Addr>,
    pub last_device_groups: Vec<DeviceGroupDraft>,
    pub last_http_port: u16,
    pub last_rtsp_ports: RtspPorts,
    pub auto_check_asset_updates: bool,
    pub manage_firewall: bool,
}

impl Default for DeviceSimulatorSettings {
    fn default() -> Self {
        Self {
            asset_server_url_override: None,
            selected_interface_id: None,
            last_platform: None,
            last_start_ip: None,
            last_device_groups: vec![],
            last_http_port: 81,
            last_rtsp_ports: RtspPorts::default(),
            auto_check_asset_updates: true,
            manage_firewall: true,
        }
    }
}

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
pub enum DiskCleanupLinuxMode {
    #[default]
    Componentized,
    Mainline,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CopyMode {
    #[default]
    BuiltIn,
    WindowsShell,
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

    /// Show native system notifications for scanned sync task milestones.
    #[serde(default = "default_sync_task_notifications_enabled")]
    pub sync_task_notifications_enabled: bool,

    #[serde(default = "default_max_log_lines")]
    pub max_log_lines: u32,

    /// Copy buffer size in KB. Controls the read/write chunk size when copying files.
    /// Larger values improve throughput on fast network shares. Default: 4096 (4 MB).
    #[serde(default = "default_copy_buffer_size_kb")]
    pub copy_buffer_size_kb: u32,

    /// Copy implementation used after filtering and stability checks.
    #[serde(default)]
    pub copy_mode: CopyMode,

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
    #[serde(default = "default_disk_cleanup_http_timeout_secs")]
    pub disk_cleanup_http_timeout_secs: u64,

    /// Which Linux disk source to use in the cache cleanup tool.
    /// Default: `Componentized` (current `/openAPI/system/v1/disk/server/list` flow).
    /// `Mainline` calls `http://<host>/distapi/status` to enumerate primary/replica nodes.
    #[serde(default)]
    pub disk_cleanup_linux_mode: DiskCleanupLinuxMode,

    #[serde(default = "default_update_server_url")]
    pub update_server_url: String,

    #[serde(default)]
    pub notify_on_new_version: bool,

    #[serde(default)]
    pub last_update_check_at: Option<String>,

    #[serde(default)]
    pub pending_update: Option<crate::updater::PendingUpdate>,

    /// Clipboard manager settings (spec §2026-04-19-clipboard-manager §7.1).
    #[serde(default)]
    pub clipboard: ClipboardSettings,

    /// Video device simulator preferences only. Runtime platform credentials,
    /// Worker/PID state, session journals, and metrics are deliberately absent.
    #[serde(default)]
    pub device_simulator: DeviceSimulatorSettings,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfigPatch {
    pub tasks: Vec<ScanTask>,
    pub local_path: String,
    pub interval_minutes: u64,
    pub time_ranges: Vec<String>,
    pub file_extensions: Vec<String>,
    pub filename_includes: Vec<String>,
    pub deploy_enabled: bool,
    pub servers: Vec<DeployServer>,
    pub command_groups: Vec<CommandGroup>,
    pub local_command_groups: Vec<LocalCommandGroup>,
    pub stability_check_secs: u64,
    pub recent_file_guard_mins: u64,
    pub copy_buffer_size_kb: u32,
    pub copy_mode: CopyMode,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppDomainConfigPatch {
    pub launch_and_auto_scan: bool,
    pub launch_and_auto_start_file_share: bool,
    pub close_to_tray: bool,
    pub sync_task_notifications_enabled: bool,
    pub max_log_lines: u32,
    pub max_task_records: u32,
    pub appliance_ssh_api_timeout_secs: u64,
    pub framework_password_api_timeout_secs: u64,
    pub disk_cleanup_http_timeout_secs: u64,
    pub disk_cleanup_linux_mode: DiskCleanupLinuxMode,
    pub update_server_url: String,
    pub notify_on_new_version: bool,
    pub clipboard: ClipboardSettings,
    pub device_simulator: DeviceSimulatorSettings,
}

pub fn apply_sync_patch(config: &mut AppConfig, patch: SyncConfigPatch) {
    config.tasks = patch.tasks;
    config.local_path = patch.local_path;
    config.interval_minutes = patch.interval_minutes;
    config.time_ranges = patch.time_ranges;
    config.file_extensions = patch.file_extensions;
    config.filename_includes = patch.filename_includes;
    config.deploy_enabled = patch.deploy_enabled;
    config.servers = patch.servers;
    config.command_groups = patch.command_groups;
    config.local_command_groups = patch.local_command_groups;
    config.stability_check_secs = patch.stability_check_secs;
    config.recent_file_guard_mins = patch.recent_file_guard_mins;
    config.copy_buffer_size_kb = patch.copy_buffer_size_kb;
    config.copy_mode = patch.copy_mode;
}

pub fn apply_app_patch(config: &mut AppConfig, patch: AppDomainConfigPatch) {
    config.launch_and_auto_scan = patch.launch_and_auto_scan;
    config.launch_and_auto_start_file_share = patch.launch_and_auto_start_file_share;
    config.close_to_tray = patch.close_to_tray;
    config.sync_task_notifications_enabled = patch.sync_task_notifications_enabled;
    config.max_log_lines = patch.max_log_lines;
    config.max_task_records = patch.max_task_records;
    config.appliance_ssh_api_timeout_secs = patch.appliance_ssh_api_timeout_secs;
    config.framework_password_api_timeout_secs = patch.framework_password_api_timeout_secs;
    config.disk_cleanup_http_timeout_secs = patch.disk_cleanup_http_timeout_secs;
    config.disk_cleanup_linux_mode = patch.disk_cleanup_linux_mode;
    config.update_server_url = patch.update_server_url;
    config.notify_on_new_version = patch.notify_on_new_version;
    config.clipboard = patch.clipboard;
    config.device_simulator = patch.device_simulator;
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
fn default_sync_task_notifications_enabled() -> bool {
    true
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
fn default_disk_cleanup_http_timeout_secs() -> u64 {
    5
}
pub(crate) fn default_update_server_url() -> String {
    "http://192.115.1.3:8080".to_string()
}

pub(crate) fn normalize_update_server_url(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.trim_end_matches('/').to_string()
    }
}

pub(crate) fn validate_update_server_url(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let url = reqwest::Url::parse(trimmed)
        .map_err(|_| "Update server URL must be a valid http(s) URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Update server URL must start with http:// or https://".to_string());
    }
    if url.host_str().is_none() {
        return Err("Update server URL must include a host".to_string());
    }

    Ok(())
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
            sync_task_notifications_enabled: true,
            max_log_lines: 200,
            copy_buffer_size_kb: 4096,
            copy_mode: CopyMode::BuiltIn,
            max_task_records: 100,
            appliance_ssh_api_timeout_secs: 5,
            framework_password_api_timeout_secs: 5,
            disk_cleanup_http_timeout_secs: 5,
            disk_cleanup_linux_mode: DiskCleanupLinuxMode::Componentized,
            update_server_url: default_update_server_url(),
            notify_on_new_version: false,
            last_update_check_at: None,
            pending_update: None,
            clipboard: default_clipboard_settings(),
            device_simulator: DeviceSimulatorSettings::default(),
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
    config.update_server_url = normalize_update_server_url(&config.update_server_url);
    config.device_simulator = normalize_device_simulator_settings(config.device_simulator);
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
    validate_update_server_url(&config.update_server_url)?;
    validate_device_simulator_settings(&config.device_simulator)?;
    Ok(())
}

pub fn normalize_device_simulator_settings(
    mut settings: DeviceSimulatorSettings,
) -> DeviceSimulatorSettings {
    settings.asset_server_url_override = settings
        .asset_server_url_override
        .take()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .filter(|value| validate_asset_server_override(value).is_ok());
    settings.selected_interface_id = settings
        .selected_interface_id
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| is_safe_interface_id(value));

    let mut seen = HashSet::new();
    let mut remaining = u32::from(MAX_PREVIEW_DEVICES);
    settings.last_device_groups.retain_mut(|group| {
        group.id = group.id.trim().to_owned();
        group.profile_id = group.profile_id.trim().to_owned();
        if !is_safe_group_id(&group.id)
            || !seen.insert(group.id.clone())
            || !is_first_release_profile(&group.profile_id)
            || group.count == 0
            || remaining == 0
        {
            return false;
        }
        group.count = group.count.min(remaining);
        remaining -= group.count;
        match group.profile_id.as_str() {
            "nvr-common" | "nvr-vehicle" => {
                group.nvr_channel_count = Some(
                    group
                        .nvr_channel_count
                        .filter(|value| validate_nvr_channel_count(*value).is_ok())
                        .unwrap_or(DEFAULT_NVR_CHANNEL_COUNT),
                );
            }
            _ => group.nvr_channel_count = None,
        }
        true
    });

    if validate_simulator_ports(settings.last_http_port, settings.last_rtsp_ports).is_err() {
        settings.last_http_port = 81;
        settings.last_rtsp_ports = RtspPorts::default();
    }
    settings
}

pub fn validate_device_simulator_settings(
    settings: &DeviceSimulatorSettings,
) -> Result<(), String> {
    if let Some(url) = &settings.asset_server_url_override {
        validate_asset_server_override(url)?;
    }
    if settings
        .selected_interface_id
        .as_deref()
        .is_some_and(|value| !is_safe_interface_id(value))
    {
        return Err("Device simulator interface id is invalid".into());
    }
    validate_simulator_ports(settings.last_http_port, settings.last_rtsp_ports)?;
    let mut ids = HashSet::new();
    let mut total = 0_u32;
    for group in &settings.last_device_groups {
        if !is_safe_group_id(&group.id) || !ids.insert(group.id.as_str()) {
            return Err(format!(
                "Device simulator group id '{}' is invalid or duplicated",
                group.id
            ));
        }
        if !is_first_release_profile(&group.profile_id) {
            return Err(format!(
                "Device simulator profile '{}' is not in the first-release scope",
                group.profile_id
            ));
        }
        if group.count == 0 {
            return Err("Device simulator group count must be non-zero".into());
        }
        total = total
            .checked_add(group.count)
            .ok_or_else(|| "Device simulator group count overflowed".to_string())?;
        match group.profile_id.as_str() {
            "nvr-common" | "nvr-vehicle" => {
                let channels = group.nvr_channel_count.ok_or_else(|| {
                    "Device simulator NVR group requires a channel count".to_string()
                })?;
                validate_nvr_channel_count(channels)
                    .map_err(|code| format!("Invalid device simulator NVR channels: {code}"))?;
            }
            _ if group.nvr_channel_count.is_some() => {
                return Err("Device simulator IPC group cannot declare NVR channels".into());
            }
            _ => {}
        }
    }
    if total > u32::from(MAX_PREVIEW_DEVICES) {
        return Err(format!(
            "Device simulator total device count exceeds {MAX_PREVIEW_DEVICES}"
        ));
    }
    Ok(())
}

fn validate_asset_server_override(value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return Err("Device simulator asset server URL is empty or too long".into());
    }
    let url = reqwest::Url::parse(trimmed)
        .map_err(|_| "Device simulator asset server URL is invalid".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Device simulator asset server URL must be http(s) with a host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Device simulator asset server URL must not contain credentials".into());
    }
    Ok(())
}

fn validate_simulator_ports(http: u16, rtsp: RtspPorts) -> Result<(), String> {
    let ports = [http, rtsp.main, rtsp.sub, rtsp.third];
    if ports.contains(&0) || ports.into_iter().collect::<HashSet<_>>().len() != ports.len() {
        return Err("Device simulator HTTP/RTSP ports must be non-zero and distinct".into());
    }
    Ok(())
}

fn is_safe_interface_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_' | b'.'))
}

fn is_safe_group_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_first_release_profile(value: &str) -> bool {
    matches!(
        value,
        "ipc-custom" | "ipc-smart" | "nvr-common" | "nvr-vehicle"
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sync_domain_snapshot(config: &AppConfig) -> serde_json::Value {
        json!({
            "tasks": config.tasks,
            "local_path": config.local_path,
            "interval_minutes": config.interval_minutes,
            "time_ranges": config.time_ranges,
            "file_extensions": config.file_extensions,
            "filename_includes": config.filename_includes,
            "deploy_enabled": config.deploy_enabled,
            "servers": config.servers,
            "command_groups": config.command_groups,
            "local_command_groups": config.local_command_groups,
            "stability_check_secs": config.stability_check_secs,
            "recent_file_guard_mins": config.recent_file_guard_mins,
            "copy_buffer_size_kb": config.copy_buffer_size_kb,
            "copy_mode": config.copy_mode,
        })
    }

    fn app_and_backend_domain_snapshot(config: &AppConfig) -> serde_json::Value {
        json!({
            "launch_and_auto_scan": config.launch_and_auto_scan,
            "launch_and_auto_start_file_share": config.launch_and_auto_start_file_share,
            "close_to_tray": config.close_to_tray,
            "sync_task_notifications_enabled": config.sync_task_notifications_enabled,
            "max_log_lines": config.max_log_lines,
            "max_task_records": config.max_task_records,
            "appliance_ssh_api_timeout_secs": config.appliance_ssh_api_timeout_secs,
            "framework_password_api_timeout_secs": config.framework_password_api_timeout_secs,
            "disk_cleanup_http_timeout_secs": config.disk_cleanup_http_timeout_secs,
            "disk_cleanup_linux_mode": config.disk_cleanup_linux_mode,
            "update_server_url": config.update_server_url,
            "notify_on_new_version": config.notify_on_new_version,
            "clipboard": config.clipboard,
            "last_update_check_at": config.last_update_check_at,
            "pending_update": config.pending_update,
            "device_simulator": config.device_simulator,
        })
    }

    #[test]
    fn apply_sync_patch_updates_only_sync_domain() {
        let mut config = AppConfig::default();
        config.launch_and_auto_scan = true;
        config.update_server_url = "http://updates.example.test".into();
        config.last_update_check_at = Some("2026-07-10T12:00:00+08:00".into());
        config.pending_update = Some(crate::updater::PendingUpdate {
            target_version: "9.9.9".into(),
            temp_path: r"C:\temp\update.exe".into(),
            target_file_name: "file-sync-tool-9.9.9.exe".into(),
            sha256: "ab".repeat(32),
            downloaded_at: "2026-07-10T12:00:00+08:00".into(),
        });
        let preserved = app_and_backend_domain_snapshot(&config);

        apply_sync_patch(
            &mut config,
            SyncConfigPatch {
                tasks: vec![],
                local_path: r"D:\sync".into(),
                interval_minutes: 15,
                time_ranges: vec!["09:00-18:00".into()],
                file_extensions: vec!["tar.gz".into()],
                filename_includes: vec!["VMS".into()],
                deploy_enabled: true,
                servers: vec![],
                command_groups: vec![],
                local_command_groups: vec![],
                stability_check_secs: 180,
                recent_file_guard_mins: 5,
                copy_buffer_size_kb: 8192,
                copy_mode: CopyMode::WindowsShell,
            },
        );

        assert_eq!(config.local_path, r"D:\sync");
        assert_eq!(config.interval_minutes, 15);
        assert!(config.deploy_enabled);
        assert_eq!(config.copy_mode, CopyMode::WindowsShell);
        assert_eq!(app_and_backend_domain_snapshot(&config), preserved);
    }

    #[test]
    fn apply_app_patch_updates_only_app_domain() {
        let mut config = AppConfig::default();
        config.local_path = r"D:\existing-sync".into();
        config.interval_minutes = 30;
        config.file_extensions = vec!["zip".into()];
        config.last_update_check_at = Some("2026-07-10T12:00:00+08:00".into());
        let preserved_sync = sync_domain_snapshot(&config);
        let preserved_last_check = config.last_update_check_at.clone();
        let preserved_pending = config.pending_update.clone();

        let mut clipboard = config.clipboard.clone();
        clipboard.enabled = false;
        apply_app_patch(
            &mut config,
            AppDomainConfigPatch {
                launch_and_auto_scan: true,
                launch_and_auto_start_file_share: true,
                close_to_tray: true,
                sync_task_notifications_enabled: false,
                max_log_lines: 500,
                max_task_records: 250,
                appliance_ssh_api_timeout_secs: 10,
                framework_password_api_timeout_secs: 11,
                disk_cleanup_http_timeout_secs: 12,
                disk_cleanup_linux_mode: DiskCleanupLinuxMode::Mainline,
                update_server_url: "http://new-updates.example.test/".into(),
                notify_on_new_version: true,
                clipboard,
                device_simulator: DeviceSimulatorSettings {
                    selected_interface_id: Some("adapter-1".into()),
                    ..DeviceSimulatorSettings::default()
                },
            },
        );

        assert!(config.launch_and_auto_scan);
        assert!(!config.sync_task_notifications_enabled);
        assert_eq!(config.max_log_lines, 500);
        assert!(!config.clipboard.enabled);
        assert_eq!(sync_domain_snapshot(&config), preserved_sync);
        assert_eq!(config.last_update_check_at, preserved_last_check);
        assert_eq!(config.pending_update, preserved_pending);
    }

    #[test]
    fn app_config_uses_simplified_clipboard_defaults() {
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
                "enabled": true
            }
        }))
        .unwrap();
        let serialized = serde_json::to_value(&config.clipboard).unwrap();

        assert!(config.clipboard.navigation.enabled);
        assert!(config.clipboard.display.show_char_count);
        assert_eq!(
            config.clipboard.display.show_source_app,
            crate::clipboard::models::ClipboardSourceAppDisplay::Both
        );
        assert_eq!(
            config.clipboard.shortcuts.focus_search,
            vec!["Ctrl+F".to_string()]
        );
        assert!(serialized.get("panel").is_none());
        assert!(serialized.get("toolbar").is_none());
    }

    #[test]
    fn legacy_config_without_update_fields_migrates_to_defaults() {
        let legacy_json = r#"{
            "tasks": [],
            "local_path": "C:\\local",
            "interval_minutes": 5,
            "time_ranges": [],
            "file_extensions": [],
            "filename_includes": [],
            "deploy_enabled": false,
            "servers": [],
            "command_groups": [],
            "local_command_groups": [],
            "stability_check_secs": 60,
            "recent_file_guard_mins": 3,
            "launch_and_auto_scan": false,
            "close_to_tray": false,
            "max_log_lines": 200
        }"#;
        let cfg: AppConfig = serde_json::from_str(legacy_json).expect("parse");
        assert_eq!(cfg.update_server_url, "http://192.115.1.3:8080");
        assert!(!cfg.notify_on_new_version);
        assert!(cfg.sync_task_notifications_enabled);
        assert_eq!(cfg.copy_mode, CopyMode::BuiltIn);
        assert!(cfg.last_update_check_at.is_none());
        assert!(cfg.pending_update.is_none());
    }

    #[test]
    fn config_round_trip_preserves_pending_update() {
        let mut cfg = AppConfig::default();
        cfg.pending_update = Some(crate::updater::PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: r"C:\Users\u\AppData\Local\Temp\fst-update.exe".into(),
            target_file_name: "file-sync-tool-1.0.8.exe".into(),
            sha256: "ab".repeat(32),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        });

        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pending_update, cfg.pending_update);
    }

    #[test]
    fn normalize_config_trims_update_server_url() {
        let mut cfg = AppConfig::default();
        cfg.update_server_url = "  http://example.com/releases/  ".into();
        let normalized = normalize_config(cfg);
        assert_eq!(normalized.update_server_url, "http://example.com/releases");
    }

    #[test]
    fn validate_config_rejects_invalid_update_server_url() {
        let mut cfg = AppConfig::default();
        cfg.update_server_url = "ftp://example.com".into();
        let error = validate_config(&cfg).unwrap_err();
        assert!(error.contains("http:// or https://"));
    }
}
