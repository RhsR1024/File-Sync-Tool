use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use tauri::Manager;

use super::model::{
    default_guest_account, default_persisted_file_share_config, FileShareAccountSaveRequest,
    FileSharePermissionSet, FileShareSettingsSaveRequest, FileShareSettingsView,
    PermissionPreset, PersistedFileShareAccount, PersistedFileShareConfig,
    DEFAULT_SESSION_TTL_MINUTES, FILE_SHARE_CONFIG_VERSION, GUEST_ACCOUNT_ID,
    GUEST_ACCOUNT_NAME, MAX_SESSION_TTL_MINUTES,
};

const FILE_SHARE_SETTINGS_FILE_NAME: &str = "file_share_v2.json";

#[tauri::command]
pub fn file_share_load_settings(app_handle: tauri::AppHandle) -> Result<FileShareSettingsView, String> {
    let mut saved = load_persisted_file_share_config(&app_handle)?;
    let app_config = crate::config::load_config(&app_handle);
    if sync_file_share_auto_start_from_app_config(&mut saved, &app_config) {
        save_persisted_file_share_config(&app_handle, &saved)?;
    }
    Ok(FileShareSettingsView::from(saved))
}

#[tauri::command]
pub fn file_share_save_settings(
    app_handle: tauri::AppHandle,
    request: FileShareSettingsSaveRequest,
) -> Result<FileShareSettingsView, String> {
    let saved = apply_save_request(load_persisted_file_share_config(&app_handle).ok(), request)?;
    let mut app_config = crate::config::load_config(&app_handle);
    let startup_changed = sync_app_config_auto_start_from_file_share(&mut app_config, &saved);

    save_persisted_file_share_config(&app_handle, &saved)?;
    if startup_changed {
        crate::config::save_config(&app_handle, &app_config)?;
        crate::sync_launch_on_startup(
            app_config.launch_and_auto_scan || app_config.launch_and_auto_start_file_share,
        )?;
    }
    Ok(FileShareSettingsView::from(saved))
}

pub fn load_persisted_file_share_config(
    app_handle: &tauri::AppHandle,
) -> Result<PersistedFileShareConfig, String> {
    let path = get_file_share_settings_path(app_handle)?;
    load_persisted_file_share_config_from_path(&path)
}

pub fn save_persisted_file_share_config(
    app_handle: &tauri::AppHandle,
    config: &PersistedFileShareConfig,
) -> Result<(), String> {
    let path = get_file_share_settings_path(app_handle)?;
    save_persisted_file_share_config_to_path(&path, config)
}

fn get_file_share_settings_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .app_config_dir()
        .map_err(|e| format!("Failed to resolve file share config directory: {}", e))
        .map(|dir| dir.join(FILE_SHARE_SETTINGS_FILE_NAME))
}

pub fn load_persisted_file_share_config_from_path(
    path: &Path,
) -> Result<PersistedFileShareConfig, String> {
    if !path.exists() {
        return Ok(default_persisted_file_share_config());
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file share settings {}: {}", path.display(), e))?;
    let parsed = serde_json::from_str::<PersistedFileShareConfig>(&content).map_err(|e| {
        format!(
            "Failed to parse file share settings {}: {}",
            path.display(),
            e
        )
    })?;

    Ok(normalize_persisted_file_share_config(parsed))
}

pub fn save_persisted_file_share_config_to_path(
    path: &Path,
    config: &PersistedFileShareConfig,
) -> Result<(), String> {
    let normalized = normalize_persisted_file_share_config(config.clone());

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create file share config directory {}: {}",
                parent.display(),
                e
            )
        })?;
    }

    let content = serde_json::to_string_pretty(&normalized)
        .map_err(|e| format!("Failed to serialize file share settings: {}", e))?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, content).map_err(|e| {
        format!(
            "Failed to write file share settings temp file {}: {}",
            tmp_path.display(),
            e
        )
    })?;
    fs::rename(&tmp_path, path).map_err(|e| {
        format!(
            "Failed to move file share settings into place {}: {}",
            path.display(),
            e
        )
    })?;

    Ok(())
}

pub fn apply_save_request(
    existing: Option<PersistedFileShareConfig>,
    request: FileShareSettingsSaveRequest,
) -> Result<PersistedFileShareConfig, String> {
    if request.port < 1024 {
        return Err("File share port must be >= 1024".to_string());
    }

    let existing = existing.map(normalize_persisted_file_share_config);
    let existing_accounts = existing
        .as_ref()
        .map(|config| {
            config
                .accounts
                .iter()
                .map(|account| (account.id.clone(), account.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let roots = request
        .roots
        .into_iter()
        .map(|root| {
            let id = root.id.trim().to_string();
            let alias = root.alias.trim().to_string();
            let path = root.path.trim().to_string();
            if id.is_empty() {
                return Err("File share root id is required".to_string());
            }
            if alias.is_empty() {
                return Err(format!("File share root alias is required for {}", id));
            }
            if path.is_empty() {
                return Err(format!("File share root path is required for {}", alias));
            }

            Ok(super::model::FileShareRoot {
                id,
                alias,
                path,
                enabled: root.enabled,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut seen_root_ids = HashSet::new();
    for root in &roots {
        if !seen_root_ids.insert(root.id.clone()) {
            return Err(format!("Duplicate file share root id: {}", root.id));
        }
    }

    let mut seen_account_ids = HashSet::new();
    let mut accounts = request
        .accounts
        .into_iter()
        .map(|account| {
            let account_id = account_id_key(&account);
            build_persisted_account(
                account,
                existing_accounts.get(&account_id).cloned(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    for account in &accounts {
        if !seen_account_ids.insert(account.id.clone()) {
            return Err(format!("Duplicate file share account id: {}", account.id));
        }
    }

    if let Some(guest) = accounts.iter_mut().find(|account| account.id == GUEST_ACCOUNT_ID) {
        guest.name = normalize_name(&guest.name, GUEST_ACCOUNT_NAME);
    } else {
        accounts.insert(0, default_guest_account());
    }

    let ip_rules = request
        .ip_rules
        .into_iter()
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty())
        .collect::<Vec<_>>();

    Ok(normalize_persisted_file_share_config(
        PersistedFileShareConfig {
            version: FILE_SHARE_CONFIG_VERSION,
            port: request.port,
            roots,
            guest_access_enabled: request.guest_access_enabled,
            accounts,
            session_ttl_minutes: normalize_session_ttl_minutes(request.session_ttl_minutes),
            ip_filter_mode: request.ip_filter_mode,
            ip_rules,
            image_preview_enabled: request.image_preview_enabled,
            thumbnail_enabled: request.thumbnail_enabled,
            delete_mode: request.delete_mode,
            remember_settings: request.remember_settings,
            auto_start_on_page_open: request.auto_start_on_page_open,
            auto_start_with_windows: request.auto_start_with_windows,
        },
    ))
}

fn build_persisted_account(
    account: FileShareAccountSaveRequest,
    existing: Option<PersistedFileShareAccount>,
) -> Result<PersistedFileShareAccount, String> {
    let id = account.id.trim().to_string();
    if id.is_empty() {
        return Err("File share account id is required".to_string());
    }

    let previous_hash = existing.and_then(|value| value.password_hash);
    let password_hash = if account.clear_password {
        None
    } else if let Some(new_password) = normalize_optional_secret(account.new_password.as_deref()) {
        Some(super::hash_password(&new_password))
    } else {
        previous_hash
    };

    Ok(PersistedFileShareAccount {
        id,
        name: normalize_name(&account.name, "Account"),
        enabled: account.enabled,
        preset: account.preset.clone(),
        permissions: permissions_for_preset(account.preset, account.permissions),
        password_hash,
    })
}

fn permissions_for_preset(
    preset: PermissionPreset,
    permissions: FileSharePermissionSet,
) -> FileSharePermissionSet {
    match preset {
        PermissionPreset::ReadOnly => FileSharePermissionSet::read_only(),
        PermissionPreset::ReadWrite => FileSharePermissionSet::read_write(),
        PermissionPreset::Custom => permissions,
    }
}

fn normalize_persisted_file_share_config(
    mut config: PersistedFileShareConfig,
) -> PersistedFileShareConfig {
    config.version = FILE_SHARE_CONFIG_VERSION;
    if config.port == 0 {
        config.port = 8080;
    }
    config.session_ttl_minutes = normalize_session_ttl_minutes(config.session_ttl_minutes);

    config.ip_rules = config
        .ip_rules
        .into_iter()
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty())
        .collect();

    config.roots = config
        .roots
        .into_iter()
        .filter_map(|root| {
            let id = root.id.trim().to_string();
            let alias = root.alias.trim().to_string();
            let path = root.path.trim().to_string();
            if id.is_empty() || alias.is_empty() || path.is_empty() {
                return None;
            }
            Some(super::model::FileShareRoot {
                id,
                alias,
                path,
                enabled: root.enabled,
            })
        })
        .collect();

    let mut seen_root_ids = HashSet::new();
    config.roots.retain(|root| seen_root_ids.insert(root.id.clone()));

    for account in &mut config.accounts {
        account.name = if account.id == GUEST_ACCOUNT_ID {
            normalize_name(&account.name, GUEST_ACCOUNT_NAME)
        } else {
            normalize_name(&account.name, "Account")
        };
        account.permissions =
            permissions_for_preset(account.preset.clone(), account.permissions.clone());
    }

    if config.accounts.iter().all(|account| account.id != GUEST_ACCOUNT_ID) {
        config.accounts.insert(0, default_guest_account());
    }

    let mut seen_account_ids = HashSet::new();
    config
        .accounts
        .retain(|account| seen_account_ids.insert(account.id.clone()));

    if config.accounts.is_empty() {
        config.accounts.push(default_guest_account());
    }

    config
}

fn normalize_session_ttl_minutes(value: u32) -> u32 {
    if value == 0 {
        DEFAULT_SESSION_TTL_MINUTES
    } else {
        value.min(MAX_SESSION_TTL_MINUTES)
    }
}

fn account_id_key(account: &FileShareAccountSaveRequest) -> String {
    account.id.trim().to_string()
}

fn normalize_name(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_optional_secret(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn sync_file_share_auto_start_from_app_config(
    file_share_config: &mut PersistedFileShareConfig,
    app_config: &crate::config::AppConfig,
) -> bool {
    if file_share_config.auto_start_with_windows == app_config.launch_and_auto_start_file_share {
        false
    } else {
        file_share_config.auto_start_with_windows = app_config.launch_and_auto_start_file_share;
        true
    }
}

fn sync_app_config_auto_start_from_file_share(
    app_config: &mut crate::config::AppConfig,
    file_share_config: &PersistedFileShareConfig,
) -> bool {
    if app_config.launch_and_auto_start_file_share == file_share_config.auto_start_with_windows {
        false
    } else {
        app_config.launch_and_auto_start_file_share = file_share_config.auto_start_with_windows;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::*;
    use crate::config::AppConfig;
    use crate::fileshare::model::{
        DeleteMode, FileShareAccountSaveRequest, FileShareRoot, IpFilterMode,
        PermissionPreset, MAX_SESSION_TTL_MINUTES,
    };

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fst-file-share-persist-{}-{}",
                label,
                Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("test temp dir should be created");
            Self(path)
        }

        fn config_path(&self) -> PathBuf {
            self.0.join(FILE_SHARE_SETTINGS_FILE_NAME)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_settings_request() -> FileShareSettingsSaveRequest {
        FileShareSettingsSaveRequest {
            port: 9800,
            roots: vec![FileShareRoot {
                id: "root-1".to_string(),
                alias: "soft".to_string(),
                path: "D:/soft".to_string(),
                enabled: true,
            }],
            guest_access_enabled: true,
            accounts: vec![FileShareAccountSaveRequest {
                id: "guest".to_string(),
                name: "Guest".to_string(),
                enabled: true,
                preset: PermissionPreset::ReadOnly,
                permissions: FileSharePermissionSet::read_only(),
                new_password: None,
                clear_password: false,
            }],
            session_ttl_minutes: 45,
            ip_filter_mode: IpFilterMode::Off,
            ip_rules: vec!["192.168.0.0/24".to_string()],
            image_preview_enabled: true,
            thumbnail_enabled: false,
            delete_mode: DeleteMode::RecycleBin,
            remember_settings: true,
            auto_start_on_page_open: false,
            auto_start_with_windows: false,
        }
    }

    #[test]
    fn returns_default_v2_config_when_no_saved_settings_exist() {
        let tempdir = TestDir::new("defaults");
        let loaded = load_persisted_file_share_config_from_path(&tempdir.config_path()).unwrap();

        assert_eq!(loaded.port, 8080);
        assert!(loaded.roots.is_empty());
        assert!(loaded.guest_access_enabled);
        assert_eq!(loaded.session_ttl_minutes, 30);
        assert_eq!(loaded.delete_mode, DeleteMode::RecycleBin);
    }

    #[test]
    fn save_request_hashes_passwords_without_exposing_plaintext() {
        let saved = apply_save_request(
            None,
            FileShareSettingsSaveRequest {
                accounts: vec![FileShareAccountSaveRequest {
                    new_password: Some("secret-123".to_string()),
                    ..test_settings_request().accounts.into_iter().next().unwrap()
                }],
                ..test_settings_request()
            },
        )
        .unwrap();

        assert!(saved
            .accounts
            .iter()
            .any(|account| account.id == "guest" && account.password_hash.is_some()));
        assert!(saved
            .accounts
            .iter()
            .all(|account| account.password_hash.as_deref() != Some("secret-123")));
        assert!(saved
            .accounts
            .iter()
            .filter_map(|account| account.password_hash.as_deref())
            .all(|hash| hash.starts_with("$argon2")));
    }

    #[test]
    fn round_trip_save_and_load_preserves_non_secret_settings() {
        let tempdir = TestDir::new("round-trip");
        let saved = apply_save_request(None, test_settings_request()).unwrap();

        save_persisted_file_share_config_to_path(&tempdir.config_path(), &saved).unwrap();
        let loaded = load_persisted_file_share_config_from_path(&tempdir.config_path()).unwrap();

        assert_eq!(loaded.port, 9800);
        assert_eq!(loaded.roots.len(), 1);
        assert_eq!(loaded.ip_rules, vec!["192.168.0.0/24".to_string()]);
    }

    #[test]
    fn save_request_clamps_session_ttl_to_maximum() {
        let saved = apply_save_request(
            None,
            FileShareSettingsSaveRequest {
                session_ttl_minutes: u32::MAX,
                ..test_settings_request()
            },
        )
        .expect("request with oversized ttl should be normalized");

        assert_eq!(
            saved.session_ttl_minutes,
            MAX_SESSION_TTL_MINUTES
        );
    }

    #[test]
    fn load_sync_prefers_global_auto_start_flag() {
        let mut file_share = default_persisted_file_share_config();
        let mut app_config = AppConfig::default();
        file_share.auto_start_with_windows = false;
        app_config.launch_and_auto_start_file_share = true;

        let changed = sync_file_share_auto_start_from_app_config(&mut file_share, &app_config);

        assert!(changed);
        assert!(file_share.auto_start_with_windows);
    }

    #[test]
    fn save_sync_updates_global_auto_start_flag() {
        let file_share = PersistedFileShareConfig {
            auto_start_with_windows: true,
            ..default_persisted_file_share_config()
        };
        let mut app_config = AppConfig::default();

        let changed = sync_app_config_auto_start_from_file_share(&mut app_config, &file_share);

        assert!(changed);
        assert!(app_config.launch_and_auto_start_file_share);
    }
}
