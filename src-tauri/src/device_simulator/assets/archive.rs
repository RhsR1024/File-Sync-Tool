use super::catalog::{CatalogPack, PackFile, PackManifest};
use super::validation::{
    validate_pack_manifest, validate_pack_path, MAX_FILES_PER_PACK, MAX_FILE_SIZE_BYTES,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const MAX_PACK_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetArchiveError {
    pub code: &'static str,
    pub message: String,
}

impl AssetArchiveError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AssetArchiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetArchiveError {}

/// Validate and extract an immutable asset pack into a new staging directory.
///
/// This function performs blocking filesystem and ZIP work. Async callers must
/// invoke it through `tokio::task::spawn_blocking`. `staging_dir` must not
/// already exist; once created by this function, it is removed on every error.
pub fn extract_pack_to_staging(
    archive_path: &Path,
    staging_dir: &Path,
    expected: &CatalogPack,
) -> Result<PackManifest, AssetArchiveError> {
    if staging_dir.exists() {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.staging_exists",
            format!(
                "asset staging directory already exists: {}",
                staging_dir.display()
            ),
        ));
    }
    fs::create_dir(staging_dir).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.staging_create_failed",
            format!("failed to create staging directory: {error}"),
        )
    })?;

    let result = extract_pack_inner(archive_path, staging_dir, expected);
    if result.is_err() {
        let _ = fs::remove_dir_all(staging_dir);
    }
    result
}

fn extract_pack_inner(
    archive_path: &Path,
    staging_dir: &Path,
    expected: &CatalogPack,
) -> Result<PackManifest, AssetArchiveError> {
    let archive_size = fs::metadata(archive_path)
        .map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.archive_open_failed",
                format!("failed to inspect asset archive: {error}"),
            )
        })?
        .len();
    if archive_size != expected.size {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.archive_size_mismatch",
            format!(
                "asset ZIP size {archive_size} does not match catalog size {}",
                expected.size
            ),
        ));
    }
    let archive_file = File::open(archive_path).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.archive_open_failed",
            format!("failed to open asset archive: {error}"),
        )
    })?;
    let mut archive = ZipArchive::new(archive_file).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.archive_invalid",
            format!("invalid asset ZIP archive: {error}"),
        )
    })?;

    if archive.len() > MAX_FILES_PER_PACK.saturating_add(1) {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.too_many_files",
            format!("asset ZIP contains {} entries", archive.len()),
        ));
    }

    let (manifest, manifest_bytes) = read_pack_manifest(&mut archive)?;
    validate_pack_manifest(&manifest, expected).map_err(|error| {
        AssetArchiveError::new(
            error.code,
            format!("pack manifest validation failed: {error}"),
        )
    })?;

    let declared = manifest
        .files
        .iter()
        .map(|file| (file.path.to_ascii_lowercase(), file))
        .collect::<HashMap<_, _>>();
    let mut seen_entries = HashSet::new();
    let mut extracted_files = HashSet::new();
    let mut extracted_total = 0_u64;

    write_new_file(&staging_dir.join("pack.json"), &manifest_bytes)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.archive_entry_invalid",
                format!("failed to read ZIP entry {index}: {error}"),
            )
        })?;
        reject_unsafe_entry(&entry)?;

        let raw_name = entry.name().to_string();
        let normalized_name = raw_name.trim_end_matches('/');
        if normalized_name.eq_ignore_ascii_case("pack.json") {
            if raw_name != "pack.json" || !entry.is_file() {
                return Err(AssetArchiveError::new(
                    "device_simulator.assets.manifest_entry_invalid",
                    "pack.json must be a regular root file with exact casing",
                ));
            }
            if !seen_entries.insert("pack.json".to_string()) {
                return Err(AssetArchiveError::new(
                    "device_simulator.assets.duplicate_archive_entry",
                    "asset ZIP contains duplicate pack.json entries",
                ));
            }
            continue;
        }

        validate_pack_path(normalized_name).map_err(|error| {
            AssetArchiveError::new(
                error.code,
                format!("unsafe ZIP entry '{raw_name}': {error}"),
            )
        })?;
        let lower_name = normalized_name.to_ascii_lowercase();
        if !seen_entries.insert(lower_name.clone()) {
            return Err(AssetArchiveError::new(
                "device_simulator.assets.duplicate_archive_entry",
                format!("asset ZIP contains duplicate entry '{normalized_name}'"),
            ));
        }

        if entry.is_dir() {
            let prefix = format!("{lower_name}/");
            if !declared.keys().any(|path| path.starts_with(&prefix)) {
                return Err(AssetArchiveError::new(
                    "device_simulator.assets.undeclared_archive_entry",
                    format!("directory '{normalized_name}' is not used by a declared file"),
                ));
            }
            continue;
        }

        let declared_file = declared.get(&lower_name).ok_or_else(|| {
            AssetArchiveError::new(
                "device_simulator.assets.undeclared_archive_entry",
                format!("file '{normalized_name}' is not declared in pack.json"),
            )
        })?;
        extract_declared_file(
            &mut entry,
            declared_file,
            staging_dir,
            &mut extracted_total,
            expected.unpacked_size,
        )?;
        extracted_files.insert(lower_name);
    }

    if !seen_entries.contains("pack.json") {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.manifest_missing",
            "asset ZIP does not contain root pack.json",
        ));
    }
    let missing = declared
        .keys()
        .filter(|path| !extracted_files.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.declared_file_missing",
            format!(
                "asset ZIP is missing declared files: {}",
                missing.join(", ")
            ),
        ));
    }

    Ok(manifest)
}

fn read_pack_manifest(
    archive: &mut ZipArchive<File>,
) -> Result<(PackManifest, Vec<u8>), AssetArchiveError> {
    let mut manifest_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.archive_entry_invalid",
                format!("failed to inspect ZIP entry {index}: {error}"),
            )
        })?;
        if entry.name().eq_ignore_ascii_case("pack.json") {
            if manifest_index.replace(index).is_some() {
                return Err(AssetArchiveError::new(
                    "device_simulator.assets.duplicate_archive_entry",
                    "asset ZIP contains duplicate pack.json entries",
                ));
            }
        }
    }
    let index = manifest_index.ok_or_else(|| {
        AssetArchiveError::new(
            "device_simulator.assets.manifest_missing",
            "asset ZIP does not contain root pack.json",
        )
    })?;
    let mut entry = archive.by_index(index).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.archive_entry_invalid",
            format!("failed to open pack.json: {error}"),
        )
    })?;
    reject_unsafe_entry(&entry)?;
    if entry.name() != "pack.json" || !entry.is_file() || entry.size() > MAX_PACK_MANIFEST_BYTES {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.manifest_entry_invalid",
            "pack.json must be a regular root file no larger than 1 MiB",
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.manifest_read_failed",
            format!("failed to read pack.json: {error}"),
        )
    })?;
    let manifest = serde_json::from_slice(&bytes).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.manifest_invalid",
            format!("pack.json is not valid schema JSON: {error}"),
        )
    })?;
    Ok((manifest, bytes))
}

fn reject_unsafe_entry(entry: &zip::read::ZipFile<'_>) -> Result<(), AssetArchiveError> {
    if entry.encrypted() {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.encrypted_entry_rejected",
            format!("encrypted ZIP entry is not allowed: {}", entry.name()),
        ));
    }
    if entry.is_symlink() || (!entry.is_file() && !entry.is_dir()) {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.special_entry_rejected",
            format!(
                "symlink or special ZIP entry is not allowed: {}",
                entry.name()
            ),
        ));
    }
    if entry.enclosed_name().is_none() {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.invalid_pack_path",
            format!("ZIP entry escapes the archive root: {}", entry.name()),
        ));
    }
    Ok(())
}

fn extract_declared_file(
    entry: &mut zip::read::ZipFile<'_>,
    declared: &PackFile,
    staging_dir: &Path,
    extracted_total: &mut u64,
    total_limit: u64,
) -> Result<(), AssetArchiveError> {
    if entry.size() != declared.size || entry.size() > MAX_FILE_SIZE_BYTES {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.file_size_mismatch",
            format!(
                "file '{}' size {} does not match declared {}",
                declared.path,
                entry.size(),
                declared.size
            ),
        ));
    }
    *extracted_total = extracted_total.checked_add(entry.size()).ok_or_else(|| {
        AssetArchiveError::new(
            "device_simulator.assets.size_limit_exceeded",
            "asset unpacked size overflow",
        )
    })?;
    if *extracted_total > total_limit {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.size_limit_exceeded",
            "asset ZIP exceeds catalog unpacked_size",
        ));
    }

    let output = staging_dir.join(path_from_pack(&declared.path));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.extract_io",
                format!("failed to create asset directory: {error}"),
            )
        })?;
    }
    let mut target = File::options()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.extract_io",
                format!("failed to create extracted file: {error}"),
            )
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    loop {
        let count = entry.read(&mut buffer).map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.extract_io",
                format!("failed to read ZIP entry '{}': {error}", declared.path),
            )
        })?;
        if count == 0 {
            break;
        }
        written = written.saturating_add(count as u64);
        if written > declared.size {
            return Err(AssetArchiveError::new(
                "device_simulator.assets.file_size_mismatch",
                format!("file '{}' expanded beyond its declared size", declared.path),
            ));
        }
        hasher.update(&buffer[..count]);
        target.write_all(&buffer[..count]).map_err(|error| {
            AssetArchiveError::new(
                "device_simulator.assets.extract_io",
                format!(
                    "failed to write extracted file '{}': {error}",
                    declared.path
                ),
            )
        })?;
    }
    target.flush().map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.extract_io",
            format!(
                "failed to flush extracted file '{}': {error}",
                declared.path
            ),
        )
    })?;
    if written != declared.size {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.file_size_mismatch",
            format!("file '{}' extracted size is incomplete", declared.path),
        ));
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != declared.sha256 {
        return Err(AssetArchiveError::new(
            "device_simulator.assets.file_hash_mismatch",
            format!("file '{}' SHA-256 does not match pack.json", declared.path),
        ));
    }
    Ok(())
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), AssetArchiveError> {
    let mut file = File::create_new(path).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.extract_io",
            format!("failed to create {}: {error}", path.display()),
        )
    })?;
    file.write_all(bytes).map_err(|error| {
        AssetArchiveError::new(
            "device_simulator.assets.extract_io",
            format!("failed to write {}: {error}", path.display()),
        )
    })
}

fn path_from_pack(path: &str) -> PathBuf {
    path.split('/').collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{non_commercial_usage, PackKind};
    use semver::Version;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn expected_pack(archive_size: u64, unpacked_size: u64) -> CatalogPack {
        CatalogPack {
            id: "protocol-core".into(),
            version: Version::new(1, 0, 0),
            kind: PackKind::ProtocolCore,
            url: "packs/protocol-core/1.0.0/protocol-core-1.0.0.zip".into(),
            sha256: "a".repeat(64),
            size: archive_size,
            unpacked_size,
            dependencies: vec![],
            min_app_version: Version::new(1, 0, 0),
        }
    }

    fn write_archive(
        root: &TempDir,
        file_path: &str,
        file_bytes: &[u8],
        declared_hash: Option<String>,
        extra: Option<(&str, &[u8])>,
    ) -> (PathBuf, CatalogPack) {
        let archive_path = root.path().join("pack.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let manifest = PackManifest {
            schema_version: 1,
            id: "protocol-core".into(),
            version: Version::new(1, 0, 0),
            engine_api: 1,
            usage: non_commercial_usage(),
            files: vec![PackFile {
                path: file_path.into(),
                sha256: declared_hash.unwrap_or_else(|| hash(file_bytes)),
                size: file_bytes.len() as u64,
            }],
        };
        writer.start_file("pack.json", options).unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer.start_file(file_path, options).unwrap();
        writer.write_all(file_bytes).unwrap();
        if let Some((path, bytes)) = extra {
            writer.start_file(path, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        let archive_size = fs::metadata(&archive_path).unwrap().len();
        (
            archive_path,
            expected_pack(archive_size, file_bytes.len() as u64),
        )
    }

    #[test]
    fn extracts_only_declared_verified_files() {
        let root = TempDir::new().unwrap();
        let (archive, expected) = write_archive(
            &root,
            "profiles/schema.json",
            br#"{"schema":1}"#,
            None,
            None,
        );
        let staging = root.path().join("staging");
        let manifest = extract_pack_to_staging(&archive, &staging, &expected).unwrap();
        assert_eq!(manifest.id, "protocol-core");
        assert_eq!(
            fs::read(staging.join("profiles/schema.json")).unwrap(),
            br#"{"schema":1}"#
        );
        assert!(staging.join("pack.json").is_file());
    }

    #[test]
    fn rejects_undeclared_and_traversal_entries_and_cleans_staging() {
        let root = TempDir::new().unwrap();
        let (archive, expected) = write_archive(
            &root,
            "profiles/schema.json",
            b"{}",
            None,
            Some(("unexpected.json", b"{}")),
        );
        let staging = root.path().join("undeclared");
        let error = extract_pack_to_staging(&archive, &staging, &expected).unwrap_err();
        assert_eq!(
            error.code,
            "device_simulator.assets.undeclared_archive_entry"
        );
        assert!(!staging.exists());

        let (archive, expected) = write_archive(&root, "../escape.json", b"{}", None, None);
        let staging = root.path().join("traversal");
        let error = extract_pack_to_staging(&archive, &staging, &expected).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.invalid_pack_path");
        assert!(!staging.exists());
        assert!(!root.path().parent().unwrap().join("escape.json").exists());
    }

    #[test]
    fn rejects_hash_mismatch_and_existing_staging_directory() {
        let root = TempDir::new().unwrap();
        let (archive, expected) = write_archive(
            &root,
            "profiles/schema.json",
            b"{}",
            Some("b".repeat(64)),
            None,
        );
        let staging = root.path().join("hash-failure");
        let error = extract_pack_to_staging(&archive, &staging, &expected).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.file_hash_mismatch");
        assert!(!staging.exists());

        fs::create_dir(&staging).unwrap();
        let error = extract_pack_to_staging(&archive, &staging, &expected).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.staging_exists");
    }

    #[test]
    fn rejects_archive_size_mismatch_before_extraction() {
        let root = TempDir::new().unwrap();
        let (archive, mut expected) =
            write_archive(&root, "profiles/schema.json", b"{}", None, None);
        expected.size += 1;
        let staging = root.path().join("size-mismatch");

        let error = extract_pack_to_staging(&archive, &staging, &expected).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.archive_size_mismatch");
        assert!(!staging.exists());
    }

    #[test]
    fn rejects_symlink_entry_metadata() {
        let root = TempDir::new().unwrap();
        let archive_path = root.path().join("symlink.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        let manifest = PackManifest {
            schema_version: 1,
            id: "protocol-core".into(),
            version: Version::new(1, 0, 0),
            engine_api: 1,
            usage: non_commercial_usage(),
            files: vec![PackFile {
                path: "profiles/link.json".into(),
                sha256: hash(b"target"),
                size: 6,
            }],
        };
        writer.start_file("pack.json", options).unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer
            .add_symlink("profiles/link.json", "target", options)
            .unwrap();
        writer.finish().unwrap();
        let expected = expected_pack(fs::metadata(&archive_path).unwrap().len(), 6);

        let error = extract_pack_to_staging(&archive_path, &root.path().join("symlink"), &expected)
            .unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.special_entry_rejected");
    }
}
