use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::clipboard::db;
use crate::clipboard::models::{ClipboardGroup, ContentKind};

const ARCHIVE_DB_NAME: &str = "clipboard.db";
const ARCHIVE_IMAGES_DIR: &str = "images";
const ARCHIVE_ICONS_DIR: &str = "icons";

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    Replace,
    Merge,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportReport {
    pub imported_items: usize,
    pub imported_groups: usize,
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportedItem {
    kind: ContentKind,
    content_preview: String,
    content_full: Option<String>,
    rtf_content: Option<String>,
    html: Option<String>,
    image_path: Option<String>,
    image_width: Option<u32>,
    image_height: Option<u32>,
    file_paths: Option<Vec<String>>,
    byte_size: i64,
    char_count: i64,
    hash: String,
    source_app: Option<String>,
    source_app_icon: Option<String>,
    group_id: Option<i64>,
    is_favorite: bool,
    is_pinned: bool,
    favorite_sort_index: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

struct ExtractedBundle {
    root_dir: PathBuf,
    db_path: PathBuf,
    image_dir: PathBuf,
    icon_dir: PathBuf,
}

impl Drop for ExtractedBundle {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root_dir);
    }
}

pub fn build_backup_path(db_path: &Path, timestamp_ms: i64) -> PathBuf {
    let file_name = db_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| ARCHIVE_DB_NAME.to_string());
    db_path.with_file_name(format!("{file_name}.bak.{timestamp_ms}"))
}

pub fn export_bundle(
    db_path: &Path,
    image_dir: &Path,
    icon_dir: &Path,
    archive_path: &Path,
    include_assets: bool,
) -> Result<(), String> {
    if !db_path.is_file() {
        return Err(format!("clipboard database not found: {}", db_path.display()));
    }

    let checkpoint_conn =
        Connection::open(db_path).map_err(|e| format!("open db for export checkpoint: {e}"))?;
    let _ = checkpoint_conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create export dir: {e}"))?;
    }

    let file = fs::File::create(archive_path).map_err(|e| format!("create archive: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    add_file_to_zip(&mut zip, db_path, ARCHIVE_DB_NAME, options)?;

    if include_assets {
        add_directory_files_to_zip(&mut zip, image_dir, ARCHIVE_IMAGES_DIR, options)?;
        add_directory_files_to_zip(&mut zip, icon_dir, ARCHIVE_ICONS_DIR, options)?;
    }

    zip.finish()
        .map_err(|e| format!("finalize export archive: {e}"))?;
    Ok(())
}

pub fn import_bundle(
    conn: &Connection,
    db_path: &Path,
    image_dir: &Path,
    icon_dir: &Path,
    archive_path: &Path,
    mode: ImportMode,
) -> Result<ImportReport, String> {
    let extracted = extract_bundle(archive_path)?;
    let import_conn =
        db::open_read(&extracted.db_path).map_err(|e| format!("open imported db: {e}"))?;
    let imported_groups = db::list_groups(&import_conn).map_err(|e| e.to_string())?;
    let imported_items = load_imported_items(&import_conn)?;

    let backup_path = create_backup(db_path, conn)?;

    fs::create_dir_all(image_dir).map_err(|e| format!("create image dir: {e}"))?;
    fs::create_dir_all(icon_dir).map_err(|e| format!("create icon dir: {e}"))?;

    run_in_transaction(conn, || {
        if matches!(mode, ImportMode::Replace) {
            conn.execute("DELETE FROM clipboard_items", [])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM clipboard_groups", [])
                .map_err(|e| e.to_string())?;
        }

        let (group_map, imported_group_count) = import_groups(conn, &imported_groups, mode)?;
        let mut imported_item_count = 0usize;
        for item in &imported_items {
            if insert_imported_item(
                conn,
                item,
                &group_map,
                &extracted.image_dir,
                &extracted.icon_dir,
                image_dir,
                icon_dir,
                matches!(mode, ImportMode::Merge),
            )? {
                imported_item_count += 1;
            }
        }

        Ok(ImportReport {
            imported_items: imported_item_count,
            imported_groups: imported_group_count,
            backup_path: backup_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
        })
    })
}

fn add_file_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    source_path: &Path,
    archive_name: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let mut file = fs::File::open(source_path)
        .map_err(|e| format!("open {} for export: {e}", source_path.display()))?;
    zip.start_file(archive_name, options)
        .map_err(|e| format!("start ZIP entry {archive_name}: {e}"))?;
    std::io::copy(&mut file, zip)
        .map_err(|e| format!("write {} into archive: {e}", source_path.display()))?;
    Ok(())
}

fn add_directory_files_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    source_dir: &Path,
    archive_prefix: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    if !source_dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(source_dir)
        .map_err(|e| format!("read {}: {e}", source_dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            add_directory_files_to_zip(
                zip,
                &path,
                &format!(
                    "{archive_prefix}/{}",
                    entry.file_name().to_string_lossy()
                ),
                options,
            )?;
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let archive_name = format!(
            "{archive_prefix}/{}",
            entry.file_name().to_string_lossy().replace('\\', "/")
        );
        add_file_to_zip(zip, &path, &archive_name, options)?;
    }

    Ok(())
}

fn extract_bundle(archive_path: &Path) -> Result<ExtractedBundle, String> {
    let file = fs::File::open(archive_path)
        .map_err(|e| format!("open import archive {}: {e}", archive_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("read import archive: {e}"))?;

    let root_dir = std::env::temp_dir().join(format!(
        "fst-clipboard-import-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root_dir).map_err(|e| format!("create temp import dir: {e}"))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("read archive entry #{index}: {e}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "archive contains an unsafe path".to_string())?
            .to_path_buf();
        let output_path = root_dir.join(relative);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|e| format!("create extracted dir {}: {e}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create extracted parent {}: {e}", parent.display()))?;
        }

        let mut output = fs::File::create(&output_path)
            .map_err(|e| format!("create extracted file {}: {e}", output_path.display()))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|e| format!("extract {}: {e}", output_path.display()))?;
    }

    let db_path = root_dir.join(ARCHIVE_DB_NAME);
    if !db_path.is_file() {
        return Err("import archive is missing clipboard.db".to_string());
    }
    let image_dir = root_dir.join(ARCHIVE_IMAGES_DIR);
    let icon_dir = root_dir.join(ARCHIVE_ICONS_DIR);

    Ok(ExtractedBundle {
        root_dir,
        db_path,
        image_dir,
        icon_dir,
    })
}

fn load_imported_items(conn: &Connection) -> Result<Vec<ImportedItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, content_preview, content_full, rtf_content, html, image_path, image_width,
                    image_height, file_paths_json, byte_size, char_count, hash, source_app,
                    source_app_icon, group_id, is_favorite, is_pinned, favorite_sort_index,
                    created_at, updated_at
             FROM clipboard_items
             ORDER BY created_at ASC, id ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let file_paths_json: Option<String> = row.get(8)?;
            Ok(ImportedItem {
                kind: ContentKind::from_sql(&row.get::<_, String>(0)?),
                content_preview: row.get(1)?,
                content_full: row.get(2)?,
                rtf_content: row.get(3)?,
                html: row.get(4)?,
                image_path: row.get(5)?,
                image_width: row.get(6)?,
                image_height: row.get(7)?,
                file_paths: file_paths_json
                    .as_deref()
                    .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok()),
                byte_size: row.get(9)?,
                char_count: row.get(10)?,
                hash: row.get(11)?,
                source_app: row.get(12)?,
                source_app_icon: row.get(13)?,
                group_id: row.get(14)?,
                is_favorite: row.get(15)?,
                is_pinned: row.get(16)?,
                favorite_sort_index: row.get(17)?,
                created_at: row.get(18)?,
                updated_at: row.get(19)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn import_groups(
    conn: &Connection,
    imported_groups: &[ClipboardGroup],
    mode: ImportMode,
) -> Result<(HashMap<i64, i64>, usize), String> {
    let mut group_map = HashMap::new();
    let mut imported_count = 0usize;

    if imported_groups.is_empty() {
        return Ok((group_map, imported_count));
    }

    if matches!(mode, ImportMode::Replace) {
        for group in imported_groups {
            conn.execute(
                "INSERT INTO clipboard_groups (name, sort_index, created_at) VALUES (?1, ?2, ?3)",
                params![group.name, group.sort_index, group.created_at],
            )
            .map_err(|e| e.to_string())?;
            group_map.insert(group.id, conn.last_insert_rowid());
            imported_count += 1;
        }
        return Ok((group_map, imported_count));
    }

    let existing_groups = db::list_groups(conn).map_err(|e| e.to_string())?;
    let mut existing_by_name = existing_groups
        .into_iter()
        .map(|group| (group.name.to_lowercase(), group.id))
        .collect::<HashMap<_, _>>();

    for group in imported_groups {
        let key = group.name.to_lowercase();
        let target_id = if let Some(existing_id) = existing_by_name.get(&key) {
            *existing_id
        } else {
            conn.execute(
                "INSERT INTO clipboard_groups (name, sort_index, created_at) VALUES (?1, ?2, ?3)",
                params![group.name, group.sort_index, group.created_at],
            )
            .map_err(|e| e.to_string())?;
            let new_id = conn.last_insert_rowid();
            existing_by_name.insert(key, new_id);
            imported_count += 1;
            new_id
        };
        group_map.insert(group.id, target_id);
    }

    Ok((group_map, imported_count))
}

fn insert_imported_item(
    conn: &Connection,
    item: &ImportedItem,
    group_map: &HashMap<i64, i64>,
    extracted_image_dir: &Path,
    extracted_icon_dir: &Path,
    target_image_dir: &Path,
    target_icon_dir: &Path,
    skip_duplicate_hash: bool,
) -> Result<bool, String> {
    if skip_duplicate_hash && db::item_exists_by_hash(conn, &item.hash).map_err(|e| e.to_string())? {
        return Ok(false);
    }

    let image_path = import_asset_path(&item.image_path, extracted_image_dir, target_image_dir)?;
    let icon_path = import_asset_path(&item.source_app_icon, extracted_icon_dir, target_icon_dir)?;
    let file_paths_json = item
        .file_paths
        .as_ref()
        .map(|paths| serde_json::to_string(paths).unwrap_or_default());
    let group_id = item.group_id.and_then(|value| group_map.get(&value).copied());

    conn.execute(
        "INSERT INTO clipboard_items
          (kind, content_preview, content_full, rtf_content, html, image_path, image_width, image_height,
           file_paths_json, byte_size, char_count, hash, source_app, source_app_icon, group_id,
           is_favorite, is_pinned, favorite_sort_index, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        params![
            item.kind.as_sql(),
            item.content_preview,
            item.content_full,
            item.rtf_content,
            item.html,
            image_path,
            item.image_width,
            item.image_height,
            file_paths_json,
            item.byte_size,
            item.char_count,
            item.hash,
            item.source_app,
            icon_path,
            group_id,
            item.is_favorite,
            item.is_pinned,
            item.favorite_sort_index,
            item.created_at,
            item.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(true)
}

fn import_asset_path(
    original_path: &Option<String>,
    extracted_dir: &Path,
    target_dir: &Path,
) -> Result<Option<String>, String> {
    let Some(original_path) = original_path.as_deref() else {
        return Ok(None);
    };

    let Some(file_name) = Path::new(original_path).file_name() else {
        return Ok(None);
    };

    let source_path = extracted_dir.join(file_name);
    if !source_path.is_file() {
        return Ok(None);
    }

    fs::create_dir_all(target_dir).map_err(|e| format!("create asset dir: {e}"))?;
    let destination_path = target_dir.join(file_name);
    if !destination_path.exists() {
        fs::copy(&source_path, &destination_path).map_err(|e| {
            format!(
                "copy asset {} -> {}: {e}",
                source_path.display(),
                destination_path.display()
            )
        })?;
    }

    Ok(Some(destination_path.to_string_lossy().to_string()))
}

fn create_backup(db_path: &Path, conn: &Connection) -> Result<Option<PathBuf>, String> {
    if !db_path.exists() {
        return Ok(None);
    }

    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    let timestamp_ms = chrono::Utc::now().timestamp_millis();
    let backup_path = build_backup_path(db_path, timestamp_ms);
    fs::copy(db_path, &backup_path).map_err(|e| {
        format!(
            "create backup {} -> {}: {e}",
            db_path.display(),
            backup_path.display()
        )
    })?;
    Ok(Some(backup_path))
}

fn run_in_transaction<T>(
    conn: &Connection,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| e.to_string())?;

    match operation() {
        Ok(value) => {
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            Ok(value)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{build_backup_path, export_bundle, import_bundle, ImportMode};
    use crate::clipboard::db::{self, NewItem};
    use crate::clipboard::models::{ClipboardFilter, ClipboardListQuery, ContentKind};

    fn sample_text_item(hash: &str) -> NewItem {
        NewItem {
            kind: ContentKind::Text,
            content_preview: hash.into(),
            content_full: Some(hash.into()),
            rtf_content: None,
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: hash.len() as i64,
            hash: hash.into(),
            source_app: Some("Notepad".into()),
            source_app_icon: None,
        }
    }

    fn list_all_items(conn: &Connection) -> Vec<crate::clipboard::models::ClipboardItem> {
        db::list_items(
            conn,
            &ClipboardListQuery {
                filter: ClipboardFilter::All,
                search: String::new(),
                search_payload: None,
                group_id: None,
                pinned_only: false,
                op_type: None,
                op_from_ms: None,
                op_to_ms: None,
                op_app: None,
                op_fav_only: false,
                op_size_gt: None,
                op_size_lt: None,
                offset: 0,
                limit: 50,
            },
        )
        .unwrap()
        .items
    }

    #[test]
    fn replace_import_restores_exported_rows_and_creates_backup() {
        let export_dir = TempDir::new().unwrap();
        let export_db_path = export_dir.path().join("clipboard.db");
        let export_conn = db::open(&export_db_path).unwrap();
        db::insert_item(&export_conn, &sample_text_item("export-row")).unwrap();

        let archive_path = export_dir.path().join("clipboard-export.zip");
        export_bundle(
            &export_db_path,
            &export_dir.path().join("clipboard_images"),
            &export_dir.path().join("clipboard_icons"),
            &archive_path,
            true,
        )
        .unwrap();

        let import_dir = TempDir::new().unwrap();
        let import_db_path = import_dir.path().join("clipboard.db");
        let import_conn = db::open(&import_db_path).unwrap();
        db::insert_item(&import_conn, &sample_text_item("old-row")).unwrap();

        let report = import_bundle(
            &import_conn,
            &import_db_path,
            &import_dir.path().join("clipboard_images"),
            &import_dir.path().join("clipboard_icons"),
            &archive_path,
            ImportMode::Replace,
        )
        .unwrap();

        let items = list_all_items(&import_conn);

        assert_eq!(report.imported_items, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].hash, "export-row");
        assert!(report.backup_path.is_some());
        assert!(Path::new(report.backup_path.as_deref().unwrap()).exists());
    }

    #[test]
    fn merge_import_keeps_existing_rows_and_skips_duplicate_hashes() {
        let export_dir = TempDir::new().unwrap();
        let export_db_path = export_dir.path().join("clipboard.db");
        let export_conn = db::open(&export_db_path).unwrap();
        db::insert_item(&export_conn, &sample_text_item("duplicate-row")).unwrap();
        db::insert_item(&export_conn, &sample_text_item("merged-row")).unwrap();

        let archive_path = export_dir.path().join("clipboard-export.zip");
        export_bundle(
            &export_db_path,
            &export_dir.path().join("clipboard_images"),
            &export_dir.path().join("clipboard_icons"),
            &archive_path,
            false,
        )
        .unwrap();

        let import_dir = TempDir::new().unwrap();
        let import_db_path = import_dir.path().join("clipboard.db");
        let import_conn = db::open(&import_db_path).unwrap();
        db::insert_item(&import_conn, &sample_text_item("duplicate-row")).unwrap();
        db::insert_item(&import_conn, &sample_text_item("existing-row")).unwrap();

        let report = import_bundle(
            &import_conn,
            &import_db_path,
            &import_dir.path().join("clipboard_images"),
            &import_dir.path().join("clipboard_icons"),
            &archive_path,
            ImportMode::Merge,
        )
        .unwrap();

        let items = list_all_items(&import_conn);
        let hashes = items.iter().map(|item| item.hash.as_str()).collect::<Vec<_>>();
        assert_eq!(report.imported_items, 1);
        assert_eq!(items.len(), 3);
        assert!(hashes.contains(&"existing-row"));
        assert!(hashes.contains(&"duplicate-row"));
        assert!(hashes.contains(&"merged-row"));
    }

    #[test]
    fn build_backup_path_uses_timestamped_suffix() {
        let backup_path = build_backup_path(Path::new(r"C:\temp\clipboard.db"), 1_713_200_000_000);
        assert_eq!(
            backup_path,
            Path::new(r"C:\temp\clipboard.db.bak.1713200000000"),
        );
    }
}
