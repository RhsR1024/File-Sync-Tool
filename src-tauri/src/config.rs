use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

use crate::clipboard::models::ClipboardSettings;
use app_lib::device_simulator::api::{
    DeviceGroupDraft, PlatformAccessMode, RtspPorts, TargetPlatformServer,
    DEFAULT_ALARM_RECEIVER_PORT, DEFAULT_MEDIA_THEME_ID,
};
use app_lib::device_simulator::profiles::identity::MAX_PREVIEW_DEVICES;
use app_lib::device_simulator::profiles::scope::TargetPlatform;

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
    pub last_device_ips: Vec<std::net::Ipv4Addr>,
    pub last_subnet_prefix: u8,
    pub last_platform_servers: Vec<TargetPlatformServer>,
    pub last_platform_access_mode: PlatformAccessMode,
    pub last_alarm_receiver_url: Option<String>,
    pub last_alarm_receiver_port: Option<u16>,
    pub last_device_groups: Vec<DeviceGroupDraft>,
    pub last_http_port: u16,
    pub last_rtsp_ports: RtspPorts,
    pub last_media_theme_id: String,
    pub last_time_watermark_enabled: bool,
    pub auto_check_asset_updates: bool,
    pub manage_firewall: bool,
    /// UMS platform login account used only by the main process when registering devices.
    pub platform_username: String,
    /// Intentionally persisted as clear text so the settings UI can reveal it on demand.
    pub platform_password: String,
    /// Register all devices with every configured UMS after the simulator reaches Running.
    pub platform_auto_add_devices: bool,
    /// Remove UMS resources whose IP matches a virtual device before registering it.
    pub platform_replace_existing_devices: bool,
}

impl Default for DeviceSimulatorSettings {
    fn default() -> Self {
        Self {
            asset_server_url_override: None,
            selected_interface_id: None,
            last_platform: Some(TargetPlatform::Ums),
            last_start_ip: None,
            last_device_ips: vec![],
            last_subnet_prefix: 24,
            last_platform_servers: vec![],
            last_platform_access_mode: PlatformAccessMode::Open,
            last_alarm_receiver_url: None,
            last_alarm_receiver_port: Some(DEFAULT_ALARM_RECEIVER_PORT),
            last_device_groups: vec![],
            last_http_port: 81,
            last_rtsp_ports: RtspPorts::default(),
            last_media_theme_id: DEFAULT_MEDIA_THEME_ID.into(),
            last_time_watermark_enabled: true,
            auto_check_asset_updates: true,
            manage_firewall: true,
            platform_username: "loadmin".into(),
            platform_password: "admin_123".into(),
            platform_auto_add_devices: true,
            platform_replace_existing_devices: false,
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct PortalLoginSettings {
    pub enabled: bool,
    pub host: String,
    pub login_url: String,
    pub portal_url: String,
    pub username: String,
    pub password: String,
    /// Frontend-only update intent: keep the existing secret when the password is redacted.
    /// The in-memory backend value is kept in `password`; config files store DPAPI ciphertext.
    pub password_saved: bool,
    pub remember_pwd: bool,
    pub retry_count: u32,
    pub retry_interval_secs: u64,
    pub network_wait_secs: u64,
    pub request_timeout_secs: u64,
}

impl Default for PortalLoginSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "http://1.1.1.3".to_string(),
            login_url: "/ac_portal/login.php".to_string(),
            portal_url: "/ac_portal/default/pc.html?template=default&tabs=pwd&dual_stack=0&vlanid=0&_ID_=0&switch_url=&url=http://1.1.1.3/homepage/index.html&controller_type=&mac=".to_string(),
            username: String::new(),
            password: String::new(),
            password_saved: false,
            remember_pwd: true,
            retry_count: 3,
            retry_interval_secs: 5,
            network_wait_secs: 30,
            request_timeout_secs: 15,
        }
    }
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

    /// Video device simulator preferences, including main-process-only UMS login
    /// credentials. Worker/PID state, session journals, and metrics are absent.
    #[serde(default)]
    pub device_simulator: DeviceSimulatorSettings,

    /// Captive-portal authentication settings. The in-memory password is persisted
    /// with Windows DPAPI protection because unattended login cannot run without it.
    #[serde(default)]
    pub portal_login: PortalLoginSettings,
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
    pub portal_login: PortalLoginSettings,
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
    let mut portal_login = patch.portal_login;
    merge_redacted_portal_password(&mut portal_login, &config.portal_login);
    config.portal_login = portal_login;
}

/// Restore a redacted password sent back by the frontend, or honor an explicit clear.
pub fn merge_redacted_portal_password(
    incoming: &mut PortalLoginSettings,
    previous: &PortalLoginSettings,
) {
    if incoming.password.is_empty() && incoming.password_saved {
        incoming.password.clone_from(&previous.password);
    }
    incoming.password_saved = !incoming.password.is_empty();
}

/// Return config to the webview without ever exposing the saved portal password.
pub fn redact_secrets_for_frontend(mut config: AppConfig) -> AppConfig {
    config.portal_login.password_saved = !config.portal_login.password.is_empty();
    config.portal_login.password.clear();
    config
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
            portal_login: PortalLoginSettings::default(),
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
    config.portal_login = normalize_portal_login_settings(config.portal_login);
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
    validate_portal_login_settings(&config.portal_login)?;
    Ok(())
}

pub fn normalize_portal_login_settings(mut settings: PortalLoginSettings) -> PortalLoginSettings {
    settings.host = settings.host.trim().trim_end_matches('/').to_string();
    settings.login_url = settings.login_url.trim().to_string();
    settings.portal_url = settings.portal_url.trim().to_string();
    settings.username = settings.username.trim().to_string();
    settings.password_saved = !settings.password.is_empty();
    settings
}

pub fn validate_portal_login_settings(settings: &PortalLoginSettings) -> Result<(), String> {
    let host = reqwest::Url::parse(settings.host.trim())
        .map_err(|_| "portal_login.invalid_host".to_string())?;
    if !matches!(host.scheme(), "http" | "https")
        || host.host_str().is_none()
        || host.path() != "/"
        || host.query().is_some()
        || host.fragment().is_some()
    {
        return Err("portal_login.invalid_host".to_string());
    }
    for value in [&settings.login_url, &settings.portal_url] {
        if value.trim().is_empty() {
            return Err("portal_login.url_required".to_string());
        }
        if let Ok(url) = reqwest::Url::parse(value.trim()) {
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err("portal_login.invalid_url".to_string());
            }
        } else if host.join(value.trim().trim_start_matches('/')).is_err() {
            return Err("portal_login.invalid_url".to_string());
        }
    }
    if !(1..=10).contains(&settings.retry_count) {
        return Err("portal_login.invalid_retry_count".to_string());
    }
    if !(1..=300).contains(&settings.retry_interval_secs) {
        return Err("portal_login.invalid_retry_interval".to_string());
    }
    if settings.network_wait_secs > 300 {
        return Err("portal_login.invalid_network_wait".to_string());
    }
    if !(1..=120).contains(&settings.request_timeout_secs) {
        return Err("portal_login.invalid_request_timeout".to_string());
    }
    if settings.enabled && settings.username.trim().is_empty() {
        return Err("portal_login.username_required".to_string());
    }
    if settings.enabled && settings.password.is_empty() {
        return Err("portal_login.password_required".to_string());
    }
    Ok(())
}

pub fn normalize_device_simulator_settings(
    mut settings: DeviceSimulatorSettings,
) -> DeviceSimulatorSettings {
    settings.last_platform = Some(TargetPlatform::Ums);
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
    settings.last_subnet_prefix = settings.last_subnet_prefix.clamp(1, 30);
    settings
        .last_device_ips
        .truncate(MAX_PREVIEW_DEVICES as usize);
    let mut seen_ips = HashSet::new();
    settings
        .last_device_ips
        .retain(|address| seen_ips.insert(*address));
    settings.last_platform_servers.truncate(8);
    settings.last_platform_servers.retain_mut(|server| {
        server.id = server.id.trim().to_owned();
        server.host = server.host.trim().to_owned();
        !server.id.is_empty()
            && server.id.len() <= 128
            && !server.host.is_empty()
            && server.host.len() <= 253
            && server.port > 0
    });
    settings.platform_username = settings.platform_username.trim().to_owned();
    settings.last_alarm_receiver_url = settings
        .last_alarm_receiver_url
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value.len() <= 2048);
    if settings.last_alarm_receiver_port == Some(0) {
        settings.last_alarm_receiver_port = Some(DEFAULT_ALARM_RECEIVER_PORT);
    }

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
        true
    });

    if validate_simulator_ports(settings.last_http_port, settings.last_rtsp_ports).is_err() {
        settings.last_http_port = 81;
        settings.last_rtsp_ports = RtspPorts::default();
    }
    settings.last_media_theme_id = settings.last_media_theme_id.trim().to_owned();
    if !is_safe_media_theme_id(&settings.last_media_theme_id) {
        settings.last_media_theme_id = DEFAULT_MEDIA_THEME_ID.into();
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
    if !is_safe_media_theme_id(&settings.last_media_theme_id) {
        return Err("Device simulator media theme id is invalid".into());
    }
    if settings.last_alarm_receiver_port == Some(0) {
        return Err("Device simulator alarm receiver port must be non-zero".into());
    }
    if !(1..=30).contains(&settings.last_subnet_prefix) {
        return Err("Device simulator subnet prefix must be between 1 and 30".into());
    }
    if settings.platform_auto_add_devices && !settings.last_platform_servers.is_empty() {
        if settings.platform_username.is_empty() || settings.platform_password.is_empty() {
            return Err(
                "Automatic platform registration requires a UMS username and password".into(),
            );
        }
    }
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
    }
    if total > u32::from(MAX_PREVIEW_DEVICES) {
        return Err(format!(
            "Device simulator total device count exceeds {MAX_PREVIEW_DEVICES}"
        ));
    }
    Ok(())
}

fn is_safe_media_theme_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
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

/// Groups naming any other profile are dropped by `normalize_device_simulator_settings`,
/// which is how a config saved before the other five device types were removed
/// migrates itself on load.
fn is_first_release_profile(value: &str) -> bool {
    value == "ipc-structured"
}

const PORTAL_PASSWORD_DPAPI_PREFIX: &str = "dpapi:v1:";

#[cfg(target_os = "windows")]
fn protect_portal_password(password: &str) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if password.is_empty() {
        return Ok(String::new());
    }

    let bytes = password.as_bytes();
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())
            .map_err(|_| "portal_login.password_too_long".to_string())?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &mut input,
            windows_core::PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|error| format!("portal_login.password_protect_failed: {error}"))?;
    }

    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
    }
    Ok(format!(
        "{PORTAL_PASSWORD_DPAPI_PREFIX}{}",
        STANDARD.encode(protected)
    ))
}

#[cfg(target_os = "windows")]
fn unprotect_portal_password(value: &str) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if value.is_empty() {
        return Ok(String::new());
    }
    let Some(encoded) = value.strip_prefix(PORTAL_PASSWORD_DPAPI_PREFIX) else {
        return Ok(value.to_string());
    };
    let mut protected = STANDARD
        .decode(encoded)
        .map_err(|error| format!("portal_login.password_decode_failed: {error}"))?;
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(protected.len())
            .map_err(|_| "portal_login.password_ciphertext_too_long".to_string())?,
        pbData: protected.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|error| format!("portal_login.password_unprotect_failed: {error}"))?;
    }

    let plaintext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
    }
    String::from_utf8(plaintext)
        .map_err(|error| format!("portal_login.password_utf8_failed: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn protect_portal_password(_password: &str) -> Result<String, String> {
    Err("portal_login.password_protection_requires_windows".to_string())
}

#[cfg(not(target_os = "windows"))]
fn unprotect_portal_password(value: &str) -> Result<String, String> {
    if value.starts_with(PORTAL_PASSWORD_DPAPI_PREFIX) {
        Err("portal_login.password_protection_requires_windows".to_string())
    } else {
        Ok(value.to_string())
    }
}

fn prepare_config_for_storage(config: &AppConfig) -> Result<AppConfig, String> {
    let mut stored = config.clone();
    stored.portal_login.password = protect_portal_password(&config.portal_login.password)?;
    stored.portal_login.password_saved = !stored.portal_login.password.is_empty();
    Ok(stored)
}

pub fn load_config(app_handle: &tauri::AppHandle) -> AppConfig {
    let config_path = get_config_path(app_handle);
    if config_path.exists() {
        if let Ok(content) = fs::read_to_string(&config_path) {
            if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                let stored_password = config.portal_login.password.clone();
                match unprotect_portal_password(&stored_password) {
                    Ok(password) => {
                        config.portal_login.password = password;
                        config.portal_login.password_saved =
                            !config.portal_login.password.is_empty();
                        let config = normalize_config(config);
                        if !stored_password.is_empty()
                            && !stored_password.starts_with(PORTAL_PASSWORD_DPAPI_PREFIX)
                        {
                            if let Err(error) = save_config(app_handle, &config) {
                                log::warn!(
                                    "Failed to migrate the Portal password to DPAPI: {error}"
                                );
                            }
                        }
                        return config;
                    }
                    Err(error) => {
                        log::warn!("Failed to decrypt the saved Portal password: {error}");
                        config.portal_login.password.clear();
                        config.portal_login.password_saved = false;
                        config.portal_login.enabled = false;
                        return normalize_config(config);
                    }
                }
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
    let stored = prepare_config_for_storage(config)?;
    let content = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
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
            "portal_login": config.portal_login,
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
                portal_login: PortalLoginSettings {
                    enabled: true,
                    username: "portal-user".into(),
                    password: "portal-password".into(),
                    ..PortalLoginSettings::default()
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
    fn device_simulator_defaults_alarm_receiver_port_to_the_observed_ums_port() {
        let value = serde_json::to_value(DeviceSimulatorSettings::default()).unwrap();
        assert_eq!(
            value["last_alarm_receiver_port"],
            DEFAULT_ALARM_RECEIVER_PORT
        );
    }

    #[test]
    fn device_simulator_legacy_settings_default_time_watermark_on() {
        let settings: DeviceSimulatorSettings = serde_json::from_value(serde_json::json!({
            "last_media_theme_id": DEFAULT_MEDIA_THEME_ID
        }))
        .unwrap();

        assert!(settings.last_time_watermark_enabled);
    }

    #[test]
    fn device_simulator_legacy_settings_receive_platform_registration_defaults() {
        let settings: DeviceSimulatorSettings = serde_json::from_value(serde_json::json!({
            "last_media_theme_id": DEFAULT_MEDIA_THEME_ID
        }))
        .unwrap();

        assert_eq!(settings.platform_username, "loadmin");
        assert_eq!(settings.platform_password, "admin_123");
        assert!(settings.platform_auto_add_devices);
        assert!(!settings.platform_replace_existing_devices);
    }

    #[test]
    fn platform_auto_add_preference_round_trips_both_states() {
        for expected in [true, false] {
            let settings = DeviceSimulatorSettings {
                platform_auto_add_devices: expected,
                ..DeviceSimulatorSettings::default()
            };
            let restored: DeviceSimulatorSettings =
                serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();

            assert_eq!(restored.platform_auto_add_devices, expected);
        }
    }

    #[test]
    fn platform_replace_existing_preference_round_trips_both_states() {
        for expected in [true, false] {
            let settings = DeviceSimulatorSettings {
                platform_replace_existing_devices: expected,
                ..DeviceSimulatorSettings::default()
            };
            let restored: DeviceSimulatorSettings =
                serde_json::from_value(serde_json::to_value(settings).unwrap()).unwrap();

            assert_eq!(restored.platform_replace_existing_devices, expected);
        }
    }

    #[test]
    fn automatic_platform_registration_allows_an_empty_server_draft() {
        let mut settings = DeviceSimulatorSettings {
            platform_auto_add_devices: true,
            ..DeviceSimulatorSettings::default()
        };
        assert!(validate_device_simulator_settings(&settings).is_ok());

        settings.last_platform_servers.push(TargetPlatformServer {
            id: "ums-1".into(),
            host: "192.115.1.17".into(),
            port: 80,
        });
        assert!(validate_device_simulator_settings(&settings).is_ok());

        settings.platform_password.clear();
        assert!(validate_device_simulator_settings(&settings).is_err());

        settings.last_platform_servers.clear();
        assert!(validate_device_simulator_settings(&settings).is_ok());
    }

    #[test]
    fn default_app_config_remains_valid_with_auto_add_enabled() {
        assert!(validate_config(&AppConfig::default()).is_ok());
    }

    #[test]
    fn device_simulator_alarm_receiver_port_is_normalized_and_validated() {
        let mut invalid = DeviceSimulatorSettings::default();
        invalid.last_alarm_receiver_port = Some(0);
        assert!(validate_device_simulator_settings(&invalid).is_err());
        assert_eq!(
            normalize_device_simulator_settings(invalid).last_alarm_receiver_port,
            Some(DEFAULT_ALARM_RECEIVER_PORT)
        );

        let mut automatic = DeviceSimulatorSettings::default();
        automatic.last_alarm_receiver_port = None;
        assert_eq!(
            normalize_device_simulator_settings(automatic).last_alarm_receiver_port,
            None
        );
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

    #[test]
    fn portal_password_is_redacted_and_preserved_until_explicitly_cleared() {
        let previous = PortalLoginSettings {
            password: "secret-value".into(),
            password_saved: true,
            ..PortalLoginSettings::default()
        };
        let frontend = redact_secrets_for_frontend(AppConfig {
            portal_login: previous.clone(),
            ..AppConfig::default()
        });
        assert!(frontend.portal_login.password.is_empty());
        assert!(frontend.portal_login.password_saved);

        let mut keep = frontend.portal_login.clone();
        merge_redacted_portal_password(&mut keep, &previous);
        assert_eq!(keep.password, "secret-value");
        assert!(keep.password_saved);

        let mut clear = frontend.portal_login;
        clear.password_saved = false;
        merge_redacted_portal_password(&mut clear, &previous);
        assert!(clear.password.is_empty());
        assert!(!clear.password_saved);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn portal_password_uses_dpapi_ciphertext_at_rest() {
        let config = AppConfig {
            portal_login: PortalLoginSettings {
                password: "domain-secret-测试".into(),
                password_saved: true,
                ..PortalLoginSettings::default()
            },
            ..AppConfig::default()
        };
        let stored = prepare_config_for_storage(&config).unwrap();
        assert!(stored
            .portal_login
            .password
            .starts_with(PORTAL_PASSWORD_DPAPI_PREFIX));
        assert!(!stored.portal_login.password.contains("domain-secret"));
        assert_eq!(
            unprotect_portal_password(&stored.portal_login.password).unwrap(),
            "domain-secret-测试"
        );
    }
}
