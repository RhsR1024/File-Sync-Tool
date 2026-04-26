use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use chrono::Local;

use crate::error_code::{
    cache::{self, CacheMeta},
    gitlab::{self, SyncError},
    parser, ErrorCodeEntry, ErrorCodeStore, SyncReport,
};

pub async fn run_sync(
    cache_root: &Path,
    store: &Mutex<ErrorCodeStore>,
) -> Result<SyncReport, SyncError> {
    let bytes = gitlab::fetch_archive().await?;

    let files = extract_csvs_from_zip(&bytes)?;
    if files.is_empty() {
        return Err(SyncError::Archive("no_csv_in_archive".to_string()));
    }

    let mut all_entries: Vec<ErrorCodeEntry> = Vec::new();
    for (name, raw) in &files {
        let parsed = parser::parse_csv_bytes(raw, name);
        log::info!("[error_code] parsed {} -> {} rows", name, parsed.len());
        all_entries.extend(parsed);
    }

    let row_count = all_entries.len();
    let file_count = files.len();
    let synced_at = Local::now().to_rfc3339();
    let meta = CacheMeta {
        last_synced_at: Some(synced_at.clone()),
        file_count,
        row_count,
    };

    let dir = cache::cache_dir(cache_root);
    cache::write_cache(&dir, &files, &meta).map_err(|error| SyncError::Io(error.to_string()))?;

    {
        let mut guard = store
            .lock()
            .map_err(|_| SyncError::Io("store_poisoned".to_string()))?;
        guard.ingest(all_entries);
        guard.last_synced_at = Some(synced_at.clone());
        guard.loaded = true;
    }

    log::info!(
        "[error_code] sync complete: {} file(s) / {} row(s) @ {}",
        file_count,
        row_count,
        synced_at
    );

    Ok(SyncReport {
        file_count,
        row_count,
        last_synced_at: synced_at,
    })
}

pub fn ensure_loaded(cache_root: &Path, store: &Mutex<ErrorCodeStore>) -> Result<(), String> {
    {
        let guard = store.lock().map_err(|_| "store_poisoned".to_string())?;
        if guard.loaded {
            return Ok(());
        }
    }

    let dir = cache::cache_dir(cache_root);
    let entries = cache::load_cache_entries(&dir);
    let last_synced_at = cache::read_meta(&dir).and_then(|meta| meta.last_synced_at);

    let mut guard = store.lock().map_err(|_| "store_poisoned".to_string())?;
    if !guard.loaded {
        guard.ingest(entries);
        guard.last_synced_at = last_synced_at;
        guard.loaded = true;
    }
    Ok(())
}

fn extract_csvs_from_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|error| SyncError::Archive(error.to_string()))?;
    let mut output = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| SyncError::Archive(error.to_string()))?;
        if !file.is_file() {
            continue;
        }

        let raw_name = file.name().to_string();
        let Some(basename) = std::path::Path::new(&raw_name)
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !basename.to_ascii_lowercase().ends_with(".csv") {
            continue;
        }

        let mut buffer = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buffer)
            .map_err(|error| SyncError::Archive(error.to_string()))?;
        output.push((basename, buffer));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn build_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buffer);
            let mut writer = zip::ZipWriter::new(cursor);
            let options: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            for (name, data) in files {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }

        buffer
    }

    #[test]
    fn extract_csvs_filters_non_csv_and_strips_directory_prefix() {
        let zip_bytes = build_zip(&[
            (
                "errorcode-main-abc/10w.csv",
                b"code,cn,en,solution,module,remark\n0,A,A,,,",
            ),
            ("errorcode-main-abc/README.md", b"# readme"),
            (
                "errorcode-main-abc/sub/20w.csv",
                b"code,cn,en,solution,module,remark\n200,B,B,,,",
            ),
        ]);

        let result = extract_csvs_from_zip(&zip_bytes).unwrap();
        let names: Vec<&str> = result.iter().map(|(name, _)| name.as_str()).collect();

        assert!(names.contains(&"10w.csv"));
        assert!(names.contains(&"20w.csv"));
        assert!(!names.iter().any(|name| name.contains("README")));
    }
}
