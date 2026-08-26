#![allow(clippy::too_many_arguments)]

use crate::config::{CommandGroup, DeployServer, TaskServerBinding};
use crate::task_domain::DeployStage;
use crate::task_manager::{DeployTarget, DeployTrackingContext};
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::Instant;
use tauri::Emitter;

#[derive(Debug, serde::Serialize, Clone)]
struct LogEvent {
    msg: String,
    level: String,
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, serde::Serialize, Clone)]
struct ProgressEvent {
    folder: String,
    total_bytes: u64,
    copied_bytes: u64,
    percentage: f64,
    speed: u64,
    eta_seconds: u64,
    elapsed_seconds: u64,
    local_path: String,
    remote_path: String,
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManualDeployTransferPolicy {
    Smart,
    #[default]
    Always,
    RemoteOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManualDeployExtractPolicy {
    Auto,
    Force,
    #[default]
    Skip,
}

#[derive(Debug, Clone, Default)]
pub struct ManualDeployOptions {
    pub transfer_policy: ManualDeployTransferPolicy,
    pub extract_policy: ManualDeployExtractPolicy,
    pub extract_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualDeployTransferAction {
    Upload,
    Reuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualDeployExtractAction {
    Extract,
    Reuse,
    Skip,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManualDeployPreflightResult {
    pub server_id: String,
    pub server_name: String,
    pub remote_package_path: String,
    pub extract_dir: String,
    pub package_exists: bool,
    pub package_matches: Option<bool>,
    pub extraction_ready: bool,
    pub transfer_action: ManualDeployTransferAction,
    pub extract_action: ManualDeployExtractAction,
}

#[derive(Debug, Clone)]
struct ResolvedManualDeployPaths {
    local_root: Option<PathBuf>,
    local_package: Option<PathBuf>,
    upload_target: String,
    remote_package: String,
    command_target: String,
    extract_dir: String,
    folder_display: String,
    package_base: String,
}

#[derive(Debug, Clone)]
struct ManualDeployInspection {
    paths: ResolvedManualDeployPaths,
    package_hash: String,
    package_exists: bool,
    package_matches: Option<bool>,
    extraction_ready: bool,
    transfer_action: ManualDeployTransferAction,
    extract_action: ManualDeployExtractAction,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ManualDeployMarker {
    package_sha256: String,
    remote_package_path: String,
}

fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit(
        "log-message",
        LogEvent {
            msg: msg.clone(),
            level: level.to_string(),
        },
    );
    crate::scanner::write_log_to_file(app_handle, &msg, level);
}

fn emit_log_with_tracking<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    msg: String,
    level: &str,
    tracking: Option<&DeployTrackingContext>,
    server_id: Option<&str>,
    server_name: Option<&str>,
) {
    emit_log(app_handle, msg.clone(), level);
    if let Some(tracking) = tracking {
        let _ = tracking.record_log(server_id, server_name, level, &msg);
    }
}

fn emit_progress<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    folder: &str,
    copied: u64,
    total: u64,
    speed: u64,
    eta_seconds: u64,
    elapsed_seconds: u64,
    local_path: &str,
    remote_path: &str,
    source: &str,
) {
    let percentage = if total > 0 {
        (copied as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let _ = app_handle.emit(
        "copy-progress",
        ProgressEvent {
            folder: folder.to_string(),
            total_bytes: total,
            copied_bytes: copied,
            percentage,
            speed,
            eta_seconds,
            elapsed_seconds,
            local_path: local_path.to_string(),
            remote_path: remote_path.to_string(),
            source: source.to_string(),
        },
    );
}

pub fn check_connection(server: &DeployServer) -> Result<String, String> {
    let addr = format!("{}:{}", server.host, server.port)
        .to_socket_addrs()
        .map_err(|e| format!("Address resolution failed for {}: {}", server.host, e))?
        .next()
        .ok_or_else(|| format!("No address found for {}", server.host))?;
    let timeout = Duration::from_secs(server.ssh_timeout_secs);
    let tcp = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("TCP Connect failed to {}: {}", server.host, e))?;

    let mut sess = Session::new().map_err(|e| format!("SSH Session init failed: {}", e))?;
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH Handshake failed: {}", e))?;

    sess.userauth_password(&server.user, &server.password)
        .map_err(|e| format!("Authentication failed: {}", e))?;

    Ok(format!("Connected to {}", server.name))
}

/// Resolve ordered commands from a list of command group IDs.
fn resolve_commands(command_group_ids: &[String], all_groups: &[CommandGroup]) -> Vec<String> {
    let mut commands = Vec::new();
    for gid in command_group_ids {
        if let Some(group) = all_groups.iter().find(|g| &g.id == gid) {
            commands.extend(group.commands.iter().cloned());
        }
    }
    commands
}

pub fn deploy_to_remote<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server_bindings: &[TaskServerBinding],
    all_servers: &[DeployServer],
    command_groups: &[CommandGroup],
    local_folder_path: &Path,
    folder_name: &str,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    tracking: Option<DeployTrackingContext>,
) -> Result<(), String> {
    deploy_to_remote_with_trigger(
        app_handle,
        server_bindings,
        all_servers,
        command_groups,
        local_folder_path,
        folder_name,
        should_cancel,
        is_paused,
        tracking,
        crate::task_domain::TaskTriggerSource::Scheduled,
    )
}

pub fn retry_deploy_to_remote<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server_bindings: &[TaskServerBinding],
    all_servers: &[DeployServer],
    command_groups: &[CommandGroup],
    local_folder_path: &Path,
    folder_name: &str,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    tracking: Option<DeployTrackingContext>,
) -> Result<(), String> {
    deploy_to_remote_with_trigger(
        app_handle,
        server_bindings,
        all_servers,
        command_groups,
        local_folder_path,
        folder_name,
        should_cancel,
        is_paused,
        tracking,
        crate::task_domain::TaskTriggerSource::Recovery,
    )
}

#[allow(clippy::too_many_arguments)]
fn deploy_to_remote_with_trigger<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server_bindings: &[TaskServerBinding],
    all_servers: &[DeployServer],
    command_groups: &[CommandGroup],
    local_folder_path: &Path,
    folder_name: &str,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    tracking: Option<DeployTrackingContext>,
    trigger_source: crate::task_domain::TaskTriggerSource,
) -> Result<(), String> {
    let progress_source = match &trigger_source {
        crate::task_domain::TaskTriggerSource::Recovery => "recovery",
        crate::task_domain::TaskTriggerSource::Manual => "manual",
        crate::task_domain::TaskTriggerSource::Scheduled => "scheduled",
    };
    if server_bindings.is_empty() {
        return Ok(());
    }

    emit_log_with_tracking(
        app_handle,
        format!(
            "Starting deployment for {} server(s)...",
            server_bindings.len()
        ),
        "info",
        tracking.as_ref(),
        None,
        None,
    );

    let total_size = calculate_size(local_folder_path);
    let resolved_targets: Vec<DeployTarget> = server_bindings
        .iter()
        .filter_map(|binding| {
            all_servers
                .iter()
                .find(|server| server.id == binding.server_id && server.enabled)
                .map(|server| DeployTarget {
                    server_id: server.id.clone(),
                    server_name: server.name.clone(),
                    server_host: server.host.clone(),
                    remote_target: format!(
                        "{}/{}",
                        server.remote_path.trim_end_matches('/'),
                        folder_name
                    ),
                    trigger_source: trigger_source.clone(),
                })
        })
        .collect();

    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.register_targets(&resolved_targets);
    }

    for (idx, binding) in server_bindings.iter().enumerate() {
        if should_cancel.load(Ordering::SeqCst) {
            emit_log_with_tracking(
                app_handle,
                "Remaining deployments cancelled.".to_string(),
                "warn",
                tracking.as_ref(),
                None,
                None,
            );
            if let Some(tracking) = tracking.as_ref() {
                let _ = tracking.cancel_pending();
            }
            break;
        }

        let server = match all_servers.iter().find(|s| s.id == binding.server_id) {
            Some(s) => s,
            None => {
                emit_log_with_tracking(
                    app_handle,
                    format!("Server ID '{}' not found, skipping.", binding.server_id),
                    "warn",
                    tracking.as_ref(),
                    Some(binding.server_id.as_str()),
                    None,
                );
                continue;
            }
        };

        if !server.enabled {
            emit_log_with_tracking(
                app_handle,
                format!("[{}] Server is disabled, skipping.", server.name),
                "info",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
            continue;
        }

        let commands = resolve_commands(&binding.command_group_ids, command_groups);

        emit_log_with_tracking(
            app_handle,
            format!(
                "Deploying to server {}/{} [{}]",
                idx + 1,
                server_bindings.len(),
                server.name
            ),
            "info",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );

        if let Err(e) = deploy_single_server(
            app_handle,
            server,
            local_folder_path,
            folder_name,
            &commands,
            total_size,
            should_cancel.clone(),
            is_paused.clone(),
            progress_source,
            tracking.clone(),
        ) {
            emit_log_with_tracking(
                app_handle,
                format!("[{}] Deployment failed: {}", server.name, e),
                "error",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
        } else {
            emit_log_with_tracking(
                app_handle,
                format!("[{}] Deployment successful", server.name),
                "success",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
        }
    }

    Ok(())
}

/// `${filename}` is the package base name, i.e. the archive name without its
/// suffix (`pkg_x86.tar.gz` -> `pkg_x86`). A folder deploy looks for the
/// archive inside the folder; a single-file deploy uses the file itself.
fn resolve_package_base_name(local_path: &Path, folder_name: &str) -> String {
    if local_path.is_file() {
        return local_path
            .file_name()
            .map(|name| strip_archive_suffix(&name.to_string_lossy()))
            .unwrap_or_else(|| folder_name.to_string());
    }

    if let Ok(entries) = fs::read_dir(local_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.ends_with(".tar.gz") {
                    return name_str.trim_end_matches(".tar.gz").to_string();
                }
            }
        }
    }

    folder_name.to_string()
}

/// `.tar.gz` is a double extension, so `Path::file_stem` alone would leave a
/// trailing `.tar`.
fn strip_archive_suffix(file_name: &str) -> String {
    match file_name.strip_suffix(".tar.gz") {
        Some(stem) => stem.to_string(),
        None => Path::new(file_name)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| file_name.to_string()),
    }
}

/// `${remote_target}` must name a directory that post commands can `cd` into.
/// A file upload lands at `<dir>/<file name>`, so its commands run against the
/// containing directory; folder deploys already target a directory.
fn remote_command_target(local_path: &Path, remote_target: &str) -> String {
    if !local_path.is_file() {
        return remote_target.to_string();
    }

    match remote_target.rsplit_once('/') {
        Some(("", _)) => "/".to_string(),
        Some((parent, _)) => parent.to_string(),
        None => ".".to_string(),
    }
}

fn substitute_variables(
    cmd: &str,
    folder_name: &str,
    local_path: &Path,
    remote_target: &str,
) -> String {
    let mut result = cmd.to_string();

    result = result.replace("${folder_name}", folder_name);
    result = result.replace("${remote_target}", remote_target);

    if result.contains("${filename}") {
        let replacement = resolve_package_base_name(local_path, folder_name);
        result = result.replace("${filename}", &replacement);
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn deploy_single_server<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    local_folder_path: &Path,
    folder_name: &str,
    post_commands: &[String],
    total_size: u64,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    source: &str,
    tracking: Option<DeployTrackingContext>,
) -> Result<(), String> {
    let remote_target = format!(
        "{}/{}",
        server.remote_path.trim_end_matches('/'),
        folder_name
    );

    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_stage(
            &server.id,
            DeployStage::Connecting,
            None,
            Some(remote_target.clone()),
        );
    }

    emit_log_with_tracking(
        app_handle,
        format!(
            "[{}] Connecting to {}:{}",
            server.name, server.host, server.remote_path
        ),
        "info",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );

    let tcp = {
        let addr = format!("{}:{}", server.host, server.port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .ok_or_else(|| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Connecting,
                    format!("Address resolution failed for {}", server.host),
                )
            })?;
        TcpStream::connect_timeout(&addr, Duration::from_secs(server.ssh_timeout_secs)).map_err(
            |e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Connecting,
                    format!("TCP Connect failed to {}: {}", server.host, e),
                )
            },
        )?
    };
    let mut sess = Session::new().map_err(|e| {
        report_stage_failure(
            tracking.as_ref(),
            &server.id,
            DeployStage::Connecting,
            format!("SSH Session init failed: {}", e),
        )
    })?;
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| {
        report_stage_failure(
            tracking.as_ref(),
            &server.id,
            DeployStage::Connecting,
            format!("SSH Handshake failed: {}", e),
        )
    })?;
    sess.userauth_password(&server.user, &server.password)
        .map_err(|e| {
            report_stage_failure(
                tracking.as_ref(),
                &server.id,
                DeployStage::Connecting,
                format!("Authentication failed: {}", e),
            )
        })?;

    emit_log_with_tracking(
        app_handle,
        format!("[{}] Connected", server.name),
        "info",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );
    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_stage(
            &server.id,
            DeployStage::Uploading,
            Some(0.0),
            Some(remote_target.clone()),
        );
    }

    let sftp = sess.sftp().map_err(|e| {
        report_stage_failure(
            tracking.as_ref(),
            &server.id,
            DeployStage::Uploading,
            format!("SFTP init failed: {}", e),
        )
    })?;

    match sftp.stat(Path::new(&remote_target)) {
        Ok(_) => {
            emit_log_with_tracking(
                app_handle,
                format!(
                    "[{}] Remote directory {} already exists. Continuing upload/overwrite.",
                    server.name, remote_target
                ),
                "info",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
        }
        Err(_) => {
            emit_log_with_tracking(
                app_handle,
                format!("[{}] Uploading to {}", server.name, remote_target),
                "info",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
            let mut channel = sess.channel_session().map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Uploading,
                    format!("channel_session failed: {}", e),
                )
            })?;
            channel
                .exec(&format!("mkdir -p {}", remote_target))
                .map_err(|e| {
                    report_stage_failure(
                        tracking.as_ref(),
                        &server.id,
                        DeployStage::Uploading,
                        format!("mkdir failed: {}", e),
                    )
                })?;
            channel.send_eof().map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Uploading,
                    format!("send_eof failed: {}", e),
                )
            })?;
            let mut s = String::new();
            channel.read_to_string(&mut s).map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Uploading,
                    format!("read failed: {}", e),
                )
            })?;
            channel.wait_close().map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Uploading,
                    format!("wait_close failed: {}", e),
                )
            })?;
        }
    };

    let mut copied_bytes = 0u64;
    let start_time = Instant::now();
    let mut last_emit_time = Instant::now();
    let local_path_str = local_folder_path.to_string_lossy().to_string();
    let server_display = format!("[{}] {}", server.name, remote_target);

    upload_with_progress(
        app_handle,
        &sftp,
        local_folder_path,
        Path::new(&remote_target),
        total_size,
        &mut copied_bytes,
        start_time,
        &mut last_emit_time,
        folder_name,
        &local_path_str,
        &server_display,
        &should_cancel,
        &is_paused,
        source,
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(remote_target.as_str()),
    )
    .map_err(|message| {
        report_transfer_issue(
            tracking.as_ref(),
            &server.id,
            DeployStage::Uploading,
            message,
        )
    })?;

    emit_progress(
        app_handle,
        folder_name,
        total_size,
        total_size,
        0,
        0,
        start_time.elapsed().as_secs(),
        &local_path_str,
        &server_display,
        source,
    );

    if !post_commands.is_empty() {
        if let Some(tracking) = tracking.as_ref() {
            let _ = tracking.mark_stage(
                &server.id,
                DeployStage::ExecutingCommands,
                Some(100.0),
                Some(remote_target.clone()),
            );
        }
        emit_log_with_tracking(
            app_handle,
            format!("[{}] Executing post commands...", server.name),
            "info",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );

        for cmd in post_commands {
            if should_cancel.load(Ordering::SeqCst) {
                return Err(report_transfer_issue(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    "Cancelled".to_string(),
                ));
            }

            let final_cmd =
                substitute_variables(cmd, folder_name, local_folder_path, &remote_target);
            emit_log_with_tracking(
                app_handle,
                format!("[{}] $ {}", server.name, final_cmd),
                "command",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );

            let mut channel = sess.channel_session().map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    e.to_string(),
                )
            })?;
            channel
                .handle_extended_data(ssh2::ExtendedData::Merge)
                .map_err(|e| {
                    report_stage_failure(
                        tracking.as_ref(),
                        &server.id,
                        DeployStage::ExecutingCommands,
                        e.to_string(),
                    )
                })?;
            channel.exec(&final_cmd).map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    e.to_string(),
                )
            })?;
            channel.send_eof().map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    e.to_string(),
                )
            })?;

            let mut output_buf = String::new();
            let mut buf = [0u8; 4096];
            loop {
                match channel.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        output_buf.push_str(&chunk);
                        while let Some(pos) = output_buf.find('\n') {
                            let line = output_buf[..pos].trim_end_matches('\r').to_string();
                            if !line.is_empty() {
                                emit_log_with_tracking(
                                    app_handle,
                                    format!("[{}] > {}", server.name, line),
                                    "info",
                                    tracking.as_ref(),
                                    Some(server.id.as_str()),
                                    Some(server.name.as_str()),
                                );
                            }
                            output_buf = output_buf[pos + 1..].to_string();
                        }
                    }
                    Err(e) => {
                        emit_log_with_tracking(
                            app_handle,
                            format!("[{}] Read error: {}", server.name, e),
                            "warn",
                            tracking.as_ref(),
                            Some(server.id.as_str()),
                            Some(server.name.as_str()),
                        );
                        break;
                    }
                }
            }
            if !output_buf.trim().is_empty() {
                emit_log_with_tracking(
                    app_handle,
                    format!("[{}] > {}", server.name, output_buf.trim()),
                    "info",
                    tracking.as_ref(),
                    Some(server.id.as_str()),
                    Some(server.name.as_str()),
                );
            }

            channel.wait_close().map_err(|e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    format!("wait_close failed: {}", e),
                )
            })?;
            let exit_code = channel.exit_status().unwrap_or(-1);
            if exit_code != 0 {
                emit_log_with_tracking(
                    app_handle,
                    format!("[{}] Command exited with code {}", server.name, exit_code),
                    "error",
                    tracking.as_ref(),
                    Some(server.id.as_str()),
                    Some(server.name.as_str()),
                );
                return Err(report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    format!("Command exited with code {}", exit_code),
                ));
            }
        }
    }

    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_success(&server.id);
    }
    Ok(())
}

fn calculate_size(path: &Path) -> u64 {
    let mut size = 0;
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                size += calculate_size(&entry.path());
            }
        }
    } else if let Ok(meta) = fs::metadata(path) {
        size = meta.len();
    }
    size
}

/// Expands a file upload aimed at an existing remote directory into
/// `<dir>/<file name>`. `sftp.create` against a directory path fails with
/// `[SFTP(4)] failure`, so `/root` must become `/root/package.tar.gz`.
/// Returns `None` when the target is already usable as-is.
fn join_remote_file_target(
    local_path: &Path,
    remote_target: &str,
    local_is_dir: bool,
    remote_is_dir: bool,
) -> Option<String> {
    if local_is_dir || !remote_is_dir {
        return None;
    }
    let name = local_path.file_name()?;
    Some(format!(
        "{}/{}",
        remote_target.trim_end_matches('/'),
        name.to_string_lossy()
    ))
}

fn resolve_remote_file_target(
    sftp: &ssh2::Sftp,
    local_path: &Path,
    remote_target: &str,
) -> Option<String> {
    if local_path.is_dir() {
        return None;
    }
    let remote_is_dir = sftp
        .stat(Path::new(remote_target))
        .map(|stat| stat.is_dir())
        .unwrap_or(false);
    join_remote_file_target(local_path, remote_target, false, remote_is_dir)
}

fn join_remote_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{}", child.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            parent.trim_end_matches('/'),
            child.trim_start_matches('/')
        )
    }
}

fn remote_parent(path: &str) -> String {
    match path.rsplit_once('/') {
        Some(("", _)) => "/".to_string(),
        Some((parent, _)) => parent.to_string(),
        None => ".".to_string(),
    }
}

fn remote_file_name(path: &str) -> Option<&str> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
}

fn validate_remote_path(path: &str, label: &str) -> Result<(), String> {
    if path.is_empty()
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err(format!("Invalid {label}: {path}"));
    }
    Ok(())
}

fn is_archive_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar")
        || name.ends_with(".zip")
}

fn resolve_local_package_path(local_root: &Path) -> Option<PathBuf> {
    if local_root.is_file() {
        return Some(local_root.to_path_buf());
    }

    let mut archives = fs::read_dir(local_root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_archive_path(path))
        .collect::<Vec<_>>();
    archives.sort();
    archives.into_iter().next()
}

fn sha256_reader(mut reader: impl Read) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("SHA-256 read failed: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_local_file(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Cannot open local package {}: {error}", path.display()))?;
    sha256_reader(file)
}

fn sha256_remote_file(sftp: &ssh2::Sftp, path: &str) -> Result<String, String> {
    let file = sftp
        .open(Path::new(path))
        .map_err(|error| format!("Cannot open remote package {path}: {error}"))?;
    sha256_reader(file)
}

fn substitute_manual_variables(value: &str, paths: &ResolvedManualDeployPaths) -> String {
    value
        .replace("${folder_name}", &paths.folder_display)
        .replace("${remote_target}", &paths.command_target)
        .replace("${filename}", &paths.package_base)
        .replace("${remote_package}", &paths.remote_package)
        .replace("${extract_dir}", &paths.extract_dir)
}

fn resolve_manual_deploy_paths(
    sftp: &ssh2::Sftp,
    local_path: &str,
    remote_path: &str,
    options: &ManualDeployOptions,
) -> Result<ResolvedManualDeployPaths, String> {
    let normalized_remote = remote_path.trim().replace('\\', "/");
    if normalized_remote.is_empty() {
        return Err("Remote package path cannot be empty".to_string());
    }
    validate_remote_path(&normalized_remote, "remote package path")?;

    if options.transfer_policy == ManualDeployTransferPolicy::RemoteOnly {
        let remote_stat = sftp
            .stat(Path::new(&normalized_remote))
            .map_err(|_| format!("Remote package does not exist: {normalized_remote}"))?;
        if remote_stat.is_dir() {
            return Err(format!(
                "Remote-only deployment requires an exact package file path, not a directory: {normalized_remote}"
            ));
        }
        let file_name = remote_file_name(&normalized_remote)
            .ok_or_else(|| format!("Invalid remote package path: {normalized_remote}"))?
            .to_string();
        let command_target = remote_parent(&normalized_remote);
        let package_base = strip_archive_suffix(&file_name);
        let mut paths = ResolvedManualDeployPaths {
            local_root: None,
            local_package: None,
            upload_target: normalized_remote.clone(),
            remote_package: normalized_remote,
            command_target: command_target.clone(),
            extract_dir: join_remote_path(&command_target, &package_base),
            folder_display: file_name,
            package_base,
        };
        if !options.extract_dir.trim().is_empty() {
            paths.extract_dir = substitute_manual_variables(options.extract_dir.trim(), &paths);
        }
        validate_remote_path(&paths.extract_dir, "extraction directory")?;
        return Ok(paths);
    }

    let local_root = PathBuf::from(local_path.trim());
    if !local_root.exists() {
        return Err(format!(
            "Local path does not exist: {}",
            local_root.display()
        ));
    }
    let local_package = resolve_local_package_path(&local_root).ok_or_else(|| {
        format!(
            "No supported package file was found in {}",
            local_root.display()
        )
    })?;

    let mut upload_target = normalized_remote;
    if upload_target.ends_with('/') {
        let name = local_root
            .file_name()
            .ok_or_else(|| "Invalid local path: no file name".to_string())?;
        upload_target = join_remote_path(&upload_target, &name.to_string_lossy());
    }
    if let Some(resolved) = resolve_remote_file_target(sftp, &local_root, &upload_target) {
        upload_target = resolved;
    }

    let package_name = local_package
        .file_name()
        .ok_or_else(|| "Invalid local package path: no file name".to_string())?
        .to_string_lossy()
        .to_string();
    let remote_package = if local_root.is_file() {
        upload_target.clone()
    } else {
        join_remote_path(&upload_target, &package_name)
    };
    let command_target = remote_command_target(&local_root, &upload_target);
    let package_base = strip_archive_suffix(&package_name);
    let folder_display = local_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut paths = ResolvedManualDeployPaths {
        local_root: Some(local_root),
        local_package: Some(local_package),
        upload_target,
        remote_package,
        command_target: command_target.clone(),
        extract_dir: join_remote_path(&command_target, &package_base),
        folder_display,
        package_base,
    };
    if !options.extract_dir.trim().is_empty() {
        paths.extract_dir = substitute_manual_variables(options.extract_dir.trim(), &paths);
    }
    validate_remote_path(&paths.upload_target, "remote upload target")?;
    validate_remote_path(&paths.remote_package, "remote package path")?;
    validate_remote_path(&paths.extract_dir, "extraction directory")?;
    Ok(paths)
}

fn marker_path(extract_dir: &str) -> String {
    join_remote_path(extract_dir, ".file-sync-deploy.json")
}

fn marker_matches(
    sftp: &ssh2::Sftp,
    paths: &ResolvedManualDeployPaths,
    package_hash: &str,
) -> bool {
    if !sftp
        .stat(Path::new(&paths.extract_dir))
        .map(|stat| stat.is_dir())
        .unwrap_or(false)
    {
        return false;
    }
    let Ok(mut file) = sftp.open(Path::new(&marker_path(&paths.extract_dir))) else {
        return false;
    };
    let mut content = String::new();
    if file.read_to_string(&mut content).is_err() {
        return false;
    }
    serde_json::from_str::<ManualDeployMarker>(&content)
        .map(|marker| {
            marker.package_sha256 == package_hash
                && marker.remote_package_path == paths.remote_package
        })
        .unwrap_or(false)
}

fn inspect_manual_deploy_with_sftp(
    sftp: &ssh2::Sftp,
    local_path: &str,
    remote_path: &str,
    options: &ManualDeployOptions,
) -> Result<ManualDeployInspection, String> {
    let paths = resolve_manual_deploy_paths(sftp, local_path, remote_path, options)?;
    let remote_stat = sftp.stat(Path::new(&paths.remote_package)).ok();
    let package_exists = remote_stat
        .as_ref()
        .map(|stat| !stat.is_dir())
        .unwrap_or(false);

    let local_hash = paths
        .local_package
        .as_deref()
        .map(sha256_local_file)
        .transpose()?;
    let mut package_matches = None;
    let (transfer_action, package_hash) = match options.transfer_policy {
        ManualDeployTransferPolicy::Always => (
            ManualDeployTransferAction::Upload,
            local_hash.ok_or_else(|| "Always-upload mode requires a local package".to_string())?,
        ),
        ManualDeployTransferPolicy::Smart => {
            let local_package = paths
                .local_package
                .as_deref()
                .ok_or_else(|| "Smart deployment requires a local package".to_string())?;
            let local_size = fs::metadata(local_package)
                .map_err(|error| format!("Cannot read local package metadata: {error}"))?
                .len();
            let matches = if let Some(stat) = remote_stat.as_ref().filter(|stat| !stat.is_dir()) {
                if stat.size == Some(local_size) {
                    sha256_remote_file(sftp, &paths.remote_package)?
                        == local_hash.as_deref().unwrap_or_default()
                } else {
                    false
                }
            } else {
                false
            };
            package_matches = package_exists.then_some(matches);
            (
                if matches {
                    ManualDeployTransferAction::Reuse
                } else {
                    ManualDeployTransferAction::Upload
                },
                local_hash
                    .ok_or_else(|| "Smart deployment requires a local package".to_string())?,
            )
        }
        ManualDeployTransferPolicy::RemoteOnly => {
            if !package_exists {
                return Err(format!(
                    "Remote package does not exist: {}",
                    paths.remote_package
                ));
            }
            (
                ManualDeployTransferAction::Reuse,
                sha256_remote_file(sftp, &paths.remote_package)?,
            )
        }
    };

    let extraction_ready = marker_matches(sftp, &paths, &package_hash);
    let extract_action = match options.extract_policy {
        ManualDeployExtractPolicy::Skip => ManualDeployExtractAction::Skip,
        ManualDeployExtractPolicy::Force => ManualDeployExtractAction::Extract,
        ManualDeployExtractPolicy::Auto if extraction_ready => ManualDeployExtractAction::Reuse,
        ManualDeployExtractPolicy::Auto => ManualDeployExtractAction::Extract,
    };

    Ok(ManualDeployInspection {
        paths,
        package_hash,
        package_exists,
        package_matches,
        extraction_ready,
        transfer_action,
        extract_action,
    })
}

fn open_manual_deploy_session(server: &DeployServer) -> Result<Session, String> {
    let addr = format!("{}:{}", server.host, server.port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .ok_or_else(|| format!("Address resolution failed for {}", server.host))?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(server.ssh_timeout_secs))
        .map_err(|error| format!("TCP connect failed: {error}"))?;
    let mut session = Session::new().map_err(|error| error.to_string())?;
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|error| error.to_string())?;
    session
        .userauth_password(&server.user, &server.password)
        .map_err(|error| format!("Authentication failed: {error}"))?;
    Ok(session)
}

pub fn preflight_manual_deploy(
    server: &DeployServer,
    local_path: &str,
    remote_path: &str,
    options: &ManualDeployOptions,
) -> Result<ManualDeployPreflightResult, String> {
    let session = open_manual_deploy_session(server)?;
    let sftp = session
        .sftp()
        .map_err(|error| format!("SFTP init failed: {error}"))?;
    let inspection = inspect_manual_deploy_with_sftp(&sftp, local_path, remote_path, options)?;
    Ok(ManualDeployPreflightResult {
        server_id: server.id.clone(),
        server_name: server.name.clone(),
        remote_package_path: inspection.paths.remote_package,
        extract_dir: inspection.paths.extract_dir,
        package_exists: inspection.package_exists,
        package_matches: inspection.package_matches,
        extraction_ready: inspection.extraction_ready,
        transfer_action: inspection.transfer_action,
        extract_action: inspection.extract_action,
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn validate_extract_cleanup_target(extract_dir: &str, remote_package: &str) -> Result<(), String> {
    let trimmed = extract_dir.trim_end_matches('/');
    let segments = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if !trimmed.starts_with('/')
        || trimmed == "/"
        || segments.len() < 2
        || segments
            .iter()
            .any(|segment| *segment == "." || *segment == "..")
        || remote_package == trimmed
        || remote_package.starts_with(&format!("{trimmed}/"))
    {
        return Err(format!(
            "Refusing to clean unsafe extraction directory: {extract_dir}"
        ));
    }
    Ok(())
}

fn execute_session_command(session: &Session, command: &str) -> Result<Vec<String>, String> {
    let mut channel = session
        .channel_session()
        .map_err(|error| error.to_string())?;
    channel
        .handle_extended_data(ssh2::ExtendedData::Merge)
        .map_err(|error| error.to_string())?;
    channel.exec(command).map_err(|error| error.to_string())?;
    channel.send_eof().map_err(|error| error.to_string())?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|error| error.to_string())?;
    channel.wait_close().map_err(|error| error.to_string())?;
    let exit_code = channel.exit_status().unwrap_or(-1);
    if exit_code != 0 {
        return Err(format!("Command exited with code {exit_code}"));
    }
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn write_deploy_marker(
    sftp: &ssh2::Sftp,
    paths: &ResolvedManualDeployPaths,
    package_hash: &str,
) -> Result<(), String> {
    if !sftp
        .stat(Path::new(&paths.extract_dir))
        .map(|stat| stat.is_dir())
        .unwrap_or(false)
    {
        return Err(format!(
            "Extraction command completed but the extraction directory does not exist: {}",
            paths.extract_dir
        ));
    }
    let content = serde_json::to_vec_pretty(&ManualDeployMarker {
        package_sha256: package_hash.to_string(),
        remote_package_path: paths.remote_package.clone(),
    })
    .map_err(|error| format!("Cannot serialize deployment marker: {error}"))?;
    let path = marker_path(&paths.extract_dir);
    let mut marker = sftp
        .create(Path::new(&path))
        .map_err(|error| format!("Cannot create deployment marker {path}: {error}"))?;
    marker
        .write_all(&content)
        .map_err(|error| format!("Cannot write deployment marker {path}: {error}"))
}

fn execute_manual_commands<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    session: &Session,
    server: &DeployServer,
    commands: &[String],
    paths: &ResolvedManualDeployPaths,
    should_cancel: &AtomicBool,
    tracking: Option<&DeployTrackingContext>,
) -> Result<(), String> {
    for command in commands {
        if should_cancel.load(Ordering::SeqCst) {
            return Err("Deployment cancelled".to_string());
        }
        let final_command = substitute_manual_variables(command, paths);
        emit_log_with_tracking(
            app_handle,
            format!("$ {final_command}"),
            "command",
            tracking,
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );
        let lines = execute_session_command(session, &final_command)?;
        for line in lines {
            emit_log_with_tracking(
                app_handle,
                format!("> {line}"),
                "info",
                tracking,
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
        }
    }
    Ok(())
}

pub fn deploy_manual<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    extract_commands: &[String],
    post_commands: &[String],
    local_path: &str,
    remote_path: &str,
    options: &ManualDeployOptions,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    tracking: Option<DeployTrackingContext>,
) -> Result<(), String> {
    emit_log_with_tracking(
        app_handle,
        format!(
            "Starting manual deployment: {} -> [{}] {}:{} ({:?} / {:?})",
            if local_path.trim().is_empty() {
                "<remote package>"
            } else {
                local_path
            },
            server.name,
            server.host,
            remote_path,
            options.transfer_policy,
            options.extract_policy,
        ),
        "info",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );

    let target_path_str = remote_path.trim().replace('\\', "/");

    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_stage(
            &server.id,
            DeployStage::Connecting,
            None,
            Some(target_path_str.clone()),
        );
    }

    let tcp = {
        let addr = format!("{}:{}", server.host, server.port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .ok_or_else(|| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Connecting,
                    format!("Address resolution failed for {}", server.host),
                )
            })?;
        TcpStream::connect_timeout(&addr, Duration::from_secs(server.ssh_timeout_secs)).map_err(
            |e| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::Connecting,
                    e.to_string(),
                )
            },
        )?
    };
    let mut sess = Session::new().map_err(|e| {
        report_stage_failure(
            tracking.as_ref(),
            &server.id,
            DeployStage::Connecting,
            e.to_string(),
        )
    })?;
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| {
        report_stage_failure(
            tracking.as_ref(),
            &server.id,
            DeployStage::Connecting,
            e.to_string(),
        )
    })?;
    sess.userauth_password(&server.user, &server.password)
        .map_err(|e| {
            report_stage_failure(
                tracking.as_ref(),
                &server.id,
                DeployStage::Connecting,
                e.to_string(),
            )
        })?;

    emit_log_with_tracking(
        app_handle,
        "SSH Connected & Authenticated".to_string(),
        "success",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );

    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_stage(
            &server.id,
            DeployStage::Uploading,
            Some(0.0),
            Some(target_path_str.clone()),
        );
    }

    let sftp = sess.sftp().map_err(|e| {
        report_stage_failure(
            tracking.as_ref(),
            &server.id,
            DeployStage::Uploading,
            format!("SFTP init failed: {}", e),
        )
    })?;

    emit_log_with_tracking(
        app_handle,
        "Inspecting remote package and extraction state...".to_string(),
        "info",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );
    let inspection = inspect_manual_deploy_with_sftp(&sftp, local_path, remote_path, options)
        .map_err(|message| {
            report_stage_failure(
                tracking.as_ref(),
                &server.id,
                DeployStage::Uploading,
                message,
            )
        })?;
    let paths = &inspection.paths;
    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_stage(
            &server.id,
            DeployStage::Uploading,
            None,
            Some(paths.remote_package.clone()),
        );
    }
    emit_log_with_tracking(
        app_handle,
        format!(
            "Preflight plan: package={} transfer={:?}, extract_dir={} extract={:?}",
            paths.remote_package,
            inspection.transfer_action,
            paths.extract_dir,
            inspection.extract_action
        ),
        "info",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );

    let total_size = paths
        .local_root
        .as_deref()
        .map(calculate_size)
        .or_else(|| {
            sftp.stat(Path::new(&paths.remote_package))
                .ok()
                .and_then(|stat| stat.size)
        })
        .unwrap_or(0);
    let server_display = format!("[{}] {}", server.name, paths.remote_package);
    let start_time = Instant::now();

    if inspection.transfer_action == ManualDeployTransferAction::Upload {
        let local_root = paths.local_root.as_deref().ok_or_else(|| {
            report_stage_failure(
                tracking.as_ref(),
                &server.id,
                DeployStage::Uploading,
                "Upload was requested without a local source".to_string(),
            )
        })?;
        let parent = remote_parent(&paths.upload_target);
        if parent != "." {
            execute_session_command(&sess, &format!("mkdir -p -- {}", shell_quote(&parent)))
                .map_err(|message| {
                    report_stage_failure(
                        tracking.as_ref(),
                        &server.id,
                        DeployStage::Uploading,
                        format!("Cannot create remote upload directory: {message}"),
                    )
                })?;
        }
        emit_log_with_tracking(
            app_handle,
            format!("Uploading to {}", paths.upload_target),
            "info",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );
        emit_progress(
            app_handle,
            &paths.folder_display,
            0,
            total_size,
            0,
            0,
            0,
            local_path,
            &server_display,
            "manual",
        );
        let mut copied_bytes = 0_u64;
        let mut last_emit_time = Instant::now();
        upload_with_progress(
            app_handle,
            &sftp,
            local_root,
            Path::new(&paths.upload_target),
            total_size,
            &mut copied_bytes,
            start_time,
            &mut last_emit_time,
            &paths.folder_display,
            local_path,
            &server_display,
            &should_cancel,
            &is_paused,
            "manual",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(paths.upload_target.as_str()),
        )
        .map_err(|message| {
            report_transfer_issue(
                tracking.as_ref(),
                &server.id,
                DeployStage::Uploading,
                message,
            )
        })?;
        emit_log_with_tracking(
            app_handle,
            "Upload complete".to_string(),
            "success",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );
    } else {
        emit_log_with_tracking(
            app_handle,
            format!("Reusing verified remote package: {}", paths.remote_package),
            "success",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );
    }
    emit_progress(
        app_handle,
        &paths.folder_display,
        total_size,
        total_size,
        0,
        0,
        start_time.elapsed().as_secs(),
        local_path,
        &server_display,
        "manual",
    );

    if inspection.extract_action != ManualDeployExtractAction::Skip || !post_commands.is_empty() {
        if let Some(tracking) = tracking.as_ref() {
            let _ = tracking.mark_stage(
                &server.id,
                DeployStage::ExecutingCommands,
                Some(100.0),
                Some(paths.remote_package.clone()),
            );
        }
    }

    match inspection.extract_action {
        ManualDeployExtractAction::Extract => {
            if extract_commands.is_empty() {
                return Err(report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    "Extraction is required but no extraction command group was selected"
                        .to_string(),
                ));
            }
            validate_extract_cleanup_target(&paths.extract_dir, &paths.remote_package).map_err(
                |message| {
                    report_stage_failure(
                        tracking.as_ref(),
                        &server.id,
                        DeployStage::ExecutingCommands,
                        message,
                    )
                },
            )?;
            if sftp.stat(Path::new(&paths.extract_dir)).is_ok() {
                emit_log_with_tracking(
                    app_handle,
                    format!(
                        "Cleaning incomplete extraction directory: {}",
                        paths.extract_dir
                    ),
                    "warn",
                    tracking.as_ref(),
                    Some(server.id.as_str()),
                    Some(server.name.as_str()),
                );
                execute_session_command(
                    &sess,
                    &format!("rm -rf -- {}", shell_quote(&paths.extract_dir)),
                )
                .map_err(|message| {
                    report_stage_failure(
                        tracking.as_ref(),
                        &server.id,
                        DeployStage::ExecutingCommands,
                        format!("Cannot clean extraction directory: {message}"),
                    )
                })?;
            }
            emit_log_with_tracking(
                app_handle,
                "Executing extraction command group...".to_string(),
                "info",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
            execute_manual_commands(
                app_handle,
                &sess,
                server,
                extract_commands,
                paths,
                &should_cancel,
                tracking.as_ref(),
            )
            .map_err(|message| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    message,
                )
            })?;
            write_deploy_marker(&sftp, paths, &inspection.package_hash).map_err(|message| {
                report_stage_failure(
                    tracking.as_ref(),
                    &server.id,
                    DeployStage::ExecutingCommands,
                    message,
                )
            })?;
            emit_log_with_tracking(
                app_handle,
                "Extraction completed and deployment marker was written".to_string(),
                "success",
                tracking.as_ref(),
                Some(server.id.as_str()),
                Some(server.name.as_str()),
            );
        }
        ManualDeployExtractAction::Reuse => emit_log_with_tracking(
            app_handle,
            "Extraction marker matches this package; skipping extraction".to_string(),
            "success",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        ),
        ManualDeployExtractAction::Skip => emit_log_with_tracking(
            app_handle,
            "Extraction was skipped by deployment policy".to_string(),
            "info",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        ),
    }

    if !post_commands.is_empty() {
        emit_log_with_tracking(
            app_handle,
            "Executing post-deployment commands...".to_string(),
            "info",
            tracking.as_ref(),
            Some(server.id.as_str()),
            Some(server.name.as_str()),
        );
        execute_manual_commands(
            app_handle,
            &sess,
            server,
            post_commands,
            paths,
            &should_cancel,
            tracking.as_ref(),
        )
        .map_err(|message| {
            report_stage_failure(
                tracking.as_ref(),
                &server.id,
                DeployStage::ExecutingCommands,
                message,
            )
        })?;
    }

    if let Some(tracking) = tracking.as_ref() {
        let _ = tracking.mark_success(&server.id);
    }

    emit_log_with_tracking(
        app_handle,
        format!("[{}] Deployment successful", server.name),
        "success",
        tracking.as_ref(),
        Some(server.id.as_str()),
        Some(server.name.as_str()),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upload_with_progress<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    sftp: &ssh2::Sftp,
    local_path: &Path,
    remote_path: &Path,
    total_size: u64,
    copied_bytes: &mut u64,
    start_time: Instant,
    last_emit_time: &mut Instant,
    folder_display: &str,
    local_path_str: &str,
    remote_path_display: &str,
    should_cancel: &Arc<AtomicBool>,
    is_paused: &Arc<AtomicBool>,
    source: &str,
    tracking: Option<&DeployTrackingContext>,
    server_id: Option<&str>,
    remote_target_root: Option<&str>,
) -> Result<(), String> {
    if should_cancel.load(Ordering::SeqCst) {
        return Err("Deployment cancelled".to_string());
    }

    if local_path.is_dir() {
        match sftp.stat(remote_path) {
            Ok(stat) if stat.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "Remote directory target is an existing file: {}",
                    remote_path.display()
                ));
            }
            Err(_) => {
                if let Err(error) = sftp.mkdir(remote_path, 0o755) {
                    let created_by_another_writer = sftp
                        .stat(remote_path)
                        .map(|stat| stat.is_dir())
                        .unwrap_or(false);
                    if !created_by_another_writer {
                        return Err(format!(
                            "Cannot create remote directory {}: {error}",
                            remote_path.display()
                        ));
                    }
                }
            }
        }
        for entry in fs::read_dir(local_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            let remote_parent_str = remote_path.to_string_lossy().replace('\\', "/");
            let child_name_str = name.to_string_lossy();
            let remote_child_str = format!(
                "{}/{}",
                remote_parent_str.trim_end_matches('/'),
                child_name_str
            );
            let remote_child_path = Path::new(&remote_child_str);
            upload_with_progress(
                app_handle,
                sftp,
                &path,
                remote_child_path,
                total_size,
                copied_bytes,
                start_time,
                last_emit_time,
                folder_display,
                local_path_str,
                remote_path_display,
                should_cancel,
                is_paused,
                source,
                tracking,
                server_id,
                remote_target_root,
            )?;
        }
    } else {
        let mut local_file = fs::File::open(local_path).map_err(|e| e.to_string())?;
        let mut remote_file = sftp.create(remote_path).map_err(|e| e.to_string())?;

        let mut buffer = [0u8; 64 * 1024];
        loop {
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Deployment cancelled".to_string());
            }
            while is_paused.load(Ordering::SeqCst) {
                if should_cancel.load(Ordering::SeqCst) {
                    return Err("Deployment cancelled".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            let n = local_file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buffer[..n])
                .map_err(|e| e.to_string())?;
            *copied_bytes += n as u64;

            let now = Instant::now();
            if now.duration_since(*last_emit_time).as_millis() > 200 {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    (*copied_bytes as f64 / elapsed) as u64
                } else {
                    0
                };
                let eta = if speed > 0 && total_size > *copied_bytes {
                    (total_size - *copied_bytes) / speed
                } else {
                    0
                };
                emit_progress(
                    app_handle,
                    folder_display,
                    *copied_bytes,
                    total_size,
                    speed,
                    eta,
                    elapsed as u64,
                    local_path_str,
                    remote_path_display,
                    source,
                );
                if let (Some(tracking), Some(server_id), Some(remote_target_root)) =
                    (tracking, server_id, remote_target_root)
                {
                    let percentage = if total_size > 0 {
                        (*copied_bytes as f64 / total_size as f64) * 100.0
                    } else {
                        0.0
                    };
                    let _ = tracking.mark_stage(
                        server_id,
                        DeployStage::Uploading,
                        Some(percentage),
                        Some(remote_target_root.to_string()),
                    );
                }
                *last_emit_time = now;
            }
        }
    }
    Ok(())
}

fn report_stage_failure(
    tracking: Option<&DeployTrackingContext>,
    server_id: &str,
    stage: DeployStage,
    message: String,
) -> String {
    if let Some(tracking) = tracking {
        let _ = tracking.mark_failure(server_id, stage, message.clone());
        let _ = tracking.record_log(Some(server_id), None, "error", &message);
    }
    message
}

fn report_transfer_issue(
    tracking: Option<&DeployTrackingContext>,
    server_id: &str,
    stage: DeployStage,
    message: String,
) -> String {
    if message.to_lowercase().contains("cancelled") {
        if let Some(tracking) = tracking {
            let _ = tracking.record_log(Some(server_id), None, "warn", &message);
            let _ = tracking.cancel_pending();
        }
        return message;
    }

    report_stage_failure(tracking, server_id, stage, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_domain::{AttemptStatus, DeployStage, TaskTriggerSource};
    use crate::task_manager::{DeployTarget, StartManualDeployRequest, TaskManager};

    #[test]
    fn file_upload_into_existing_remote_directory_appends_file_name() {
        let local = Path::new("E:\\UMS_TEMP\\D012\\VMS_U500_x86.tar.gz");

        // `/root` exists as a directory: the file must land inside it.
        assert_eq!(
            join_remote_file_target(local, "/root", false, true).as_deref(),
            Some("/root/VMS_U500_x86.tar.gz")
        );
        assert_eq!(
            join_remote_file_target(local, "/root/", false, true).as_deref(),
            Some("/root/VMS_U500_x86.tar.gz")
        );

        // A target that is not an existing directory is a full file path.
        assert_eq!(
            join_remote_file_target(local, "/root/renamed.tar.gz", false, false),
            None
        );

        // Folder uploads keep their own recursive mkdir behaviour.
        assert_eq!(join_remote_file_target(local, "/root", true, true), None);
    }

    #[test]
    fn package_base_name_strips_archive_suffix() {
        assert_eq!(strip_archive_suffix("VMS_U500_x86.tar.gz"), "VMS_U500_x86");
        assert_eq!(strip_archive_suffix("VMS_U500_x86.zip"), "VMS_U500_x86");
        assert_eq!(strip_archive_suffix("VMS_U500_x86"), "VMS_U500_x86");
    }

    #[test]
    fn single_file_deploy_resolves_filename_and_target_from_the_file() {
        let dir = std::env::temp_dir().join(format!("fst-deploy-vars-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let package = dir.join("VMS_U500_H16-B2101.12.0.260724_x86.tar.gz");
        fs::write(&package, b"payload").unwrap();

        // `${remote_target}` becomes the directory the file was uploaded into.
        assert_eq!(
            remote_command_target(&package, "/root/VMS_U500_H16-B2101.12.0.260724_x86.tar.gz"),
            "/root"
        );
        assert_eq!(
            remote_command_target(&package, "/VMS_U500_H16-B2101.12.0.260724_x86.tar.gz"),
            "/"
        );

        // `${filename}` is the archive name without its `.tar.gz` suffix, so
        // the built-in extract command does not end up with `.tar.gz.tar.gz`.
        let extract = substitute_variables(
            "cd ${remote_target} && tar -zxvf ${filename}.tar.gz",
            "VMS_U500_H16-B2101.12.0.260724_x86.tar.gz",
            &package,
            &remote_command_target(&package, "/root/VMS_U500_H16-B2101.12.0.260724_x86.tar.gz"),
        );
        assert_eq!(
            extract,
            "cd /root && tar -zxvf VMS_U500_H16-B2101.12.0.260724_x86.tar.gz"
        );

        let install = substitute_variables(
            "cd ${remote_target}/${filename} && ./update -f",
            "VMS_U500_H16-B2101.12.0.260724_x86.tar.gz",
            &package,
            &remote_command_target(&package, "/root/VMS_U500_H16-B2101.12.0.260724_x86.tar.gz"),
        );
        assert_eq!(
            install,
            "cd /root/VMS_U500_H16-B2101.12.0.260724_x86 && ./update -f"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_deploy_keeps_scanning_the_folder_for_the_archive() {
        let dir = std::env::temp_dir().join(format!("fst-deploy-folder-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("VMS_U500_x86.tar.gz"), b"payload").unwrap();

        assert_eq!(remote_command_target(&dir, "/root/pkg"), "/root/pkg");
        assert_eq!(
            substitute_variables(
                "cd ${remote_target} && tar -zxvf ${filename}.tar.gz",
                "pkg",
                &dir,
                "/root/pkg"
            ),
            "cd /root/pkg && tar -zxvf VMS_U500_x86.tar.gz"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manual_deploy_variables_include_package_and_extract_paths() {
        let paths = ResolvedManualDeployPaths {
            local_root: None,
            local_package: None,
            upload_target: "/root/pkg.tar.gz".to_string(),
            remote_package: "/root/pkg.tar.gz".to_string(),
            command_target: "/root".to_string(),
            extract_dir: "/root/pkg".to_string(),
            folder_display: "pkg.tar.gz".to_string(),
            package_base: "pkg".to_string(),
        };

        assert_eq!(
            substitute_manual_variables(
                "cd ${remote_target} && test -f ${remote_package} && cd ${extract_dir}/${filename}",
                &paths,
            ),
            "cd /root && test -f /root/pkg.tar.gz && cd /root/pkg/pkg"
        );
    }

    #[test]
    fn extraction_cleanup_rejects_root_parent_and_package_ancestor() {
        assert!(validate_extract_cleanup_target("/", "/root/pkg.tar.gz").is_err());
        assert!(validate_extract_cleanup_target("/root", "/root/pkg.tar.gz").is_err());
        assert!(validate_extract_cleanup_target("/root/pkg", "/root/pkg.tar.gz").is_ok());
        assert!(validate_extract_cleanup_target("/opt/app/pkg", "/root/pkg.tar.gz").is_ok());
        assert!(validate_extract_cleanup_target("../root/pkg", "/root/pkg.tar.gz").is_err());
    }

    #[test]
    fn remote_paths_reject_traversal_and_control_characters() {
        assert!(validate_remote_path("/root/pkg.tar.gz", "package").is_ok());
        assert!(validate_remote_path("/root/../etc/passwd", "package").is_err());
        assert!(validate_remote_path("/root/pkg\n.tar.gz", "package").is_err());
        assert!(validate_remote_path("", "package").is_err());
    }

    #[test]
    fn local_package_resolution_is_deterministic_and_archive_only() {
        let dir = std::env::temp_dir().join(format!(
            "fst-manual-package-resolution-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.txt"), b"ignored").unwrap();
        fs::write(dir.join("z-package.tar.gz"), b"z").unwrap();
        fs::write(dir.join("a-package.tar.gz"), b"a").unwrap();

        assert_eq!(
            resolve_local_package_path(&dir)
                .unwrap()
                .file_name()
                .unwrap(),
            "a-package.tar.gz"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_stage_failure_records_structured_task_log_excerpt() {
        let manager = TaskManager::new_in_memory();
        let handle = manager
            .begin_manual_deploy_run(StartManualDeployRequest {
                task_group_id: None,
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                source_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();
        manager
            .register_deploy_targets(
                &handle.task_group_id,
                &handle.run_id,
                &[DeployTarget {
                    server_id: "server-a".to_string(),
                    server_name: "Server A".to_string(),
                    server_host: "192.0.2.10".to_string(),
                    remote_target: "/srv/pkg".to_string(),
                    trigger_source: TaskTriggerSource::Manual,
                }],
            )
            .unwrap();

        let tracking =
            manager.tracking_context(handle.task_group_id.clone(), handle.run_id.clone());
        let message = report_stage_failure(
            Some(&tracking),
            "server-a",
            DeployStage::Connecting,
            "connection failed".to_string(),
        );

        assert_eq!(message, "connection failed");

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        let attempt = &detail.runs[0].deploy_attempts[0];
        assert_eq!(attempt.error_phase, Some(DeployStage::Connecting));
        assert_eq!(attempt.error_message.as_deref(), Some("connection failed"));
        assert_eq!(
            attempt.last_log_excerpt.as_deref(),
            Some("connection failed")
        );
    }

    #[test]
    fn report_transfer_issue_records_cancelled_task_log_excerpt() {
        let manager = TaskManager::new_in_memory();
        let handle = manager
            .begin_manual_deploy_run(StartManualDeployRequest {
                task_group_id: None,
                display_name: "pkg".to_string(),
                folder_name: "pkg".to_string(),
                local_target_path: "D:\\target\\pkg".to_string(),
                source_path: "D:\\target\\pkg".to_string(),
                trigger_source: TaskTriggerSource::Manual,
            })
            .unwrap();
        manager
            .register_deploy_targets(
                &handle.task_group_id,
                &handle.run_id,
                &[DeployTarget {
                    server_id: "server-a".to_string(),
                    server_name: "Server A".to_string(),
                    server_host: "192.0.2.10".to_string(),
                    remote_target: "/srv/pkg".to_string(),
                    trigger_source: TaskTriggerSource::Manual,
                }],
            )
            .unwrap();

        let tracking =
            manager.tracking_context(handle.task_group_id.clone(), handle.run_id.clone());
        tracking
            .mark_stage(
                "server-a",
                DeployStage::Uploading,
                Some(10.0),
                Some("/srv/pkg".to_string()),
            )
            .unwrap();

        let message = report_transfer_issue(
            Some(&tracking),
            "server-a",
            DeployStage::Uploading,
            "Deployment cancelled".to_string(),
        );

        assert_eq!(message, "Deployment cancelled");

        let detail = manager.get_group_detail(&handle.task_group_id).unwrap();
        let attempt = &detail.runs[0].deploy_attempts[0];
        assert_eq!(attempt.status, AttemptStatus::Cancelled);
        assert_eq!(
            attempt.last_log_excerpt.as_deref(),
            Some("Deployment cancelled")
        );
    }
}
