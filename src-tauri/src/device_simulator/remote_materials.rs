use crate::device_simulator::local_materials::LocalMaterialPaths;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const REMOTE_FILE_LIST: &str = "files.json";
const REMOTE_STATE_FILE: &str = ".remote-files.json";
const MAX_FILE_LIST_BYTES: usize = 4 * 1024 * 1024;
const MAX_REMOTE_FILES: usize = 4_096;
const MAX_VIDEO_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteMaterialSyncReport {
    pub downloaded_files: usize,
    pub reused_files: usize,
    pub removed_files: usize,
    pub downloaded_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMaterialError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for RemoteMaterialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RemoteMaterialError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteFileList {
    files: Vec<RemoteFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteFile {
    path: String,
    size: u64,
    content_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteState {
    files: Vec<RemoteFile>,
}

pub fn build_remote_material_client() -> Result<reqwest::Client, RemoteMaterialError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(2 * 60 * 60))
        .no_proxy()
        .build()
        .map_err(|source| {
            error(
                "device_simulator.remote_materials.client_failed",
                format!("failed to create material HTTP client: {source}"),
            )
        })
}

/// Mirrors the server-managed loose files into the ordinary local material
/// directories. `content_id` is used only to avoid redundant transfers; it is
/// not signed and never establishes trust or release eligibility.
pub async fn sync_remote_materials(
    client: &reqwest::Client,
    base_url: &Url,
    paths: &LocalMaterialPaths,
) -> Result<RemoteMaterialSyncReport, RemoteMaterialError> {
    paths
        .ensure_layout()
        .map_err(|source| error(source.code, source.message))?;
    let index = fetch_file_list(client, base_url).await?;
    let validated = validate_file_list(index)?;
    let previous = read_remote_state(paths)?;
    let mut downloaded_files = 0usize;
    let mut reused_files = 0usize;
    let mut downloaded_bytes = 0u64;

    let mut files = validated.values().collect::<Vec<_>>();
    // Activate the prepared catalog only after every stream it references has
    // arrived. This keeps an interrupted sync on the previous usable catalog.
    files.sort_by_key(|file| file.path == "prepared-videos/prepared-catalog.json");
    for file in files {
        let target = target_path(paths, &file.path)?;
        if local_file_matches(&target, file).await? {
            reused_files += 1;
            continue;
        }
        let url = remote_file_url(base_url, &file.path)?;
        download_file(client, &url, &target, file.size).await?;
        downloaded_files += 1;
        downloaded_bytes = downloaded_bytes.saturating_add(file.size);
    }

    let current_paths = validated.keys().cloned().collect::<BTreeSet<_>>();
    let mut removed_files = 0usize;
    for old in previous
        .files
        .iter()
        .filter(|file| !current_paths.contains(&file.path))
    {
        let target = previous_managed_target_path(paths, &old.path)?;
        // Remove only the unchanged copy previously managed by the server.
        // A locally edited file is preserved and becomes an ordinary local material.
        if local_file_matches(&target, old).await? {
            fs::remove_file(&target).map_err(|source| {
                error(
                    "device_simulator.remote_materials.remove_failed",
                    format!("failed to remove '{}': {source}", target.display()),
                )
            })?;
            removed_files += 1;
        }
    }

    write_remote_state(
        paths,
        &RemoteState {
            files: validated.into_values().collect(),
        },
    )?;
    Ok(RemoteMaterialSyncReport {
        downloaded_files,
        reused_files,
        removed_files,
        downloaded_bytes,
    })
}

/// Removes only files recorded as upgrade-server managed. Ordinary local files
/// and imported user alarm images are deliberately outside this scope.
pub fn clear_remote_materials(paths: &LocalMaterialPaths) -> Result<usize, RemoteMaterialError> {
    let state = read_remote_state(paths)?;
    let mut removed = 0usize;
    for file in state.files {
        let target = previous_managed_target_path(paths, &file.path)?;
        match fs::remove_file(&target) {
            Ok(()) => removed += 1,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(error(
                    "device_simulator.remote_materials.remove_failed",
                    format!("failed to remove '{}': {source}", target.display()),
                ))
            }
        }
    }
    let state_path = paths.root.join(REMOTE_STATE_FILE);
    match fs::remove_file(&state_path) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(error(
                "device_simulator.remote_materials.state_failed",
                format!("failed to remove '{}': {source}", state_path.display()),
            ))
        }
    }
    Ok(removed)
}

async fn fetch_file_list(
    client: &reqwest::Client,
    base_url: &Url,
) -> Result<RemoteFileList, RemoteMaterialError> {
    let url = base_url.join(REMOTE_FILE_LIST).map_err(|source| {
        error(
            "device_simulator.remote_materials.url_invalid",
            format!("failed to build remote material list URL: {source}"),
        )
    })?;
    let mut response = client.get(url.clone()).send().await.map_err(|source| {
        error(
            "device_simulator.remote_materials.list_failed",
            format!("failed to request '{url}': {source}"),
        )
    })?;
    if !response.status().is_success() {
        return Err(error(
            "device_simulator.remote_materials.list_http_failed",
            format!("material list request returned HTTP {}", response.status()),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_FILE_LIST_BYTES as u64)
    {
        return Err(error(
            "device_simulator.remote_materials.list_too_large",
            "remote material list is too large",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|source| {
        error(
            "device_simulator.remote_materials.list_failed",
            format!("failed to read remote material list: {source}"),
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_FILE_LIST_BYTES {
            return Err(error(
                "device_simulator.remote_materials.list_too_large",
                "remote material list is too large",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|source| {
        error(
            "device_simulator.remote_materials.list_invalid",
            format!("remote material list is invalid: {source}"),
        )
    })
}

fn validate_file_list(
    list: RemoteFileList,
) -> Result<BTreeMap<String, RemoteFile>, RemoteMaterialError> {
    if list.files.len() > MAX_REMOTE_FILES {
        return Err(error(
            "device_simulator.remote_materials.list_too_large",
            "remote material list contains too many files",
        ));
    }
    let mut validated = BTreeMap::new();
    for file in list.files {
        let limit = material_size_limit(&file.path)?;
        if file.size == 0 || file.size > limit {
            return Err(error(
                "device_simulator.remote_materials.file_invalid",
                format!("remote material '{}' has an invalid size", file.path),
            ));
        }
        if file.content_id.len() != 64
            || !file.content_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(error(
                "device_simulator.remote_materials.file_invalid",
                format!("remote material '{}' has an invalid content id", file.path),
            ));
        }
        if validated.insert(file.path.clone(), file).is_some() {
            return Err(error(
                "device_simulator.remote_materials.file_duplicate",
                "remote material list contains duplicate paths",
            ));
        }
    }
    Ok(validated)
}

fn material_size_limit(relative: &str) -> Result<u64, RemoteMaterialError> {
    let path = Path::new(relative);
    if path.is_absolute() || path.components().count() < 2 {
        return Err(invalid_path(relative));
    }
    let components = relative.split('/').collect::<Vec<_>>();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_prepared_catalog = components.as_slice() == ["prepared-videos", "prepared-catalog.json"];
    let is_prepared_media = components.len() == 6
        && components[0] == "prepared-videos"
        && components[1] == "media"
        && components[2] == "themes"
        && components[3].starts_with("local-")
        && components[3]
            .bytes()
            .all(|value| value.is_ascii_lowercase() || value.is_ascii_digit() || value == b'-')
        && matches!(components[4], "main" | "sub" | "third")
        && (components[5] == "media.json" || components[5] == format!("{}.h264", components[4]));
    let is_alarm = components.len() == 4
        && components[0] == "alarm-images"
        && matches!(components[1], "face" | "car" | "person" | "nonmotor")
        && valid_alarm_group(components[1], components[2])
        && valid_alarm_role(
            components[1],
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
        )
        && matches!(extension.as_str(), "jpg" | "jpeg" | "png");
    if components.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.contains('\\')
            || part.contains(':')
    }) {
        return Err(invalid_path(relative));
    }
    if is_prepared_media && extension == "h264" {
        Ok(MAX_VIDEO_BYTES)
    } else if (is_prepared_media || is_prepared_catalog) && extension == "json" {
        Ok(MAX_IMAGE_BYTES)
    } else if is_alarm {
        Ok(MAX_IMAGE_BYTES)
    } else {
        Err(invalid_path(relative))
    }
}

fn valid_alarm_group(category: &str, group: &str) -> bool {
    group
        .strip_prefix(&format!("{category}-"))
        .is_some_and(|suffix| suffix.len() >= 3 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
}

fn valid_alarm_role(category: &str, role: &str) -> bool {
    match category {
        "person" => matches!(role, "scene" | "person"),
        "face" => matches!(role, "scene" | "face"),
        "car" => matches!(role, "scene" | "vehicle" | "plate"),
        "nonmotor" => matches!(role, "scene" | "nonmotor"),
        _ => false,
    }
}

fn target_path(paths: &LocalMaterialPaths, relative: &str) -> Result<PathBuf, RemoteMaterialError> {
    material_size_limit(relative)?;
    if relative == "prepared-videos/prepared-catalog.json" {
        return Ok(paths.remote_prepared_catalog());
    }
    if let Some(prepared) = relative.strip_prefix("prepared-videos/") {
        return Ok(paths.remote_cache.join(prepared));
    }
    Ok(paths.root.join(Path::new(relative)))
}

fn previous_managed_target_path(
    paths: &LocalMaterialPaths,
    relative: &str,
) -> Result<PathBuf, RemoteMaterialError> {
    if material_size_limit(relative).is_ok() {
        return target_path(paths, relative);
    }
    let path = Path::new(relative);
    let components = relative.split('/').collect::<Vec<_>>();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_legacy_alarm = !path.is_absolute()
        && components.len() == 3
        && components[0] == "alarm-images"
        && matches!(components[1], "face" | "car" | "person" | "nonmotor")
        && !components[2].is_empty()
        && !components[2].contains('\\')
        && !components[2].contains(':')
        && matches!(extension.as_str(), "jpg" | "jpeg" | "png");
    if is_legacy_alarm {
        return Ok(paths.root.join(path));
    }
    Err(invalid_path(relative))
}

fn remote_file_url(base_url: &Url, relative: &str) -> Result<Url, RemoteMaterialError> {
    material_size_limit(relative)?;
    let mut url = base_url.clone();
    {
        let mut segments = url.path_segments_mut().map_err(|_| {
            error(
                "device_simulator.remote_materials.url_invalid",
                "remote material URL cannot contain path segments",
            )
        })?;
        segments.pop_if_empty();
        for component in relative.split('/') {
            segments.push(component);
        }
    }
    Ok(url)
}

async fn local_file_matches(
    path: &Path,
    expected: &RemoteFile,
) -> Result<bool, RemoteMaterialError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(error(
                "device_simulator.remote_materials.local_read_failed",
                format!("failed to inspect '{}': {source}", path.display()),
            ))
        }
    };
    if !metadata.is_file() || metadata.len() != expected.size {
        return Ok(false);
    }
    let path = path.to_owned();
    let actual = tokio::task::spawn_blocking(move || hash_file(&path))
        .await
        .map_err(|source| {
            error(
                "device_simulator.remote_materials.hash_task_failed",
                source.to_string(),
            )
        })??;
    Ok(actual.eq_ignore_ascii_case(&expected.content_id))
}

async fn download_file(
    client: &reqwest::Client,
    url: &Url,
    target: &Path,
    expected_size: u64,
) -> Result<(), RemoteMaterialError> {
    let mut response = client.get(url.clone()).send().await.map_err(|source| {
        error(
            "device_simulator.remote_materials.download_failed",
            format!("failed to download '{url}': {source}"),
        )
    })?;
    if !response.status().is_success() {
        return Err(error(
            "device_simulator.remote_materials.download_http_failed",
            format!(
                "material download returned HTTP {} for '{url}'",
                response.status()
            ),
        ));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|source| {
            error(
                "device_simulator.remote_materials.local_write_failed",
                format!("failed to create '{}': {source}", parent.display()),
            )
        })?;
    }
    let temporary = target.with_extension(format!(
        "{}.{}.download",
        target
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("material"),
        uuid::Uuid::new_v4().simple()
    ));
    let mut output = tokio::fs::File::create(&temporary)
        .await
        .map_err(|source| {
            error(
                "device_simulator.remote_materials.local_write_failed",
                format!("failed to create '{}': {source}", temporary.display()),
            )
        })?;
    let mut downloaded = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|source| {
        error(
            "device_simulator.remote_materials.download_failed",
            format!("failed to read '{url}': {source}"),
        )
    })? {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > expected_size {
            drop(output);
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error(
                "device_simulator.remote_materials.download_size_mismatch",
                format!("downloaded material '{url}' exceeds its declared size"),
            ));
        }
        output.write_all(&chunk).await.map_err(|source| {
            error(
                "device_simulator.remote_materials.local_write_failed",
                format!("failed to write '{}': {source}", temporary.display()),
            )
        })?;
    }
    output.flush().await.map_err(|source| {
        error(
            "device_simulator.remote_materials.local_write_failed",
            format!("failed to flush '{}': {source}", temporary.display()),
        )
    })?;
    drop(output);
    if downloaded != expected_size {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error(
            "device_simulator.remote_materials.download_size_mismatch",
            format!("downloaded material '{url}' has {downloaded} bytes, expected {expected_size}"),
        ));
    }
    activate_download(&temporary, target)?;
    Ok(())
}

fn activate_download(temporary: &Path, target: &Path) -> Result<(), RemoteMaterialError> {
    let backup = target.with_extension(format!(
        "{}.{}.backup",
        target
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("material"),
        uuid::Uuid::new_v4().simple()
    ));
    if target.exists() {
        fs::rename(target, &backup).map_err(|source| {
            error(
                "device_simulator.remote_materials.local_write_failed",
                format!("failed to stage '{}': {source}", target.display()),
            )
        })?;
    }
    if let Err(source) = fs::rename(temporary, target) {
        if backup.exists() {
            let _ = fs::rename(&backup, target);
        }
        return Err(error(
            "device_simulator.remote_materials.local_write_failed",
            format!("failed to activate '{}': {source}", target.display()),
        ));
    }
    if backup.exists() {
        fs::remove_file(&backup).map_err(|source| {
            error(
                "device_simulator.remote_materials.local_write_failed",
                format!("failed to remove '{}': {source}", backup.display()),
            )
        })?;
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, RemoteMaterialError> {
    let file = fs::File::open(path).map_err(|source| {
        error(
            "device_simulator.remote_materials.local_read_failed",
            format!("failed to open '{}': {source}", path.display()),
        )
    })?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut digest = Sha256::new();
    loop {
        let read = reader.read(&mut buffer).map_err(|source| {
            error(
                "device_simulator.remote_materials.local_read_failed",
                format!("failed to read '{}': {source}", path.display()),
            )
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn read_remote_state(paths: &LocalMaterialPaths) -> Result<RemoteState, RemoteMaterialError> {
    let state_path = paths.root.join(REMOTE_STATE_FILE);
    let bytes = match fs::read(&state_path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RemoteState::default())
        }
        Err(source) => {
            return Err(error(
                "device_simulator.remote_materials.state_failed",
                format!("failed to read '{}': {source}", state_path.display()),
            ))
        }
    };
    serde_json::from_slice(&bytes).map_err(|source| {
        error(
            "device_simulator.remote_materials.state_failed",
            format!("failed to parse '{}': {source}", state_path.display()),
        )
    })
}

fn write_remote_state(
    paths: &LocalMaterialPaths,
    state: &RemoteState,
) -> Result<(), RemoteMaterialError> {
    let path = paths.root.join(REMOTE_STATE_FILE);
    let bytes = serde_json::to_vec_pretty(state).map_err(|source| {
        error(
            "device_simulator.remote_materials.state_failed",
            source.to_string(),
        )
    })?;
    let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, bytes).map_err(|source| {
        error(
            "device_simulator.remote_materials.state_failed",
            format!("failed to write '{}': {source}", temporary.display()),
        )
    })?;
    activate_download(&temporary, &path)
}

fn invalid_path(path: &str) -> RemoteMaterialError {
    error(
        "device_simulator.remote_materials.path_invalid",
        format!("remote material path '{path}' is not supported"),
    )
}

fn error(code: &'static str, message: impl Into<String>) -> RemoteMaterialError {
    RemoteMaterialError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn accepts_only_supported_loose_material_paths() {
        assert_eq!(
            material_size_limit("prepared-videos/media/themes/local-city/main/main.h264").unwrap(),
            MAX_VIDEO_BYTES
        );
        assert_eq!(
            material_size_limit("prepared-videos/prepared-catalog.json").unwrap(),
            MAX_IMAGE_BYTES
        );
        assert_eq!(
            material_size_limit("alarm-images/face/face-001/scene.png").unwrap(),
            MAX_IMAGE_BYTES
        );
        for invalid in [
            "catalog.json",
            "videos/raw.mp4",
            "prepared-videos/media/themes/INVALID/main/main.h264",
            "alarm-images/unknown/a.jpg",
            "alarm-images/face/人脸1.jpg",
            "alarm-images/car/car-001/face.jpg",
            "videos/a.exe",
        ] {
            assert!(material_size_limit(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn clear_removes_only_files_recorded_as_server_managed() {
        let temporary = TempDir::new().unwrap();
        let paths = LocalMaterialPaths::from_app_data_dir(temporary.path());
        paths.ensure_layout().unwrap();
        let managed = paths.alarm_images.join("face/face-001/scene.png");
        let legacy_managed = paths.alarm_images.join("face/人脸1.jpg");
        let custom = paths.alarm_images.join("face/custom-note.png");
        fs::create_dir_all(managed.parent().unwrap()).unwrap();
        fs::write(&managed, b"managed").unwrap();
        fs::write(&legacy_managed, b"legacy").unwrap();
        fs::write(&custom, b"custom").unwrap();
        write_remote_state(
            &paths,
            &RemoteState {
                files: vec![
                    RemoteFile {
                        path: "alarm-images/face/face-001/scene.png".into(),
                        size: 7,
                        content_id: format!("{:x}", Sha256::digest(b"managed")),
                    },
                    RemoteFile {
                        path: "alarm-images/face/人脸1.jpg".into(),
                        size: 6,
                        content_id: format!("{:x}", Sha256::digest(b"legacy")),
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(clear_remote_materials(&paths).unwrap(), 2);
        assert!(!managed.exists());
        assert!(!legacy_managed.exists());
        assert!(custom.exists());
        assert!(!paths.root.join(REMOTE_STATE_FILE).exists());
    }

    #[test]
    fn builds_percent_encoded_file_urls() {
        let base = Url::parse("http://example.test/virtual-device-assets/").unwrap();
        let url = remote_file_url(
            &base,
            "prepared-videos/media/themes/local-city/main/main.h264",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://example.test/virtual-device-assets/prepared-videos/media/themes/local-city/main/main.h264"
        );
    }

    #[tokio::test]
    async fn downloads_then_reuses_a_matching_loose_material() {
        let server = MockServer::start().await;
        let body = b"small mp4 fixture";
        let content_id = format!("{:x}", Sha256::digest(body));
        Mock::given(method("GET"))
            .and(path("/virtual-device-assets/files.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "files": [{
                    "path": "prepared-videos/media/themes/local-city/main/main.h264",
                    "size": body.len(),
                    "content_id": content_id,
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/virtual-device-assets/prepared-videos/media/themes/local-city/main/main.h264",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let temporary = TempDir::new().unwrap();
        let paths = LocalMaterialPaths::from_app_data_dir(temporary.path());
        let base = Url::parse(&format!("{}/virtual-device-assets/", server.uri())).unwrap();
        let client = build_remote_material_client().unwrap();
        let first = sync_remote_materials(&client, &base, &paths).await.unwrap();
        assert_eq!(first.downloaded_files, 1);
        assert_eq!(first.reused_files, 0);
        assert_eq!(
            fs::read(
                paths
                    .remote_cache
                    .join("media/themes/local-city/main/main.h264")
            )
            .unwrap(),
            body
        );

        let second = sync_remote_materials(&client, &base, &paths).await.unwrap();
        assert_eq!(second.downloaded_files, 0);
        assert_eq!(second.reused_files, 1);
    }
}
