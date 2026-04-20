use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use super::model::DeleteMode;
use chrono::{DateTime, Local};
use serde::Serialize;

// Byte-size cap is intentionally disabled so large directories can still be
// packaged into a ZIP archive. File count and nesting depth caps remain as a
// safety net against runaway recursion / zip-bomb-style inputs.
pub const ZIP_DOWNLOAD_MAX_BYTES: u64 = u64::MAX;
pub const ZIP_DOWNLOAD_MAX_FILES: usize = 200_000;
const ZIP_MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoot {
    pub id: String,
    pub alias: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FilePreview {
    pub path: PathBuf,
    pub file_name: String,
    pub content_type: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct ZipSourceStats {
    pub file_count: usize,
    pub total_bytes: u64,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn rename_entry(
    roots: &[ResolvedRoot],
    from_root_id: &str,
    from: &str,
    to_root_id: &str,
    to: &str,
) -> Result<(), String> {
    if from_root_id != to_root_id {
        return Err("Rename must stay within the same shared root".to_string());
    }

    let root = find_root(roots, from_root_id)?;
    rename_entry_within_root(root, from, to)
}

pub fn rename_entry_in_place(root: &ResolvedRoot, from: &str, to_name: &str) -> Result<(), String> {
    let from_path = normalize_relative_path(from)?;
    if from_path.as_os_str().is_empty() {
        return Err("Cannot rename the shared root itself".to_string());
    }

    let next_name = normalize_leaf_name(to_name)?;
    let next_relative_path = match from_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(next_name),
        _ => PathBuf::from(next_name),
    };

    rename_entry_within_root(root, from, &path_to_relative_string(&next_relative_path))
}

pub fn create_directory(root: &ResolvedRoot, parent: &str, name: &str) -> Result<(), String> {
    let parent_path = resolve_relative_path(root, parent)?;
    ensure_existing_directory(&parent_path)?;

    let target = parent_path.join(normalize_leaf_name(name)?);
    if target.exists() {
        return Err(format!("Target already exists: {}", target.display()));
    }

    fs::create_dir(&target)
        .map_err(|e| format!("Failed to create directory {}: {}", target.display(), e))
}

pub fn create_text_file(
    root: &ResolvedRoot,
    parent: &str,
    name: &str,
    content: &str,
) -> Result<(), String> {
    let parent_path = resolve_relative_path(root, parent)?;
    ensure_existing_directory(&parent_path)?;

    let target = parent_path.join(normalize_leaf_name(name)?);
    if target.exists() {
        return Err(format!("Target already exists: {}", target.display()));
    }

    fs::write(&target, content)
        .map_err(|e| format!("Failed to create text file {}: {}", target.display(), e))
}

pub fn rename_share_root(root: &ResolvedRoot, to_name: &str) -> Result<PathBuf, String> {
    let source = canonical_root_path(root)?;
    let parent = source
        .parent()
        .ok_or_else(|| format!("Shared root has no parent directory: {}", source.display()))?;
    let next_name = normalize_leaf_name(to_name)?;
    let destination = parent.join(&next_name);
    if destination.exists() {
        return Err(format!("Target already exists: {}", destination.display()));
    }

    fs::rename(&source, &destination).map_err(|e| {
        format!(
            "Failed to rename shared root {} to {}: {}",
            source.display(),
            destination.display(),
            e
        )
    })?;

    Ok(destination)
}

pub fn write_uploaded_file(
    root: &ResolvedRoot,
    parent: &str,
    relative_name: &str,
    content: &[u8],
    create_parents: bool,
) -> Result<(), String> {
    let parent_path = resolve_relative_path(root, parent)?;
    ensure_existing_directory(&parent_path)?;

    let upload_path = normalize_relative_path(relative_name)?;
    if upload_path.as_os_str().is_empty() {
        return Err("Upload file name cannot be empty".to_string());
    }

    let nested_parent = upload_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    if nested_parent.is_some() && !create_parents {
        return Err("nested relative paths are only allowed for directory uploads".to_string());
    }

    let target_parent = if let Some(relative_parent) = nested_parent {
        let target_parent = parent_path.join(relative_parent);
        fs::create_dir_all(&target_parent).map_err(|e| {
            format!(
                "Failed to create upload directory {}: {}",
                target_parent.display(),
                e
            )
        })?;
        target_parent
    } else {
        parent_path
    };

    let file_name = upload_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "Upload file name is invalid".to_string())?;
    let target = target_parent.join(file_name);
    if target.exists() {
        return Err(format!("Target already exists: {}", target.display()));
    }

    fs::write(&target, content)
        .map_err(|e| format!("Failed to write uploaded file {}: {}", target.display(), e))
}

pub fn delete_entry(root: &ResolvedRoot, path: &str, mode: DeleteMode) -> Result<(), String> {
    let target = resolve_relative_path(root, path)?;
    let root_path = canonical_root_path(root)?;
    if target == root_path {
        return Err("Cannot delete the shared root itself".to_string());
    }

    match mode {
        DeleteMode::RecycleBin => trash::delete(&target)
            .map_err(|e| format!("Failed to move {} to recycle bin: {}", target.display(), e)),
        DeleteMode::Permanent => remove_path_permanently(&target),
    }
}

pub fn delete_share_root(root: &ResolvedRoot, mode: DeleteMode) -> Result<(), String> {
    let target = canonical_root_path(root)?;
    match mode {
        DeleteMode::RecycleBin => trash::delete(&target)
            .map_err(|e| format!("Failed to move {} to recycle bin: {}", target.display(), e)),
        DeleteMode::Permanent => remove_path_permanently(&target),
    }
}

pub fn resolve_relative_path(root: &ResolvedRoot, relative_path: &str) -> Result<PathBuf, String> {
    let root_path = canonical_root_path(root)?;
    let normalized = normalize_relative_path(relative_path)?;
    let candidate = if normalized.as_os_str().is_empty() {
        root_path.clone()
    } else {
        root_path.join(normalized)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Failed to access {}: {}", candidate.display(), e))?;

    if canonical.starts_with(&root_path) {
        Ok(canonical)
    } else {
        Err("Resolved path escapes the shared root".to_string())
    }
}

pub fn list_directory(path: &Path) -> Result<Vec<DirEntry>, String> {
    let read_dir = fs::read_dir(path)
        .map_err(|e| format!("Failed to read directory {}: {}", path.display(), e))?;
    let mut entries = Vec::new();

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };
        let modified = metadata
            .modified()
            .ok()
            .map(format_modified_time)
            .unwrap_or_default();

        entries.push(DirEntry {
            relative_path: name.clone(),
            name,
            is_dir,
            size,
            modified,
        });
    }

    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then(left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(entries)
}

pub fn join_relative_path(parent: &str, child: &str) -> String {
    let left = parent.trim().trim_matches('/');
    let right = child.trim().trim_matches('/');

    if left.is_empty() {
        right.to_string()
    } else if right.is_empty() {
        left.to_string()
    } else {
        format!("{left}/{right}")
    }
}

pub fn stream_preview(root: &ResolvedRoot, path: &str) -> Result<FilePreview, String> {
    let target = resolve_relative_path(root, path)?;
    if !target.is_file() {
        return Err(format!(
            "Preview target is not a file: {}",
            target.display()
        ));
    }
    if !is_previewable_image(&target) {
        return Err("Preview is only available for image files".to_string());
    }

    Ok(FilePreview {
        file_name: target
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("preview")
            .to_string(),
        content_type: mime_guess::from_path(&target)
            .first_or_octet_stream()
            .to_string(),
        path: target,
    })
}

pub fn validate_zip_source(path: &Path) -> Result<ZipSourceStats, String> {
    validate_zip_source_with_limits(path, ZIP_DOWNLOAD_MAX_BYTES, ZIP_DOWNLOAD_MAX_FILES)
}

pub fn validate_zip_source_with_limits(
    path: &Path,
    max_total_bytes: u64,
    max_files: usize,
) -> Result<ZipSourceStats, String> {
    let mut stats = ZipSourceStats {
        file_count: 0,
        total_bytes: 0,
    };
    collect_zip_source_stats(path, 0, &mut stats, max_total_bytes, max_files)?;
    Ok(stats)
}

pub fn zip_dir<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    current: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    zip_dir_inner(zip, base, current, options, 0)
}

fn zip_dir_inner<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    current: &Path,
    options: zip::write::SimpleFileOptions,
    depth: usize,
) -> Result<(), String> {
    if depth > ZIP_MAX_DEPTH {
        return Err(format!(
            "Directory nesting too deep (>{ZIP_MAX_DEPTH} levels)"
        ));
    }

    for entry in fs::read_dir(current)
        .map_err(|e| format!("Failed to read {}: {}", current.display(), e))?
        .flatten()
    {
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to inspect {}: {}", path.display(), e))?;
        let relative = path
            .strip_prefix(base)
            .map_err(|e| format!("Failed to build ZIP entry path: {}", e))?;
        let relative = path_to_relative_string(relative);

        if metadata.is_dir() {
            zip.add_directory(&relative, options)
                .map_err(|e| format!("Failed to add directory {} to ZIP: {}", path.display(), e))?;
            zip_dir_inner(zip, base, &path, options, depth + 1)?;
        } else if metadata.is_file() {
            zip.start_file(&relative, options)
                .map_err(|e| format!("Failed to add file {} to ZIP: {}", path.display(), e))?;
            let mut file = fs::File::open(&path)
                .map_err(|e| format!("Failed to open {} for ZIP: {}", path.display(), e))?;
            std::io::copy(&mut file, zip)
                .map_err(|e| format!("Failed to write {} into ZIP: {}", path.display(), e))?;
        }
    }

    Ok(())
}

fn rename_entry_within_root(root: &ResolvedRoot, from: &str, to: &str) -> Result<(), String> {
    let source = resolve_relative_path(root, from)?;
    let root_path = canonical_root_path(root)?;
    if source == root_path {
        return Err("Cannot rename the shared root itself".to_string());
    }

    let destination = resolve_new_relative_path(root, to)?;
    fs::rename(&source, &destination).map_err(|e| {
        format!(
            "Failed to rename {} to {}: {}",
            source.display(),
            destination.display(),
            e
        )
    })
}

fn resolve_new_relative_path(root: &ResolvedRoot, relative_path: &str) -> Result<PathBuf, String> {
    let normalized = normalize_relative_path(relative_path)?;
    if normalized.as_os_str().is_empty() {
        return Err("Target path cannot be empty".to_string());
    }

    let file_name = normalized
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "Target path must end with a valid name".to_string())?;
    let parent = normalized.parent().unwrap_or_else(|| Path::new(""));
    let parent_path = resolve_relative_path(root, &path_to_relative_string(parent))?;
    ensure_existing_directory(&parent_path)?;

    let candidate = parent_path.join(file_name);
    if candidate.exists() {
        return Err(format!("Target already exists: {}", candidate.display()));
    }

    Ok(candidate)
}

fn normalize_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }

    let normalized = trimmed.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err("Absolute paths are not allowed".to_string());
    }

    let mut result = PathBuf::new();
    for segment in normalized.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            return Err("Path traversal is not allowed".to_string());
        }
        if segment.contains('\0') {
            return Err("Path contains invalid characters".to_string());
        }
        result.push(segment);
    }

    Ok(result)
}

fn normalize_leaf_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Entry name cannot be empty".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Entry name is invalid".to_string());
    }
    if trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains(':')
        || trimmed.contains('\0')
    {
        return Err("Entry name must not contain path separators".to_string());
    }
    Ok(trimmed.to_string())
}

fn path_to_relative_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg_attr(not(test), allow(dead_code))]
fn find_root<'a>(roots: &'a [ResolvedRoot], root_id: &str) -> Result<&'a ResolvedRoot, String> {
    roots
        .iter()
        .find(|root| root.id == root_id || root.alias == root_id)
        .ok_or_else(|| format!("Shared root not found: {}", root_id))
}

fn canonical_root_path(root: &ResolvedRoot) -> Result<PathBuf, String> {
    root.path.canonicalize().map_err(|e| {
        format!(
            "Failed to access shared root {}: {}",
            root.path.display(),
            e
        )
    })
}

fn ensure_existing_directory(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("Directory not found: {}", path.display()))
    }
}

fn remove_path_permanently(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("Failed to delete directory {}: {}", path.display(), e))
    } else {
        fs::remove_file(path)
            .map_err(|e| format!("Failed to delete file {}: {}", path.display(), e))
    }
}

fn is_previewable_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(OsStr::to_str)
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp")
    )
}

pub fn format_modified_time(time: std::time::SystemTime) -> String {
    let date_time: DateTime<Local> = time.into();
    date_time.format("%Y-%m-%d %H:%M").to_string()
}

fn collect_zip_source_stats(
    current: &Path,
    depth: usize,
    stats: &mut ZipSourceStats,
    max_total_bytes: u64,
    max_files: usize,
) -> Result<(), String> {
    if depth > ZIP_MAX_DEPTH {
        return Err(format!(
            "Directory nesting too deep (>{ZIP_MAX_DEPTH} levels)"
        ));
    }

    for entry in fs::read_dir(current)
        .map_err(|e| format!("Failed to read {}: {}", current.display(), e))?
        .flatten()
    {
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to inspect {}: {}", path.display(), e))?;

        if metadata.is_dir() {
            collect_zip_source_stats(&path, depth + 1, stats, max_total_bytes, max_files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        stats.file_count += 1;
        stats.total_bytes = stats.total_bytes.saturating_add(metadata.len());

        if stats.file_count > max_files {
            return Err(format!(
                "Directory contains too many files to download as ZIP (limit: {})",
                max_files
            ));
        }

        if stats.total_bytes > max_total_bytes {
            return Err(format!(
                "Directory is too large to download as ZIP (limit: {})",
                format_byte_limit(max_total_bytes)
            ));
        }
    }

    Ok(())
}

fn format_byte_limit(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{} GB", bytes / (1024 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use uuid::Uuid;

    use super::*;
    use crate::fileshare::model::DeleteMode;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fst-fileshare-ops-{}-{}",
                label,
                Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("test temp dir should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_roots() -> Vec<ResolvedRoot> {
        let root_a = TestDir::new("root-a");
        let root_b = TestDir::new("root-b");

        fs::create_dir_all(root_a.path().join("sub")).expect("source sub dir should exist");
        fs::write(root_a.path().join("sub").join("a.txt"), b"hello")
            .expect("source file should exist");
        fs::write(root_b.path().join("b.txt"), b"world").expect("target file should exist");

        vec![
            ResolvedRoot {
                id: "root-a".to_string(),
                alias: "root-a".to_string(),
                path: root_a.path().to_path_buf(),
            },
            ResolvedRoot {
                id: "root-b".to_string(),
                alias: "root-b".to_string(),
                path: root_b.path().to_path_buf(),
            },
        ]
    }

    #[test]
    fn rename_cannot_cross_share_root_boundary() {
        let roots = test_roots();
        let error = rename_entry(&roots, "root-a", "sub/a.txt", "root-b", "b.txt").unwrap_err();
        assert!(error.contains("same shared root"));
    }

    #[test]
    fn delete_entry_permanently_removes_files() {
        let dir = TestDir::new("delete");
        let root = ResolvedRoot {
            id: "root".to_string(),
            alias: "root".to_string(),
            path: dir.path().to_path_buf(),
        };
        let target = dir.path().join("remove-me.txt");
        fs::write(&target, b"delete me").expect("target file should exist");

        delete_entry(&root, "remove-me.txt", DeleteMode::Permanent).expect("delete should succeed");

        assert!(!target.exists());
    }

    #[test]
    fn stream_preview_rejects_non_image_files() {
        let dir = TestDir::new("preview");
        let root = ResolvedRoot {
            id: "root".to_string(),
            alias: "root".to_string(),
            path: dir.path().to_path_buf(),
        };
        fs::write(dir.path().join("notes.txt"), b"plain text").expect("text file should exist");

        let error = stream_preview(&root, "notes.txt").expect_err("text file should not preview");

        assert!(error.contains("image"));
    }

    #[test]
    fn file_upload_rejects_nested_relative_paths() {
        let dir = TestDir::new("upload-flat");
        let root = ResolvedRoot {
            id: "root".to_string(),
            alias: "root".to_string(),
            path: dir.path().to_path_buf(),
        };

        let error = write_uploaded_file(&root, "", "nested/readme.txt", b"hello", false)
            .expect_err("flat file uploads should not accept nested paths");

        assert!(error.contains("nested"));
    }

    #[test]
    fn directory_upload_creates_nested_parent_directories() {
        let dir = TestDir::new("upload-dir");
        let root = ResolvedRoot {
            id: "root".to_string(),
            alias: "root".to_string(),
            path: dir.path().to_path_buf(),
        };

        write_uploaded_file(&root, "", "photos/2026/april/cover.txt", b"hello", true)
            .expect("directory uploads should create nested folders");

        assert!(dir
            .path()
            .join("photos")
            .join("2026")
            .join("april")
            .join("cover.txt")
            .exists());
    }
}
