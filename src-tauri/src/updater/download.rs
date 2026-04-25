//! Streaming download + SHA-256 verification helpers for the updater feature.

use crate::updater::UpdaterError;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tokio::sync::watch;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

pub fn verify_bytes(bytes: &[u8], expected_sha256_hex: &str) -> bool {
    sha256_hex(bytes).eq_ignore_ascii_case(expected_sha256_hex)
}

pub async fn download_to_file<F>(
    url: &str,
    dest: &Path,
    expected_sha256_hex: &str,
    cancel: watch::Receiver<bool>,
    mut on_progress: F,
) -> Result<(), UpdaterError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .no_proxy()
        .build()
        .map_err(|error| UpdaterError::Network(error.to_string()))?;

    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| UpdaterError::Network(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(UpdaterError::Http(status.as_u16()));
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| UpdaterError::Io(error.to_string()))?;
    }
    let mut file =
        std::fs::File::create(dest).map_err(|error| UpdaterError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let total = response.content_length();
    let cancel = cancel;

    let abort_with = |reason: UpdaterError, path: &Path| -> UpdaterError {
        let _ = std::fs::remove_file(path);
        reason
    };

    loop {
        if *cancel.borrow() {
            drop(file);
            return Err(abort_with(UpdaterError::Cancelled, dest));
        }

        let next = response
            .chunk()
            .await
            .map_err(|error| abort_with(UpdaterError::Network(error.to_string()), dest))?;
        let Some(bytes) = next else {
            break;
        };

        if let Err(error) = file.write_all(&bytes) {
            return Err(abort_with(UpdaterError::Io(error.to_string()), dest));
        }
        hasher.update(&bytes);
        downloaded = downloaded.saturating_add(bytes.len() as u64);
        on_progress(downloaded, total);
    }

    if let Err(error) = file.flush() {
        return Err(abort_with(UpdaterError::Io(error.to_string()), dest));
    }
    drop(file);

    let actual = hex_encode(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256_hex) {
        return Err(abort_with(UpdaterError::VerifyFailed, dest));
    }

    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
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
