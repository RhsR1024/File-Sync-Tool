use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use tauri::Manager;

use super::model::{
    default_guest_account, default_persisted_file_share_config, FileSharePermissionSet,
    FileShareSettingsSaveRequest, FileShareSettingsView, FileShareUserSaveRequest,
    PermissionPreset, PersistedFileShareConfig, PersistedFileShareUser, UserRootPermissions,
    DEFAULT_GUEST_USERNAME, DEFAULT_SESSION_TTL_MINUTES, FILE_SHARE_CONFIG_VERSION,
    MAX_SESSION_TTL_MINUTES,
};

const FILE_SHARE_SETTINGS_FILE_NAME: &str = "file_share_v3.json";

#[tauri::command]
pub fn file_share_load_settings(
    app_handle: tauri::AppHandle,
) -> Result<FileShareSettingsView, String> {
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

pub(super) fn get_file_share_settings_path(
    app_handle: &tauri::AppHandle,
) -> Result<PathBuf, String> {
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

    let content = fs::read_to_string(path).map_err(|e| {
        format!(
            "Failed to read file share settings {}: {}",
            path.display(),
            e
        )
    })?;
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
    let existing_guest = existing.as_ref().map(|config| config.guest_account.clone());
    let existing_accounts = existing
        .as_ref()
        .map(|config| {
            config
                .accounts
                .iter()
                .map(|account| (username_key(&account.username), account.clone()))
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

    let guest_account = build_persisted_user(request.guest_account, existing_guest)?;
    let mut seen_usernames = HashSet::from([username_key(&guest_account.username)]);
    let accounts = request
        .accounts
        .into_iter()
        .map(|account| {
            let account_username = previous_username_key(&account);
            build_persisted_user(account, existing_accounts.get(&account_username).cloned())
        })
        .collect::<Result<Vec<_>, _>>()?;

    for account in &accounts {
        if !seen_usernames.insert(username_key(&account.username)) {
            return Err(format!(
                "Duplicate file share username: {}",
                account.username
            ));
        }
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
            guest_account,
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

fn build_persisted_user(
    account: FileShareUserSaveRequest,
    existing: Option<PersistedFileShareUser>,
) -> Result<PersistedFileShareUser, String> {
    let username = normalize_required_username(&account.username)?;
    if username.is_empty() {
        return Err("File share username is required".to_string());
    }

    let existing = existing.map(normalize_persisted_file_share_user_password);
    let previous_hash = existing
        .as_ref()
        .and_then(|value| value.password_hash.clone());
    let previous_plain = existing.and_then(|value| value.password_plain);
    let new_password = normalize_optional_secret(account.new_password.as_deref());
    let (password_hash, password_plain) = if account.clear_password {
        (None, None)
    } else if let Some(new_password) = new_password {
        (
            Some(super::hash_password(&new_password)),
            Some(new_password),
        )
    } else {
        (previous_hash, previous_plain)
    };

    let root_permissions = account
        .root_permissions
        .into_iter()
        .map(normalize_user_root_permissions)
        .collect();

    Ok(PersistedFileShareUser {
        username,
        enabled: account.enabled,
        root_permissions,
        password_plain,
        password_hash,
    })
}

fn normalize_user_root_permissions(entry: UserRootPermissions) -> UserRootPermissions {
    UserRootPermissions {
        permissions: permissions_for_preset(entry.preset.clone(), entry.permissions),
        preset: entry.preset,
        root_id: entry.root_id.trim().to_string(),
    }
}

fn normalize_root_aliases(
    roots: Vec<super::model::FileShareRoot>,
) -> Vec<super::model::FileShareRoot> {
    let mut seen_aliases = HashSet::new();

    roots
        .into_iter()
        .map(|mut root| {
            let base = super::make_alias(&root.path);
            let mut alias = base.clone();
            let mut n = 2;
            while !seen_aliases.insert(alias.to_lowercase()) {
                alias = format!("{base} ({n})");
                n += 1;
            }
            root.alias = alias;
            root
        })
        .collect()
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

    config.roots = normalize_root_aliases(
        config
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
            .collect(),
    );

    let mut seen_root_ids = HashSet::new();
    config
        .roots
        .retain(|root| seen_root_ids.insert(root.id.clone()));

    let valid_root_ids: HashSet<String> = config.roots.iter().map(|root| root.id.clone()).collect();

    config.guest_account =
        normalize_persisted_user(config.guest_account, Some(DEFAULT_GUEST_USERNAME))
            .map(|user| prune_unknown_root_permissions(user, &valid_root_ids))
            .unwrap_or_else(default_guest_account);

    let mut seen_usernames = HashSet::from([username_key(&config.guest_account.username)]);
    config.accounts = config
        .accounts
        .into_iter()
        .filter_map(|account| normalize_persisted_user(account, None))
        .map(|user| prune_unknown_root_permissions(user, &valid_root_ids))
        .filter(|account| seen_usernames.insert(username_key(&account.username)))
        .collect();

    config
}

fn prune_unknown_root_permissions(
    mut user: PersistedFileShareUser,
    valid_root_ids: &HashSet<String>,
) -> PersistedFileShareUser {
    user.root_permissions
        .retain(|entry| valid_root_ids.contains(&entry.root_id));
    let mut seen = HashSet::new();
    user.root_permissions
        .retain(|entry| seen.insert(entry.root_id.clone()));
    user
}

fn normalize_session_ttl_minutes(value: u32) -> u32 {
    if value == 0 {
        DEFAULT_SESSION_TTL_MINUTES
    } else {
        value.min(MAX_SESSION_TTL_MINUTES)
    }
}

fn request_username_key(account: &FileShareUserSaveRequest) -> String {
    username_key(&account.username)
}

fn previous_username_key(account: &FileShareUserSaveRequest) -> String {
    account
        .previous_username
        .as_deref()
        .map(username_key)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request_username_key(account))
}

fn normalize_required_username(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err("File share username is required".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn normalize_optional_secret(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_persisted_user(
    mut user: PersistedFileShareUser,
    fallback_username: Option<&str>,
) -> Option<PersistedFileShareUser> {
    let trimmed_username = user.username.trim();
    user.username = if trimmed_username.is_empty() {
        fallback_username?.to_string()
    } else {
        trimmed_username.to_string()
    };
    user.root_permissions = user
        .root_permissions
        .into_iter()
        .map(normalize_user_root_permissions)
        .filter(|entry| !entry.root_id.is_empty())
        .collect();
    Some(normalize_persisted_file_share_user_password(user))
}

fn normalize_persisted_file_share_user_password(
    mut user: PersistedFileShareUser,
) -> PersistedFileShareUser {
    user.password_plain = normalize_optional_secret(user.password_plain.as_deref());
    if user.password_hash.is_none() {
        if let Some(password_plain) = user.password_plain.as_deref() {
            user.password_hash = Some(super::hash_password(password_plain));
        }
    }
    user
}

fn username_key(username: &str) -> String {
    username.trim().to_lowercase()
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

    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::config::AppConfig;
    use crate::fileshare::model::{
        FileShareSettingsSaveRequest, FileShareSettingsView, MAX_SESSION_TTL_MINUTES,
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

    fn test_settings_request_value() -> serde_json::Value {
        json!({
            "port": 9800,
            "roots": [
                {
                    "id": "root-1",
                    "alias": "soft",
                    "path": "D:/soft",
                    "enabled": true
                }
            ],
            "guest_access_enabled": true,
            "guest_account": {
                "username": "visitor",
                "enabled": true,
                "root_permissions": [
                    {
                        "root_id": "root-1",
                        "preset": "read_only",
                        "permissions": FileSharePermissionSet::read_only()
                    }
                ],
                "new_password": null,
                "clear_password": false
            },
            "accounts": [
                {
                    "username": "operator",
                    "enabled": true,
                    "root_permissions": [
                        {
                            "root_id": "root-1",
                            "preset": "read_only",
                            "permissions": FileSharePermissionSet::read_only()
                        }
                    ],
                    "new_password": null,
                    "clear_password": false
                }
            ],
            "session_ttl_minutes": 45,
            "ip_filter_mode": "off",
            "ip_rules": ["192.168.0.0/24"],
            "image_preview_enabled": true,
            "thumbnail_enabled": false,
            "delete_mode": "recycle_bin",
            "remember_settings": true,
            "auto_start_on_page_open": false,
            "auto_start_with_windows": false
        })
    }

    fn test_settings_request() -> FileShareSettingsSaveRequest {
        serde_json::from_value(test_settings_request_value())
            .expect("single-username request should deserialize")
    }

    #[test]
    fn returns_default_v3_config_when_no_saved_settings_exist() {
        let tempdir = TestDir::new("defaults");
        let loaded = load_persisted_file_share_config_from_path(&tempdir.config_path()).unwrap();
        let serialized = serde_json::to_value(loaded).expect("defaults should serialize");

        assert_eq!(serialized["version"], 3);
        assert_eq!(serialized["port"], 8080);
        assert_eq!(serialized["guest_access_enabled"], true);
        assert_eq!(serialized["accounts"].as_array().map(Vec::len), Some(0));
        assert!(!serialized["guest_account"]["username"]
            .as_str()
            .unwrap_or_default()
            .is_empty());
        assert_eq!(serialized["session_ttl_minutes"], 30);
        assert_eq!(serialized["delete_mode"], "recycle_bin");
    }

    #[test]
    fn save_request_hashes_and_retains_plaintext_for_settings_ui() {
        let mut request_value = test_settings_request_value();
        request_value["guest_account"]["new_password"] = json!("secret-123");

        let saved = apply_save_request(
            None,
            serde_json::from_value(request_value)
                .expect("single-username request with password should deserialize"),
        )
        .unwrap();
        let serialized = serde_json::to_value(&saved).expect("saved config should serialize");
        let password_hash = serialized["guest_account"]["password_hash"]
            .as_str()
            .expect("guest password hash should be present");

        assert_ne!(password_hash, "secret-123");
        assert!(password_hash.starts_with("$argon2"));
        assert_eq!(serialized["guest_account"]["password_plain"], "secret-123");

        let view = FileShareSettingsView::from(&saved);
        assert_eq!(
            view.guest_account.password_plain.as_deref(),
            Some("secret-123")
        );
    }

    #[test]
    fn save_request_preserves_password_hash_when_username_changes_with_previous_username() {
        let mut initial_request_value = test_settings_request_value();
        initial_request_value["accounts"][0]["new_password"] = json!("secret-123");
        let initial_request: FileShareSettingsSaveRequest =
            serde_json::from_value(initial_request_value)
                .expect("initial request should deserialize");
        let saved = apply_save_request(None, initial_request).expect("initial request should save");
        let original_hash = saved.accounts[0]
            .password_hash
            .clone()
            .expect("initial account password hash should exist");

        let mut rename_request_value = test_settings_request_value();
        rename_request_value["accounts"][0]["username"] = json!("operator-renamed");
        rename_request_value["accounts"][0]["previous_username"] = json!("operator");
        let rename_request: FileShareSettingsSaveRequest =
            serde_json::from_value(rename_request_value)
                .expect("rename request should deserialize");
        let renamed = apply_save_request(Some(saved), rename_request)
            .expect("rename request should keep the existing password hash");

        assert_eq!(renamed.accounts[0].username, "operator-renamed");
        assert_eq!(
            renamed.accounts[0].password_hash.as_deref(),
            Some(original_hash.as_str())
        );
    }

    #[test]
    fn round_trip_save_and_load_preserves_non_secret_settings() {
        let tempdir = TestDir::new("round-trip");
        let saved = apply_save_request(None, test_settings_request()).unwrap();

        save_persisted_file_share_config_to_path(&tempdir.config_path(), &saved).unwrap();
        let loaded = load_persisted_file_share_config_from_path(&tempdir.config_path()).unwrap();
        let serialized = serde_json::to_value(loaded).expect("loaded config should serialize");

        assert_eq!(serialized["port"], 9800);
        assert_eq!(serialized["guest_account"]["username"], "visitor");
        assert_eq!(serialized["accounts"][0]["username"], "operator");
        assert_eq!(serialized["ip_rules"], json!(["192.168.0.0/24"]));
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

        assert_eq!(saved.session_ttl_minutes, MAX_SESSION_TTL_MINUTES);
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

    #[test]
    fn save_request_rejects_duplicate_usernames_across_guest_and_accounts() {
        let mut request_value = test_settings_request_value();
        request_value["accounts"][0]["username"] = json!("visitor");
        let request: FileShareSettingsSaveRequest = serde_json::from_value(request_value)
            .expect("single-username request should deserialize");

        let error = apply_save_request(None, request)
            .expect_err("guest and custom accounts should not share the same username");

        assert!(error.contains("Duplicate file share username"));
    }

    #[test]
    fn load_normalizes_root_aliases_from_directory_names() {
        let tempdir = TestDir::new("root-alias-normalize");
        let saved = PersistedFileShareConfig {
            roots: vec![crate::fileshare::model::FileShareRoot {
                id: "root-1".to_string(),
                alias: "1-3-9-p10".to_string(),
                path: r"E:\UMS_TEMP\1.3.9.P10".to_string(),
                enabled: true,
            }],
            ..default_persisted_file_share_config()
        };

        save_persisted_file_share_config_to_path(&tempdir.config_path(), &saved)
            .expect("old config should be writable");
        let loaded = load_persisted_file_share_config_from_path(&tempdir.config_path())
            .expect("saved config should load");

        assert_eq!(loaded.roots[0].alias, "1.3.9.P10");
    }
}
