use semver::Version;
use serde::Deserialize;
use std::time::Duration;

use crate::updater::{Manifest, ManifestVersion, UpdaterError};

/// Build the manifest endpoint from a configured base URL.
/// Trailing `/` is normalized away so we don't produce double slashes.
pub fn manifest_url(server_url: &str) -> String {
    let trimmed = server_url.trim_end_matches('/');
    format!("{trimmed}/manifest.json")
}

/// Turn a possibly-relative `url` from the manifest into an absolute URL.
pub fn resolve_download_url(server_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }

    let base = server_url.trim_end_matches('/');
    if let Some(stripped) = url.strip_prefix('/') {
        if let Some(authority_end) = find_authority_end(base) {
            let authority = &base[..authority_end];
            return format!("{authority}/{stripped}");
        }
        return format!("{base}/{stripped}");
    }

    format!("{base}/{url}")
}

fn find_authority_end(base: &str) -> Option<usize> {
    let scheme_end = base.find("://")? + 3;
    match base[scheme_end..].find('/') {
        Some(index) => Some(scheme_end + index),
        None => Some(base.len()),
    }
}

/// Returns true iff `latest` is strictly newer than `current` per semver.
/// Returns false on any parse error to fail closed.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let latest = match Version::parse(latest) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let current = match Version::parse(current) {
        Ok(value) => value,
        Err(_) => return false,
    };
    latest > current
}

#[derive(Debug, Deserialize)]
struct RawManifest {
    latest: String,
    versions: Vec<serde_json::Value>,
}

/// Parse manifest text. Invalid `versions[]` entries are dropped, but the
/// manifest stays usable as long as at least one entry survives or the array
/// was intentionally empty.
pub fn parse_manifest(text: &str, server_url: &str) -> Result<Manifest, UpdaterError> {
    let raw: RawManifest = serde_json::from_str(text)
        .map_err(|error| UpdaterError::ManifestInvalid(error.to_string()))?;

    let total_entries = raw.versions.len();
    let mut versions = Vec::with_capacity(total_entries);
    for (index, value) in raw.versions.into_iter().enumerate() {
        match parse_version_entry(value, server_url) {
            Ok(entry) => versions.push(entry),
            Err(error) => log::warn!("[updater] manifest versions[{index}] dropped: {error}"),
        }
    }

    if total_entries > 0 && versions.is_empty() {
        return Err(UpdaterError::ManifestInvalid(
            "all version entries were invalid".to_string(),
        ));
    }

    let latest = versions
        .first()
        .map(|entry| entry.version.clone())
        .unwrap_or(raw.latest);

    Ok(Manifest { latest, versions })
}

pub async fn fetch_manifest(server_url: &str) -> Result<Manifest, UpdaterError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
        .map_err(|error| UpdaterError::Network(error.to_string()))?;

    let response = client
        .get(manifest_url(server_url))
        .send()
        .await
        .map_err(|error| UpdaterError::Network(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(UpdaterError::Http(status.as_u16()));
    }

    let text = response
        .text()
        .await
        .map_err(|error| UpdaterError::Network(error.to_string()))?;
    parse_manifest(&text, server_url)
}

fn parse_version_entry(
    value: serde_json::Value,
    server_url: &str,
) -> Result<ManifestVersion, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "entry root must be an object".to_string())?;

    let version = object
        .get("version")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "missing version".to_string())?;
    Version::parse(version).map_err(|error| format!("invalid semver: {error}"))?;

    let url = object
        .get("url")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "missing url".to_string())?;
    let sha256 = object
        .get("sha256")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "missing sha256".to_string())?;
    let released_at = object
        .get("released_at")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "missing released_at".to_string())?;
    let changelog = object
        .get("changelog")
        .and_then(|item| item.as_array())
        .ok_or_else(|| "missing changelog".to_string())?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| "changelog items must be strings".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ManifestVersion {
        version: version.to_string(),
        url: resolve_download_url(server_url, url),
        sha256: sha256.to_ascii_lowercase(),
        released_at: released_at.to_string(),
        changelog,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_url_strips_trailing_slash_and_appends_path() {
        assert_eq!(
            manifest_url("http://1.2.3.4:8080/"),
            "http://1.2.3.4:8080/manifest.json"
        );
        assert_eq!(
            manifest_url("http://1.2.3.4:8080"),
            "http://1.2.3.4:8080/manifest.json"
        );
        assert_eq!(
            manifest_url("http://srv/releases/"),
            "http://srv/releases/manifest.json"
        );
    }

    #[test]
    fn resolve_download_url_keeps_absolute() {
        assert_eq!(
            resolve_download_url("http://srv:8080", "http://other/foo.exe"),
            "http://other/foo.exe"
        );
        assert_eq!(
            resolve_download_url("http://srv:8080/", "https://other/foo.exe"),
            "https://other/foo.exe"
        );
    }

    #[test]
    fn resolve_download_url_joins_relative() {
        assert_eq!(
            resolve_download_url("http://srv:8080", "foo.exe"),
            "http://srv:8080/foo.exe"
        );
        assert_eq!(
            resolve_download_url("http://srv:8080/", "/abs/foo.exe"),
            "http://srv:8080/abs/foo.exe"
        );
        assert_eq!(
            resolve_download_url("http://srv:8080/dir/", "foo.exe"),
            "http://srv:8080/dir/foo.exe"
        );
    }

    #[test]
    fn compare_versions_basic_ordering() {
        assert!(is_newer("1.0.8", "1.0.7"));
        assert!(!is_newer("1.0.7", "1.0.7"));
        assert!(!is_newer("1.0.6", "1.0.7"));
        assert!(is_newer("2.0.0", "1.99.99"));
    }

    #[test]
    fn compare_versions_handles_pre_release() {
        assert!(is_newer("1.0.8", "1.0.8-beta.1"));
        assert!(!is_newer("1.0.8-beta.1", "1.0.8"));
    }

    #[test]
    fn compare_versions_invalid_returns_false() {
        assert!(!is_newer("not-a-version", "1.0.0"));
        assert!(!is_newer("1.0.0", "garbage"));
    }

    #[test]
    fn parse_manifest_strips_invalid_entries_and_normalizes_urls() {
        let raw = r#"{
            "latest": "1.0.8",
            "versions": [
                {"version":"1.0.8","url":"file-sync-tool-1.0.8.exe","sha256":"AB","released_at":"2026-04-26","changelog":["a","b"]},
                {"version":"1.0.7","url":"http://other/x.exe","sha256":"CD","released_at":"2026-04-19","changelog":["c"]},
                {"version":"bad-version","url":"x","sha256":"EF","released_at":"2026-04-10","changelog":[]},
                {"version":"1.0.5"}
            ]
        }"#;

        let manifest = parse_manifest(raw, "http://srv:8080/").expect("parse");
        assert_eq!(manifest.latest, "1.0.8");
        assert_eq!(manifest.versions.len(), 2);
        assert_eq!(
            manifest.versions[0].url,
            "http://srv:8080/file-sync-tool-1.0.8.exe"
        );
        assert_eq!(manifest.versions[0].sha256, "ab");
        assert_eq!(manifest.versions[1].url, "http://other/x.exe");
    }

    #[test]
    fn parse_manifest_rejects_non_object_root() {
        let err = parse_manifest("[1,2,3]", "http://srv").unwrap_err();
        assert!(matches!(err, UpdaterError::ManifestInvalid(_)));
    }

    #[test]
    fn parse_manifest_accepts_empty_versions_array() {
        let manifest =
            parse_manifest(r#"{"latest":"1.0.0","versions":[]}"#, "http://srv").expect("parse");
        assert_eq!(manifest.latest, "1.0.0");
        assert!(manifest.versions.is_empty());
    }

    #[test]
    fn parse_manifest_trusts_first_valid_entry_when_latest_mismatches() {
        let raw = r#"{
            "latest": "9.9.9",
            "versions": [
                {"version":"1.0.8","url":"file-sync-tool-1.0.8.exe","sha256":"AB","released_at":"2026-04-26","changelog":["a"]}
            ]
        }"#;

        let manifest = parse_manifest(raw, "http://srv").expect("parse");
        assert_eq!(manifest.latest, "1.0.8");
    }

    #[test]
    fn parse_manifest_drops_all_invalid_returns_err() {
        let raw = r#"{"latest":"1.0.0","versions":[{"version":"abc"}]}"#;
        let err = parse_manifest(raw, "http://srv").unwrap_err();
        assert!(matches!(err, UpdaterError::ManifestInvalid(_)));
    }

    #[tokio::test]
    async fn fetch_manifest_downloads_and_parses() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{
                    "latest":"1.0.8",
                    "versions":[
                        {"version":"1.0.8","url":"file-sync-tool.exe","sha256":"AB","released_at":"2026-04-26","changelog":["a"]}
                    ]
                }"#,
            ))
            .mount(&server)
            .await;

        let manifest = fetch_manifest(&server.uri()).await.expect("fetch");
        assert_eq!(manifest.latest, "1.0.8");
        assert_eq!(manifest.versions.len(), 1);
        assert_eq!(
            manifest.versions[0].url,
            format!("{}/file-sync-tool.exe", server.uri())
        );
    }

    #[tokio::test]
    async fn fetch_manifest_returns_http_error_on_non_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest.json"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let err = fetch_manifest(&server.uri()).await.unwrap_err();
        assert!(matches!(err, UpdaterError::Http(503)));
    }
}
