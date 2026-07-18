use super::archive::extract_pack_to_staging;
use super::catalog::{CatalogPack, CatalogV1, PackManifest, PackRef};
use super::resolver::resolve_profile_dependencies;
use super::validation::{validate_pack_manifest, validate_pack_path};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const ACTIVE_STATE_SCHEMA_VERSION: u32 = 1;
pub const INSTALL_DISK_RESERVE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetStorePaths {
    pub root: PathBuf,
    pub catalog: PathBuf,
    pub catalog_signature: PathBuf,
    pub active: PathBuf,
    pub packs: PathBuf,
    pub staging: PathBuf,
}

impl AssetStorePaths {
    /// `app_data_dir` must come from Tauri's path resolver or the configured
    /// custom data directory. This function never reads `%APPDATA%` directly.
    pub fn from_app_data_dir(app_data_dir: &Path) -> Self {
        let root = app_data_dir.join("device-simulator").join("assets");
        Self {
            catalog: root.join("catalog-v1.json"),
            catalog_signature: root.join("catalog-v1.json.sig"),
            active: root.join("active.json"),
            packs: root.join("packs"),
            staging: root.join("staging"),
            root,
        }
    }

    pub fn ensure_layout(&self) -> Result<(), AssetCacheError> {
        fs::create_dir_all(&self.packs)
            .map_err(|error| io_error("create packs directory", error))?;
        fs::create_dir_all(&self.staging)
            .map_err(|error| io_error("create staging directory", error))?;
        Ok(())
    }

    pub fn pack_dir(&self, pack: &PackRef) -> Result<PathBuf, AssetCacheError> {
        validate_pack_identity_path(pack)?;
        Ok(self.packs.join(&pack.id).join(pack.version.to_string()))
    }

    pub fn archive_part_path(&self, pack: &PackRef) -> Result<PathBuf, AssetCacheError> {
        validate_pack_identity_path(pack)?;
        Ok(self
            .staging
            .join(format!("{}-{}.zip.part", pack.id, pack.version)))
    }

    pub fn archive_path(&self, pack: &PackRef) -> Result<PathBuf, AssetCacheError> {
        validate_pack_identity_path(pack)?;
        Ok(self
            .staging
            .join(format!("{}-{}.zip", pack.id, pack.version)))
    }

    fn unpacked_staging_dir(&self, pack: &PackRef) -> Result<PathBuf, AssetCacheError> {
        validate_pack_identity_path(pack)?;
        Ok(self
            .staging
            .join(format!("{}-{}.unpacked", pack.id, pack.version)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveAssetStateV1 {
    pub schema_version: u32,
    pub active: AssetSelection,
    pub previous: Option<AssetSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetSelection {
    pub catalog_generated_at: String,
    pub profiles: Vec<String>,
    pub packs: Vec<PackRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSessionPin {
    pub selection: AssetSelection,
    pub pack_directories: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetCacheError {
    pub code: &'static str,
    pub message: String,
}

impl AssetCacheError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AssetCacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetCacheError {}

#[derive(Debug, Clone)]
pub struct AssetStore {
    paths: AssetStorePaths,
}

impl AssetStore {
    pub fn new(paths: AssetStorePaths) -> Self {
        Self { paths }
    }

    pub(crate) fn paths(&self) -> &AssetStorePaths {
        &self.paths
    }

    /// Installs an already downloaded ZIP into its immutable version directory.
    /// Extraction and filesystem hashing are blocking and should be wrapped in
    /// `tokio::task::spawn_blocking` by async callers.
    pub fn install_archive(
        &self,
        archive: &Path,
        expected: &CatalogPack,
    ) -> Result<PathBuf, AssetCacheError> {
        self.paths.ensure_layout()?;
        let pack_ref = pack_ref(expected);
        let staging = self.paths.unpacked_staging_dir(&pack_ref)?;
        if staging.exists() {
            return Err(AssetCacheError::new(
                "device_simulator.assets.staging_exists",
                format!("stale unpack staging exists: {}", staging.display()),
            ));
        }

        extract_pack_to_staging(archive, &staging, expected).map_err(|error| {
            AssetCacheError::new(error.code, format!("asset extraction failed: {error}"))
        })?;
        let target = self.paths.pack_dir(&pack_ref)?;
        if target.exists() {
            if validate_installed_pack(&target, expected).is_ok() {
                fs::remove_dir_all(&staging)
                    .map_err(|error| io_error("remove duplicate staging directory", error))?;
                return Ok(target);
            }
            return Err(AssetCacheError::new(
                "device_simulator.assets.installed_pack_invalid",
                format!(
                    "refusing to overwrite invalid installed pack: {}",
                    target.display()
                ),
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("create pack version parent", error))?;
        }
        fs::rename(&staging, &target)
            .map_err(|error| io_error("atomically install verified pack", error))?;
        validate_installed_pack(&target, expected)?;
        Ok(target)
    }

    pub fn ensure_install_space(&self, packs: &[&CatalogPack]) -> Result<u64, AssetCacheError> {
        self.paths.ensure_layout()?;
        let required = required_install_space(packs)?;
        let available = fs2::available_space(&self.paths.staging)
            .map_err(|error| io_error("query available asset disk space", error))?;
        if available < required {
            return Err(AssetCacheError::new(
                "device_simulator.assets.disk_space_insufficient",
                format!(
                    "asset installation requires {required} bytes, only {available} are available"
                ),
            ));
        }
        Ok(required)
    }

    /// Moves a corrupt immutable pack out of the active pack tree before a
    /// clean reinstall. The quarantine remains under store-owned staging for
    /// diagnosis and never becomes eligible for session pinning.
    pub fn quarantine_invalid_pack(
        &self,
        expected: &CatalogPack,
    ) -> Result<Option<PathBuf>, AssetCacheError> {
        self.paths.ensure_layout()?;
        let pack = pack_ref(expected);
        let installed = self.paths.pack_dir(&pack)?;
        if !installed.exists() || validate_installed_pack(&installed, expected).is_ok() {
            return Ok(None);
        }
        let quarantine = self.paths.staging.join(format!(
            "{}-{}.corrupt-{}",
            pack.id,
            pack.version,
            uuid::Uuid::new_v4()
        ));
        fs::rename(&installed, &quarantine)
            .map_err(|error| io_error("quarantine invalid installed pack", error))?;
        Ok(Some(quarantine))
    }

    pub fn activate_profiles(
        &self,
        catalog: &CatalogV1,
        profile_ids: &[String],
    ) -> Result<ActiveAssetStateV1, AssetCacheError> {
        let resolved = resolve_profile_dependencies(catalog, profile_ids).map_err(|error| {
            AssetCacheError::new(error.code, format!("asset resolution failed: {error}"))
        })?;
        validate_selection_packs(&self.paths, catalog, &resolved)?;

        let mut profiles = profile_ids.to_vec();
        profiles.sort();
        profiles.dedup();
        let selection = AssetSelection {
            catalog_generated_at: catalog.generated_at.clone(),
            profiles,
            packs: resolved,
        };
        let previous = self.load_active()?.map(|state| state.active);
        let state = ActiveAssetStateV1 {
            schema_version: ACTIVE_STATE_SCHEMA_VERSION,
            previous: previous.filter(|candidate| candidate != &selection),
            active: selection,
        };
        self.write_active(&state)?;
        Ok(state)
    }

    pub fn rollback(&self, catalog: &CatalogV1) -> Result<ActiveAssetStateV1, AssetCacheError> {
        let current = self.load_active()?.ok_or_else(|| {
            AssetCacheError::new(
                "device_simulator.assets.active_state_missing",
                "there is no active asset selection to roll back",
            )
        })?;
        let previous = current.previous.ok_or_else(|| {
            AssetCacheError::new(
                "device_simulator.assets.rollback_unavailable",
                "there is no previous asset selection",
            )
        })?;
        validate_selection(catalog, &previous)?;
        validate_selection_packs(&self.paths, catalog, &previous.packs)?;
        let state = ActiveAssetStateV1 {
            schema_version: ACTIVE_STATE_SCHEMA_VERSION,
            active: previous,
            previous: Some(current.active),
        };
        self.write_active(&state)?;
        Ok(state)
    }

    /// Creates an immutable in-memory version pin for one simulator session.
    /// Future active.json updates do not mutate the returned selection.
    pub fn pin_active(&self, catalog: &CatalogV1) -> Result<AssetSessionPin, AssetCacheError> {
        let state = self.load_active()?.ok_or_else(|| {
            AssetCacheError::new(
                "device_simulator.assets.active_state_missing",
                "there is no active asset selection",
            )
        })?;
        validate_selection(catalog, &state.active)?;
        validate_selection_packs(&self.paths, catalog, &state.active.packs)?;
        let pack_directories = state
            .active
            .packs
            .iter()
            .map(|pack| self.paths.pack_dir(pack))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AssetSessionPin {
            selection: state.active,
            pack_directories,
        })
    }

    /// Removes only immutable pack versions which are not referenced by the
    /// active selection, its rollback selection, or any live session pin.
    pub fn cleanup_unprotected_packs(
        &self,
        session_pins: &[AssetSessionPin],
    ) -> Result<Vec<PackRef>, AssetCacheError> {
        self.paths.ensure_layout()?;
        let mut protected = BTreeSet::new();
        if let Some(state) = self.load_active()? {
            protected.extend(state.active.packs.iter().map(ToString::to_string));
            if let Some(previous) = state.previous {
                protected.extend(previous.packs.iter().map(ToString::to_string));
            }
        }
        for pin in session_pins {
            protected.extend(pin.selection.packs.iter().map(ToString::to_string));
        }

        let mut removed = Vec::new();
        for id_entry in
            fs::read_dir(&self.paths.packs).map_err(|error| io_error("read pack cache", error))?
        {
            let id_entry = id_entry.map_err(|error| io_error("read pack cache entry", error))?;
            let id_path = id_entry.path();
            let id_metadata = fs::symlink_metadata(&id_path)
                .map_err(|error| io_error("inspect pack cache entry", error))?;
            if !id_metadata.is_dir() || id_metadata.file_type().is_symlink() {
                continue;
            }
            let id = id_entry.file_name().to_string_lossy().to_string();
            for version_entry in
                fs::read_dir(&id_path).map_err(|error| io_error("read pack versions", error))?
            {
                let version_entry =
                    version_entry.map_err(|error| io_error("read pack version entry", error))?;
                let version_path = version_entry.path();
                let metadata = fs::symlink_metadata(&version_path)
                    .map_err(|error| io_error("inspect pack version entry", error))?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    continue;
                }
                let Ok(version) = Version::parse(&version_entry.file_name().to_string_lossy())
                else {
                    continue;
                };
                let pack = PackRef {
                    id: id.clone(),
                    version,
                };
                validate_pack_identity_path(&pack)?;
                if protected.contains(&pack.to_string()) {
                    continue;
                }
                if version_path.parent() != Some(id_path.as_path())
                    || id_path.parent() != Some(self.paths.packs.as_path())
                {
                    return Err(AssetCacheError::new(
                        "device_simulator.assets.cache_path_invalid",
                        "pack cleanup target escaped the cache root",
                    ));
                }
                fs::remove_dir_all(&version_path)
                    .map_err(|error| io_error("remove unprotected pack version", error))?;
                removed.push(pack);
            }
            if fs::read_dir(&id_path)
                .map_err(|error| io_error("inspect pack id directory", error))?
                .next()
                .is_none()
            {
                fs::remove_dir(&id_path)
                    .map_err(|error| io_error("remove empty pack id directory", error))?;
            }
        }
        Ok(removed)
    }

    pub fn load_active(&self) -> Result<Option<ActiveAssetStateV1>, AssetCacheError> {
        match read_active_file(&self.paths.active) {
            Ok(Some(state)) => Ok(Some(state)),
            Ok(None) => read_active_file(&active_backup_path(&self.paths.active)),
            Err(primary_error) => {
                let backup = active_backup_path(&self.paths.active);
                match read_active_file(&backup) {
                    Ok(Some(state)) => Ok(Some(state)),
                    _ => Err(primary_error),
                }
            }
        }
    }

    fn write_active(&self, state: &ActiveAssetStateV1) -> Result<(), AssetCacheError> {
        if state.schema_version != ACTIVE_STATE_SCHEMA_VERSION {
            return Err(AssetCacheError::new(
                "device_simulator.assets.active_schema_unsupported",
                "cannot write an unsupported active state schema",
            ));
        }
        self.paths.ensure_layout()?;
        write_json_recoverable(&self.paths.active, state)
    }
}

pub fn required_install_space(packs: &[&CatalogPack]) -> Result<u64, AssetCacheError> {
    packs
        .iter()
        .try_fold(INSTALL_DISK_RESERVE_BYTES, |total, pack| {
            total
                .checked_add(pack.size)
                .and_then(|value| value.checked_add(pack.unpacked_size))
                .ok_or_else(|| {
                    AssetCacheError::new(
                        "device_simulator.assets.size_limit_exceeded",
                        "asset installation disk requirement overflowed u64",
                    )
                })
        })
}

pub fn validate_installed_pack(
    directory: &Path,
    expected: &CatalogPack,
) -> Result<PackManifest, AssetCacheError> {
    let manifest_path = directory.join("pack.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|error| io_error("read installed pack manifest", error))?;
    let manifest: PackManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        AssetCacheError::new(
            "device_simulator.assets.manifest_invalid",
            format!("installed pack manifest is invalid: {error}"),
        )
    })?;
    validate_pack_manifest(&manifest, expected).map_err(|error| {
        AssetCacheError::new(
            error.code,
            format!("installed manifest failed validation: {error}"),
        )
    })?;

    let declared = manifest
        .files
        .iter()
        .map(|file| (file.path.to_ascii_lowercase(), file))
        .collect::<HashMap<_, _>>();
    let mut seen = BTreeSet::new();
    visit_installed_files(directory, directory, &declared, &mut seen)?;
    let missing = declared
        .keys()
        .filter(|path| !seen.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(AssetCacheError::new(
            "device_simulator.assets.declared_file_missing",
            format!("installed pack is missing files: {}", missing.join(", ")),
        ));
    }
    Ok(manifest)
}

fn visit_installed_files<'a>(
    root: &Path,
    directory: &Path,
    declared: &HashMap<String, &'a super::catalog::PackFile>,
    seen: &mut BTreeSet<String>,
) -> Result<(), AssetCacheError> {
    for entry in fs::read_dir(directory).map_err(|error| io_error("read installed pack", error))? {
        let entry = entry.map_err(|error| io_error("read installed pack entry", error))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error("inspect installed pack entry", error))?;
        if metadata.file_type().is_symlink() {
            return Err(AssetCacheError::new(
                "device_simulator.assets.special_entry_rejected",
                format!("installed pack contains a symlink: {}", path.display()),
            ));
        }
        if metadata.is_dir() {
            visit_installed_files(root, &path, declared, seen)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(AssetCacheError::new(
                "device_simulator.assets.special_entry_rejected",
                format!("installed pack contains a special file: {}", path.display()),
            ));
        }
        let relative = path.strip_prefix(root).map_err(|_| {
            AssetCacheError::new(
                "device_simulator.assets.invalid_pack_path",
                "installed pack entry escapes its root",
            )
        })?;
        let normalized = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if normalized == "pack.json" {
            continue;
        }
        validate_pack_path(&normalized).map_err(|error| {
            AssetCacheError::new(error.code, format!("invalid installed path: {error}"))
        })?;
        let key = normalized.to_ascii_lowercase();
        let expected = declared.get(&key).ok_or_else(|| {
            AssetCacheError::new(
                "device_simulator.assets.undeclared_installed_file",
                format!("installed file is not declared: {normalized}"),
            )
        })?;
        if metadata.len() != expected.size {
            return Err(AssetCacheError::new(
                "device_simulator.assets.file_size_mismatch",
                format!("installed file size mismatch: {normalized}"),
            ));
        }
        if hash_file(&path)? != expected.sha256 {
            return Err(AssetCacheError::new(
                "device_simulator.assets.file_hash_mismatch",
                format!("installed file hash mismatch: {normalized}"),
            ));
        }
        seen.insert(key);
    }
    Ok(())
}

fn validate_selection(
    catalog: &CatalogV1,
    selection: &AssetSelection,
) -> Result<(), AssetCacheError> {
    if selection.catalog_generated_at.trim().is_empty()
        || selection.profiles.is_empty()
        || selection.packs.is_empty()
    {
        return Err(AssetCacheError::new(
            "device_simulator.assets.active_selection_mismatch",
            "active selection is incomplete",
        ));
    }

    let selected = selection
        .packs
        .iter()
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    if selected.len() != selection.packs.len() {
        return Err(AssetCacheError::new(
            "device_simulator.assets.active_selection_mismatch",
            "active selection contains duplicate packs",
        ));
    }
    for profile_id in &selection.profiles {
        let profile = catalog
            .profiles
            .iter()
            .find(|profile| profile.id == *profile_id)
            .ok_or_else(|| {
                AssetCacheError::new(
                    "device_simulator.assets.active_selection_mismatch",
                    format!("active profile '{profile_id}' is absent from the catalog"),
                )
            })?;
        for required in &profile.required_packs {
            if !selection.packs.iter().any(|pack| pack.id == required.id) {
                return Err(AssetCacheError::new(
                    "device_simulator.assets.active_selection_mismatch",
                    format!(
                        "active profile '{profile_id}' has no selected '{}' pack",
                        required.id
                    ),
                ));
            }
        }
    }
    for pack_ref in &selection.packs {
        let pack = catalog
            .packs
            .iter()
            .find(|pack| pack.id == pack_ref.id && pack.version == pack_ref.version)
            .ok_or_else(|| {
                AssetCacheError::new(
                    "device_simulator.assets.active_selection_mismatch",
                    format!("active pack '{pack_ref}' is absent from the catalog"),
                )
            })?;
        for dependency in &pack.dependencies {
            if !selected.contains(&dependency.to_string()) {
                return Err(AssetCacheError::new(
                    "device_simulator.assets.active_selection_mismatch",
                    format!("active pack '{pack_ref}' is missing dependency '{dependency}'"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_selection_packs(
    paths: &AssetStorePaths,
    catalog: &CatalogV1,
    packs: &[PackRef],
) -> Result<(), AssetCacheError> {
    for selected in packs {
        let expected = catalog
            .packs
            .iter()
            .find(|pack| pack.id == selected.id && pack.version == selected.version)
            .ok_or_else(|| {
                AssetCacheError::new(
                    "device_simulator.assets.missing_pack",
                    format!("selected pack {selected} is absent from the catalog"),
                )
            })?;
        validate_installed_pack(&paths.pack_dir(selected)?, expected)?;
    }
    Ok(())
}

fn validate_pack_identity_path(pack: &PackRef) -> Result<(), AssetCacheError> {
    let candidate = format!("packs/{}/{}/pack.json", pack.id, pack.version);
    validate_pack_path(&candidate)
        .map_err(|error| AssetCacheError::new(error.code, format!("unsafe pack identity: {error}")))
}

fn pack_ref(pack: &CatalogPack) -> PackRef {
    PackRef {
        id: pack.id.clone(),
        version: pack.version.clone(),
    }
}

fn hash_file(path: &Path) -> Result<String, AssetCacheError> {
    let mut file = File::open(path).map_err(|error| io_error("open installed file", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_error("hash installed file", error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_active_file(path: &Path) -> Result<Option<ActiveAssetStateV1>, AssetCacheError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("read active asset state", error)),
    };
    let state: ActiveAssetStateV1 = serde_json::from_slice(&bytes).map_err(|error| {
        AssetCacheError::new(
            "device_simulator.assets.active_state_invalid",
            format!("active asset state is invalid: {error}"),
        )
    })?;
    if state.schema_version != ACTIVE_STATE_SCHEMA_VERSION {
        return Err(AssetCacheError::new(
            "device_simulator.assets.active_schema_unsupported",
            format!("unsupported active state schema {}", state.schema_version),
        ));
    }
    Ok(Some(state))
}

fn write_json_recoverable<T: Serialize>(path: &Path, value: &T) -> Result<(), AssetCacheError> {
    let parent = path.parent().ok_or_else(|| {
        AssetCacheError::new(
            "device_simulator.assets.cache_path_invalid",
            "active state path has no parent",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error("create active state parent", error))?;
    let temporary = path.with_extension("json.new");
    let backup = active_backup_path(path);
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        AssetCacheError::new(
            "device_simulator.assets.active_state_invalid",
            format!("failed to serialize active state: {error}"),
        )
    })?;
    let mut file =
        File::create(&temporary).map_err(|error| io_error("create active state", error))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error("persist active state", error))?;
    drop(file);

    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| io_error("remove stale active backup", error))?;
    }
    let had_active = path.exists();
    if had_active {
        fs::rename(path, &backup).map_err(|error| io_error("backup active state", error))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_active {
            let _ = fs::rename(&backup, path);
        }
        return Err(io_error("activate new asset state", error));
    }
    if backup.exists() {
        fs::remove_file(&backup).map_err(|error| io_error("remove active backup", error))?;
    }
    Ok(())
}

fn active_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn io_error(action: &str, error: std::io::Error) -> AssetCacheError {
    AssetCacheError::new(
        "device_simulator.assets.cache_io",
        format!("failed to {action}: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{
        non_commercial_usage, CatalogProfile, DeviceKind, PackFile, PackKind,
    };
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn build_pack(
        root: &TempDir,
        id: &str,
        version: Version,
        body: &[u8],
    ) -> (PathBuf, CatalogPack) {
        let archive = root.path().join(format!("{id}-{version}.zip"));
        let payload_hash = format!("{:x}", Sha256::digest(body));
        let manifest = PackManifest {
            schema_version: 1,
            id: id.into(),
            version: version.clone(),
            engine_api: 1,
            usage: non_commercial_usage(),
            files: vec![PackFile {
                path: "profiles/profile.json".into(),
                sha256: payload_hash,
                size: body.len() as u64,
            }],
        };
        let file = File::create(&archive).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("pack.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer
            .start_file("profiles/profile.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body).unwrap();
        writer.finish().unwrap();
        let archive_bytes = fs::read(&archive).unwrap();
        let pack = CatalogPack {
            id: id.into(),
            version,
            kind: PackKind::DeviceProfile,
            url: format!("packs/{id}.zip"),
            sha256: format!("{:x}", Sha256::digest(&archive_bytes)),
            size: archive_bytes.len() as u64,
            unpacked_size: body.len() as u64,
            dependencies: vec![],
            min_app_version: Version::new(1, 2, 0),
        };
        (archive, pack)
    }

    fn catalog(pack: CatalogPack) -> CatalogV1 {
        let pack_ref = pack_ref(&pack);
        CatalogV1 {
            schema_version: 1,
            generated_at: "2026-07-18T12:00:00+08:00".into(),
            engine_api: 1,
            packs: vec![pack],
            profiles: vec![CatalogProfile {
                id: "ipc-smart".into(),
                device_kind: DeviceKind::Ipc,
                required_packs: vec![pack_ref],
            }],
        }
    }

    #[test]
    fn installs_validates_activates_and_pins_an_immutable_pack() {
        let root = TempDir::new().unwrap();
        let (archive, pack) = build_pack(&root, "ipc-smart", Version::new(1, 0, 0), b"v1");
        let catalog = catalog(pack.clone());
        let store = AssetStore::new(AssetStorePaths::from_app_data_dir(root.path()));

        let installed = store.install_archive(&archive, &pack).unwrap();
        validate_installed_pack(&installed, &pack).unwrap();
        let state = store
            .activate_profiles(&catalog, &["ipc-smart".into()])
            .unwrap();
        assert_eq!(state.active.packs, vec![pack_ref(&pack)]);
        let pin = store.pin_active(&catalog).unwrap();
        assert_eq!(pin.selection, state.active);
        assert_eq!(pin.pack_directories, vec![installed]);
    }

    #[test]
    fn rejects_corrupt_or_extra_cached_files() {
        let root = TempDir::new().unwrap();
        let (archive, pack) = build_pack(&root, "ipc-smart", Version::new(1, 0, 0), b"v1");
        let store = AssetStore::new(AssetStorePaths::from_app_data_dir(root.path()));
        let installed = store.install_archive(&archive, &pack).unwrap();

        fs::write(installed.join("profiles/profile.json"), b"tampered").unwrap();
        assert_eq!(
            validate_installed_pack(&installed, &pack).unwrap_err().code,
            "device_simulator.assets.file_size_mismatch"
        );
        fs::write(installed.join("profiles/profile.json"), b"v1").unwrap();
        fs::write(installed.join("extra.json"), b"{}").unwrap();
        assert_eq!(
            validate_installed_pack(&installed, &pack).unwrap_err().code,
            "device_simulator.assets.undeclared_installed_file"
        );
    }

    #[test]
    fn activation_keeps_one_previous_selection_and_rolls_back() {
        let root = TempDir::new().unwrap();
        let store = AssetStore::new(AssetStorePaths::from_app_data_dir(root.path()));
        let (archive_v1, pack_v1) = build_pack(&root, "ipc-smart", Version::new(1, 0, 0), b"v1");
        store.install_archive(&archive_v1, &pack_v1).unwrap();
        let catalog_v1 = catalog(pack_v1.clone());
        store
            .activate_profiles(&catalog_v1, &["ipc-smart".into()])
            .unwrap();

        let (archive_v2, pack_v2) = build_pack(&root, "ipc-smart", Version::new(1, 1, 0), b"v2");
        store.install_archive(&archive_v2, &pack_v2).unwrap();
        let mut catalog_v2 = catalog(pack_v2.clone());
        catalog_v2.packs.push(pack_v1.clone());
        store
            .activate_profiles(&catalog_v2, &["ipc-smart".into()])
            .unwrap();
        let rolled_back = store.rollback(&catalog_v2).unwrap();
        assert_eq!(rolled_back.active.packs, vec![pack_ref(&pack_v1)]);
        assert_eq!(
            rolled_back.previous.unwrap().packs,
            vec![pack_ref(&pack_v2)]
        );
    }

    #[test]
    fn active_state_recovers_from_interrupted_replace_backup() {
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        let store = AssetStore::new(paths.clone());
        let state = ActiveAssetStateV1 {
            schema_version: ACTIVE_STATE_SCHEMA_VERSION,
            active: AssetSelection {
                catalog_generated_at: "2026-07-18T12:00:00+08:00".into(),
                profiles: vec!["ipc-smart".into()],
                packs: vec![PackRef {
                    id: "ipc-smart".into(),
                    version: Version::new(1, 0, 0),
                }],
            },
            previous: None,
        };
        fs::write(
            active_backup_path(&paths.active),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        assert_eq!(store.load_active().unwrap(), Some(state));
    }

    #[test]
    fn disk_preflight_counts_download_unpack_and_reserve() {
        let root = TempDir::new().unwrap();
        let (_archive, pack) = build_pack(&root, "ipc-smart", Version::new(1, 0, 0), b"v1");
        assert_eq!(
            required_install_space(&[&pack]).unwrap(),
            INSTALL_DISK_RESERVE_BYTES + pack.size + pack.unpacked_size
        );
        let store = AssetStore::new(AssetStorePaths::from_app_data_dir(root.path()));
        assert!(store.ensure_install_space(&[&pack]).unwrap() > INSTALL_DISK_RESERVE_BYTES);
    }

    #[test]
    fn cleanup_preserves_active_previous_and_session_pinned_packs() {
        let root = TempDir::new().unwrap();
        let store = AssetStore::new(AssetStorePaths::from_app_data_dir(root.path()));
        let (archive_v1, pack_v1) = build_pack(&root, "ipc-smart", Version::new(1, 0, 0), b"v1");
        let (archive_v2, pack_v2) = build_pack(&root, "ipc-smart", Version::new(2, 0, 0), b"v2");
        let (archive_old, pack_old) = build_pack(&root, "unused", Version::new(1, 0, 0), b"old");
        store.install_archive(&archive_v1, &pack_v1).unwrap();
        store.install_archive(&archive_v2, &pack_v2).unwrap();
        store.install_archive(&archive_old, &pack_old).unwrap();

        let initial_catalog = catalog(pack_v1.clone());
        store
            .activate_profiles(&initial_catalog, &["ipc-smart".into()])
            .unwrap();
        let mut current_catalog = catalog(pack_v2.clone());
        current_catalog.packs.push(pack_v1.clone());
        store
            .activate_profiles(&current_catalog, &["ipc-smart".into()])
            .unwrap();
        let current_pin = store.pin_active(&current_catalog).unwrap();

        let removed = store.cleanup_unprotected_packs(&[current_pin]).unwrap();
        assert_eq!(removed, vec![pack_ref(&pack_old)]);
        assert!(store.paths.pack_dir(&pack_ref(&pack_v2)).unwrap().exists());
        assert!(store.paths.pack_dir(&pack_ref(&pack_v1)).unwrap().exists());
    }
}
