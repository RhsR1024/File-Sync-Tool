use super::cache::AssetStorePaths;
use super::catalog::CatalogV1;
use super::signature::{verify_signed_catalog, TrustedCatalogKey};
use super::validation::validate_catalog;
use semver::Version;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CATALOG_SIGNATURE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedCatalog {
    pub catalog: CatalogV1,
    pub catalog_bytes: Vec<u8>,
    pub signature_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogCacheError {
    pub code: &'static str,
    pub message: String,
}

impl CatalogCacheError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CatalogCacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CatalogCacheError {}

pub async fn fetch_and_cache_signed_catalog(
    client: &reqwest::Client,
    catalog_url: &reqwest::Url,
    paths: &AssetStorePaths,
    trusted_keys: &[TrustedCatalogKey],
    current_app_version: &Version,
) -> Result<CachedCatalog, CatalogCacheError> {
    paths
        .ensure_layout()
        .map_err(|error| CatalogCacheError::new(error.code, error.message))?;
    let signature_url = signature_url(catalog_url);
    let (catalog_result, signature_result) = tokio::join!(
        fetch_bounded(client, catalog_url, MAX_CATALOG_BYTES),
        fetch_bounded(client, &signature_url, MAX_CATALOG_SIGNATURE_BYTES)
    );
    let catalog_bytes = catalog_result?;
    let signature_bytes = signature_result?;
    let cached = verify_pair(
        catalog_bytes,
        signature_bytes,
        trusted_keys,
        current_app_version,
    )?;

    // Blocking metadata durability work stays out of the async executor.
    let catalog_path = paths.catalog.clone();
    let signature_path = paths.catalog_signature.clone();
    let catalog_to_write = cached.catalog_bytes.clone();
    let signature_to_write = cached.signature_bytes.clone();
    tokio::task::spawn_blocking(move || {
        write_pair_recoverable(
            &catalog_path,
            &catalog_to_write,
            &signature_path,
            &signature_to_write,
        )
    })
    .await
    .map_err(|error| {
        CatalogCacheError::new(
            "device_simulator.assets.catalog_cache_io",
            format!("catalog cache task failed: {error}"),
        )
    })??;
    Ok(cached)
}

/// Loads the last verified catalog without network access. A pair left in the
/// backup location by an interrupted replacement is accepted after full
/// signature and schema validation.
pub fn load_cached_signed_catalog(
    paths: &AssetStorePaths,
    trusted_keys: &[TrustedCatalogKey],
    current_app_version: &Version,
) -> Result<Option<CachedCatalog>, CatalogCacheError> {
    match read_and_verify_pair(
        &paths.catalog,
        &paths.catalog_signature,
        trusted_keys,
        current_app_version,
    ) {
        Ok(Some(catalog)) => Ok(Some(catalog)),
        Ok(None) => read_and_verify_pair(
            &backup_path(&paths.catalog),
            &backup_path(&paths.catalog_signature),
            trusted_keys,
            current_app_version,
        ),
        Err(primary_error) => match read_and_verify_pair(
            &backup_path(&paths.catalog),
            &backup_path(&paths.catalog_signature),
            trusted_keys,
            current_app_version,
        ) {
            Ok(Some(catalog)) => Ok(Some(catalog)),
            _ => Err(primary_error),
        },
    }
}

pub fn load_previous_cached_catalog(
    paths: &AssetStorePaths,
    trusted_keys: &[TrustedCatalogKey],
    current_app_version: &Version,
) -> Result<Option<CachedCatalog>, CatalogCacheError> {
    read_and_verify_pair(
        &backup_path(&paths.catalog),
        &backup_path(&paths.catalog_signature),
        trusted_keys,
        current_app_version,
    )
}

fn signature_url(catalog_url: &reqwest::Url) -> reqwest::Url {
    let mut url = catalog_url.clone();
    url.set_path(&format!("{}.sig", catalog_url.path()));
    url
}

async fn fetch_bounded(
    client: &reqwest::Client,
    url: &reqwest::Url,
    limit: usize,
) -> Result<Vec<u8>, CatalogCacheError> {
    let mut response = client.get(url.clone()).send().await.map_err(|error| {
        CatalogCacheError::new(
            "device_simulator.assets.server_unreachable",
            format!("failed to fetch {url}: {error}"),
        )
    })?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(CatalogCacheError::new(
            "device_simulator.assets.authentication_unsupported",
            format!(
                "asset server returned HTTP {status}; application credentials are not supported"
            ),
        ));
    }
    if !status.is_success() {
        return Err(CatalogCacheError::new(
            "device_simulator.assets.http_status",
            format!("asset server returned HTTP {status} for {url}"),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(CatalogCacheError::new(
            "device_simulator.assets.catalog_size_exceeded",
            format!("response from {url} exceeds {limit} bytes"),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        CatalogCacheError::new(
            "device_simulator.assets.download_interrupted",
            format!("catalog response stream failed: {error}"),
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(CatalogCacheError::new(
                "device_simulator.assets.catalog_size_exceeded",
                format!("response from {url} exceeds {limit} bytes"),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn read_and_verify_pair(
    catalog_path: &Path,
    signature_path: &Path,
    trusted_keys: &[TrustedCatalogKey],
    current_app_version: &Version,
) -> Result<Option<CachedCatalog>, CatalogCacheError> {
    let catalog_bytes = match fs::read(catalog_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("read cached catalog", error)),
    };
    let signature_bytes = fs::read(signature_path)
        .map_err(|error| io_error("read cached catalog signature", error))?;
    verify_pair(
        catalog_bytes,
        signature_bytes,
        trusted_keys,
        current_app_version,
    )
    .map(Some)
}

fn verify_pair(
    catalog_bytes: Vec<u8>,
    signature_bytes: Vec<u8>,
    trusted_keys: &[TrustedCatalogKey],
    current_app_version: &Version,
) -> Result<CachedCatalog, CatalogCacheError> {
    let catalog =
        verify_signed_catalog(&catalog_bytes, &signature_bytes, trusted_keys).map_err(|error| {
            CatalogCacheError::new(error.code, format!("catalog signature failed: {error}"))
        })?;
    validate_catalog(&catalog, current_app_version).map_err(|error| {
        CatalogCacheError::new(error.code, format!("catalog validation failed: {error}"))
    })?;
    Ok(CachedCatalog {
        catalog,
        catalog_bytes,
        signature_bytes,
    })
}

fn write_pair_recoverable(
    catalog_path: &Path,
    catalog_bytes: &[u8],
    signature_path: &Path,
    signature_bytes: &[u8],
) -> Result<(), CatalogCacheError> {
    let parent = catalog_path.parent().ok_or_else(|| {
        CatalogCacheError::new(
            "device_simulator.assets.catalog_cache_io",
            "catalog cache path has no parent",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| io_error("create catalog cache", error))?;
    let catalog_new = new_path(catalog_path);
    let signature_new = new_path(signature_path);
    write_synced(&catalog_new, catalog_bytes)?;
    write_synced(&signature_new, signature_bytes)?;

    let catalog_backup = backup_path(catalog_path);
    let signature_backup = backup_path(signature_path);
    remove_if_exists(&catalog_backup)?;
    remove_if_exists(&signature_backup)?;
    let had_pair = catalog_path.exists() && signature_path.exists();
    if had_pair {
        fs::rename(catalog_path, &catalog_backup)
            .map_err(|error| io_error("backup cached catalog", error))?;
        if let Err(error) = fs::rename(signature_path, &signature_backup) {
            let _ = fs::rename(&catalog_backup, catalog_path);
            return Err(io_error("backup cached catalog signature", error));
        }
    }

    if let Err(error) = fs::rename(&catalog_new, catalog_path) {
        restore_pair(
            catalog_path,
            signature_path,
            &catalog_backup,
            &signature_backup,
            had_pair,
        );
        return Err(io_error("activate cached catalog", error));
    }
    if let Err(error) = fs::rename(&signature_new, signature_path) {
        let _ = fs::remove_file(catalog_path);
        restore_pair(
            catalog_path,
            signature_path,
            &catalog_backup,
            &signature_backup,
            had_pair,
        );
        return Err(io_error("activate cached catalog signature", error));
    }
    // Keep exactly one prior signed pair for offline rollback. The next update
    // replaces these backups only after its new files are fully durable.
    Ok(())
}

fn restore_pair(
    catalog_path: &Path,
    signature_path: &Path,
    catalog_backup: &Path,
    signature_backup: &Path,
    had_pair: bool,
) {
    if had_pair {
        let _ = fs::rename(catalog_backup, catalog_path);
        let _ = fs::rename(signature_backup, signature_path);
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), CatalogCacheError> {
    let mut file =
        File::create(path).map_err(|error| io_error("create catalog cache file", error))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error("persist catalog cache file", error))
}

fn remove_if_exists(path: &Path) -> Result<(), CatalogCacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove stale catalog cache file", error)),
    }
}

fn new_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.new",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn io_error(action: &str, error: std::io::Error) -> CatalogCacheError {
    CatalogCacheError::new(
        "device_simulator.assets.catalog_cache_io",
        format!("failed to {action}: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::download::build_asset_http_client;
    use crate::device_simulator::assets::signature::{
        CatalogSignatureAlgorithm, CatalogSignatureV1, CATALOG_SIGNATURE_VERSION,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn signed_catalog() -> (Vec<u8>, Vec<u8>, TrustedCatalogKey) {
        let catalog = serde_json::to_vec(&json!({
            "schema_version": 1,
            "generated_at": "2026-07-18T12:00:00+08:00",
            "engine_api": 1,
            "packs": [],
            "profiles": []
        }))
        .unwrap();
        let signing_key = SigningKey::from_bytes(&[11_u8; 32]);
        let signature = signing_key.sign(&catalog);
        let envelope = serde_json::to_vec(&CatalogSignatureV1 {
            version: CATALOG_SIGNATURE_VERSION,
            algorithm: CatalogSignatureAlgorithm::Ed25519,
            key_id: "assets-test-catalog".into(),
            catalog_sha256: format!("{:x}", Sha256::digest(&catalog)),
            signature: BASE64_STANDARD.encode(signature.to_bytes()),
        })
        .unwrap();
        let key = TrustedCatalogKey {
            key_id: "assets-test-catalog".into(),
            public_key: signing_key.verifying_key().to_bytes(),
        };
        (catalog, envelope, key)
    }

    #[tokio::test]
    async fn fetches_verifies_caches_and_loads_offline() {
        let (catalog, signature, key) = signed_catalog();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/catalog-v1.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(catalog.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/catalog-v1.json.sig"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(signature.clone()))
            .mount(&server)
            .await;
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        let fetched = fetch_and_cache_signed_catalog(
            &build_asset_http_client().unwrap(),
            &reqwest::Url::parse(&format!("{}/catalog-v1.json", server.uri())).unwrap(),
            &paths,
            &[key.clone()],
            &Version::new(1, 2, 0),
        )
        .await
        .unwrap();
        assert_eq!(fetched.catalog_bytes, catalog);

        let offline = load_cached_signed_catalog(&paths, &[key], &Version::new(1, 2, 0))
            .unwrap()
            .unwrap();
        assert_eq!(offline.signature_bytes, signature);
    }

    #[test]
    fn offline_load_rejects_tampering_and_recovers_backup_pair() {
        let (catalog, signature, key) = signed_catalog();
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        fs::write(backup_path(&paths.catalog), &catalog).unwrap();
        fs::write(backup_path(&paths.catalog_signature), &signature).unwrap();
        let previous = load_previous_cached_catalog(&paths, &[key.clone()], &Version::new(1, 2, 0))
            .unwrap()
            .unwrap();
        assert_eq!(previous.catalog_bytes, catalog);
        let recovered = load_cached_signed_catalog(&paths, &[key.clone()], &Version::new(1, 2, 0))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.catalog_bytes, catalog);

        fs::write(&paths.catalog, b"tampered").unwrap();
        fs::write(&paths.catalog_signature, &signature).unwrap();
        let recovered = load_cached_signed_catalog(&paths, &[key], &Version::new(1, 2, 0))
            .unwrap()
            .unwrap();
        assert_eq!(recovered.catalog_bytes, catalog);
    }
}
