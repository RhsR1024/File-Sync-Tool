use crate::device_simulator::assets::catalog::{PackManifest, PackRef};
use crate::device_simulator::assets::validation::validate_pack_path;
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
            let expected = item.pack_ref()?;
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

        let media_root = &packs
            .get("media-h264-live")
            .expect("required media pack checked")
            .directory;
        let cache = MediaPackCache::new();
        let mut media = BTreeMap::new();
        for kind in [
            RuntimeMediaKind::Main,
            RuntimeMediaKind::Sub,
            RuntimeMediaKind::Third,
        ] {
            let pack = cache
                .load(media_root, kind.manifest_path())
                .map_err(|source| error(source.code, source.message))?;
            media.insert(kind, pack);
        }

        Ok(Self {
            packs,
            manifests,
            profiles,
            media,
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
    use crate::device_simulator::profiles::scope::FirstReleaseProfileId;

    #[test]
    fn pinned_pack_contract_rejects_relative_paths_and_invalid_versions() {
        let relative = PinnedPackDirectory {
            id: "ipc-smart".into(),
            version: "1.0.0".into(),
            directory: PathBuf::from("relative"),
        };
        assert_eq!(
            RuntimeAssetLayout::load(&[relative], &["ipc-smart".into()])
                .unwrap_err()
                .code,
            "device_simulator.assets.pin_path_invalid"
        );
        assert!(PinnedPackDirectory {
            id: "ipc-smart".into(),
            version: "latest".into(),
            directory: PathBuf::from(r"C:\assets"),
        }
        .pack_ref()
        .is_err());
    }

    #[test]
    fn approved_release_fixture_loads_when_explicitly_configured() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.2".into());
        let pins = [
            "protocol-core",
            "media-h264-live",
            "ipc-custom",
            "ipc-smart",
            "nvr-common",
            "nvr-vehicle",
        ]
        .into_iter()
        .map(|id| PinnedPackDirectory {
            id: id.into(),
            version: version.clone(),
            directory: root.join(id).join(&version),
        })
        .collect::<Vec<_>>();
        let profiles = ["ipc-custom", "ipc-smart", "nvr-common", "nvr-vehicle"].map(str::to_owned);
        let layout = RuntimeAssetLayout::load(&pins, &profiles).unwrap();
        let smart = layout.profile(FirstReleaseProfileId::IpcSmart).unwrap();
        assert_eq!(smart.identity.model, "IPC3615SB-ADF28KM-I0");
        assert!(!layout.media(RuntimeMediaKind::Main).frames().is_empty());
        assert!(!layout.media(RuntimeMediaKind::Third).frames().is_empty());
    }
}
