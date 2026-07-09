//! Resolve the effective `update_server_url` with pure filesystem logic,
//! mirroring `config::get_config_path` (pivot.json custom_data_dir aware)
//! because no Tauri AppHandle exists during bootstrap.

use std::path::{Path, PathBuf};

pub fn resolve_update_server_url() -> Result<String, String> {
    let root = crate::default_app_data_dir()
        .ok_or_else(|| "Cannot resolve %APPDATA% for startup config".to_string())?;
    resolve_from_root(&root)
}

fn resolve_from_root(root: &Path) -> Result<String, String> {
    let raw = read_update_server_url(&effective_config_path(root))
        .unwrap_or_else(crate::config::default_update_server_url);
    let normalized = crate::config::normalize_update_server_url(&raw);
    if normalized.is_empty() {
        return Err(
            "Update server URL is not configured. Please contact your administrator.".to_string(),
        );
    }
    crate::config::validate_update_server_url(&normalized)?;
    Ok(normalized)
}

fn effective_config_path(default_root: &Path) -> PathBuf {
    let pivot_path = default_root.join("pivot.json");
    if let Ok(content) = std::fs::read_to_string(&pivot_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(dir) = value.get("custom_data_dir").and_then(|v| v.as_str()) {
                let dir = PathBuf::from(dir);
                if dir.is_dir() {
                    return dir.join("config.json");
                }
            }
        }
    }
    default_root.join("config.json")
}

fn read_update_server_url(config_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(config_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    value
        .get("update_server_url")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn missing_config_falls_back_to_default_url() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_from_root(root.path()).unwrap(),
            "http://192.115.1.3:8080"
        );
    }

    #[test]
    fn reads_url_from_default_config_and_normalizes() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": " http://10.0.0.9:9000/ "}"#,
        );
        assert_eq!(
            resolve_from_root(root.path()).unwrap(),
            "http://10.0.0.9:9000"
        );
    }

    #[test]
    fn pivot_custom_data_dir_overrides_default_config() {
        let root = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": "http://default.example"}"#,
        );
        write(
            &custom.path().join("config.json"),
            r#"{"update_server_url": "http://custom.example"}"#,
        );
        let pivot = format!(
            r#"{{"custom_data_dir": {}}}"#,
            serde_json::to_string(custom.path().to_str().unwrap()).unwrap()
        );
        write(&root.path().join("pivot.json"), &pivot);
        assert_eq!(
            resolve_from_root(root.path()).unwrap(),
            "http://custom.example"
        );
    }

    #[test]
    fn pivot_to_missing_dir_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("pivot.json"),
            r#"{"custom_data_dir": "C:\\does\\not\\exist\\anywhere"}"#,
        );
        assert_eq!(
            resolve_from_root(root.path()).unwrap(),
            "http://192.115.1.3:8080"
        );
    }

    #[test]
    fn empty_url_is_error() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": "  "}"#,
        );
        assert!(resolve_from_root(root.path()).is_err());
    }

    #[test]
    fn non_http_url_is_error() {
        let root = tempfile::tempdir().unwrap();
        write(
            &root.path().join("config.json"),
            r#"{"update_server_url": "ftp://192.115.1.3"}"#,
        );
        assert!(resolve_from_root(root.path()).is_err());
    }
}
