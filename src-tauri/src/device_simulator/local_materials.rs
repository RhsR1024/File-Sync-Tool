use crate::device_simulator::alarms::{ImageAssetRef, ImageExtension};
use crate::device_simulator::api::MediaThemeSummary;
use crate::device_simulator::media::{
    Codec, EvidenceSourceKind, FrameIndex, MediaCompatibility, MediaEvidence, MediaManifestV1,
    NalIndex, ParameterSetKind, ParameterSetRef, VIDEO_CLOCK_RATE,
};
use crate::device_simulator::media::{MediaPackCache, SharedMediaPack};
use crate::device_simulator::runtime_assets::RuntimeMediaKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

const LOCAL_CATALOG_SCHEMA: u32 = 1;
const PREPARED_CATALOG_NAME: &str = "prepared-catalog.json";
const USER_CATALOG_NAME: &str = "catalog.json";
const MANAGED_MATERIAL_ENTRIES: [&str; 5] = [
    "videos",
    "alarm-images",
    "cache",
    "remote-cache",
    USER_CATALOG_NAME,
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalMaterialMigrationReport {
    pub copied_files: u64,
    pub reused_files: u64,
    pub copied_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct LocalMaterialPaths {
    pub root: PathBuf,
    pub videos: PathBuf,
    pub alarm_images: PathBuf,
    pub cache: PathBuf,
    pub remote_cache: PathBuf,
    pub user_alarm_images: PathBuf,
}

impl LocalMaterialPaths {
    pub fn from_app_data_dir(app_data_dir: &Path) -> Self {
        let root = app_data_dir
            .join("device-simulator")
            .join("local-materials");
        Self::from_root(app_data_dir, &root)
    }

    pub fn from_root(app_data_dir: &Path, root: &Path) -> Self {
        Self {
            videos: root.join("videos"),
            alarm_images: root.join("alarm-images"),
            cache: root.join("cache"),
            remote_cache: root.join("remote-cache"),
            user_alarm_images: app_data_dir
                .join("device-simulator")
                .join("user-alarm-images"),
            root: root.to_path_buf(),
        }
    }

    pub fn from_configured_directory(
        app_data_dir: &Path,
        configured_directory: Option<&str>,
    ) -> Self {
        configured_directory
            .map(Path::new)
            .map(|root| Self::from_root(app_data_dir, root))
            .unwrap_or_else(|| Self::from_app_data_dir(app_data_dir))
    }

    pub fn ensure_layout(&self) -> Result<(), LocalMaterialError> {
        fs::create_dir_all(&self.videos).map_err(io_error("create local video directory"))?;
        for category in ["face", "car", "person", "nonmotor"] {
            fs::create_dir_all(self.alarm_images.join(category))
                .map_err(io_error("create local alarm image directory"))?;
        }
        fs::create_dir_all(&self.cache).map_err(io_error("create local material cache"))?;
        fs::create_dir_all(&self.remote_cache).map_err(io_error("create remote material cache"))?;
        fs::create_dir_all(&self.user_alarm_images)
            .map_err(io_error("create local alarm image cache"))?;
        let catalog = self.root.join(USER_CATALOG_NAME);
        if !catalog.exists() {
            fs::write(
                catalog,
                b"{\n  \"schema_version\": 1,\n  \"default_theme_id\": null,\n  \"themes\": []\n}\n",
            )
            .map_err(io_error("create local material catalog"))?;
        }
        Ok(())
    }

    fn prepared_catalog(&self) -> PathBuf {
        self.cache.join(PREPARED_CATALOG_NAME)
    }

    pub fn remote_prepared_catalog(&self) -> PathBuf {
        self.remote_cache.join(PREPARED_CATALOG_NAME)
    }
}

pub fn copy_local_materials_verified(
    source: &LocalMaterialPaths,
    destination: &LocalMaterialPaths,
) -> Result<LocalMaterialMigrationReport, LocalMaterialError> {
    source.ensure_layout()?;
    destination.ensure_layout()?;
    let mut report = LocalMaterialMigrationReport::default();
    for entry in MANAGED_MATERIAL_ENTRIES {
        let source_entry = source.root.join(entry);
        if source_entry.exists() {
            copy_material_entry(&source_entry, &destination.root.join(entry), &mut report)?;
        }
    }
    Ok(report)
}

/// Removes only application-managed material entries and only after every file
/// still matches its destination copy. Unrelated files beside those entries are
/// deliberately preserved.
pub fn remove_verified_local_materials(
    source: &LocalMaterialPaths,
    destination: &LocalMaterialPaths,
) -> Result<u64, LocalMaterialError> {
    let mut removed_files = 0;
    for entry in MANAGED_MATERIAL_ENTRIES {
        let source_entry = source.root.join(entry);
        if source_entry.exists() {
            remove_verified_entry(
                &source_entry,
                &destination.root.join(entry),
                &mut removed_files,
            )?;
        }
    }
    match fs::remove_dir(&source.root) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(error) => return Err(io_error("remove old material root")(error)),
    }
    Ok(removed_files)
}

fn copy_material_entry(
    source: &Path,
    destination: &Path,
    report: &mut LocalMaterialMigrationReport,
) -> Result<(), LocalMaterialError> {
    let metadata = fs::symlink_metadata(source).map_err(io_error("inspect material entry"))?;
    if metadata.file_type().is_symlink() {
        return Err(local_error(
            "device_simulator.local_materials.migration_symlink_unsupported",
            format!(
                "material migration does not follow links: {}",
                source.display()
            ),
        ));
    }
    if destination.exists()
        && fs::symlink_metadata(destination)
            .map_err(io_error("inspect migration target"))?
            .file_type()
            .is_symlink()
    {
        return Err(local_error(
            "device_simulator.local_materials.migration_symlink_unsupported",
            format!("migration target is a link: {}", destination.display()),
        ));
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination).map_err(io_error("create migration directory"))?;
        for entry in fs::read_dir(source).map_err(io_error("read migration directory"))? {
            let entry = entry.map_err(io_error("read migration entry"))?;
            copy_material_entry(&entry.path(), &destination.join(entry.file_name()), report)?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(local_error(
            "device_simulator.local_materials.migration_entry_unsupported",
            format!("material entry is not a regular file: {}", source.display()),
        ));
    }

    let source_hash = sha256_file(source)?;
    if destination.is_file()
        && fs::metadata(destination).is_ok_and(|target| target.len() == metadata.len())
        && sha256_file(destination)? == source_hash
    {
        report.reused_files = report.reused_files.saturating_add(1);
        return Ok(());
    }
    if destination.is_dir() {
        return Err(local_error(
            "device_simulator.local_materials.migration_target_conflict",
            format!("migration target is a directory: {}", destination.display()),
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(io_error("create migration file parent"))?;
    }
    let temporary =
        destination.with_extension(format!("migration-{}.tmp", uuid::Uuid::new_v4().simple()));
    let copied = fs::copy(source, &temporary).map_err(io_error("copy material file"))?;
    let temporary_hash = match sha256_file(&temporary) {
        Ok(hash) => hash,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    let verified = copied == metadata.len() && temporary_hash == source_hash;
    if !verified {
        let _ = fs::remove_file(&temporary);
        return Err(local_error(
            "device_simulator.local_materials.migration_verify_failed",
            format!("copied material did not verify: {}", source.display()),
        ));
    }
    if destination.exists() {
        fs::remove_file(destination).map_err(io_error("replace migrated material"))?;
    }
    fs::rename(&temporary, destination).map_err(io_error("activate migrated material"))?;
    report.copied_files = report.copied_files.saturating_add(1);
    report.copied_bytes = report.copied_bytes.saturating_add(copied);
    Ok(())
}

fn remove_verified_entry(
    source: &Path,
    destination: &Path,
    removed_files: &mut u64,
) -> Result<(), LocalMaterialError> {
    let metadata = fs::symlink_metadata(source).map_err(io_error("inspect old material entry"))?;
    if metadata.file_type().is_symlink() {
        return Err(local_error(
            "device_simulator.local_materials.migration_symlink_unsupported",
            format!("old material contains a link: {}", source.display()),
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(source).map_err(io_error("read old material directory"))? {
            let entry = entry.map_err(io_error("read old material entry"))?;
            remove_verified_entry(
                &entry.path(),
                &destination.join(entry.file_name()),
                removed_files,
            )?;
        }
        fs::remove_dir(source).map_err(io_error("remove empty old material directory"))?;
        return Ok(());
    }
    if !metadata.is_file()
        || !destination.is_file()
        || fs::metadata(destination).map_or(true, |target| target.len() != metadata.len())
        || sha256_file(source)? != sha256_file(destination)?
    {
        return Err(local_error(
            "device_simulator.local_materials.migration_cleanup_unverified",
            format!(
                "old material was preserved because its destination copy is not identical: {}",
                source.display()
            ),
        ));
    }
    fs::remove_file(source).map_err(io_error("remove verified old material file"))?;
    *removed_files = removed_files.saturating_add(1);
    Ok(())
}

fn sha256_file(path: &Path) -> Result<[u8; 32], LocalMaterialError> {
    let mut file = fs::File::open(path).map_err(io_error("open material for verification"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(io_error("read material for verification"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMaterialError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for LocalMaterialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LocalMaterialError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserCatalogV1 {
    schema_version: u32,
    default_theme_id: Option<String>,
    themes: Vec<UserThemeV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserThemeV1 {
    id: String,
    display_name: String,
    file: String,
    #[serde(skip)]
    source_content_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedCatalogV1 {
    schema_version: u32,
    default_theme_id: Option<String>,
    themes: Vec<PreparedThemeV1>,
    #[serde(default)]
    alarm_images: BTreeMap<String, PreparedAlarmVariantsV1>,
    #[serde(default)]
    alarm_image_groups: BTreeMap<String, Vec<PreparedAlarmGroupV1>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedAlarmGroupV1 {
    group_id: String,
    variants: PreparedAlarmVariantsV1,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedAlarmVariantsV1 {
    small: Vec<PreparedAlarmImageV1>,
    normal: Vec<PreparedAlarmImageV1>,
    big: Vec<PreparedAlarmImageV1>,
}

impl PreparedAlarmVariantsV1 {
    fn get(&self, variant: &str) -> &[PreparedAlarmImageV1] {
        match variant {
            "small" => &self.small,
            "big" => &self.big,
            _ => &self.normal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedAlarmImageV1 {
    image_id: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedThemeV1 {
    id: String,
    display_name: String,
    source_file: String,
    source_size: u64,
    source_modified_ms: u64,
    #[serde(default)]
    source_content_id: Option<String>,
    streams: PreparedStreamsV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedStreamsV1 {
    main: String,
    sub: String,
    third: String,
}

impl PreparedStreamsV1 {
    fn get(&self, kind: RuntimeMediaKind) -> &str {
        match kind {
            RuntimeMediaKind::Main => &self.main,
            RuntimeMediaKind::Sub => &self.sub,
            RuntimeMediaKind::Third => &self.third,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Rendition {
    kind: &'static str,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u64,
    payload_type: u8,
}

const RENDITIONS: [Rendition; 3] = [
    Rendition {
        kind: "main",
        width: 1920,
        height: 1080,
        fps: 25,
        bitrate: 3_400_000,
        payload_type: 105,
    },
    Rendition {
        kind: "sub",
        width: 640,
        height: 360,
        fps: 20,
        bitrate: 500_000,
        payload_type: 105,
    },
    Rendition {
        kind: "third",
        width: 640,
        height: 360,
        fps: 20,
        bitrate: 500_000,
        payload_type: 105,
    },
];

pub fn list_local_media_themes(
    paths: &LocalMaterialPaths,
) -> Result<Vec<MediaThemeSummary>, LocalMaterialError> {
    let mut themes = Vec::new();
    let mut theme_ids = std::collections::BTreeSet::new();
    for catalog_path in [paths.remote_prepared_catalog(), paths.prepared_catalog()] {
        let Some(catalog) = read_prepared_catalog_at(&catalog_path)? else {
            continue;
        };
        for theme in catalog.themes {
            if theme_ids.insert(theme.id.clone()) {
                themes.push(MediaThemeSummary {
                    id: theme.id.clone(),
                    display_name_key: String::new(),
                    display_name: Some(theme.display_name),
                    is_default: catalog.default_theme_id.as_deref() == Some(theme.id.as_str()),
                    is_local: true,
                });
            }
        }
    }
    Ok(themes)
}

pub fn load_local_media_theme(
    paths: &LocalMaterialPaths,
    theme_id: &str,
) -> Result<Option<BTreeMap<RuntimeMediaKind, Arc<SharedMediaPack>>>, LocalMaterialError> {
    for (catalog_path, cache_root) in [
        (paths.prepared_catalog(), &paths.cache),
        (paths.remote_prepared_catalog(), &paths.remote_cache),
    ] {
        let Some(catalog) = read_prepared_catalog_at(&catalog_path)? else {
            continue;
        };
        let Some(theme) = catalog.themes.iter().find(|theme| theme.id == theme_id) else {
            continue;
        };
        let cache = MediaPackCache::new();
        let mut media = BTreeMap::new();
        for kind in [
            RuntimeMediaKind::Main,
            RuntimeMediaKind::Sub,
            RuntimeMediaKind::Third,
        ] {
            let pack = cache
                .load_local(cache_root, theme.streams.get(kind))
                .map_err(|error| local_error(error.code, error.message))?;
            media.insert(kind, pack);
        }
        return Ok(Some(media));
    }
    Ok(None)
}

pub fn validate_remote_media_themes(paths: &LocalMaterialPaths) -> Result<(), LocalMaterialError> {
    let Some(catalog) = read_prepared_catalog_at(&paths.remote_prepared_catalog())? else {
        return Ok(());
    };
    let cache = MediaPackCache::new();
    for theme in &catalog.themes {
        for kind in [
            RuntimeMediaKind::Main,
            RuntimeMediaKind::Sub,
            RuntimeMediaKind::Third,
        ] {
            cache
                .load_local(&paths.remote_cache, theme.streams.get(kind))
                .map_err(|error| local_error(error.code, error.message))?;
        }
    }
    Ok(())
}

pub fn load_local_alarm_images(
    paths: &LocalMaterialPaths,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<Vec<ImageAssetRef>>>>, LocalMaterialError> {
    let Some(catalog) = read_prepared_catalog(paths)? else {
        return Ok(BTreeMap::new());
    };
    Ok(catalog
        .alarm_image_groups
        .into_iter()
        .map(|(category, groups)| {
            let variants = ["small", "normal", "big"]
                .into_iter()
                .map(|variant| {
                    let image_groups = groups
                        .iter()
                        .map(|group| {
                            group
                                .variants
                                .get(variant)
                                .iter()
                                .map(|image| ImageAssetRef::UserAsset {
                                    image_id: image.image_id.clone(),
                                    extension: ImageExtension::Jpg,
                                    sha256: image.image_id.clone(),
                                    size: image.size,
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>();
                    (variant.to_owned(), image_groups)
                })
                .collect::<BTreeMap<_, _>>();
            (category, variants)
        })
        .collect())
}

pub fn refresh_local_media(
    paths: &LocalMaterialPaths,
) -> Result<Vec<MediaThemeSummary>, LocalMaterialError> {
    paths.ensure_layout()?;
    let definitions = read_user_catalog_or_scan(paths)?;
    let previous = read_prepared_catalog(paths)?.unwrap_or(PreparedCatalogV1 {
        schema_version: LOCAL_CATALOG_SCHEMA,
        default_theme_id: None,
        themes: Vec::new(),
        alarm_images: BTreeMap::new(),
        alarm_image_groups: BTreeMap::new(),
    });
    let mut ffmpeg = None;
    let mut themes = Vec::new();
    for definition in &definitions.themes {
        validate_theme_definition(definition)?;
        let source = resolve_source_video(paths, &definition.file)?;
        let metadata = fs::metadata(&source).map_err(io_error("inspect local MP4"))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(local_error(
                "device_simulator.local_materials.video_invalid",
                format!("{} is not a non-empty MP4", source.display()),
            ));
        }
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |value| {
                value.as_millis().min(u128::from(u64::MAX)) as u64
            });
        let source_content_id = definition
            .source_content_id
            .clone()
            .map(Ok)
            .unwrap_or_else(|| hash_source_file(&source))?;
        let cached = previous.themes.iter().find(|theme| {
            theme.source_content_id.as_deref() == Some(source_content_id.as_str())
                && theme_streams_exist(paths, theme)
        });
        let streams = if let Some(cached) = cached {
            cached.streams.clone()
        } else {
            let ffmpeg = match ffmpeg.as_deref() {
                Some(ffmpeg) => ffmpeg,
                None => ffmpeg.insert(resolve_ffmpeg()?),
            };
            prepare_theme(paths, ffmpeg, definition, &source)?
        };
        themes.push(PreparedThemeV1 {
            id: definition.id.clone(),
            display_name: definition.display_name.clone(),
            source_file: definition.file.clone(),
            source_size: metadata.len(),
            source_modified_ms: modified_ms,
            source_content_id: Some(source_content_id),
            streams,
        });
    }
    let default_theme_id = definitions
        .default_theme_id
        .filter(|id| themes.iter().any(|theme| &theme.id == id))
        .or_else(|| themes.first().map(|theme| theme.id.clone()));
    let prepared = PreparedCatalogV1 {
        schema_version: LOCAL_CATALOG_SCHEMA,
        default_theme_id,
        themes,
        alarm_images: BTreeMap::new(),
        alarm_image_groups: refresh_local_alarm_image_groups(paths)?,
    };
    write_json_atomic(&paths.prepared_catalog(), &prepared)?;
    list_local_media_themes(paths)
}

/// Refreshes downloaded alarm pictures without inspecting or transcoding MP4.
/// Server-provided videos have already been prepared by the publisher.
pub fn refresh_local_alarm_materials(
    paths: &LocalMaterialPaths,
) -> Result<Vec<MediaThemeSummary>, LocalMaterialError> {
    paths.ensure_layout()?;
    let mut prepared = read_prepared_catalog(paths)?.unwrap_or(PreparedCatalogV1 {
        schema_version: LOCAL_CATALOG_SCHEMA,
        default_theme_id: None,
        themes: Vec::new(),
        alarm_images: BTreeMap::new(),
        alarm_image_groups: BTreeMap::new(),
    });
    prepared.alarm_images.clear();
    prepared.alarm_image_groups = refresh_local_alarm_image_groups(paths)?;
    write_json_atomic(&paths.prepared_catalog(), &prepared)?;
    list_local_media_themes(paths)
}

pub fn clear_prepared_alarm_materials(
    paths: &LocalMaterialPaths,
) -> Result<(), LocalMaterialError> {
    let Some(mut prepared) = read_prepared_catalog(paths)? else {
        return Ok(());
    };
    prepared.alarm_images.clear();
    prepared.alarm_image_groups.clear();
    write_json_atomic(&paths.prepared_catalog(), &prepared)
}

/// Prepares publisher-side MP4 files into the exact file-backed format clients
/// consume. The output directory is incremental: unchanged content reuses its
/// existing renditions, while changed files are transcoded with FFmpeg once.
pub fn prepare_server_video_materials(
    source_videos: &Path,
    output: &Path,
    requested_default_video: Option<&str>,
) -> Result<Vec<MediaThemeSummary>, LocalMaterialError> {
    if !source_videos.is_dir() {
        return Err(local_error(
            "device_simulator.local_materials.video_directory_missing",
            format!(
                "source video directory '{}' does not exist",
                source_videos.display()
            ),
        ));
    }
    fs::create_dir_all(output).map_err(io_error("create prepared video directory"))?;
    let working_root = output.parent().unwrap_or(output).join(format!(
        ".material-builder-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let previous_default_video = read_prepared_catalog_at(&output.join(PREPARED_CATALOG_NAME))?
        .and_then(|catalog| {
            catalog
                .default_theme_id
                .and_then(|default_id| {
                    catalog
                        .themes
                        .into_iter()
                        .find(|theme| theme.id == default_id)
                })
                .map(|theme| theme.source_file)
        });
    let paths = LocalMaterialPaths {
        root: working_root.clone(),
        videos: source_videos.to_path_buf(),
        alarm_images: working_root.join("alarm-images"),
        cache: output.to_path_buf(),
        remote_cache: working_root.join("remote-cache"),
        user_alarm_images: working_root.join("alarm-jpeg-cache"),
    };
    let result = (|| {
        paths.ensure_layout()?;
        let mut themes = refresh_local_media(&paths)?;
        let desired_default = requested_default_video
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .or(previous_default_video);
        if let Some(desired_default) = desired_default {
            let mut catalog = read_prepared_catalog(&paths)?.ok_or_else(|| {
                local_error(
                    "device_simulator.local_materials.prepared_catalog_missing",
                    "prepared catalog disappeared after video preparation",
                )
            })?;
            let default_theme = catalog
                .themes
                .iter()
                .find(|theme| theme.source_file.eq_ignore_ascii_case(&desired_default))
                .ok_or_else(|| {
                    local_error(
                        "device_simulator.local_materials.default_video_missing",
                        format!("default video '{desired_default}' was not found"),
                    )
                })?;
            catalog.default_theme_id = Some(default_theme.id.clone());
            write_json_atomic(&paths.prepared_catalog(), &catalog)?;
            themes = list_local_media_themes(&paths)?;
        }
        for theme in &themes {
            load_local_media_theme(&paths, &theme.id)?.ok_or_else(|| {
                local_error(
                    "device_simulator.local_materials.prepared_theme_missing",
                    format!("prepared theme '{}' is missing", theme.id),
                )
            })?;
        }
        let catalog = read_prepared_catalog(&paths)?.ok_or_else(|| {
            local_error(
                "device_simulator.local_materials.prepared_catalog_missing",
                "prepared catalog disappeared after video preparation",
            )
        })?;
        let active_directories = catalog
            .themes
            .iter()
            .filter_map(|theme| theme.streams.main.split('/').nth(2))
            .collect::<std::collections::BTreeSet<_>>();
        let themes_directory = output.join("media").join("themes");
        if themes_directory.is_dir() {
            for entry in fs::read_dir(&themes_directory)
                .map_err(io_error("scan obsolete prepared themes"))?
            {
                let entry = entry.map_err(io_error("inspect obsolete prepared theme"))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && !active_directories.contains(name.as_str())
                {
                    fs::remove_dir_all(entry.path())
                        .map_err(io_error("remove obsolete prepared theme"))?;
                }
            }
        }
        Ok(themes)
    })();
    if working_root.is_dir() {
        let _ = fs::remove_dir_all(&working_root);
    }
    result
}

fn refresh_local_alarm_image_groups(
    paths: &LocalMaterialPaths,
) -> Result<BTreeMap<String, Vec<PreparedAlarmGroupV1>>, LocalMaterialError> {
    let required = BTreeMap::from([
        ("person", &["scene", "person"][..]),
        ("face", &["scene", "face"][..]),
        ("car", &["scene", "vehicle", "plate"][..]),
        ("nonmotor", &["scene", "nonmotor"][..]),
    ]);
    let mut result = BTreeMap::new();
    for (category, roles) in required {
        let directory = paths.alarm_images.join(category);
        let mut group_directories = Vec::new();
        for entry in fs::read_dir(&directory)
            .map_err(io_error("scan local alarm images"))?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let group_id = entry.file_name().to_string_lossy().into_owned();
            if !valid_alarm_group_id(category, &group_id) {
                return Err(local_error(
                    "device_simulator.local_materials.alarm_group_invalid",
                    format!(
                        "alarm material directory '{group_id}' must use the form {category}-001"
                    ),
                ));
            }
            group_directories.push(entry.path());
        }
        group_directories.sort();
        if group_directories.is_empty() {
            continue;
        }
        let mut groups = Vec::with_capacity(group_directories.len());
        for group_directory in group_directories {
            let group_id = group_directory
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned();
            let mut sources = BTreeMap::new();
            for entry in fs::read_dir(&group_directory)
                .map_err(io_error("scan local alarm image group"))?
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            {
                let path = entry.path();
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
                    continue;
                }
                let role = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !roles.contains(&role.as_str()) || sources.insert(role.clone(), path).is_some() {
                    return Err(local_error(
                        "device_simulator.local_materials.alarm_image_role_invalid",
                        format!("alarm material group '{group_id}' contains an unexpected or duplicate image role '{role}'"),
                    ));
                }
            }
            let mut variants = PreparedAlarmVariantsV1::default();
            for role in roles {
                let source = sources.get(*role).ok_or_else(|| {
                    local_error(
                        "device_simulator.local_materials.alarm_images_incomplete",
                        format!("alarm material group '{group_id}' is missing required image '{role}.jpg'"),
                    )
                })?;
                append_alarm_image_variants(paths, source, &mut variants)?;
            }
            groups.push(PreparedAlarmGroupV1 { group_id, variants });
        }
        result.insert(category.to_owned(), groups);
    }
    Ok(result)
}

fn valid_alarm_group_id(category: &str, group_id: &str) -> bool {
    group_id
        .strip_prefix(&format!("{category}-"))
        .is_some_and(|suffix| suffix.len() >= 3 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
}

fn append_alarm_image_variants(
    paths: &LocalMaterialPaths,
    source: &Path,
    variants: &mut PreparedAlarmVariantsV1,
) -> Result<(), LocalMaterialError> {
    let decoded = image::open(source).map_err(|error| {
        local_error(
            "device_simulator.local_materials.alarm_image_invalid",
            format!("failed to decode {}: {error}", source.display()),
        )
    })?;
    for (variant, width, height, quality) in [
        ("big", 1920, 1080, 90u8),
        ("normal", 1280, 720, 84u8),
        ("small", 640, 360, 76u8),
    ] {
        let resized = decoded.thumbnail(width, height).to_rgb8();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode_image(&resized)
            .map_err(|error| {
                local_error(
                    "device_simulator.local_materials.alarm_image_invalid",
                    format!("failed to encode {}: {error}", source.display()),
                )
            })?;
        let image_id = format!("{:x}", Sha256::digest(&jpeg));
        let target = paths.user_alarm_images.join(format!("{image_id}.jpg"));
        if !target.exists() {
            fs::write(&target, &jpeg).map_err(io_error("write local alarm JPEG"))?;
        }
        let item = PreparedAlarmImageV1 {
            image_id,
            size: jpeg.len() as u64,
        };
        match variant {
            "small" => variants.small.push(item),
            "big" => variants.big.push(item),
            _ => variants.normal.push(item),
        }
    }
    Ok(())
}

fn prepare_theme(
    paths: &LocalMaterialPaths,
    ffmpeg: &Path,
    definition: &UserThemeV1,
    source: &Path,
) -> Result<PreparedStreamsV1, LocalMaterialError> {
    let theme_root = paths
        .cache
        .join("media")
        .join("themes")
        .join(&definition.id);
    let staging = paths.cache.join("staging").join(format!(
        "{}-{}",
        definition.id,
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&staging).map_err(io_error("create local media staging directory"))?;
    let result = (|| {
        let mut manifests = BTreeMap::new();
        for rendition in RENDITIONS {
            let raw = staging.join(format!("{}.annexb.h264", rendition.kind));
            encode_rendition(ffmpeg, source, &raw, rendition)?;
            let target = staging.join(rendition.kind);
            fs::create_dir_all(&target).map_err(io_error("create rendition cache"))?;
            let manifest = normalize_annex_b(&raw, &target, definition, rendition)?;
            fs::remove_file(&raw).map_err(io_error("remove intermediate Annex B stream"))?;
            fs::write(
                target.join("media.json"),
                serde_json::to_vec_pretty(&manifest).map_err(|error| {
                    local_error(
                        "device_simulator.local_materials.catalog_invalid",
                        error.to_string(),
                    )
                })?,
            )
            .map_err(io_error("write local media manifest"))?;
            manifests.insert(
                rendition.kind,
                format!(
                    "media/themes/{}/{}/media.json",
                    definition.id, rendition.kind
                ),
            );
        }
        if theme_root.exists() {
            fs::remove_dir_all(&theme_root).map_err(io_error("replace local media cache"))?;
        }
        if let Some(parent) = theme_root.parent() {
            fs::create_dir_all(parent).map_err(io_error("create local theme cache parent"))?;
        }
        fs::rename(&staging, &theme_root).map_err(io_error("activate local media cache"))?;
        Ok(PreparedStreamsV1 {
            main: manifests.remove("main").unwrap(),
            sub: manifests.remove("sub").unwrap(),
            third: manifests.remove("third").unwrap(),
        })
    })();
    if result.is_err() && staging.starts_with(&paths.cache) {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn encode_rendition(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    config: Rendition,
) -> Result<(), LocalMaterialError> {
    let scale = format!(
        "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black,fps={}",
        config.width, config.height, config.width, config.height, config.fps
    );
    let gop = config.fps * 2;
    let status = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "warning", "-y", "-i"])
        .arg(input)
        .args(["-map", "0:v:0", "-an", "-vf"])
        .arg(scale)
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-profile:v",
            "high",
            "-pix_fmt",
            "yuv420p",
        ])
        .args([
            "-b:v",
            &config.bitrate.to_string(),
            "-maxrate",
            &config.bitrate.to_string(),
            "-bufsize",
            &(config.bitrate * 2).to_string(),
        ])
        .args([
            "-g",
            &gop.to_string(),
            "-keyint_min",
            &gop.to_string(),
            "-sc_threshold",
            "0",
            "-bf",
            "0",
        ])
        .args([
            "-x264-params",
            "repeat-headers=1:aud=1:open-gop=0",
            "-fps_mode",
            "cfr",
            "-f",
            "h264",
        ])
        .arg(output)
        .status()
        .map_err(|error| {
            local_error(
                "device_simulator.local_materials.ffmpeg_unavailable",
                format!("failed to start {}: {error}", ffmpeg.display()),
            )
        })?;
    if !status.success() {
        return Err(local_error(
            "device_simulator.local_materials.ffmpeg_failed",
            format!("FFmpeg failed while preparing {}", input.display()),
        ));
    }
    Ok(())
}

fn normalize_annex_b(
    raw_path: &Path,
    target: &Path,
    definition: &UserThemeV1,
    config: Rendition,
) -> Result<MediaManifestV1, LocalMaterialError> {
    let bytes = fs::read(raw_path).map_err(io_error("read FFmpeg output"))?;
    let nals = annex_b_nals(&bytes)?;
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut frames = Vec::<FrameIndex>::new();
    let mut current = Vec::<NalIndex>::new();
    let mut current_offset = 0_u64;
    let mut parameter_sets = Vec::new();
    for nal in nals {
        let nal_type = nal[0] & 0x1f;
        if nal_type == 9 && !current.is_empty() {
            finish_frame(&mut frames, &mut current, current_offset, config.fps)?;
            current_offset = normalized.len() as u64;
        }
        let offset = normalized.len() as u64;
        normalized.extend_from_slice(nal);
        let nal_index = current.len();
        current.push(NalIndex {
            offset,
            length: nal.len() as u64,
            nal_type,
        });
        if let Some(kind) = match nal_type {
            7 => Some(ParameterSetKind::Sps),
            8 => Some(ParameterSetKind::Pps),
            _ => None,
        } {
            if !parameter_sets
                .iter()
                .any(|item: &ParameterSetRef| item.kind == kind)
            {
                parameter_sets.push(ParameterSetRef {
                    kind,
                    frame_index: frames.len(),
                    nal_index,
                });
            }
        }
    }
    if !current.is_empty() {
        finish_frame(&mut frames, &mut current, current_offset, config.fps)?;
    }
    if frames.len() < 2 || !frames[0].keyframe || parameter_sets.len() != 2 {
        return Err(local_error(
            "device_simulator.local_materials.index_invalid",
            "encoded H.264 stream has no usable frame sequence or SPS/PPS",
        ));
    }
    let media_name = format!("{}.h264", config.kind);
    fs::write(target.join(&media_name), &normalized)
        .map_err(io_error("write normalized local media"))?;
    Ok(MediaManifestV1 {
        schema_version: 1,
        id: format!("{}-{}", definition.id, config.kind),
        codec: Codec::H264,
        clock_rate: VIDEO_CLOCK_RATE,
        payload_type: config.payload_type,
        frame_rate_numerator: config.fps,
        frame_rate_denominator: 1,
        recommended_bitrate_bps: config.bitrate,
        media_file: media_name,
        media_file_size: normalized.len() as u64,
        media_file_sha256: format!("{:x}", Sha256::digest(&normalized)),
        frames,
        parameter_sets,
        evidence: MediaEvidence {
            source_kind: EvidenceSourceKind::SyntheticFixture,
            pcap_source_id: None,
            sdp_source_id: None,
            compatibility: MediaCompatibility::Unverified,
            verified_platforms: Vec::new(),
            differences: Vec::new(),
        },
    })
}

fn finish_frame(
    frames: &mut Vec<FrameIndex>,
    nals: &mut Vec<NalIndex>,
    offset: u64,
    fps: u32,
) -> Result<(), LocalMaterialError> {
    let end = nals
        .last()
        .map(|nal| nal.offset + nal.length)
        .unwrap_or(offset);
    let keyframe = nals.iter().any(|nal| nal.nal_type == 5);
    frames.push(FrameIndex {
        offset,
        length: end.saturating_sub(offset),
        duration_ticks: VIDEO_CLOCK_RATE / fps,
        keyframe,
        nals: std::mem::take(nals),
    });
    Ok(())
}

fn annex_b_nals(bytes: &[u8]) -> Result<Vec<&[u8]>, LocalMaterialError> {
    fn start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
        (from..bytes.len().saturating_sub(2)).find_map(|index| {
            if bytes[index] == 0 && bytes[index + 1] == 0 && bytes[index + 2] == 1 {
                Some((index, 3))
            } else if index + 3 < bytes.len()
                && bytes[index] == 0
                && bytes[index + 1] == 0
                && bytes[index + 2] == 0
                && bytes[index + 3] == 1
            {
                Some((index, 4))
            } else {
                None
            }
        })
    }
    let mut marker = start_code(bytes, 0).ok_or_else(|| {
        local_error(
            "device_simulator.local_materials.index_invalid",
            "FFmpeg H.264 output has no Annex B start code",
        )
    })?;
    let mut result = Vec::new();
    loop {
        let begin = marker.0 + marker.1;
        let next = start_code(bytes, begin);
        let mut end = next.map_or(bytes.len(), |value| value.0);
        while end > begin && bytes[end - 1] == 0 {
            end -= 1;
        }
        if end > begin {
            result.push(&bytes[begin..end]);
        }
        let Some(value) = next else {
            break;
        };
        marker = value;
    }
    if result.is_empty() {
        return Err(local_error(
            "device_simulator.local_materials.index_invalid",
            "FFmpeg H.264 output contains no NAL units",
        ));
    }
    Ok(result)
}

fn read_user_catalog_or_scan(
    paths: &LocalMaterialPaths,
) -> Result<UserCatalogV1, LocalMaterialError> {
    let path = paths.root.join(USER_CATALOG_NAME);
    let bytes = fs::read(&path).map_err(io_error("read local material catalog"))?;
    let configured: UserCatalogV1 = serde_json::from_slice(&bytes).map_err(|error| {
        local_error(
            "device_simulator.local_materials.catalog_invalid",
            error.to_string(),
        )
    })?;
    if configured.schema_version != LOCAL_CATALOG_SCHEMA {
        return Err(local_error(
            "device_simulator.local_materials.catalog_invalid",
            "unsupported local material catalog schema",
        ));
    }
    if !configured.themes.is_empty() {
        return Ok(configured);
    }
    let mut videos = fs::read_dir(&paths.videos)
        .map_err(io_error("scan local videos"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("mp4"))
        })
        .collect::<Vec<_>>();
    videos.sort_by_key(|entry| entry.file_name());
    let themes = videos
        .into_iter()
        .map(|entry| -> Result<UserThemeV1, LocalMaterialError> {
            let file = entry.file_name().to_string_lossy().into_owned();
            let stem = entry
                .path()
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Local video")
                .to_owned();
            let source_content_id = hash_source_file(&entry.path())?;
            Ok(UserThemeV1 {
                id: format!("local-{}", &source_content_id[..12]),
                display_name: stem,
                file,
                source_content_id: Some(source_content_id),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(UserCatalogV1 {
        schema_version: LOCAL_CATALOG_SCHEMA,
        default_theme_id: themes.first().map(|theme| theme.id.clone()),
        themes,
    })
}

fn hash_source_file(path: &Path) -> Result<String, LocalMaterialError> {
    use std::io::{BufReader, Read};

    let file = fs::File::open(path).map_err(io_error("open local MP4 for hashing"))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut digest = Sha256::new();
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(io_error("hash local MP4"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn read_prepared_catalog(
    paths: &LocalMaterialPaths,
) -> Result<Option<PreparedCatalogV1>, LocalMaterialError> {
    read_prepared_catalog_at(&paths.prepared_catalog())
}

fn read_prepared_catalog_at(path: &Path) -> Result<Option<PreparedCatalogV1>, LocalMaterialError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("read prepared local catalog")(error)),
    };
    let catalog: PreparedCatalogV1 = serde_json::from_slice(&bytes).map_err(|error| {
        local_error(
            "device_simulator.local_materials.catalog_invalid",
            error.to_string(),
        )
    })?;
    if catalog.schema_version != LOCAL_CATALOG_SCHEMA {
        return Err(local_error(
            "device_simulator.local_materials.catalog_invalid",
            "unsupported prepared local catalog schema",
        ));
    }
    Ok(Some(catalog))
}

fn resolve_source_video(
    paths: &LocalMaterialPaths,
    file: &str,
) -> Result<PathBuf, LocalMaterialError> {
    let relative = Path::new(file);
    if relative.is_absolute()
        || relative.components().count() != 1
        || relative
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("mp4"))
    {
        return Err(local_error(
            "device_simulator.local_materials.video_path_invalid",
            format!("local video path must be one MP4 filename: {file}"),
        ));
    }
    Ok(paths.videos.join(relative))
}

fn validate_theme_definition(theme: &UserThemeV1) -> Result<(), LocalMaterialError> {
    let valid_id = theme.id.starts_with("local-")
        && theme.id.len() <= 64
        && theme
            .id
            .bytes()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'-');
    if !valid_id || theme.display_name.trim().is_empty() || theme.display_name.len() > 160 {
        return Err(local_error(
            "device_simulator.local_materials.catalog_invalid",
            format!("invalid local theme {}", theme.id),
        ));
    }
    Ok(())
}

fn theme_streams_exist(paths: &LocalMaterialPaths, theme: &PreparedThemeV1) -> bool {
    [
        RuntimeMediaKind::Main,
        RuntimeMediaKind::Sub,
        RuntimeMediaKind::Third,
    ]
    .into_iter()
    .all(|kind| paths.cache.join(theme.streams.get(kind)).is_file())
}

fn resolve_ffmpeg() -> Result<PathBuf, LocalMaterialError> {
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            let sibling = parent.join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            });
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if Command::new(name)
        .arg("-version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return Ok(PathBuf::from(name));
    }
    Err(local_error(
        "device_simulator.local_materials.ffmpeg_unavailable",
        "place ffmpeg beside the application executable or add it to PATH",
    ))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), LocalMaterialError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        local_error(
            "device_simulator.local_materials.catalog_invalid",
            error.to_string(),
        )
    })?;
    let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, bytes).map_err(io_error("write local catalog staging file"))?;
    let backup = path.with_extension(format!("json.{}.bak", uuid::Uuid::new_v4().simple()));
    if path.exists() {
        fs::rename(path, &backup).map_err(io_error("stage previous local catalog"))?;
    }
    if let Err(source) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(io_error("activate local catalog")(source));
    }
    if backup.exists() {
        fs::remove_file(backup).map_err(io_error("remove previous local catalog"))?;
    }
    Ok(())
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> LocalMaterialError {
    move |error| {
        local_error(
            "device_simulator.local_materials.io_failed",
            format!("{context}: {error}"),
        )
    }
}

fn local_error(code: &'static str, message: impl Into<String>) -> LocalMaterialError {
    LocalMaterialError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_directory_changes_only_the_material_root() {
        let app_data = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        let paths = LocalMaterialPaths::from_configured_directory(
            app_data.path(),
            Some(custom.path().to_str().unwrap()),
        );

        assert_eq!(paths.root, custom.path());
        assert_eq!(paths.videos, custom.path().join("videos"));
        assert_eq!(
            paths.user_alarm_images,
            app_data.path().join("device-simulator/user-alarm-images")
        );
    }

    #[test]
    fn migration_verifies_files_and_preserves_unmanaged_entries() {
        let root = tempfile::tempdir().unwrap();
        let source = LocalMaterialPaths::from_root(root.path(), &root.path().join("old"));
        let destination = LocalMaterialPaths::from_root(root.path(), &root.path().join("new"));
        source.ensure_layout().unwrap();
        destination.ensure_layout().unwrap();
        fs::write(source.videos.join("sample.mp4"), b"video-material").unwrap();
        fs::write(source.remote_cache.join("state.json"), b"remote-state").unwrap();
        fs::write(source.root.join("keep-me.txt"), b"unmanaged").unwrap();
        fs::write(destination.videos.join("sample.mp4"), b"stale").unwrap();

        let report = copy_local_materials_verified(&source, &destination).unwrap();
        assert_eq!(
            fs::read(destination.videos.join("sample.mp4")).unwrap(),
            b"video-material"
        );
        assert!(report.copied_files >= 2);
        assert!(report.reused_files >= 1);

        let removed = remove_verified_local_materials(&source, &destination).unwrap();
        assert!(removed >= 3);
        assert!(!source.videos.exists());
        assert!(!source.remote_cache.exists());
        assert_eq!(
            fs::read(source.root.join("keep-me.txt")).unwrap(),
            b"unmanaged"
        );
    }

    #[test]
    fn cleanup_keeps_an_old_file_when_the_destination_changed() {
        let root = tempfile::tempdir().unwrap();
        let source = LocalMaterialPaths::from_root(root.path(), &root.path().join("old"));
        let destination = LocalMaterialPaths::from_root(root.path(), &root.path().join("new"));
        source.ensure_layout().unwrap();
        fs::write(source.videos.join("sample.mp4"), b"video-material").unwrap();
        copy_local_materials_verified(&source, &destination).unwrap();
        fs::write(destination.videos.join("sample.mp4"), b"changed-after-copy").unwrap();

        let error = remove_verified_local_materials(&source, &destination).unwrap_err();
        assert_eq!(
            error.code,
            "device_simulator.local_materials.migration_cleanup_unverified"
        );
        assert!(source.videos.join("sample.mp4").exists());
    }
    use image::{Rgb, RgbImage};
    use tempfile::TempDir;

    fn write_alarm_png(path: &Path, color: [u8; 3]) {
        RgbImage::from_pixel(16, 12, Rgb(color)).save(path).unwrap();
    }

    #[test]
    fn grouped_alarm_materials_preserve_roles_and_support_numbered_sets() {
        let root = TempDir::new().unwrap();
        let paths = LocalMaterialPaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        for (group, roles, seed) in [
            ("person/person-001", &["scene", "person"][..], 10u8),
            ("face/face-001", &["scene", "face"][..], 30u8),
            ("face/face-002", &["scene", "face"][..], 50u8),
            ("car/car-001", &["scene", "vehicle", "plate"][..], 70u8),
            ("nonmotor/nonmotor-001", &["scene", "nonmotor"][..], 100u8),
        ] {
            let directory = paths.alarm_images.join(group);
            fs::create_dir_all(&directory).unwrap();
            for (index, role) in roles.iter().enumerate() {
                write_alarm_png(
                    &directory.join(format!("{role}.png")),
                    [seed.saturating_add(index as u8), 20, 30],
                );
            }
        }

        refresh_local_alarm_materials(&paths).unwrap();
        let loaded = load_local_alarm_images(&paths).unwrap();
        assert_eq!(loaded["person"]["normal"].len(), 1);
        assert_eq!(loaded["person"]["normal"][0].len(), 2);
        assert_eq!(loaded["face"]["normal"].len(), 2);
        assert_eq!(loaded["face"]["normal"][0].len(), 2);
        assert_eq!(loaded["face"]["normal"][1].len(), 2);
        assert_ne!(loaded["face"]["normal"][0], loaded["face"]["normal"][1]);
        assert_eq!(loaded["car"]["normal"][0].len(), 3);
        assert_eq!(loaded["nonmotor"]["normal"][0].len(), 2);
    }

    #[test]
    fn grouped_alarm_material_rejects_a_missing_role() {
        let root = TempDir::new().unwrap();
        let paths = LocalMaterialPaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        let directory = paths.alarm_images.join("car/car-001");
        fs::create_dir_all(&directory).unwrap();
        write_alarm_png(&directory.join("scene.png"), [1, 2, 3]);
        write_alarm_png(&directory.join("vehicle.png"), [4, 5, 6]);

        let error = refresh_local_alarm_materials(&paths).unwrap_err();
        assert_eq!(
            error.code,
            "device_simulator.local_materials.alarm_images_incomplete"
        );
        assert!(error.message.contains("plate.jpg"));
    }

    #[test]
    fn empty_catalog_scans_mp4_files_into_stable_local_ids() {
        let root = TempDir::new().unwrap();
        let paths = LocalMaterialPaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        fs::write(paths.videos.join("城市夜景.mp4"), b"fixture").unwrap();
        let catalog = read_user_catalog_or_scan(&paths).unwrap();
        assert_eq!(catalog.themes.len(), 1);
        assert!(catalog.themes[0].id.starts_with("local-"));
        assert_eq!(catalog.themes[0].display_name, "城市夜景");
    }

    #[test]
    fn renamed_mp4_keeps_content_id_and_reuses_prepared_streams() {
        let root = TempDir::new().unwrap();
        let paths = LocalMaterialPaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        let old_source = paths.videos.join("old-name.mp4");
        fs::write(&old_source, b"same video bytes").unwrap();
        let first = read_user_catalog_or_scan(&paths).unwrap();
        let definition = &first.themes[0];
        let streams = PreparedStreamsV1 {
            main: "media/reused/main.json".into(),
            sub: "media/reused/sub.json".into(),
            third: "media/reused/third.json".into(),
        };
        for relative in [&streams.main, &streams.sub, &streams.third] {
            let path = paths.cache.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"cached").unwrap();
        }
        write_json_atomic(
            &paths.prepared_catalog(),
            &PreparedCatalogV1 {
                schema_version: LOCAL_CATALOG_SCHEMA,
                default_theme_id: Some(definition.id.clone()),
                themes: vec![PreparedThemeV1 {
                    id: definition.id.clone(),
                    display_name: definition.display_name.clone(),
                    source_file: definition.file.clone(),
                    source_size: fs::metadata(&old_source).unwrap().len(),
                    source_modified_ms: 0,
                    source_content_id: definition.source_content_id.clone(),
                    streams: streams.clone(),
                }],
                alarm_images: BTreeMap::new(),
                alarm_image_groups: BTreeMap::new(),
            },
        )
        .unwrap();

        fs::rename(old_source, paths.videos.join("new-name.mp4")).unwrap();
        let refreshed = refresh_local_media(&paths).unwrap();
        assert_eq!(refreshed[0].id, definition.id);
        assert_eq!(refreshed[0].display_name.as_deref(), Some("new-name"));
        let prepared = read_prepared_catalog(&paths).unwrap().unwrap();
        assert_eq!(prepared.themes[0].streams.main, streams.main);
        assert_eq!(prepared.themes[0].source_file, "new-name.mp4");
    }

    #[test]
    fn annex_b_normalizer_splits_nals_and_rejects_missing_start_code() {
        let bytes = [
            0, 0, 0, 1, 0x09, 0xf0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x65, 1,
        ];
        let nals = annex_b_nals(&bytes).unwrap();
        assert_eq!(
            nals.iter().map(|nal| nal[0] & 0x1f).collect::<Vec<_>>(),
            [9, 7, 5]
        );
        assert!(annex_b_nals(b"not annex b").is_err());
    }
}
