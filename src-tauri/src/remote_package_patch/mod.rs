//! Remote product package patching: Windows control plane + Linux server-side
//! archive rewrite.

pub mod inventory;
pub mod protocol;
pub mod script;
pub mod ssh;

use self::inventory::{InternalLayer, PackageInventory};
use self::protocol::ScriptLine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, WebviewWindow};

pub const EVENT_NAME: &str = "remote-package-patch-event";

static PATCH_BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct BusyGuard;

impl Drop for BusyGuard {
    fn drop(&mut self) {
        PATCH_BUSY.store(false, Ordering::SeqCst);
    }
}

fn reserve_busy() -> Result<BusyGuard, String> {
    PATCH_BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map(|_| BusyGuard)
        .map_err(|_| "Remote package scan or patch is already running".to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RemoteAuth {
    Password {
        password: String,
    },
    KeyFile {
        key_path: String,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: RemoteAuth,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub modified_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirListing {
    pub path: String,
    pub entries: Vec<RemoteDirEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedLocalFile {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum PatchOutputPolicy {
    NewFile { output_path: String },
    Overwrite,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePatchRequest {
    pub config: RemoteSshConfig,
    pub package_path: String,
    pub replacement_local_path: String,
    pub target_internal_path: String,
    pub target_layer: Option<InternalLayer>,
    pub output: PatchOutputPolicy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePatchResult {
    pub output_path: String,
    pub backup_path: Option<String>,
    pub target_md5: String,
    pub workdir: String,
    pub updated_manifests: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePackagePatchEvent {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

fn validate_config(config: &RemoteSshConfig) -> Result<(), String> {
    if config.host.trim().is_empty() {
        return Err("Host is required".into());
    }
    if config.username.trim().is_empty() {
        return Err("Username is required".into());
    }
    if config.port == 0 {
        return Err("SSH port is invalid".into());
    }
    match &config.auth {
        RemoteAuth::Password { password } if password.is_empty() => {
            Err("Password is required".into())
        }
        RemoteAuth::KeyFile { key_path, .. } if key_path.trim().is_empty() => {
            Err("Private key path is required".into())
        }
        _ => Ok(()),
    }
}

fn validate_patch_request(request: &PackagePatchRequest) -> Result<(), String> {
    validate_config(&request.config)?;
    if request.package_path.trim().is_empty() {
        return Err("Remote package path is required".into());
    }
    if !request.package_path.ends_with(".tar.gz") {
        return Err("Remote package must be a .tar.gz file".into());
    }
    if request.replacement_local_path.trim().is_empty() {
        return Err("Replacement local file is required".into());
    }
    if !Path::new(&request.replacement_local_path).is_file() {
        return Err(format!(
            "Replacement local file not found: {}",
            request.replacement_local_path
        ));
    }
    if request.target_internal_path.trim().is_empty() {
        return Err("Internal target path is required".into());
    }
    if request.target_internal_path.starts_with('/')
        || request
            .target_internal_path
            .split('/')
            .any(|part| part == "..")
    {
        return Err("Internal target path must be a safe relative tar member path".into());
    }
    match &request.output {
        PatchOutputPolicy::NewFile { output_path } if output_path.trim().is_empty() => {
            Err("Output package path is required".into())
        }
        _ => Ok(()),
    }
}

fn emit_event(app_handle: &tauri::AppHandle, event: RemotePackagePatchEvent) {
    let _ = app_handle.emit(EVENT_NAME, event);
}

fn emit_stage(app_handle: &tauri::AppHandle, stage: &str) {
    emit_event(
        app_handle,
        RemotePackagePatchEvent {
            kind: "stage".into(),
            stage: Some(stage.into()),
            level: None,
            message: None,
            key: None,
            value: None,
            sent: None,
            total: None,
        },
    );
}

fn emit_log(app_handle: &tauri::AppHandle, level: &str, message: impl Into<String>) {
    emit_event(
        app_handle,
        RemotePackagePatchEvent {
            kind: "log".into(),
            stage: None,
            level: Some(level.into()),
            message: Some(message.into()),
            key: None,
            value: None,
            sent: None,
            total: None,
        },
    );
}

fn emit_result_event(app_handle: &tauri::AppHandle, key: &str, value: &str) {
    emit_event(
        app_handle,
        RemotePackagePatchEvent {
            kind: "result".into(),
            stage: None,
            level: None,
            message: None,
            key: Some(key.into()),
            value: Some(value.into()),
            sent: None,
            total: None,
        },
    );
}

fn emit_upload_progress(app_handle: &tauri::AppHandle, sent: u64, total: u64) {
    emit_event(
        app_handle,
        RemotePackagePatchEvent {
            kind: "uploadProgress".into(),
            stage: None,
            level: None,
            message: None,
            key: None,
            value: None,
            sent: Some(sent),
            total: Some(total),
        },
    );
}

fn remote_dirname(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return "/".into();
    }
    match trimmed.rfind('/') {
        Some(0) => "/".into(),
        Some(index) => trimmed[..index].to_string(),
        None => ".".into(),
    }
}

fn remote_join(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), child)
    }
}

fn local_file_name(path: &str) -> Result<String, String> {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| format!("Cannot determine file name from {path}"))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn parse_scan_output(
    package_path: &str,
    raw_entries: Vec<(String, String)>,
    middle_tar_path: Option<String>,
) -> Result<PackageInventory, String> {
    let middle_tar_path =
        middle_tar_path.ok_or_else(|| "Scan did not report middle tar path".to_string())?;
    let mut entries = Vec::new();
    for (layer_tag, line) in raw_entries {
        let Some(layer) = inventory::parse_raw_layer(&layer_tag) else {
            continue;
        };
        if let Some(entry) = inventory::parse_tar_verbose_line(layer, &line) {
            entries.push(entry);
        }
    }

    Ok(PackageInventory {
        package_path: package_path.to_string(),
        middle_tar_path,
        entries,
    })
}

fn parse_patch_result(
    result_pairs: Vec<(String, String)>,
    fallback_workdir: String,
) -> Result<PackagePatchResult, String> {
    let mut output_path = None;
    let mut backup_path = None;
    let mut target_md5 = None;
    let mut workdir = Some(fallback_workdir);
    let mut updated_manifests = Vec::new();

    for (key, value) in result_pairs {
        match key.as_str() {
            "output_path" => output_path = Some(value),
            "backup_path" if !value.is_empty() => backup_path = Some(value),
            "replacement_md5" | "target_md5" => target_md5 = Some(value),
            "workdir" => workdir = Some(value),
            "updated_manifest" => updated_manifests.push(value),
            _ => {}
        }
    }

    Ok(PackagePatchResult {
        output_path: output_path.ok_or_else(|| "Patch did not report output path".to_string())?,
        backup_path,
        target_md5: target_md5.ok_or_else(|| "Patch did not report target md5".to_string())?,
        workdir: workdir.unwrap_or_default(),
        updated_manifests,
    })
}

fn run_connection_test<F>(config: RemoteSshConfig, connect: F) -> Result<String, String>
where
    F: FnOnce(&RemoteSshConfig) -> Result<(), String>,
{
    validate_config(&config)?;
    connect(&config)?;
    Ok(format!(
        "SSH authentication succeeded on {}:{}",
        config.host.trim(),
        config.port
    ))
}

#[tauri::command]
pub async fn remote_package_test_connection(config: RemoteSshConfig) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_connection_test(config, |config| ssh::connect(config).map(|_| ()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn remote_package_list_dir(
    config: RemoteSshConfig,
    path: String,
) -> Result<RemoteDirListing, String> {
    validate_config(&config)?;
    tauri::async_runtime::spawn_blocking(move || {
        let session = ssh::connect(&config)?;
        ssh::list_dir(&session, &path)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn remote_package_pick_local_file(
    window: WebviewWindow,
    kind: String,
) -> Result<Option<PickedLocalFile>, String> {
    let title = if kind == "privateKey" {
        "Select SSH Private Key"
    } else {
        "Select Replacement File"
    };
    let picked: Option<PathBuf> = crate::run_dialog_task_on_main_thread(&window, move || {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if kind == "privateKey" {
            dialog = dialog.add_filter("SSH private key", &["pem", "key", "ppk"]);
        }
        Ok(dialog.pick_file())
    })
    .await?;

    let Some(path) = picked else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(Some(PickedLocalFile {
        path: path.to_string_lossy().to_string(),
        name,
        size: metadata.len(),
    }))
}

#[tauri::command]
pub async fn remote_package_scan_package(
    app_handle: tauri::AppHandle,
    config: RemoteSshConfig,
    package_path: String,
) -> Result<PackageInventory, String> {
    validate_config(&config)?;
    if package_path.trim().is_empty() {
        return Err("Remote package path is required".into());
    }
    let busy = reserve_busy()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        emit_log(
            &app_handle,
            "info",
            "Connecting to remote server for package scan",
        );
        let session = ssh::connect(&config)?;
        let script = script::build_scan_script(&package_path);
        let command = script::bash_stdin_command(&script);
        let mut raw_entries = Vec::<(String, String)>::new();
        let mut middle_tar_path = None;
        let mut script_error = None;
        let exit_code =
            ssh::exec_stream(
                &session,
                &command,
                |line| match protocol::parse_script_line(line) {
                    ScriptLine::Stage(stage) => emit_stage(&app_handle, &stage),
                    ScriptLine::Log { level, message } => emit_log(&app_handle, &level, message),
                    ScriptLine::Result { key, value } => {
                        if key == "middle_tar_path" {
                            middle_tar_path = Some(value.clone());
                        }
                        emit_result_event(&app_handle, &key, &value);
                    }
                    ScriptLine::Error(message) => {
                        script_error = Some(message.clone());
                        emit_log(&app_handle, "error", message);
                    }
                    ScriptLine::Raw { layer_tag, line } => raw_entries.push((layer_tag, line)),
                    ScriptLine::Plain(line) if !line.trim().is_empty() => {
                        emit_log(&app_handle, "info", line)
                    }
                    ScriptLine::Plain(_) => {}
                },
            )?;
        if exit_code != 0 {
            return Err(
                script_error.unwrap_or_else(|| format!("Scan script exited with {exit_code}"))
            );
        }
        parse_scan_output(&package_path, raw_entries, middle_tar_path)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn remote_package_start_patch(
    app_handle: tauri::AppHandle,
    request: PackagePatchRequest,
) -> Result<PackagePatchResult, String> {
    validate_patch_request(&request)?;
    let busy = reserve_busy()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        emit_stage(&app_handle, "upload");
        let session = ssh::connect(&request.config)?;
        let sftp = session
            .sftp()
            .map_err(|error| format!("SFTP init failed: {error}"))?;

        let package_dir = remote_dirname(&request.package_path);
        let workdir = remote_join(
            &package_dir,
            &format!(
                ".file-sync-tool-patch-{}-{}",
                unix_seconds(),
                std::process::id()
            ),
        );
        ssh::exec_capture(
            &session,
            &format!("mkdir -p -- {}", script::sh_quote(&workdir)),
        )?;
        emit_log(&app_handle, "info", format!("Remote workdir: {workdir}"));
        emit_result_event(&app_handle, "workdir", &workdir);

        let replacement_name = local_file_name(&request.replacement_local_path)?;
        let remote_replacement = remote_join(&workdir, &format!("replacement-{replacement_name}"));
        let replacement_total = std::fs::metadata(&request.replacement_local_path)
            .map_err(|error| error.to_string())?
            .len();
        ssh::upload_file_with_progress(
            &sftp,
            Path::new(&request.replacement_local_path),
            Path::new(&remote_replacement),
            |sent| emit_upload_progress(&app_handle, sent, replacement_total),
        )?;

        let output = match &request.output {
            PatchOutputPolicy::NewFile { output_path } => {
                script::PatchScriptOutput::NewFile { output_path }
            }
            PatchOutputPolicy::Overwrite => script::PatchScriptOutput::Overwrite,
        };
        let patch_script = script::build_patch_script(script::PatchScriptArgs {
            package_path: &request.package_path,
            replacement_path: &remote_replacement,
            target_internal_path: &request.target_internal_path,
            target_layer: request.target_layer.as_ref(),
            output,
            workdir: &workdir,
        });
        let remote_script = remote_join(&workdir, "patch.sh");
        ssh::write_remote_file(
            &sftp,
            Path::new(&remote_script),
            patch_script.as_bytes(),
            0o700,
        )?;
        emit_log(
            &app_handle,
            "info",
            "Uploaded replacement file and patch script",
        );

        let mut result_pairs = vec![("workdir".to_string(), workdir.clone())];
        let mut script_error = None;
        let command = format!("bash {}", script::sh_quote(&remote_script));
        let exit_code =
            ssh::exec_stream(
                &session,
                &command,
                |line| match protocol::parse_script_line(line) {
                    ScriptLine::Stage(stage) => emit_stage(&app_handle, &stage),
                    ScriptLine::Log { level, message } => emit_log(&app_handle, &level, message),
                    ScriptLine::Result { key, value } => {
                        result_pairs.push((key.clone(), value.clone()));
                        emit_result_event(&app_handle, &key, &value);
                    }
                    ScriptLine::Error(message) => {
                        script_error = Some(message.clone());
                        emit_log(&app_handle, "error", message);
                    }
                    ScriptLine::Raw { .. } => {}
                    ScriptLine::Plain(line) if !line.trim().is_empty() => {
                        emit_log(&app_handle, "info", line)
                    }
                    ScriptLine::Plain(_) => {}
                },
            )?;
        if exit_code != 0 {
            return Err(
                script_error.unwrap_or_else(|| format!("Patch script exited with {exit_code}"))
            );
        }
        parse_patch_result(result_pairs, workdir)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password_config() -> RemoteSshConfig {
        RemoteSshConfig {
            host: "10.0.0.1".into(),
            port: 22,
            username: "root".into(),
            auth: RemoteAuth::Password {
                password: "secret".into(),
            },
        }
    }

    #[test]
    fn validates_required_connection_fields() {
        let mut config = password_config();
        config.host = "".into();
        assert!(validate_config(&config).unwrap_err().contains("Host"));
        config.host = "10.0.0.1".into();
        config.username = "".into();
        assert!(validate_config(&config).unwrap_err().contains("Username"));
        config.username = "root".into();
        config.port = 0;
        assert!(validate_config(&config).unwrap_err().contains("port"));
    }

    #[test]
    fn rejects_empty_password_and_key_path() {
        let mut config = password_config();
        config.auth = RemoteAuth::Password {
            password: "".into(),
        };
        assert!(validate_config(&config).unwrap_err().contains("Password"));
        config.auth = RemoteAuth::KeyFile {
            key_path: "".into(),
            passphrase: None,
        };
        assert!(validate_config(&config)
            .unwrap_err()
            .contains("Private key"));
    }

    #[test]
    fn remote_path_helpers_handle_root_and_nested_paths() {
        assert_eq!(remote_dirname("/tmp/a.tar.gz"), "/tmp");
        assert_eq!(remote_dirname("/a.tar.gz"), "/");
        assert_eq!(remote_join("/", "a"), "/a");
        assert_eq!(remote_join("/tmp", "a"), "/tmp/a");
    }

    #[test]
    fn parses_patch_result_pairs() {
        let result = parse_patch_result(
            vec![
                ("output_path".into(), "/tmp/a.patched.tar.gz".into()),
                ("replacement_md5".into(), "abc".into()),
                ("updated_manifest".into(), "md5".into()),
            ],
            "/tmp/work".into(),
        )
        .unwrap();
        assert_eq!(result.output_path, "/tmp/a.patched.tar.gz");
        assert_eq!(result.target_md5, "abc");
        assert_eq!(result.workdir, "/tmp/work");
        assert_eq!(result.updated_manifests, vec!["md5"]);
    }

    #[test]
    fn test_connection_success_depends_only_on_ssh_authentication() {
        let message = run_connection_test(password_config(), |config| {
            assert_eq!(config.host, "10.0.0.1");
            assert_eq!(config.port, 22);
            Ok(())
        })
        .unwrap();

        assert!(message.contains("SSH authentication succeeded"));
    }

    #[test]
    fn test_connection_surfaces_connect_error_without_remote_command_probe() {
        let error = run_connection_test(password_config(), |_| {
            Err("TCP connect failed: connection refused".to_string())
        })
        .unwrap_err();

        assert_eq!(error, "TCP connect failed: connection refused");
    }
}
