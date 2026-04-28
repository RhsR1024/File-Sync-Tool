//! Pending-update validation helpers for the updater feature.

use crate::updater::PendingUpdate;
use std::path::Path;

pub fn validate(pending: Option<PendingUpdate>) -> Option<PendingUpdate> {
    let pending = pending?;
    if pending.target_file_name.trim().is_empty() {
        let _ = std::fs::remove_file(&pending.temp_path);
        return None;
    }

    let path = Path::new(&pending.temp_path);
    if !path.exists() {
        return None;
    }

    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => {
            let _ = std::fs::remove_file(path);
            return None;
        }
    };

    if !crate::updater::download::verify_bytes(&bytes, &pending.sha256) {
        let _ = std::fs::remove_file(path);
        return None;
    }

    Some(pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updater::PendingUpdate;

    fn write_temp(bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "fst-pending-{}-{}.bin",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn returns_none_when_pending_is_none() {
        assert!(validate(None).is_none());
    }

    #[test]
    fn returns_pending_when_file_exists_and_sha_matches() {
        let bytes = b"hello world";
        let path = write_temp(bytes);
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            target_file_name: "file-sync-tool-1.0.8.exe".into(),
            sha256: crate::updater::download::sha256_hex(bytes),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };

        let result = validate(Some(pending.clone()));
        assert_eq!(result, Some(pending));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn returns_none_and_deletes_when_sha_mismatches() {
        let path = write_temp(b"correct content");
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            target_file_name: "file-sync-tool-1.0.8.exe".into(),
            sha256: "deadbeef".into(),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };

        assert!(validate(Some(pending)).is_none());
        assert!(
            !path.exists(),
            "stale file with mismatched sha must be deleted"
        );
    }

    #[test]
    fn returns_none_when_temp_file_missing() {
        let path = std::env::temp_dir().join("fst-pending-nonexistent-xxxx.bin");
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            target_file_name: "file-sync-tool-1.0.8.exe".into(),
            sha256: "ab".repeat(32),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };

        assert!(validate(Some(pending)).is_none());
    }

    #[test]
    fn returns_none_and_deletes_when_target_file_name_missing() {
        let bytes = b"hello world";
        let path = write_temp(bytes);
        let pending = PendingUpdate {
            target_version: "1.0.8".into(),
            temp_path: path.to_string_lossy().into_owned(),
            target_file_name: String::new(),
            sha256: crate::updater::download::sha256_hex(bytes),
            downloaded_at: "2026-04-25T10:00:00+08:00".into(),
        };

        assert!(validate(Some(pending)).is_none());
        assert!(!path.exists());
    }
}
