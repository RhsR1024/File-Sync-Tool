use super::cache::AssetStorePaths;
use super::catalog::{CatalogPack, PackRef};
use reqwest::header::{CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

pub const DEFAULT_DOWNLOAD_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetDownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub resumed_from: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDownloadOutcome {
    pub archive_path: PathBuf,
    pub resumed_from: u64,
    pub downloaded_this_run: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDownloadError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl AssetDownloadError {
    fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

impl std::fmt::Display for AssetDownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetDownloadError {}

pub fn build_asset_http_client() -> Result<reqwest::Client, AssetDownloadError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .connect_timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|error| {
            AssetDownloadError::new(
                "device_simulator.assets.client_create_failed",
                format!("failed to create asset HTTP client: {error}"),
                false,
            )
        })
}

/// Downloads one immutable pack with resumable `.part` storage. Cancellation
/// deliberately keeps the partial file so a later explicit retry can resume.
pub async fn download_pack<F>(
    client: &reqwest::Client,
    asset_base_url: &reqwest::Url,
    paths: &AssetStorePaths,
    expected: &CatalogPack,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<AssetDownloadOutcome, AssetDownloadError>
where
    F: FnMut(AssetDownloadProgress) + Send,
{
    download_pack_with_retry(
        client,
        asset_base_url,
        paths,
        expected,
        cancel,
        DEFAULT_DOWNLOAD_ATTEMPTS,
        on_progress,
    )
    .await
}

pub async fn download_pack_with_retry<F>(
    client: &reqwest::Client,
    asset_base_url: &reqwest::Url,
    paths: &AssetStorePaths,
    expected: &CatalogPack,
    cancel: watch::Receiver<bool>,
    attempts: u8,
    mut on_progress: F,
) -> Result<AssetDownloadOutcome, AssetDownloadError>
where
    F: FnMut(AssetDownloadProgress) + Send,
{
    let attempts = attempts.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match download_pack_once(
            client,
            asset_base_url,
            paths,
            expected,
            cancel.clone(),
            &mut on_progress,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.retryable && attempt < attempts => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AssetDownloadError::new(
            "device_simulator.assets.download_failed",
            "asset download failed without an error",
            false,
        )
    }))
}

async fn download_pack_once<F>(
    client: &reqwest::Client,
    asset_base_url: &reqwest::Url,
    paths: &AssetStorePaths,
    expected: &CatalogPack,
    cancel: watch::Receiver<bool>,
    on_progress: &mut F,
) -> Result<AssetDownloadOutcome, AssetDownloadError>
where
    F: FnMut(AssetDownloadProgress) + Send,
{
    if *cancel.borrow() {
        return Err(cancelled());
    }
    paths
        .ensure_layout()
        .map_err(|error| AssetDownloadError::new(error.code, error.message, false))?;
    let pack_ref = PackRef {
        id: expected.id.clone(),
        version: expected.version.clone(),
    };
    let part_path = paths.archive_part_path(&pack_ref).map_err(cache_error)?;
    let archive_path = paths.archive_path(&pack_ref).map_err(cache_error)?;

    if archive_path.exists() {
        if verify_file(&archive_path, expected.size, &expected.sha256).await? {
            return Ok(AssetDownloadOutcome {
                archive_path,
                resumed_from: expected.size,
                downloaded_this_run: 0,
            });
        }
        tokio::fs::remove_file(&archive_path)
            .await
            .map_err(|error| io_error("remove invalid completed archive", error, false))?;
    }

    let (mut offset, mut hasher) = hash_partial_file(&part_path, expected.size).await?;
    let resumed_from = offset;
    let url = resolve_pack_url(asset_base_url, &expected.url)?;
    let mut request = client.get(url.clone());
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = request.send().await.map_err(|error| {
        AssetDownloadError::new(
            "device_simulator.assets.server_unreachable",
            format!("failed to download {url}: {error}"),
            true,
        )
    })?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AssetDownloadError::new(
            "device_simulator.assets.authentication_unsupported",
            format!(
                "asset server returned HTTP {status}; application credentials are not supported"
            ),
            false,
        ));
    }
    if status.as_u16() == 416 && offset == expected.size {
        return finalize_part(part_path, archive_path, expected, resumed_from).await;
    }
    if !status.is_success() {
        return Err(AssetDownloadError::new(
            "device_simulator.assets.http_status",
            format!("asset server returned HTTP {status} for {url}"),
            status.is_server_error() || matches!(status.as_u16(), 408 | 429),
        ));
    }

    if offset > 0 && status.as_u16() == 206 {
        validate_content_range(&response, offset, expected.size)?;
    } else if status.as_u16() == 200 {
        offset = 0;
        hasher = Sha256::new();
    } else if offset > 0 {
        return Err(AssetDownloadError::new(
            "device_simulator.assets.range_response_invalid",
            format!("asset server ignored an HTTP Range request with status {status}"),
            true,
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(offset == 0)
        .append(offset > 0)
        .open(&part_path)
        .await
        .map_err(|error| io_error("open partial asset archive", error, false))?;
    let mut downloaded = offset;
    on_progress(AssetDownloadProgress {
        downloaded,
        total: expected.size,
        resumed_from,
    });

    loop {
        if *cancel.borrow() {
            file.flush()
                .await
                .map_err(|error| io_error("flush cancelled partial archive", error, false))?;
            return Err(cancelled());
        }
        let chunk = response.chunk().await.map_err(|error| {
            AssetDownloadError::new(
                "device_simulator.assets.download_interrupted",
                format!("asset response stream failed: {error}"),
                true,
            )
        })?;
        let Some(chunk) = chunk else {
            break;
        };
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| size_mismatch("download byte count overflow"))?;
        if downloaded > expected.size {
            return Err(size_mismatch("download exceeded catalog size"));
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| io_error("write partial asset archive", error, false))?;
        hasher.update(&chunk);
        on_progress(AssetDownloadProgress {
            downloaded,
            total: expected.size,
            resumed_from,
        });
    }
    file.flush()
        .await
        .map_err(|error| io_error("flush partial asset archive", error, false))?;
    drop(file);

    if downloaded != expected.size {
        return Err(AssetDownloadError::new(
            "device_simulator.assets.download_incomplete",
            format!("downloaded {downloaded} bytes, expected {}", expected.size),
            true,
        ));
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected.sha256 {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(AssetDownloadError::new(
            "device_simulator.assets.archive_hash_mismatch",
            "downloaded archive SHA-256 does not match the signed catalog",
            true,
        ));
    }
    tokio::fs::rename(&part_path, &archive_path)
        .await
        .map_err(|error| io_error("finalize verified asset archive", error, false))?;
    Ok(AssetDownloadOutcome {
        archive_path,
        resumed_from,
        downloaded_this_run: downloaded.saturating_sub(resumed_from),
    })
}

fn resolve_pack_url(
    asset_base_url: &reqwest::Url,
    value: &str,
) -> Result<reqwest::Url, AssetDownloadError> {
    match reqwest::Url::parse(value) {
        Ok(url) => Ok(url),
        Err(_) => asset_base_url.join(value).map_err(|error| {
            AssetDownloadError::new(
                "device_simulator.assets.invalid_url",
                format!("failed to resolve pack URL '{value}': {error}"),
                false,
            )
        }),
    }
}

fn validate_content_range(
    response: &reqwest::Response,
    offset: u64,
    expected_size: u64,
) -> Result<(), AssetDownloadError> {
    let value = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            AssetDownloadError::new(
                "device_simulator.assets.range_response_invalid",
                "partial response has no valid Content-Range header",
                true,
            )
        })?;
    let prefix = format!("bytes {offset}-");
    let suffix = format!("/{expected_size}");
    if !value.starts_with(&prefix) || !value.ends_with(&suffix) {
        return Err(AssetDownloadError::new(
            "device_simulator.assets.range_response_invalid",
            format!("unexpected Content-Range '{value}'"),
            true,
        ));
    }
    Ok(())
}

async fn hash_partial_file(
    path: &Path,
    expected_size: u64,
) -> Result<(u64, Sha256), AssetDownloadError> {
    let mut hasher = Sha256::new();
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, hasher)),
        Err(error) => return Err(io_error("open partial asset archive", error, false)),
    };
    let size = file
        .metadata()
        .await
        .map_err(|error| io_error("inspect partial asset archive", error, false))?
        .len();
    if size > expected_size {
        drop(file);
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| io_error("remove oversized partial archive", error, false))?;
        return Ok((0, hasher));
    }
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|error| io_error("hash partial asset archive", error, false))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok((size, hasher))
}

async fn verify_file(
    path: &Path,
    expected_size: u64,
    expected_hash: &str,
) -> Result<bool, AssetDownloadError> {
    let (size, hasher) = hash_partial_file(path, expected_size).await?;
    Ok(size == expected_size && format!("{:x}", hasher.finalize()) == expected_hash)
}

async fn finalize_part(
    part_path: PathBuf,
    archive_path: PathBuf,
    expected: &CatalogPack,
    resumed_from: u64,
) -> Result<AssetDownloadOutcome, AssetDownloadError> {
    if !verify_file(&part_path, expected.size, &expected.sha256).await? {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(AssetDownloadError::new(
            "device_simulator.assets.archive_hash_mismatch",
            "completed partial archive does not match the signed catalog",
            true,
        ));
    }
    tokio::fs::rename(&part_path, &archive_path)
        .await
        .map_err(|error| io_error("finalize verified asset archive", error, false))?;
    Ok(AssetDownloadOutcome {
        archive_path,
        resumed_from,
        downloaded_this_run: 0,
    })
}

fn cache_error(error: super::cache::AssetCacheError) -> AssetDownloadError {
    AssetDownloadError::new(error.code, error.message, false)
}

fn io_error(action: &str, error: std::io::Error, retryable: bool) -> AssetDownloadError {
    AssetDownloadError::new(
        "device_simulator.assets.download_io",
        format!("failed to {action}: {error}"),
        retryable,
    )
}

fn size_mismatch(message: &str) -> AssetDownloadError {
    AssetDownloadError::new(
        "device_simulator.assets.archive_size_mismatch",
        message,
        true,
    )
}

fn cancelled() -> AssetDownloadError {
    AssetDownloadError::new(
        "device_simulator.assets.download_cancelled",
        "asset download was cancelled",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::PackKind;
    use semver::Version;
    use tempfile::TempDir;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn expected_pack(url: String, bytes: &[u8]) -> CatalogPack {
        CatalogPack {
            id: "ipc-smart".into(),
            version: Version::new(1, 0, 0),
            kind: PackKind::DeviceProfile,
            url,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            size: bytes.len() as u64,
            unpacked_size: 1,
            dependencies: vec![],
            min_app_version: Version::new(1, 2, 0),
        }
    }

    #[tokio::test]
    async fn downloads_and_verifies_a_fresh_pack() {
        let server = MockServer::start().await;
        let bytes = b"verified-pack";
        Mock::given(method("GET"))
            .and(path("/packs/ipc-smart.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&server)
            .await;
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        let pack = expected_pack("packs/ipc-smart.zip".into(), bytes);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let outcome = download_pack(
            &build_asset_http_client().unwrap(),
            &reqwest::Url::parse(&format!("{}/", server.uri())).unwrap(),
            &paths,
            &pack,
            cancel_rx,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(tokio::fs::read(outcome.archive_path).await.unwrap(), bytes);
        assert_eq!(outcome.downloaded_this_run, bytes.len() as u64);
    }

    #[tokio::test]
    async fn resumes_with_range_and_keeps_cancelled_partial_files() {
        let server = MockServer::start().await;
        let bytes = b"resume-this-pack";
        let offset = 7_u64;
        Mock::given(method("GET"))
            .and(path("/pack.zip"))
            .and(header("range", format!("bytes={offset}-")))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header(
                        "content-range",
                        format!("bytes {offset}-{}/{}", bytes.len() - 1, bytes.len()),
                    )
                    .set_body_bytes(bytes[offset as usize..].to_vec()),
            )
            .mount(&server)
            .await;
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        paths.ensure_layout().unwrap();
        let pack = expected_pack(format!("{}/pack.zip", server.uri()), bytes);
        let pack_ref = PackRef {
            id: pack.id.clone(),
            version: pack.version.clone(),
        };
        let part = paths.archive_part_path(&pack_ref).unwrap();
        tokio::fs::write(&part, &bytes[..offset as usize])
            .await
            .unwrap();
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let outcome = download_pack_with_retry(
            &build_asset_http_client().unwrap(),
            &reqwest::Url::parse(&format!("{}/", server.uri())).unwrap(),
            &paths,
            &pack,
            cancel_rx,
            1,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(outcome.resumed_from, offset);
        assert_eq!(tokio::fs::read(outcome.archive_path).await.unwrap(), bytes);

        let mut cancelled_pack = expected_pack("packs/cancel.zip".into(), b"cancel");
        cancelled_pack.id = "ipc-cancel".into();
        let cancelled_ref = PackRef {
            id: cancelled_pack.id.clone(),
            version: cancelled_pack.version.clone(),
        };
        let cancelled_part = paths.archive_part_path(&cancelled_ref).unwrap();
        tokio::fs::write(&cancelled_part, b"ca").await.unwrap();
        let (_cancel_tx, cancel_rx) = watch::channel(true);
        let error = download_pack_with_retry(
            &build_asset_http_client().unwrap(),
            &reqwest::Url::parse(&format!("{}/", server.uri())).unwrap(),
            &paths,
            &cancelled_pack,
            cancel_rx,
            1,
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.download_cancelled");
        assert!(cancelled_part.exists());
    }

    #[tokio::test]
    async fn rejects_authentication_and_hash_mismatch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/auth.zip"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/bad.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"bad"))
            .mount(&server)
            .await;
        let root = TempDir::new().unwrap();
        let paths = AssetStorePaths::from_app_data_dir(root.path());
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let auth_pack = expected_pack(format!("{}/auth.zip", server.uri()), b"auth");
        let error = download_pack_with_retry(
            &build_asset_http_client().unwrap(),
            &reqwest::Url::parse(&server.uri()).unwrap(),
            &paths,
            &auth_pack,
            cancel_rx.clone(),
            1,
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.code,
            "device_simulator.assets.authentication_unsupported"
        );

        let bad_pack = expected_pack(format!("{}/bad.zip", server.uri()), b"good");
        let error = download_pack_with_retry(
            &build_asset_http_client().unwrap(),
            &reqwest::Url::parse(&server.uri()).unwrap(),
            &paths,
            &bad_pack,
            cancel_rx,
            1,
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error.code,
            "device_simulator.assets.download_incomplete"
                | "device_simulator.assets.archive_hash_mismatch"
        ));
    }
}
