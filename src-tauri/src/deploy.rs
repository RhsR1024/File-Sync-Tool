use crate::config::{CommandGroup, DeployServer, TaskServerBinding};
use ssh2::Session;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
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
    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
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
) -> Result<(), String> {
    if server_bindings.is_empty() {
        return Ok(());
    }

    emit_log(
        app_handle,
        format!(
            "Starting deployment for {} server(s)...",
            server_bindings.len()
        ),
        "info",
    );

    let total_size = calculate_size(local_folder_path);

    for (idx, binding) in server_bindings.iter().enumerate() {
        if should_cancel.load(Ordering::SeqCst) {
            emit_log(
                app_handle,
                "Remaining deployments cancelled.".to_string(),
                "warn",
            );
            break;
        }

        let server = match all_servers.iter().find(|s| s.id == binding.server_id) {
            Some(s) => s,
            None => {
                emit_log(
                    app_handle,
                    format!("Server ID '{}' not found, skipping.", binding.server_id),
                    "warn",
                );
                continue;
            }
        };

        if !server.enabled {
            emit_log(
                app_handle,
                format!("[{}] Server is disabled, skipping.", server.name),
                "info",
            );
            continue;
        }

        let commands = resolve_commands(&binding.command_group_ids, command_groups);

        emit_log(
            app_handle,
            format!(
                "Deploying to server {}/{} [{}]",
                idx + 1,
                server_bindings.len(),
                server.name
            ),
            "info",
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
            "scheduled",
        ) {
            emit_log(
                app_handle,
                format!("[{}] Deployment failed: {}", server.name, e),
                "error",
            );
        } else {
            emit_log(
                app_handle,
                format!("[{}] Deployment successful", server.name),
                "success",
            );
        }
    }

    Ok(())
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
        let replacement = if let Ok(entries) = fs::read_dir(local_path) {
            let mut found_name = folder_name.to_string();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy();
                        if name_str.ends_with(".tar.gz") {
                            found_name = name_str.trim_end_matches(".tar.gz").to_string();
                            break;
                        }
                    }
                }
            }
            found_name
        } else {
            folder_name.to_string()
        };

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
) -> Result<(), String> {
    emit_log(
        app_handle,
        format!(
            "[{}] Connecting to {}:{}",
            server.name, server.host, server.remote_path
        ),
        "info",
    );

    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
        .map_err(|e| e.to_string())?;
    let mut sess = Session::new().map_err(|e| e.to_string())?;
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| e.to_string())?;
    sess.userauth_password(&server.user, &server.password)
        .map_err(|e| e.to_string())?;

    emit_log(app_handle, format!("[{}] Connected", server.name), "info");

    let remote_target = format!(
        "{}/{}",
        server.remote_path.trim_end_matches('/'),
        folder_name
    );

    let sftp = sess
        .sftp()
        .map_err(|e| format!("SFTP init failed: {}", e))?;

    match sftp.stat(Path::new(&remote_target)) {
        Ok(_) => {
            emit_log(
                app_handle,
                format!(
                    "[{}] Remote directory {} already exists. Continuing upload/overwrite.",
                    server.name, remote_target
                ),
                "info",
            );
        }
        Err(_) => {
            emit_log(
                app_handle,
                format!("[{}] Uploading to {}", server.name, remote_target),
                "info",
            );
            let mut channel = sess
                .channel_session()
                .map_err(|e| format!("channel_session failed: {}", e))?;
            channel
                .exec(&format!("mkdir -p {}", remote_target))
                .map_err(|e| format!("mkdir failed: {}", e))?;
            channel
                .send_eof()
                .map_err(|e| format!("send_eof failed: {}", e))?;
            let mut s = String::new();
            channel
                .read_to_string(&mut s)
                .map_err(|e| format!("read failed: {}", e))?;
            channel
                .wait_close()
                .map_err(|e| format!("wait_close failed: {}", e))?;
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
    )?;

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
        emit_log(
            app_handle,
            format!("[{}] Executing post commands...", server.name),
            "info",
        );

        for cmd in post_commands {
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Cancelled".to_string());
            }

            let final_cmd =
                substitute_variables(cmd, folder_name, local_folder_path, &remote_target);
            emit_log(
                app_handle,
                format!("[{}] $ {}", server.name, final_cmd),
                "command",
            );

            let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
            channel
                .handle_extended_data(ssh2::ExtendedData::Merge)
                .map_err(|e| e.to_string())?;
            channel.exec(&final_cmd).map_err(|e| e.to_string())?;
            channel.send_eof().map_err(|e| e.to_string())?;

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
                                emit_log(
                                    app_handle,
                                    format!("[{}] > {}", server.name, line),
                                    "info",
                                );
                            }
                            output_buf = output_buf[pos + 1..].to_string();
                        }
                    }
                    Err(e) => {
                        emit_log(
                            app_handle,
                            format!("[{}] Read error: {}", server.name, e),
                            "warn",
                        );
                        break;
                    }
                }
            }
            if !output_buf.trim().is_empty() {
                emit_log(
                    app_handle,
                    format!("[{}] > {}", server.name, output_buf.trim()),
                    "info",
                );
            }

            channel
                .wait_close()
                .map_err(|e| format!("wait_close failed: {}", e))?;
            let exit_code = channel.exit_status().unwrap_or(-1);
            if exit_code != 0 {
                emit_log(
                    app_handle,
                    format!("[{}] Command exited with code {}", server.name, exit_code),
                    "error",
                );
            }
        }
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

pub fn deploy_manual<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    post_commands: &[String],
    local_path: &str,
    remote_path: &str,
    should_cancel: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
) -> Result<(), String> {
    emit_log(
        app_handle,
        format!(
            "Starting manual deployment: {} -> [{}] {}:{}",
            local_path, server.name, server.host, remote_path
        ),
        "info",
    );

    let local_p = Path::new(local_path);
    if !local_p.exists() {
        return Err(format!("Local path does not exist: {}", local_path));
    }

    emit_log(app_handle, "Calculating size...".to_string(), "info");
    let total_size = calculate_size(local_p);
    emit_log(
        app_handle,
        format!("Total size: {} bytes", total_size),
        "info",
    );

    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
        .map_err(|e| e.to_string())?;
    let mut sess = Session::new().map_err(|e| e.to_string())?;
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| e.to_string())?;
    sess.userauth_password(&server.user, &server.password)
        .map_err(|e| e.to_string())?;

    emit_log(
        app_handle,
        "SSH Connected & Authenticated".to_string(),
        "success",
    );

    let sftp = sess
        .sftp()
        .map_err(|e| format!("SFTP init failed: {}", e))?;

    let mut target_path_str = remote_path.to_string();
    if target_path_str.ends_with('/') || target_path_str.ends_with('\\') {
        let name = local_p
            .file_name()
            .ok_or("Invalid local path: no file name")?
            .to_string_lossy();
        target_path_str = format!(
            "{}/{}",
            target_path_str.trim_end_matches(&['/', '\\'][..]),
            name
        );
    }
    let target_path_str = target_path_str.replace('\\', "/");
    let target_p = Path::new(&target_path_str);

    emit_log(
        app_handle,
        format!("Uploading to {}", target_path_str),
        "info",
    );

    if let Some(parent) = target_p.parent() {
        let parent_str = parent.to_string_lossy().replace('\\', "/");
        if !parent_str.is_empty() {
            let mut channel = sess
                .channel_session()
                .map_err(|e| format!("channel_session failed: {}", e))?;
            channel
                .exec(&format!("mkdir -p {}", parent_str))
                .map_err(|e| format!("mkdir failed: {}", e))?;
            channel
                .send_eof()
                .map_err(|e| format!("send_eof failed: {}", e))?;
            let mut s = String::new();
            channel
                .read_to_string(&mut s)
                .map_err(|e| format!("read failed: {}", e))?;
            channel
                .wait_close()
                .map_err(|e| format!("wait_close failed: {}", e))?;
        }
    }

    let mut copied_bytes = 0u64;
    let start_time = Instant::now();
    let mut last_emit_time = Instant::now();
    let server_display = format!("[{}] {}", server.name, target_path_str);
    let folder_display = local_p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    emit_progress(
        app_handle,
        &folder_display,
        0,
        total_size,
        0,
        0,
        0,
        local_path,
        &server_display,
        "manual",
    );

    upload_with_progress(
        app_handle,
        &sftp,
        local_p,
        target_p,
        total_size,
        &mut copied_bytes,
        start_time,
        &mut last_emit_time,
        &folder_display,
        local_path,
        &server_display,
        &should_cancel,
        &is_paused,
        "manual",
    )?;

    emit_log(app_handle, "Upload complete".to_string(), "success");
    emit_progress(
        app_handle,
        &folder_display,
        total_size,
        total_size,
        0,
        0,
        start_time.elapsed().as_secs(),
        local_path,
        &server_display,
        "manual",
    );

    if !post_commands.is_empty() {
        emit_log(
            app_handle,
            "Executing post-deployment commands...".to_string(),
            "info",
        );

        for cmd in post_commands {
            if should_cancel.load(Ordering::SeqCst) {
                return Err("Deployment cancelled".to_string());
            }

            let final_cmd = substitute_variables(cmd, &folder_display, local_p, &target_path_str);
            emit_log(app_handle, format!("$ {}", final_cmd), "command");

            let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
            channel
                .handle_extended_data(ssh2::ExtendedData::Merge)
                .map_err(|e| e.to_string())?;
            channel.exec(&final_cmd).map_err(|e| e.to_string())?;
            channel.send_eof().map_err(|e| e.to_string())?;

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
                                emit_log(app_handle, format!("> {}", line), "info");
                            }
                            output_buf = output_buf[pos + 1..].to_string();
                        }
                    }
                    Err(e) => {
                        emit_log(app_handle, format!("Read error: {}", e), "warn");
                        break;
                    }
                }
            }
            if !output_buf.trim().is_empty() {
                emit_log(app_handle, format!("> {}", output_buf.trim()), "info");
            }

            channel
                .wait_close()
                .map_err(|e| format!("wait_close failed: {}", e))?;
            let exit_code = channel.exit_status().unwrap_or(-1);
            if exit_code != 0 {
                emit_log(
                    app_handle,
                    format!("Command exited with code {}", exit_code),
                    "error",
                );
            }
        }
    }

    emit_log(
        app_handle,
        format!("[{}] Deployment successful", server.name),
        "success",
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
) -> Result<(), String> {
    if should_cancel.load(Ordering::SeqCst) {
        return Err("Deployment cancelled".to_string());
    }

    if local_path.is_dir() {
        let _ = sftp.mkdir(remote_path, 0o755);
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
                *last_emit_time = now;
            }
        }
    }
    Ok(())
}
