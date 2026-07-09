//! Shared streaming download + SHA-256 verification, usable by the updater and
//! the WebView2 bootstrap.

use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug)]
pub enum DownloadError {
    Network(String),
    Http(u16),
    Io(String),
    VerifyFailed,
    Cancelled,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Network(message) => write!(f, "network: {message}"),
            DownloadError::Http(status) => write!(f, "http_{status}"),
            DownloadError::Io(message) => write!(f, "io: {message}"),
            DownloadError::VerifyFailed => write!(f, "verify_failed"),
            DownloadError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for DownloadError {}

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
) -> Result<(), DownloadError>
where
    F: FnMut(u64, Option<u64>) + Send + 'static,
{
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30 * 60))
        .no_proxy()
        .build()
        .map_err(|error| DownloadError::Network(error.to_string()))?;

    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| DownloadError::Network(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Http(status.as_u16()));
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| DownloadError::Io(error.to_string()))?;
    }
    let mut file =
        std::fs::File::create(dest).map_err(|error| DownloadError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let total = response.content_length();
    let cancel = cancel;

    let abort_with = |reason: DownloadError, path: &Path| -> DownloadError {
        let _ = std::fs::remove_file(path);
        reason
    };

    loop {
        if *cancel.borrow() {
            drop(file);
            return Err(abort_with(DownloadError::Cancelled, dest));
        }

        let next = response
            .chunk()
            .await
            .map_err(|error| abort_with(DownloadError::Network(error.to_string()), dest))?;
        let Some(bytes) = next else {
            break;
        };

        if let Err(error) = file.write_all(&bytes) {
            return Err(abort_with(DownloadError::Io(error.to_string()), dest));
        }
        hasher.update(&bytes);
        downloaded = downloaded.saturating_add(bytes.len() as u64);
        on_progress(downloaded, total);
    }

    if let Err(error) = file.flush() {
        return Err(abort_with(DownloadError::Io(error.to_string()), dest));
    }
    drop(file);

    let actual = hex_encode(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha256_hex) {
        return Err(abort_with(DownloadError::VerifyFailed, dest));
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
