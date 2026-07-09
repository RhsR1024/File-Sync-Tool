//! Updater-flavored wrapper over the shared verified downloader
//! (`crate::download_verify`). Kept so existing call sites and tests are
//! unchanged; the WebView2 bootstrap uses `download_verify` directly.

use crate::download_verify::{self, DownloadError};
use crate::updater::UpdaterError;
use std::path::Path;
use tokio::sync::watch;

#[cfg(test)]
pub use crate::download_verify::sha256_hex;
pub use crate::download_verify::verify_bytes;

impl From<DownloadError> for UpdaterError {
    fn from(error: DownloadError) -> Self {
        match error {
            DownloadError::Network(message) => UpdaterError::Network(message),
            DownloadError::Http(status) => UpdaterError::Http(status),
            DownloadError::Io(message) => UpdaterError::Io(message),
            DownloadError::VerifyFailed => UpdaterError::VerifyFailed,
            DownloadError::Cancelled => UpdaterError::Cancelled,
        }
    }
}

pub async fn download_to_file<F>(
    url: &str,
    dest: &Path,
    expected_sha256_hex: &str,
    cancel: watch::Receiver<bool>,
    on_progress: F,
) -> Result<(), UpdaterError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    download_verify::download_to_file(url, dest, expected_sha256_hex, cancel, on_progress)
        .await
        .map_err(UpdaterError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_value() {
        assert_eq!(
            sha256_hex(b"hello world"),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn verify_bytes_is_case_insensitive() {
        let bytes = b"abc123";
        let hash = sha256_hex(bytes);
        assert!(verify_bytes(bytes, &hash));
        assert!(verify_bytes(bytes, &hash.to_ascii_uppercase()));
        assert!(!verify_bytes(bytes, "deadbeef"));
    }

    #[tokio::test]
    async fn download_success_writes_file_and_reports_progress() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let payload: Vec<u8> = (0u8..=255).cycle().take(50_000).collect();
        let expected = sha256_hex(&payload);

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
            .mount(&server)
            .await;

        let url = format!("{}/file.exe", server.uri());
        let dest = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let dest_path = dest.to_path_buf();
        drop(dest);

        let cancel = tokio::sync::watch::channel(false).1;
        let progress = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u64>::new()));
        let progress_clone = progress.clone();

        let result = download_to_file(&url, &dest_path, &expected, cancel, move |downloaded, _| {
            progress_clone.lock().unwrap().push(downloaded);
        })
        .await;

        result.expect("download should succeed");
        let written = std::fs::read(&dest_path).unwrap();
        assert_eq!(written, payload);
        assert!(progress.lock().unwrap().last().copied().unwrap_or(0) >= 50_000);
        let _ = std::fs::remove_file(&dest_path);
    }

    #[tokio::test]
    async fn download_aborts_on_cancel_and_cleans_up() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let payload = vec![0u8; 1_000_000];
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/big.exe"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(payload)
                    .set_delay(std::time::Duration::from_millis(50)),
            )
            .mount(&server)
            .await;

        let url = format!("{}/big.exe", server.uri());
        let dest = std::env::temp_dir().join(format!("fst-cancel-{}.bin", std::process::id()));
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

        let dest_clone = dest.clone();
        let task = tokio::spawn(async move {
            download_to_file(&url, &dest_clone, "deadbeef", cancel_rx, |_, _| {}).await
        });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        cancel_tx.send(true).unwrap();
        let result = task.await.unwrap();
        assert!(matches!(
            result,
            Err(crate::updater::UpdaterError::Cancelled)
        ));
        assert!(!dest.exists(), "partial file should be cleaned up");
    }

    #[tokio::test]
    async fn download_verify_failure_deletes_file() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x.exe"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
            .mount(&server)
            .await;

        let url = format!("{}/x.exe", server.uri());
        let dest = std::env::temp_dir().join(format!("fst-verify-{}.bin", std::process::id()));
        let cancel = tokio::sync::watch::channel(false).1;

        let result = download_to_file(&url, &dest, "deadbeef", cancel, |_, _| {}).await;
        assert!(matches!(
            result,
            Err(crate::updater::UpdaterError::VerifyFailed)
        ));
        assert!(!dest.exists());
    }
}
