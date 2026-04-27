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
    emit_log: &mut impl FnMut(&str, String),
) -> Result<SyncReport, SyncError> {
    emit_log("info", "Start syncing error code dictionary".to_string());
    let (files, source_url) = gitlab::fetch_archive_with_logger(emit_log).await?;
    emit_log("info", format!("Error code sync source: {source_url}"));

    if files.is_empty() {
        emit_log("error", "No CSV files found in repository".to_string());
        return Err(SyncError::Archive("no_csv_in_repository".to_string()));
    }
    emit_log("info", format!("Collected {} CSV files", files.len()));

    let mut all_entries: Vec<ErrorCodeEntry> = Vec::new();
    for (name, raw) in &files {
        let parsed = parser::parse_csv_bytes(raw, name);
        log::info!("[error_code] parsed {} -> {} rows", name, parsed.len());
        emit_log("info", format!("Parsed {name} -> {} rows", parsed.len()));
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
    emit_log(
        "success",
        format!(
            "Error code sync complete: {} file(s), {} row(s), at {}",
            file_count, row_count, synced_at
        ),
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
