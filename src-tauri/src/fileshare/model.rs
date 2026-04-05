use serde::{Deserialize, Serialize};

pub const FILE_SHARE_CONFIG_VERSION: u32 = 2;
pub const GUEST_ACCOUNT_ID: &str = "guest";
pub const GUEST_ACCOUNT_NAME: &str = "Guest";
pub const DEFAULT_SESSION_TTL_MINUTES: u32 = 30;
pub const MAX_SESSION_TTL_MINUTES: u32 = 7 * 24 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPreset {
    #[default]
    ReadOnly,
    ReadWrite,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeleteMode {
    #[default]
    RecycleBin,
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum IpFilterMode {
    #[default]
    Off,
    Whitelist,
    Blacklist,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileShareRoot {
    pub id: String,
    pub alias: String,
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSharePermissionSet {
    pub browse: bool,
    pub download_file: bool,
    pub download_archive: bool,
    pub upload_file: bool,
    pub upload_directory: bool,
    pub create_directory: bool,
    pub create_text: bool,
    pub rename: bool,
    pub delete: bool,
    pub preview_image: bool,
    pub search_current: bool,
    pub search_global: bool,
}

impl FileSharePermissionSet {
    pub fn read_only() -> Self {
        Self {
            browse: true,
            download_file: true,
            download_archive: true,
            upload_file: false,
            upload_directory: false,
            create_directory: false,
            create_text: false,
            rename: false,
            delete: false,
            preview_image: true,
            search_current: true,
            search_global: true,
        }
    }

    pub fn read_write() -> Self {
        Self {
            browse: true,
            download_file: true,
            download_archive: true,
            upload_file: true,
            upload_directory: true,
            create_directory: true,
            create_text: true,
            rename: true,
            delete: true,
            preview_image: true,
            search_current: true,
            search_global: true,
        }
    }

    pub fn allows(&self, permission: FileSharePermission) -> bool {
        match permission {
            FileSharePermission::Browse => self.browse,
            FileSharePermission::DownloadFile => self.download_file,
            FileSharePermission::DownloadArchive => self.download_archive,
            FileSharePermission::UploadFile => self.upload_file,
            FileSharePermission::UploadDirectory => self.upload_directory,
            FileSharePermission::CreateDirectory => self.create_directory,
            FileSharePermission::CreateText => self.create_text,
            FileSharePermission::Rename => self.rename,
            FileSharePermission::Delete => self.delete,
            FileSharePermission::PreviewImage => self.preview_image,
            FileSharePermission::SearchCurrent => self.search_current,
            FileSharePermission::SearchGlobal => self.search_global,
        }
    }
}

impl Default for FileSharePermissionSet {
    fn default() -> Self {
        Self::read_only()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSharePermission {
    Browse,
    DownloadFile,
    DownloadArchive,
    UploadFile,
    UploadDirectory,
    CreateDirectory,
    CreateText,
    Rename,
    Delete,
    PreviewImage,
    SearchCurrent,
    SearchGlobal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedFileShareAccount {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub preset: PermissionPreset,
    pub permissions: FileSharePermissionSet,
    pub password_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedFileShareConfig {
    pub version: u32,
    pub port: u16,
    pub roots: Vec<FileShareRoot>,
    pub guest_access_enabled: bool,
    pub accounts: Vec<PersistedFileShareAccount>,
    pub session_ttl_minutes: u32,
    pub ip_filter_mode: IpFilterMode,
    pub ip_rules: Vec<String>,
    pub image_preview_enabled: bool,
    // TODO(file-share): wire this into the planned thumbnail list mode at runtime.
    pub thumbnail_enabled: bool,
    pub delete_mode: DeleteMode,
    pub remember_settings: bool,
    pub auto_start_on_page_open: bool,
    pub auto_start_with_windows: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileShareAccountView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub preset: PermissionPreset,
    pub permissions: FileSharePermissionSet,
    pub password_set: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileShareSettingsView {
    pub port: u16,
    pub roots: Vec<FileShareRoot>,
    pub guest_access_enabled: bool,
    pub accounts: Vec<FileShareAccountView>,
    pub session_ttl_minutes: u32,
    pub ip_filter_mode: IpFilterMode,
    pub ip_rules: Vec<String>,
    pub image_preview_enabled: bool,
    // TODO(file-share): wire this into the planned thumbnail list mode at runtime.
    pub thumbnail_enabled: bool,
    pub delete_mode: DeleteMode,
    pub remember_settings: bool,
    pub auto_start_on_page_open: bool,
    pub auto_start_with_windows: bool,
}

impl From<PersistedFileShareConfig> for FileShareSettingsView {
    fn from(value: PersistedFileShareConfig) -> Self {
        Self::from(&value)
    }
}

impl From<&PersistedFileShareConfig> for FileShareSettingsView {
    fn from(value: &PersistedFileShareConfig) -> Self {
        Self {
            port: value.port,
            roots: value.roots.clone(),
            guest_access_enabled: value.guest_access_enabled,
            accounts: value
                .accounts
                .iter()
                .map(|account| FileShareAccountView {
                    id: account.id.clone(),
                    name: account.name.clone(),
                    enabled: account.enabled,
                    preset: account.preset.clone(),
                    permissions: account.permissions.clone(),
                    password_set: account.password_hash.is_some(),
                })
                .collect(),
            session_ttl_minutes: value.session_ttl_minutes,
            ip_filter_mode: value.ip_filter_mode.clone(),
            ip_rules: value.ip_rules.clone(),
            image_preview_enabled: value.image_preview_enabled,
            thumbnail_enabled: value.thumbnail_enabled,
            delete_mode: value.delete_mode.clone(),
            remember_settings: value.remember_settings,
            auto_start_on_page_open: value.auto_start_on_page_open,
            auto_start_with_windows: value.auto_start_with_windows,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileShareAccountSaveRequest {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub preset: PermissionPreset,
    pub permissions: FileSharePermissionSet,
    #[serde(default)]
    pub new_password: Option<String>,
    #[serde(default)]
    pub clear_password: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileShareSettingsSaveRequest {
    pub port: u16,
    pub roots: Vec<FileShareRoot>,
    pub guest_access_enabled: bool,
    pub accounts: Vec<FileShareAccountSaveRequest>,
    pub session_ttl_minutes: u32,
    pub ip_filter_mode: IpFilterMode,
    pub ip_rules: Vec<String>,
    pub image_preview_enabled: bool,
    // TODO(file-share): wire this into the planned thumbnail list mode at runtime.
    pub thumbnail_enabled: bool,
    pub delete_mode: DeleteMode,
    pub remember_settings: bool,
    pub auto_start_on_page_open: bool,
    pub auto_start_with_windows: bool,
}

pub fn default_guest_account() -> PersistedFileShareAccount {
    PersistedFileShareAccount {
        id: GUEST_ACCOUNT_ID.to_string(),
        name: GUEST_ACCOUNT_NAME.to_string(),
        enabled: true,
        preset: PermissionPreset::ReadOnly,
        permissions: FileSharePermissionSet::read_only(),
        password_hash: None,
    }
}

pub fn default_persisted_file_share_config() -> PersistedFileShareConfig {
    PersistedFileShareConfig {
        version: FILE_SHARE_CONFIG_VERSION,
        port: 8080,
        roots: Vec::new(),
        guest_access_enabled: true,
        accounts: vec![default_guest_account()],
        session_ttl_minutes: DEFAULT_SESSION_TTL_MINUTES,
        ip_filter_mode: IpFilterMode::Off,
        ip_rules: Vec::new(),
        image_preview_enabled: true,
        thumbnail_enabled: false,
        delete_mode: DeleteMode::RecycleBin,
        remember_settings: true,
        auto_start_on_page_open: false,
        auto_start_with_windows: false,
    }
}
