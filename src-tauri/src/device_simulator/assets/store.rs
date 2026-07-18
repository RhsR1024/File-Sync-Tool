use super::cache::{validate_installed_pack, ActiveAssetStateV1, AssetStore, AssetStorePaths};
use super::catalog::{CatalogPack, CatalogV1, PackRef};
use super::download::{download_pack, AssetDownloadProgress};
use super::resolver::resolve_profile_dependencies;
use std::path::PathBuf;
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetPreparationPhase {
    Resolving,
    CheckingDisk,
    Downloading,
    Installing,
    Activating,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPreparationProgress {
    pub phase: AssetPreparationPhase,
    pub current_pack: Option<PackRef>,
    pub completed_packs: usize,
    pub total_packs: usize,
    pub downloaded_bytes: u64,
    pub total_download_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPreparationOutcome {
    pub active_state: ActiveAssetStateV1,
    pub cache_hits: Vec<PackRef>,
    pub installed: Vec<PackRef>,
    pub quarantined: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetStoreError {
    pub code: &'static str,
    pub message: String,
}

impl AssetStoreError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AssetStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetStoreError {}

#[derive(Clone)]
pub struct AssetPreparationService {
    cache: AssetStore,
    client: reqwest::Client,
    asset_base_url: reqwest::Url,
}

impl AssetPreparationService {
    pub fn new(
        paths: AssetStorePaths,
        client: reqwest::Client,
        asset_base_url: reqwest::Url,
    ) -> Self {
        Self {
            cache: AssetStore::new(paths),
            client,
            asset_base_url,
        }
    }

    /// Resolves, downloads, installs, and activates a complete profile closure.
    /// A session that already holds an `AssetSessionPin` remains on its pinned
    /// versions; this activation affects only subsequent sessions.
    pub async fn prepare_profiles<F>(
        &self,
        catalog: &CatalogV1,
        profile_ids: &[String],
        cancel: watch::Receiver<bool>,
        mut on_progress: F,
    ) -> Result<AssetPreparationOutcome, AssetStoreError>
    where
        F: FnMut(AssetPreparationProgress) + Send,
    {
        on_progress(progress(AssetPreparationPhase::Resolving, None, 0, 0, 0, 0));
        let resolved = resolve_profile_dependencies(catalog, profile_ids).map_err(|error| {
            AssetStoreError::new(error.code, format!("asset resolution failed: {error}"))
        })?;
        let expected = resolved
            .iter()
            .map(|pack_ref| find_pack(catalog, pack_ref))
            .collect::<Result<Vec<_>, _>>()?;

        let mut cache_hits = Vec::new();
        let mut missing = Vec::new();
        for (pack_ref, pack) in resolved.iter().zip(expected.iter().copied()) {
            let directory = self.cache.paths().pack_dir(pack_ref).map_err(cache_error)?;
            let expected_owned = pack.clone();
            let valid = tokio::task::spawn_blocking(move || {
                directory.exists() && validate_installed_pack(&directory, &expected_owned).is_ok()
            })
            .await
            .map_err(join_error)?;
            if valid {
                cache_hits.push(pack_ref.clone());
            } else {
                missing.push((pack_ref.clone(), pack.clone()));
            }
        }

        on_progress(progress(
            AssetPreparationPhase::CheckingDisk,
            None,
            cache_hits.len(),
            resolved.len(),
            0,
            missing.iter().map(|(_, pack)| pack.size).sum(),
        ));
        if !missing.is_empty() {
            let cache = self.cache.clone();
            let preflight = missing
                .iter()
                .map(|(_, pack)| pack.clone())
                .collect::<Vec<_>>();
            tokio::task::spawn_blocking(move || {
                cache.ensure_install_space(&preflight.iter().collect::<Vec<_>>())
            })
            .await
            .map_err(join_error)?
            .map_err(cache_error)?;
        }

        let total_download_bytes = missing.iter().map(|(_, pack)| pack.size).sum();
        let mut completed_download_bytes = 0_u64;
        let mut installed = Vec::new();
        let mut quarantined = Vec::new();
        for (index, (pack_ref, pack)) in missing.iter().enumerate() {
            if *cancel.borrow() {
                return Err(AssetStoreError::new(
                    "device_simulator.assets.download_cancelled",
                    "asset preparation was cancelled",
                ));
            }
            let cache = self.cache.clone();
            let pack_for_quarantine = pack.clone();
            if let Some(path) = tokio::task::spawn_blocking(move || {
                cache.quarantine_invalid_pack(&pack_for_quarantine)
            })
            .await
            .map_err(join_error)?
            .map_err(cache_error)?
            {
                quarantined.push(path);
            }

            let completed_packs = cache_hits.len() + index;
            let pack_for_progress = pack_ref.clone();
            let outcome = download_pack(
                &self.client,
                &self.asset_base_url,
                self.cache.paths(),
                pack,
                cancel.clone(),
                |download: AssetDownloadProgress| {
                    on_progress(progress(
                        AssetPreparationPhase::Downloading,
                        Some(pack_for_progress.clone()),
                        completed_packs,
                        resolved.len(),
                        completed_download_bytes.saturating_add(download.downloaded),
                        total_download_bytes,
                    ));
                },
            )
            .await
            .map_err(|error| AssetStoreError::new(error.code, error.message))?;

            on_progress(progress(
                AssetPreparationPhase::Installing,
                Some(pack_ref.clone()),
                completed_packs,
                resolved.len(),
                completed_download_bytes.saturating_add(pack.size),
                total_download_bytes,
            ));
            let cache = self.cache.clone();
            let pack_to_install = pack.clone();
            let archive = outcome.archive_path.clone();
            tokio::task::spawn_blocking(move || cache.install_archive(&archive, &pack_to_install))
                .await
                .map_err(join_error)?
                .map_err(cache_error)?;
            tokio::fs::remove_file(&outcome.archive_path)
                .await
                .map_err(|error| {
                    AssetStoreError::new(
                        "device_simulator.assets.cache_io",
                        format!("failed to remove installed staging ZIP: {error}"),
                    )
                })?;
            installed.push(pack_ref.clone());
            completed_download_bytes = completed_download_bytes.saturating_add(pack.size);
        }

        on_progress(progress(
            AssetPreparationPhase::Activating,
            None,
            resolved.len(),
            resolved.len(),
            total_download_bytes,
            total_download_bytes,
        ));
        let cache = self.cache.clone();
        let catalog = catalog.clone();
        let profile_ids = profile_ids.to_vec();
        let active_state =
            tokio::task::spawn_blocking(move || cache.activate_profiles(&catalog, &profile_ids))
                .await
                .map_err(join_error)?
                .map_err(cache_error)?;
        Ok(AssetPreparationOutcome {
            active_state,
            cache_hits,
            installed,
            quarantined,
        })
    }
}

fn find_pack<'a>(
    catalog: &'a CatalogV1,
    pack_ref: &PackRef,
) -> Result<&'a CatalogPack, AssetStoreError> {
    catalog
        .packs
        .iter()
        .find(|pack| pack.id == pack_ref.id && pack.version == pack_ref.version)
        .ok_or_else(|| {
            AssetStoreError::new(
                "device_simulator.assets.missing_pack",
                format!("resolved pack '{pack_ref}' is absent from catalog"),
            )
        })
}

fn progress(
    phase: AssetPreparationPhase,
    current_pack: Option<PackRef>,
    completed_packs: usize,
    total_packs: usize,
    downloaded_bytes: u64,
    total_download_bytes: u64,
) -> AssetPreparationProgress {
    AssetPreparationProgress {
        phase,
        current_pack,
        completed_packs,
        total_packs,
        downloaded_bytes,
        total_download_bytes,
    }
}

fn cache_error(error: super::cache::AssetCacheError) -> AssetStoreError {
    AssetStoreError::new(error.code, error.message)
}

fn join_error(error: tokio::task::JoinError) -> AssetStoreError {
    AssetStoreError::new(
        "device_simulator.assets.blocking_task_failed",
        format!("asset filesystem task failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{
        non_commercial_usage, CatalogProfile, DeviceKind, PackFile, PackKind, PackManifest,
    };
    use crate::device_simulator::assets::download::build_asset_http_client;
    use semver::Version;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zip::write::SimpleFileOptions;

    fn pack_zip() -> Vec<u8> {
        let body = b"profile";
        let manifest = PackManifest {
            schema_version: 1,
            id: "ipc-smart".into(),
            version: Version::new(1, 0, 0),
            engine_api: 1,
            usage: non_commercial_usage(),
            files: vec![PackFile {
                path: "profiles/ipc-smart.json".into(),
                sha256: format!("{:x}", Sha256::digest(body)),
                size: body.len() as u64,
            }],
        };
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer
            .start_file("pack.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer
            .start_file("profiles/ipc-smart.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(body).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn prepares_then_reuses_a_complete_offline_ready_cache() {
        let zip = pack_zip();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/packs/ipc-smart.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip.clone()))
            .expect(1)
            .mount(&server)
            .await;
        let pack = CatalogPack {
            id: "ipc-smart".into(),
            version: Version::new(1, 0, 0),
            kind: PackKind::DeviceProfile,
            url: "packs/ipc-smart.zip".into(),
            sha256: format!("{:x}", Sha256::digest(&zip)),
            size: zip.len() as u64,
            unpacked_size: 7,
            dependencies: vec![],
            min_app_version: Version::new(1, 2, 0),
        };
        let catalog = CatalogV1 {
            schema_version: 1,
            generated_at: "2026-07-18T12:00:00+08:00".into(),
            engine_api: 1,
            profiles: vec![CatalogProfile {
                id: "ipc-smart".into(),
                device_kind: DeviceKind::Ipc,
                required_packs: vec![PackRef {
                    id: pack.id.clone(),
                    version: pack.version.clone(),
                }],
            }],
            packs: vec![pack],
        };
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        let service = AssetPreparationService::new(
            paths.clone(),
            build_asset_http_client().unwrap(),
            reqwest::Url::parse(&format!("{}/", server.uri())).unwrap(),
        );
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let first = service
            .prepare_profiles(&catalog, &["ipc-smart".into()], cancel_rx.clone(), |_| {})
            .await
            .unwrap();
        assert_eq!(first.installed.len(), 1);
        assert!(first.cache_hits.is_empty());

        let second = service
            .prepare_profiles(&catalog, &["ipc-smart".into()], cancel_rx, |_| {})
            .await
            .unwrap();
        assert!(second.installed.is_empty());
        assert_eq!(second.cache_hits.len(), 1);
        assert!(AssetStore::new(paths).pin_active(&catalog).is_ok());
    }
}
