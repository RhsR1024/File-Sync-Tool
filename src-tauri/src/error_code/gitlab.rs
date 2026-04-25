use std::time::Duration;

use base64::Engine;

pub const GITLAB_BASE_URL: &str = "http://igcode.uniview.com";
pub const GITLAB_PROJECT_PATH: &str = "RD-UNIVIEW/public/pubResList/errorcode";
pub const GITLAB_BRANCH: &str = "main";
pub const GITLAB_USERNAME: &str = "cmo_ipc";
pub const GITLAB_PASSWORD: &str = "*Ab64799254";

#[derive(Debug)]
pub enum SyncError {
    Network(String),
    Auth,
    Http(u16),
    Archive(String),
    Io(String),
}

impl SyncError {
    pub fn toast_key(&self) -> &'static str {
        match self {
            SyncError::Network(_) => "errorCodeLookup.toast.networkFail",
            SyncError::Auth => "errorCodeLookup.toast.authFail",
            SyncError::Http(_) => "errorCodeLookup.toast.httpError",
            SyncError::Archive(_) | SyncError::Io(_) => "errorCodeLookup.toast.archiveError",
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Network(message) => write!(f, "network: {message}"),
            SyncError::Auth => write!(f, "auth_failed"),
            SyncError::Http(status) => write!(f, "http_{status}"),
            SyncError::Archive(message) => write!(f, "archive: {message}"),
            SyncError::Io(message) => write!(f, "io: {message}"),
        }
    }
}

impl std::error::Error for SyncError {}

pub fn build_archive_url() -> String {
    let encoded = percent_encode(GITLAB_PROJECT_PATH);
    format!(
        "{}/api/v4/projects/{}/repository/archive.zip?sha={}",
        GITLAB_BASE_URL, encoded, GITLAB_BRANCH
    )
}

pub fn build_basic_auth_header() -> String {
    let credentials = format!("{GITLAB_USERNAME}:{GITLAB_PASSWORD}");
    let b64 = base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes());
    format!("Basic {b64}")
}

pub async fn fetch_archive() -> Result<bytes::Bytes, SyncError> {
    let url = build_archive_url();
    let auth = build_basic_auth_header();
    log::info!("[error_code] start downloading GitLab archive: {url}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| SyncError::Network(error.to_string()))?;

    let response = client
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, auth)
        .send()
        .await
        .map_err(|error| SyncError::Network(error.to_string()))?;

    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        return Err(SyncError::Auth);
    }
    if !status.is_success() {
        return Err(SyncError::Http(status.as_u16()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| SyncError::Network(error.to_string()))?;

    log::info!("[error_code] archive downloaded: {} bytes", bytes.len());
    Ok(bytes)
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn archive_url_url_encodes_project_path_and_branch() {
        let url = build_archive_url();
        assert!(url.starts_with("http://igcode.uniview.com/api/v4/projects/"));
        assert!(url.contains("RD-UNIVIEW%2Fpublic%2FpubResList%2Ferrorcode"));
        assert!(url.ends_with("/repository/archive.zip?sha=main"));
    }

    #[test]
    fn basic_auth_header_round_trips_to_credentials() {
        let header = build_basic_auth_header();
        let b64 = header
            .strip_prefix("Basic ")
            .expect("header must begin with Basic ");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), "cmo_ipc:*Ab64799254");
    }
}
