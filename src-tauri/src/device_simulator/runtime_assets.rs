use crate::device_simulator::alarms::ImageAssetRef;
use crate::device_simulator::api::{MediaThemeSummary, DEFAULT_MEDIA_THEME_ID};
use crate::device_simulator::assets::catalog::{
    non_commercial_usage, PackFile, PackManifest, PackRef,
};
use crate::device_simulator::assets::validation::validate_pack_path;
use crate::device_simulator::local_materials::{
    load_local_alarm_images, load_local_media_theme, LocalMaterialPaths,
};
use crate::device_simulator::media::{MediaPackCache, SharedMediaPack};
use crate::device_simulator::profiles::loader::load_profile_from_pack;
use crate::device_simulator::profiles::registry::ProfileRegistry;
use crate::device_simulator::profiles::scope::FirstReleaseProfileId;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_RUNTIME_ASSET_BYTES: u64 = 32 * 1024 * 1024;
const MEDIA_THEME_CATALOG_PATH: &str = "media/themes.json";
const MEDIA_THEME_CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedPackDirectory {
    pub id: String,
    pub version: String,
    pub directory: PathBuf,
}

impl PinnedPackDirectory {
    pub fn pack_ref(&self) -> Result<PackRef, RuntimeAssetError> {
        Ok(PackRef {
            id: self.id.clone(),
            version: Version::parse(&self.version).map_err(|source| {
                error(
                    "device_simulator.assets.pin_version_invalid",
                    format!("invalid pinned pack version '{}': {source}", self.version),
                )
            })?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeAssetLayout {
    packs: BTreeMap<String, PinnedPackDirectory>,
    manifests: BTreeMap<String, PackManifest>,
    profiles: ProfileRegistry,
    media: BTreeMap<RuntimeMediaKind, Arc<SharedMediaPack>>,
    local_alarm_images: BTreeMap<String, BTreeMap<String, Vec<Vec<ImageAssetRef>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaThemeCatalogV1 {
    schema_version: u32,
    default_theme_id: String,
    themes: Vec<MediaThemeDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaThemeDefinition {
    id: String,
    display_name_key: String,
    streams: MediaThemeStreams,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaThemeStreams {
    main: String,
    sub: String,
    third: String,
}

impl MediaThemeStreams {
    fn manifest_path(&self, kind: RuntimeMediaKind) -> &str {
        match kind {
            RuntimeMediaKind::Main => &self.main,
            RuntimeMediaKind::Sub => &self.sub,
            RuntimeMediaKind::Third => &self.third,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeMediaKind {
    Main,
    Sub,
    Third,
}

impl RuntimeMediaKind {
    pub const fn manifest_path(self) -> &'static str {
        match self {
            Self::Main => "media/main/media.json",
            Self::Sub => "media/sub/media.json",
            Self::Third => "media/third/media.json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAssetError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for RuntimeAssetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeAssetError {}

impl RuntimeAssetLayout {
    pub fn load(
        pinned: &[PinnedPackDirectory],
        selected_profiles: &[String],
    ) -> Result<Self, RuntimeAssetError> {
        Self::load_for_theme(pinned, selected_profiles, DEFAULT_MEDIA_THEME_ID)
    }

    pub fn load_for_theme(
        pinned: &[PinnedPackDirectory],
        selected_profiles: &[String],
        media_theme_id: &str,
    ) -> Result<Self, RuntimeAssetError> {
        Self::load_for_theme_with_local(pinned, selected_profiles, media_theme_id, None)
    }

    pub fn load_for_theme_with_local(
        pinned: &[PinnedPackDirectory],
        selected_profiles: &[String],
        media_theme_id: &str,
        app_data_dir: Option<&Path>,
    ) -> Result<Self, RuntimeAssetError> {
        let local_paths = app_data_dir.map(LocalMaterialPaths::from_app_data_dir);
        Self::load_for_theme_with_local_paths(
            pinned,
            selected_profiles,
            media_theme_id,
            local_paths.as_ref(),
        )
    }

    pub fn load_for_theme_with_local_paths(
        pinned: &[PinnedPackDirectory],
        selected_profiles: &[String],
        media_theme_id: &str,
        local_paths: Option<&LocalMaterialPaths>,
    ) -> Result<Self, RuntimeAssetError> {
        if pinned.is_empty() || selected_profiles.is_empty() {
            return Err(error(
                "device_simulator.assets.pin_empty",
                "runtime asset pins and selected profiles must not be empty",
            ));
        }
        let mut packs = BTreeMap::new();
        let mut manifests = BTreeMap::new();
        for item in pinned {
            validate_pack_id(&item.id)?;
            if !item.directory.is_absolute() {
                return Err(error(
                    "device_simulator.assets.pin_path_invalid",
                    format!("pinned pack '{}' path is not absolute", item.id),
                ));
            }
            let metadata = fs::metadata(&item.directory).map_err(|source| {
                error(
                    "device_simulator.assets.pin_path_invalid",
                    format!("failed to inspect pinned pack '{}': {source}", item.id),
                )
            })?;
            if !metadata.is_dir() {
                return Err(error(
                    "device_simulator.assets.pin_path_invalid",
                    format!("pinned pack '{}' is not a directory", item.id),
                ));
            }
            let manifest = if item.version.trim().is_empty() {
                build_loose_manifest(&item.id, &item.directory)?
            } else {
                let expected = item.pack_ref()?;
                let manifest_bytes = read_bounded(&item.directory.join("pack.json"))?;
                let manifest: PackManifest =
                    serde_json::from_slice(&manifest_bytes).map_err(|source| {
                        error(
                            "device_simulator.assets.manifest_invalid",
                            format!("pinned pack '{}' manifest is invalid: {source}", item.id),
                        )
                    })?;
                if manifest.id != expected.id || manifest.version != expected.version {
                    return Err(error(
                        "device_simulator.assets.pin_identity_mismatch",
                        format!("pinned pack '{}' manifest identity does not match", item.id),
                    ));
                }
                manifest
            };
            if packs.insert(item.id.clone(), item.clone()).is_some() {
                return Err(error(
                    "device_simulator.assets.pin_duplicate",
                    format!("duplicate pinned pack '{}'", item.id),
                ));
            }
            manifests.insert(item.id.clone(), manifest);
        }

        for required in ["protocol-core", "media-h264-live"] {
            if !packs.contains_key(required) {
                return Err(error(
                    "device_simulator.assets.pin_dependency_missing",
                    format!("required runtime pack '{required}' is not pinned"),
                ));
            }
        }

        let selected = selected_profiles.iter().cloned().collect::<BTreeSet<_>>();
        let mut profiles = Vec::with_capacity(selected.len());
        for profile_id in selected {
            let pack = packs.get(&profile_id).ok_or_else(|| {
                error(
                    "device_simulator.assets.profile_pack_missing",
                    format!("profile pack '{profile_id}' is not pinned"),
                )
            })?;
            let profile = load_profile_from_pack(&pack.directory, &profile_id)
                .map_err(|source| error(source.code, source.message))?;
            profiles.push(profile);
        }
        let profiles = ProfileRegistry::from_profiles(profiles)
            .map_err(|source| error(source.code, source.message))?;

        let local_media = local_paths
            .map(|paths| load_local_media_theme(paths, media_theme_id))
            .transpose()
            .map_err(|source| error(source.code, source.message))?
            .flatten();
        let local_alarm_images = local_paths
            .map(load_local_alarm_images)
            .transpose()
            .map_err(|source| error(source.code, source.message))?
            .unwrap_or_default();
        let media_root = &packs
            .get("media-h264-live")
            .expect("required media pack checked")
            .directory;
        let media_manifest = manifests
            .get("media-h264-live")
            .expect("required media pack manifest checked");
        let media = if let Some(media) = local_media {
            media
        } else {
            let catalog = load_media_theme_catalog(media_root, media_manifest)?;
            let selected_theme = catalog
                .themes
                .iter()
                .find(|theme| theme.id == media_theme_id)
                .ok_or_else(|| {
                    error(
                        "device_simulator.assets.media_theme_missing",
                        format!(
                            "media theme '{media_theme_id}' is not available in the active pack"
                        ),
                    )
                })?;
            let cache = MediaPackCache::new();
            let mut media = BTreeMap::new();
            for kind in [
                RuntimeMediaKind::Main,
                RuntimeMediaKind::Sub,
                RuntimeMediaKind::Third,
            ] {
                let manifest_path = selected_theme.streams.manifest_path(kind);
                ensure_declared_media_path(media_manifest, manifest_path)?;
                let pack = cache
                    .load(media_root, manifest_path)
                    .map_err(|source| error(source.code, source.message))?;
                media.insert(kind, pack);
            }
            media
        };

        Ok(Self {
            packs,
            manifests,
            profiles,
            media,
            local_alarm_images,
        })
    }

    pub fn profile(
        &self,
        id: FirstReleaseProfileId,
    ) -> Option<&crate::device_simulator::profiles::schema::DeviceProfileV1> {
        self.profiles.get(id.as_str())
    }

    pub fn media(&self, kind: RuntimeMediaKind) -> Arc<SharedMediaPack> {
        Arc::clone(self.media.get(&kind).expect("all media kinds are loaded"))
    }

    pub fn local_alarm_image_groups(
        &self,
        alarm_type_id: &str,
        variant: &str,
    ) -> Option<&[Vec<ImageAssetRef>]> {
        self.local_alarm_images
            .get(alarm_type_id)
            .and_then(|variants| variants.get(variant))
            .map(Vec::as_slice)
    }

    pub fn local_alarm_image_references(&self) -> Vec<ImageAssetRef> {
        self.local_alarm_images
            .values()
            .flat_map(BTreeMap::values)
            .flat_map(|groups| groups.iter())
            .flat_map(|images| images.iter().cloned())
            .collect()
    }

    pub fn pack(&self, id: &str) -> Option<&PinnedPackDirectory> {
        self.packs.get(id)
    }

    pub fn manifest(&self, id: &str) -> Option<&PackManifest> {
        self.manifests.get(id)
    }

    pub fn pinned_pack_directories(&self) -> Vec<PinnedPackDirectory> {
        self.packs.values().cloned().collect()
    }

    pub fn read_from_pack(
        &self,
        pack_id: &str,
        relative: &str,
    ) -> Result<Vec<u8>, RuntimeAssetError> {
        validate_pack_path(relative).map_err(|source| error(source.code, source.message))?;
        let pack = self.packs.get(pack_id).ok_or_else(|| {
            error(
                "device_simulator.assets.pack_not_pinned",
                format!("pack '{pack_id}' is not pinned"),
            )
        })?;
        let manifest = self.manifests.get(pack_id).expect("pinned manifest exists");
        if !manifest.files.iter().any(|file| file.path == relative) {
            return Err(error(
                "device_simulator.assets.file_not_declared",
                format!("asset '{relative}' is not declared by pack '{pack_id}'"),
            ));
        }
        read_bounded(&pack.directory.join(relative))
    }

    pub fn read_profile_or_core(
        &self,
        profile_id: FirstReleaseProfileId,
        relative: &str,
    ) -> Result<Vec<u8>, RuntimeAssetError> {
        match self.read_from_pack(profile_id.as_str(), relative) {
            Ok(bytes) => Ok(bytes),
            Err(source) if source.code == "device_simulator.assets.file_not_declared" => {
                self.read_from_pack("protocol-core", relative)
            }
            Err(source) => Err(source),
        }
    }

    pub fn declared_files_under(
        &self,
        pack_id: &str,
        prefix: &str,
    ) -> Result<Vec<String>, RuntimeAssetError> {
        validate_pack_path(prefix).map_err(|source| error(source.code, source.message))?;
        let manifest = self.manifests.get(pack_id).ok_or_else(|| {
            error(
                "device_simulator.assets.pack_not_pinned",
                format!("pack '{pack_id}' is not pinned"),
            )
        })?;
        let normalized = format!("{}/", prefix.trim_end_matches('/'));
        let mut files = manifest
            .files
            .iter()
            .map(|file| file.path.as_str())
            .filter(|path| path.starts_with(&normalized))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        files.sort();
        Ok(files)
    }
}

pub fn list_media_themes(pack_dir: &Path) -> Result<Vec<MediaThemeSummary>, RuntimeAssetError> {
    let manifest_path = pack_dir.join("pack.json");
    let manifest: PackManifest = if manifest_path.is_file() {
        let manifest_bytes = read_bounded(&manifest_path)?;
        serde_json::from_slice(&manifest_bytes).map_err(|source| {
            error(
                "device_simulator.assets.manifest_invalid",
                format!("media pack manifest is invalid: {source}"),
            )
        })?
    } else {
        build_loose_manifest("media-h264-live", pack_dir)?
    };
    if manifest.id != "media-h264-live" {
        return Err(error(
            "device_simulator.assets.pin_identity_mismatch",
            "selected pack is not media-h264-live",
        ));
    }
    let has_catalog = manifest
        .files
        .iter()
        .any(|file| file.path == MEDIA_THEME_CATALOG_PATH);
    let has_legacy_streams = [
        RuntimeMediaKind::Main,
        RuntimeMediaKind::Sub,
        RuntimeMediaKind::Third,
    ]
    .into_iter()
    .all(|kind| {
        manifest
            .files
            .iter()
            .any(|file| file.path == kind.manifest_path())
    });
    // The lightweight built-in media root is intentionally only a runtime
    // contract marker. All selectable videos are distributed by the upgrade
    // server, so a build with neither catalog nor legacy streams has no built-in
    // themes instead of being treated as a corrupt release pack.
    if !has_catalog && !has_legacy_streams {
        return Ok(Vec::new());
    }
    let catalog = load_media_theme_catalog(pack_dir, &manifest)?;
    Ok(catalog
        .themes
        .into_iter()
        .map(|theme| MediaThemeSummary {
            is_default: theme.id == catalog.default_theme_id,
            id: theme.id,
            display_name_key: theme.display_name_key,
            display_name: None,
            is_local: false,
        })
        .collect())
}

fn build_loose_manifest(id: &str, root: &Path) -> Result<PackManifest, RuntimeAssetError> {
    let mut files = Vec::new();
    collect_loose_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.is_empty() {
        return Err(error(
            "device_simulator.assets.loose_directory_empty",
            format!("built-in asset directory '{id}' is empty"),
        ));
    }
    Ok(PackManifest {
        schema_version: 1,
        id: id.to_owned(),
        // Loose assets have no release version. This placeholder only satisfies
        // the legacy in-memory manifest type and is never persisted or exposed.
        version: Version::new(0, 0, 0),
        engine_api: 1,
        usage: non_commercial_usage(),
        files,
    })
}

fn collect_loose_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<PackFile>,
) -> Result<(), RuntimeAssetError> {
    if files.len() > 8_192 {
        return Err(error(
            "device_simulator.assets.loose_directory_too_large",
            "built-in asset directory contains too many files",
        ));
    }
    for entry in fs::read_dir(directory).map_err(|source| {
        error(
            "device_simulator.assets.file_read_failed",
            format!("failed to enumerate '{}': {source}", directory.display()),
        )
    })? {
        let entry = entry.map_err(|source| {
            error(
                "device_simulator.assets.file_read_failed",
                format!("failed to enumerate '{}': {source}", directory.display()),
            )
        })?;
        let metadata = entry.metadata().map_err(|source| {
            error(
                "device_simulator.assets.file_read_failed",
                format!("failed to inspect '{}': {source}", entry.path().display()),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(error(
                "device_simulator.assets.file_read_failed",
                format!("asset '{}' must not be a symlink", entry.path().display()),
            ));
        }
        if metadata.is_dir() {
            collect_loose_files(root, &entry.path(), files)?;
        } else if metadata.is_file() && metadata.len() > 0 {
            let entry_path = entry.path();
            let relative = entry_path.strip_prefix(root).map_err(|source| {
                error(
                    "device_simulator.assets.file_read_failed",
                    format!("failed to resolve loose asset path: {source}"),
                )
            })?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            validate_pack_path(&relative).map_err(|source| error(source.code, source.message))?;
            files.push(PackFile {
                path: relative,
                sha256: String::new(),
                size: metadata.len(),
            });
        }
    }
    Ok(())
}

fn load_media_theme_catalog(
    media_root: &Path,
    manifest: &PackManifest,
) -> Result<MediaThemeCatalogV1, RuntimeAssetError> {
    if !manifest
        .files
        .iter()
        .any(|file| file.path == MEDIA_THEME_CATALOG_PATH)
    {
        for kind in [
            RuntimeMediaKind::Main,
            RuntimeMediaKind::Sub,
            RuntimeMediaKind::Third,
        ] {
            ensure_declared_media_path(manifest, kind.manifest_path())?;
        }
        return Ok(MediaThemeCatalogV1 {
            schema_version: MEDIA_THEME_CATALOG_SCHEMA_VERSION,
            default_theme_id: DEFAULT_MEDIA_THEME_ID.into(),
            themes: vec![MediaThemeDefinition {
                id: DEFAULT_MEDIA_THEME_ID.into(),
                display_name_key: "deviceSimulator.mediaThemes.fanrenXiuxian".into(),
                streams: MediaThemeStreams {
                    main: RuntimeMediaKind::Main.manifest_path().into(),
                    sub: RuntimeMediaKind::Sub.manifest_path().into(),
                    third: RuntimeMediaKind::Third.manifest_path().into(),
                },
            }],
        });
    }

    let bytes = read_bounded(&media_root.join(MEDIA_THEME_CATALOG_PATH))?;
    let catalog: MediaThemeCatalogV1 = serde_json::from_slice(&bytes).map_err(|source| {
        error(
            "device_simulator.assets.media_theme_catalog_invalid",
            format!("media theme catalog is invalid: {source}"),
        )
    })?;
    validate_media_theme_catalog(&catalog, manifest)?;
    Ok(catalog)
}

fn validate_media_theme_catalog(
    catalog: &MediaThemeCatalogV1,
    manifest: &PackManifest,
) -> Result<(), RuntimeAssetError> {
    if catalog.schema_version != MEDIA_THEME_CATALOG_SCHEMA_VERSION
        || catalog.themes.is_empty()
        || catalog.themes.len() > 64
    {
        return Err(error(
            "device_simulator.assets.media_theme_catalog_invalid",
            "media theme catalog schema or theme count is invalid",
        ));
    }
    let mut ids = BTreeSet::new();
    for theme in &catalog.themes {
        validate_theme_id(&theme.id)?;
        if !ids.insert(theme.id.as_str()) {
            return Err(error(
                "device_simulator.assets.media_theme_duplicate",
                format!("media theme '{}' is duplicated", theme.id),
            ));
        }
        if theme.display_name_key.trim().is_empty() || theme.display_name_key.len() > 160 {
            return Err(error(
                "device_simulator.assets.media_theme_catalog_invalid",
                format!("media theme '{}' has an invalid display name key", theme.id),
            ));
        }
        for path in [
            &theme.streams.main,
            &theme.streams.sub,
            &theme.streams.third,
        ] {
            ensure_declared_media_path(manifest, path)?;
        }
    }
    if !ids.contains(catalog.default_theme_id.as_str()) {
        return Err(error(
            "device_simulator.assets.media_theme_default_missing",
            "default media theme is absent from the catalog",
        ));
    }
    Ok(())
}

fn ensure_declared_media_path(
    manifest: &PackManifest,
    relative_path: &str,
) -> Result<(), RuntimeAssetError> {
    validate_pack_path(relative_path)
        .map_err(|source| error(source.code, source.message.clone()))?;
    if !relative_path.ends_with("/media.json")
        || !manifest.files.iter().any(|file| file.path == relative_path)
    {
        return Err(error(
            "device_simulator.assets.media_theme_file_missing",
            format!("media manifest '{relative_path}' is not declared by the media pack"),
        ));
    }
    Ok(())
}

fn validate_theme_id(id: &str) -> Result<(), RuntimeAssetError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(error(
            "device_simulator.assets.media_theme_id_invalid",
            format!("media theme id '{id}' is invalid"),
        ));
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, RuntimeAssetError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| {
        error(
            "device_simulator.assets.file_read_failed",
            format!("failed to inspect '{}': {source}", path.display()),
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return Err(error(
            "device_simulator.assets.file_read_failed",
            format!("asset '{}' is not a non-empty regular file", path.display()),
        ));
    }
    if metadata.len() > MAX_RUNTIME_ASSET_BYTES {
        return Err(error(
            "device_simulator.assets.file_size_exceeded",
            format!("asset '{}' exceeds the runtime read limit", path.display()),
        ));
    }
    fs::read(path).map_err(|source| {
        error(
            "device_simulator.assets.file_read_failed",
            format!("failed to read '{}': {source}", path.display()),
        )
    })
}

fn validate_pack_id(id: &str) -> Result<(), RuntimeAssetError> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(error(
            "device_simulator.assets.pin_id_invalid",
            format!("pinned pack id '{id}' is invalid"),
        ));
    }
    Ok(())
}

fn error(code: &'static str, message: impl Into<String>) -> RuntimeAssetError {
    RuntimeAssetError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{non_commercial_usage, PackFile};
    use crate::device_simulator::profiles::scope::FirstReleaseProfileId;
    use tempfile::TempDir;

    fn media_pack_manifest(paths: &[&str]) -> PackManifest {
        PackManifest {
            schema_version: 1,
            id: "media-h264-live".into(),
            version: Version::new(1, 1, 0),
            engine_api: 1,
            usage: non_commercial_usage(),
            files: paths
                .iter()
                .map(|path| PackFile {
                    path: (*path).into(),
                    sha256: "a".repeat(64),
                    size: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn pinned_pack_contract_rejects_relative_paths_and_invalid_versions() {
        let relative = PinnedPackDirectory {
            id: "ipc-structured".into(),
            version: "1.0.0".into(),
            directory: PathBuf::from("relative"),
        };
        assert_eq!(
            RuntimeAssetLayout::load(&[relative], &["ipc-structured".into()])
                .unwrap_err()
                .code,
            "device_simulator.assets.pin_path_invalid"
        );
        assert!(PinnedPackDirectory {
            id: "ipc-structured".into(),
            version: "latest".into(),
            directory: PathBuf::from(r"C:\assets"),
        }
        .pack_ref()
        .is_err());
    }

    #[test]
    fn lists_catalog_themes_and_falls_back_for_legacy_media_packs() {
        let modern = TempDir::new().unwrap();
        let modern_paths = [
            MEDIA_THEME_CATALOG_PATH,
            "media/themes/windows-tech/main/media.json",
            "media/themes/windows-tech/sub/media.json",
            "media/themes/windows-tech/third/media.json",
        ];
        fs::create_dir_all(modern.path().join("media")).unwrap();
        fs::write(
            modern.path().join("pack.json"),
            serde_json::to_vec(&media_pack_manifest(&modern_paths)).unwrap(),
        )
        .unwrap();
        fs::write(
            modern.path().join(MEDIA_THEME_CATALOG_PATH),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "default_theme_id": "windows-tech",
                "themes": [{
                    "id": "windows-tech",
                    "display_name_key": "deviceSimulator.mediaThemes.windowsTech",
                    "streams": {
                        "main": modern_paths[1],
                        "sub": modern_paths[2],
                        "third": modern_paths[3]
                    }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let themes = list_media_themes(modern.path()).unwrap();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "windows-tech");
        assert!(themes[0].is_default);

        let legacy = TempDir::new().unwrap();
        let legacy_paths = [
            "media/main/media.json",
            "media/sub/media.json",
            "media/third/media.json",
        ];
        fs::write(
            legacy.path().join("pack.json"),
            serde_json::to_vec(&media_pack_manifest(&legacy_paths)).unwrap(),
        )
        .unwrap();
        let themes = list_media_themes(legacy.path()).unwrap();
        assert_eq!(themes[0].id, DEFAULT_MEDIA_THEME_ID);
        assert!(themes[0].is_default);
    }

    #[test]
    fn approved_release_fixture_loads_when_explicitly_configured() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let pins = ["protocol-core", "media-h264-live", "ipc-structured"]
            .into_iter()
            .map(|id| PinnedPackDirectory {
                id: id.into(),
                version: version.clone(),
                directory: root.join(id).join(&version),
            })
            .collect::<Vec<_>>();
        let profiles = ["ipc-structured"].map(str::to_owned);
        let layout = RuntimeAssetLayout::load(&pins, &profiles).unwrap();
        let structured = layout
            .profile(FirstReleaseProfileId::IpcStructured)
            .unwrap();
        assert_eq!(structured.identity.model, "HIC6881-IR@X38-L-WSGB-VC");
        assert!(!layout.media(RuntimeMediaKind::Main).frames().is_empty());
        assert!(!layout.media(RuntimeMediaKind::Third).frames().is_empty());
    }
}
