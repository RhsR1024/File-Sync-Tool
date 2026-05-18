//! Migration self-heal for installs whose on-disk filename does not match the
//! running binary's version. This covers users upgrading from 1.1.0 (whose
//! installer batch always renamed the new exe to the previous file name) to
//! 1.1.1+.

use tauri::{AppHandle, Manager};

use crate::updater::{installer, manifest, Manifest, SharedUpdaterState, CURRENT_VERSION};

#[derive(Debug, PartialEq, Eq)]
pub enum HealOutcome {
    Healthy,
    NoManifestEntry,
    TargetExists,
    Skipped(&'static str),
    Spawned,
}

/// Parses `file-sync-tool-<version>-<timestamp>.exe` into `(version, timestamp)`.
/// Returns `None` for any other shape, including user-renamed binaries.
pub fn parse_versioned_exe_filename(name: &str) -> Option<(String, String)> {
    let stripped = name.strip_prefix("file-sync-tool-")?;
    let stripped = stripped.strip_suffix(".exe")?;
    let (version, timestamp) = stripped.rsplit_once('-')?;
    if timestamp.len() != 12 || !timestamp.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3
        || !parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some((version.to_string(), timestamp.to_string()))
}

/// Returns the canonical exe filename declared by the manifest for `version`,
/// or `None` if the manifest has no entry for that version.
pub fn canonical_filename_for_version(manifest: &Manifest, version: &str) -> Option<String> {
    let entry = manifest.versions.iter().find(|v| v.version == version)?;
    manifest::download_file_name_from_url(&entry.url)
}

pub fn check_and_repair_filename(
    app_handle: &AppHandle,
    updater_state: &SharedUpdaterState,
) -> HealOutcome {
    let Ok(current_exe) = std::env::current_exe() else {
        return HealOutcome::Skipped("current_exe_failed");
    };
    let Some(current_name) = current_exe.file_name().and_then(|s| s.to_str()) else {
        return HealOutcome::Skipped("invalid_current_exe_name");
    };

    let Some((version_in_name, _)) = parse_versioned_exe_filename(current_name) else {
        return HealOutcome::Skipped("non_versioned_name");
    };

    if version_in_name == CURRENT_VERSION {
        return HealOutcome::Healthy;
    }

    let manifest = match updater_state.manifest.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return HealOutcome::Skipped("manifest_lock_poisoned"),
    };
    let Some(manifest) = manifest else {
        return HealOutcome::NoManifestEntry;
    };

    let Some(canonical) = canonical_filename_for_version(&manifest, CURRENT_VERSION) else {
        return HealOutcome::NoManifestEntry;
    };

    if canonical == current_name {
        return HealOutcome::Healthy;
    }

    let Some(parent) = current_exe.parent() else {
        return HealOutcome::Skipped("no_parent_dir");
    };
    let target_path = parent.join(&canonical);
    if target_path.exists() {
        log::warn!(
            "[updater] self-heal target already exists, skipping: {}",
            target_path.display()
        );
        return HealOutcome::TargetExists;
    }

    if let Err(error) = installer::spawn_rename_helper(&current_exe, &target_path) {
        log::warn!("[updater] self-heal spawn_rename_helper failed: {error}");
        return HealOutcome::Skipped("spawn_failed");
    }

    log::info!(
        "[updater] self-heal: renaming {} -> {} after exit",
        current_exe.display(),
        target_path.display()
    );

    for window in app_handle.webview_windows().values() {
        let _ = window.close();
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    app_handle.exit(0);
    HealOutcome::Spawned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::ManifestVersion;

    #[test]
    fn parse_extracts_version_and_timestamp() {
        let (version, timestamp) =
            parse_versioned_exe_filename("file-sync-tool-1.1.1-202605181737.exe").expect("parse");
        assert_eq!(version, "1.1.1");
        assert_eq!(timestamp, "202605181737");
    }

    #[test]
    fn parse_rejects_missing_prefix() {
        assert!(parse_versioned_exe_filename("other-tool-1.1.1-202605181737.exe").is_none());
    }

    #[test]
    fn parse_rejects_missing_extension() {
        assert!(parse_versioned_exe_filename("file-sync-tool-1.1.1-202605181737").is_none());
    }

    #[test]
    fn parse_rejects_short_timestamp() {
        assert!(parse_versioned_exe_filename("file-sync-tool-1.1.1-20260518.exe").is_none());
    }

    #[test]
    fn parse_rejects_non_numeric_timestamp() {
        assert!(parse_versioned_exe_filename("file-sync-tool-1.1.1-abcdefghijkl.exe").is_none());
    }

    #[test]
    fn parse_rejects_non_semver_version() {
        assert!(parse_versioned_exe_filename("file-sync-tool-1.1-202605181737.exe").is_none());
        assert!(parse_versioned_exe_filename("file-sync-tool-1.1.a-202605181737.exe").is_none());
    }

    #[test]
    fn parse_rejects_renamed_user_binary() {
        assert!(parse_versioned_exe_filename("my-renamed.exe").is_none());
    }

    fn make_manifest(version: &str, url: &str) -> Manifest {
        Manifest {
            latest: version.to_string(),
            versions: vec![ManifestVersion {
                version: version.to_string(),
                url: url.to_string(),
                sha256: "ab".repeat(32),
                released_at: "2026-05-18".to_string(),
                changelog: vec!["x".to_string()],
            }],
        }
    }

    #[test]
    fn canonical_filename_is_extracted_from_manifest_entry() {
        let manifest = make_manifest(
            "1.1.1",
            "http://srv/releases/file-sync-tool-1.1.1-202605181737.exe",
        );
        assert_eq!(
            canonical_filename_for_version(&manifest, "1.1.1"),
            Some("file-sync-tool-1.1.1-202605181737.exe".to_string())
        );
    }

    #[test]
    fn canonical_filename_returns_none_when_version_missing() {
        let manifest = make_manifest("1.1.1", "http://srv/x.exe");
        assert!(canonical_filename_for_version(&manifest, "1.0.0").is_none());
    }
}
