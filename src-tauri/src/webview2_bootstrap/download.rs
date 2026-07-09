//! Orchestrate the installer download: fetch `.sha256`, stream the installer
//! to `<dir>\<name>.part` with verification, then rename to the final path.

use crate::download_verify::{self, DownloadError};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

use super::{server, sha256_file};

#[derive(Debug)]
pub enum InstallerDownloadError {
    Cancelled,
    Failed(String),
}

pub fn default_download_dir() -> PathBuf {
    std::env::temp_dir().join("file-sync-tool-webview2")
}

pub fn download_installer_blocking<F>(
    base_url: &str,
    dir: &Path,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<PathBuf, InstallerDownloadError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| InstallerDownloadError::Failed(format!("tokio runtime: {error}")))?;
    runtime.block_on(download_installer(base_url, dir, cancel, on_progress))
}

async fn download_installer<F>(
    base_url: &str,
    dir: &Path,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<PathBuf, InstallerDownloadError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let sha_text = fetch_text(&server::sha256_url(base_url)).await?;
    let expected =
        sha256_file::parse_sha256_file(&sha_text).map_err(InstallerDownloadError::Failed)?;

    let final_path = dir.join(server::INSTALLER_FILENAME);
    let part_path = dir.join(format!("{}.part", server::INSTALLER_FILENAME));
    let _ = std::fs::remove_file(&final_path);

    download_verify::download_to_file(
        &server::installer_url(base_url),
        &part_path,
        &expected,
        cancel,
        on_progress,
    )
    .await
    .map_err(|error| match error {
        DownloadError::Cancelled => InstallerDownloadError::Cancelled,
        other => InstallerDownloadError::Failed(other.to_string()),
    })?;

    std::fs::rename(&part_path, &final_path).map_err(|error| {
        let _ = std::fs::remove_file(&part_path);
        InstallerDownloadError::Failed(format!("rename installer: {error}"))
    })?;
    Ok(final_path)
}

async fn fetch_text(url: &str) -> Result<String, InstallerDownloadError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .build()
        .map_err(|error| InstallerDownloadError::Failed(format!("http client: {error}")))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| InstallerDownloadError::Failed(format!("network: {error}")))?;
    if !response.status().is_success() {
        return Err(InstallerDownloadError::Failed(format!(
            "HTTP {} for {url}",
            response.status().as_u16()
        )));
    }
    response
        .text()
        .await
        .map_err(|error| InstallerDownloadError::Failed(format!("read body: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const EXE_PATH: &str = "/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe";
    const SHA_PATH: &str = "/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256";

    async fn mock_server(payload: &[u8], sha_body: String) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(SHA_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string(sha_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(EXE_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn success_finalizes_installer_and_removes_part() {
        let payload = vec![7u8; 20_000];
        let sha = download_verify::sha256_hex(&payload);
        let server = mock_server(&payload, format!("{sha}  installer.exe")).await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;

        let result = download_installer(&server.uri(), dir.path(), cancel, |_, _| {}).await;

        let final_path = result.expect("download should succeed");
        assert_eq!(std::fs::read(&final_path).unwrap(), payload);
        assert!(!dir
            .path()
            .join(format!("{}.part", server::INSTALLER_FILENAME))
            .exists());
    }

    #[tokio::test]
    async fn hash_mismatch_deletes_files_and_fails() {
        let payload = vec![7u8; 1_000];
        let wrong = download_verify::sha256_hex(b"something else");
        let server = mock_server(&payload, wrong).await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;

        let result = download_installer(&server.uri(), dir.path(), cancel, |_, _| {}).await;

        assert!(matches!(result, Err(InstallerDownloadError::Failed(_))));
        assert!(!dir.path().join(server::INSTALLER_FILENAME).exists());
        assert!(!dir
            .path()
            .join(format!("{}.part", server::INSTALLER_FILENAME))
            .exists());
    }

    #[tokio::test]
    async fn missing_sha256_fails_before_installer_download() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;

        let result = download_installer(&server.uri(), dir.path(), cancel, |_, _| {}).await;

        assert!(matches!(result, Err(InstallerDownloadError::Failed(_))));
    }

    #[test]
    fn blocking_wrapper_runs_without_ambient_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = watch::channel(false).1;
        let result =
            download_installer_blocking("http://127.0.0.1:1", dir.path(), cancel, |_, _| {});
        assert!(matches!(result, Err(InstallerDownloadError::Failed(_))));
    }
}
