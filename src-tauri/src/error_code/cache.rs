use std::fs;
use std::path::{Path, PathBuf};
use std::{collections::HashSet, io};

use serde::{Deserialize, Serialize};

use crate::error_code::parser::parse_csv_bytes;
use crate::error_code::ErrorCodeEntry;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CacheMeta {
    pub last_synced_at: Option<String>,
    pub file_count: usize,
    pub row_count: usize,
}

pub fn cache_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("errorcode_cache")
}

pub fn write_cache(dir: &Path, files: &[(String, Vec<u8>)], meta: &CacheMeta) -> io::Result<()> {
    fs::create_dir_all(dir)?;

    let new_names: HashSet<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    if let Ok(read_dir) = fs::read_dir(dir) {
        for item in read_dir.flatten() {
            let path = item.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.eq_ignore_ascii_case("meta.json") {
                continue;
            }
            if name.to_ascii_lowercase().ends_with(".csv") && !new_names.contains(name) {
                let _ = fs::remove_file(&path);
            }
        }
    }

    for (name, bytes) in files {
        fs::write(dir.join(name), bytes)?;
    }

    let meta_json = serde_json::to_vec_pretty(meta).map_err(io::Error::other)?;
    fs::write(dir.join("meta.json"), meta_json)?;
    Ok(())
}

pub fn load_cache_entries(dir: &Path) -> Vec<ErrorCodeEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = fs::read_dir(dir) else {
        return entries;
    };

    for item in read_dir.flatten() {
        let path = item.path();
        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".csv") {
            continue;
        }

        match fs::read(&path) {
            Ok(bytes) => entries.extend(parse_csv_bytes(&bytes, name)),
            Err(error) => {
                log::warn!(
                    "[error_code] failed to read cached CSV {}: {}",
                    path.display(),
                    error
                );
            }
        }
    }

    entries
}

pub fn read_meta(dir: &Path) -> Option<CacheMeta> {
    let path = dir.join("meta.json");
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn has_cache(dir: &Path) -> bool {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return false;
    };

    for item in read_dir.flatten() {
        if item
            .file_name()
            .to_str()
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".csv"))
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        let files = vec![
            (
                "10w.csv".to_string(),
                b"code,cn,en,solution,module,remark\n0,A,A,,,".to_vec(),
            ),
            (
                "20w.csv".to_string(),
                b"code,cn,en,solution,module,remark\n200,B,B,,,".to_vec(),
            ),
        ];
        let meta = CacheMeta {
            last_synced_at: Some("2026-04-25T10:00:00+08:00".to_string()),
            file_count: 2,
            row_count: 2,
        };

        write_cache(&dir, &files, &meta).unwrap();

        assert!(dir.join("10w.csv").exists());
        assert!(dir.join("20w.csv").exists());
        assert!(dir.join("meta.json").exists());

        let entries = load_cache_entries(&dir);
        assert_eq!(entries.len(), 2);

        let read_meta = read_meta(&dir).unwrap();
        assert_eq!(read_meta.file_count, 2);
        assert_eq!(read_meta.row_count, 2);
    }

    #[test]
    fn write_cache_sweeps_orphan_csvs_only() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("legacy.csv"),
            b"code,cn,en,solution,module,remark\n1,X,X,,,",
        )
        .unwrap();
        fs::write(dir.join("README.txt"), b"keep me").unwrap();

        let files = vec![(
            "10w.csv".to_string(),
            b"code,cn,en,solution,module,remark\n0,A,A,,,".to_vec(),
        )];
        let meta = CacheMeta::default();

        write_cache(&dir, &files, &meta).unwrap();

        assert!(dir.join("10w.csv").exists());
        assert!(!dir.join("legacy.csv").exists());
        assert!(dir.join("README.txt").exists());
    }

    #[test]
    fn read_meta_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        fs::create_dir_all(&dir).unwrap();
        assert!(read_meta(&dir).is_none());
    }

    #[test]
    fn load_cache_entries_returns_empty_for_missing_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = cache_dir(tmp.path());
        assert!(load_cache_entries(&dir).is_empty());
    }
}
