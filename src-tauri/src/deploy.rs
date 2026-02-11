use crate::config::{AppConfig, DeployServer};
use std::net::TcpStream;
use std::path::Path;
use ssh2::Session;
use std::io::{Read, Write};
use std::fs;
use tauri::Emitter;
use std::time::Instant;

#[derive(Debug, serde::Serialize, Clone)]
struct LogEvent {
    msg: String,
    level: String,
}

#[derive(Debug, serde::Serialize, Clone)]
struct ProgressEvent {
    folder: String,
    total_bytes: u64,
    copied_bytes: u64,
    percentage: f64,
    speed: u64, // bytes per second
    eta_seconds: u64,
}

fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit("log-message", LogEvent {
        msg,
        level: level.to_string(),
    });
}

fn emit_progress<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>, 
    folder: &str, 
    copied: u64, 
    total: u64,
    speed: u64,
    eta_seconds: u64
) {
    let percentage = if total > 0 {
        (copied as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    
    let _ = app_handle.emit("copy-progress", ProgressEvent {
        folder: folder.to_string(),
        total_bytes: total,
        copied_bytes: copied,
        percentage,
        speed,
        eta_seconds,
    });
}

pub fn check_connection(server: &DeployServer) -> Result<String, String> {
    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
        .map_err(|e| format!("TCP Connect failed to {}: {}", server.host, e))?;
    
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| format!("SSH Handshake failed: {}", e))?;
    
    sess.userauth_password(&server.user, &server.password)
        .map_err(|e| format!("Authentication failed: {}", e))?;
    
    Ok(format!("Connected to {}", server.name))
}

pub fn deploy_to_remote<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    config: &AppConfig,
    local_folder_path: &Path,
    folder_name: &str
) -> Result<(), String> {
    if !config.deploy_enabled {
        return Ok(());
    }

    if config.servers.is_empty() {
        emit_log(app_handle, "Deployment enabled but no servers configured.".to_string(), "warn");
        return Ok(());
    }

    emit_log(app_handle, format!("Starting deployment for {} servers...", config.servers.len()), "info");

    let servers = config.servers.clone();
    let local_path_buf = local_folder_path.to_path_buf();
    let folder_name_owned = folder_name.to_string();
    let app_handle = app_handle.clone();
    let post_commands = config.post_commands.clone();

    // Use a thread for each server to deploy in parallel
    for server in servers {
        if !server.enabled {
            continue;
        }
        
        let handle = app_handle.clone();
        let local = local_path_buf.clone();
        let name = folder_name_owned.clone();
        let commands = post_commands.clone();
        
        std::thread::spawn(move || {
             if let Err(e) = deploy_single_server(&handle, &server, &local, &name, &commands) {
                 emit_log(&handle, format!("[{}] Deployment failed: {}", server.name, e), "error");
             } else {
                 emit_log(&handle, format!("[{}] Deployment successful", server.name), "success");
             }
        });
    }

    Ok(())
}

fn deploy_single_server<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    local_folder_path: &Path,
    folder_name: &str,
    post_commands: &[String]
) -> Result<(), String> {
    emit_log(app_handle, format!("[{}] Connecting to {}:{}", server.name, server.host, server.remote_path), "info");

    // 1. Connect
    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
        .map_err(|e| e.to_string())?;
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| e.to_string())?;
    sess.userauth_password(&server.user, &server.password).map_err(|e| e.to_string())?;

    emit_log(app_handle, format!("[{}] Connected", server.name), "info");

    // 2. Create remote directory
    let remote_target = format!("{}/{}", server.remote_path.trim_end_matches('/'), folder_name);
    
    let sftp = sess.sftp().map_err(|e| format!("SFTP init failed: {}", e))?;
    
    // Check if exists
    match sftp.stat(Path::new(&remote_target)) {
        Ok(_) => {
             emit_log(app_handle, format!("[{}] Remote directory {} already exists. Skipping upload.", server.name, remote_target), "warn");
        },
        Err(_) => {
             emit_log(app_handle, format!("[{}] Uploading to {}", server.name, remote_target), "info");
             
             let mut channel = sess.channel_session().unwrap();
             channel.exec(&format!("mkdir -p {}", remote_target)).unwrap();
             channel.send_eof().unwrap();
             let mut s = String::new();
             channel.read_to_string(&mut s).unwrap();
             channel.wait_close().unwrap();
             
             // Use simple upload for auto-deploy (no progress bar to avoid spam)
             upload_recursive(app_handle, &sftp, local_folder_path, Path::new(&remote_target))?;
        }
    }

    // 3. Exec commands
    if !post_commands.is_empty() {
        emit_log(app_handle, format!("[{}] Executing post commands...", server.name), "info");
        
        for cmd in post_commands {
            emit_log(app_handle, format!("[{}] $ {}", server.name, cmd), "info");
            
            let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
            channel.exec(cmd).map_err(|e| e.to_string())?;
            channel.send_eof().map_err(|e| e.to_string())?;
            
            let mut s = String::new();
            channel.read_to_string(&mut s).map_err(|e| e.to_string())?;
            channel.wait_close().unwrap();
            
            if !s.is_empty() {
                emit_log(app_handle, format!("[{}] > {}", server.name, s.trim()), "info");
            }
            
            if channel.exit_status().unwrap() != 0 {
                emit_log(app_handle, format!("[{}] Command failed (exit {})", server.name, channel.exit_status().unwrap()), "error");
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
    remote_path: &str
) -> Result<(), String> {
    emit_log(app_handle, format!("Starting manual deployment: {} -> [{}] {}:{}", local_path, server.name, server.host, remote_path), "info");

    let local_p = Path::new(local_path);
    if !local_p.exists() {
        return Err(format!("Local path does not exist: {}", local_path));
    }

    // Calculate total size for progress
    emit_log(app_handle, "Calculating size...".to_string(), "info");
    let total_size = calculate_size(local_p);
    emit_log(app_handle, format!("Total size: {} bytes", total_size), "info");

    // 1. Connect
    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
        .map_err(|e| e.to_string())?;
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| e.to_string())?;
    sess.userauth_password(&server.user, &server.password).map_err(|e| e.to_string())?;

    emit_log(app_handle, "SSH Connected & Authenticated".to_string(), "success");

    let sftp = sess.sftp().map_err(|e| format!("SFTP init failed: {}", e))?;

    // Determine target remote path logic
    let mut target_path_str = remote_path.to_string();
    if target_path_str.ends_with('/') || target_path_str.ends_with('\\') {
         let name = local_p.file_name().unwrap().to_string_lossy();
         target_path_str = format!("{}{}", target_path_str.trim_end_matches(&['/', '\\'][..]), if target_path_str.contains('\\') { "\\" } else { "/" });
         target_path_str = format!("{}/{}", target_path_str.trim_end_matches('/'), name);
    }
    
    let target_path_str = target_path_str.replace("\\", "/");
    let target_p = Path::new(&target_path_str);

    emit_log(app_handle, format!("Uploading to {}", target_path_str), "info");

    if let Some(parent) = target_p.parent() {
        let parent_str = parent.to_string_lossy().replace("\\", "/");
        if !parent_str.is_empty() {
            let mut channel = sess.channel_session().unwrap();
            channel.exec(&format!("mkdir -p {}", parent_str)).unwrap();
            channel.send_eof().unwrap();
            let mut s = String::new();
            channel.read_to_string(&mut s).unwrap();
            channel.wait_close().unwrap();
        }
    }

    // Upload with progress
    let mut copied_bytes = 0;
    let start_time = Instant::now();
    let mut last_emit_time = Instant::now();
    
    upload_with_progress(
        app_handle, 
        &sftp, 
        local_p, 
        target_p, 
        total_size, 
        &mut copied_bytes, 
        start_time, 
        &mut last_emit_time
    )?;
    
    emit_log(app_handle, "Upload complete".to_string(), "success");
    // Emit 100%
    emit_progress(app_handle, &target_path_str, total_size, total_size, 0, 0);

    // Exec commands
    if !post_commands.is_empty() {
        emit_log(app_handle, "Executing post-deployment commands...".to_string(), "info");
        for cmd in post_commands {
            emit_log(app_handle, format!("$ {}", cmd), "info");
            let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
            channel.exec(cmd).map_err(|e| e.to_string())?;
            channel.send_eof().map_err(|e| e.to_string())?;
            
            let mut s = String::new();
            channel.read_to_string(&mut s).map_err(|e| e.to_string())?;
            channel.wait_close().unwrap();
            if !s.is_empty() {
                emit_log(app_handle, format!("> {}", s.trim()), "info");
            }
            if channel.exit_status().unwrap() != 0 {
                emit_log(app_handle, format!("Command failed with exit code {}", channel.exit_status().unwrap()), "error");
            }
        }
    }

    Ok(())
}

fn upload_recursive<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    sftp: &ssh2::Sftp,
    local_path: &Path,
    remote_path: &Path
) -> Result<(), String> {
    // Legacy simple upload
    if local_path.is_dir() {
        let _ = sftp.mkdir(remote_path, 0o755);
        for entry in fs::read_dir(local_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            let remote_parent_str = remote_path.to_string_lossy().to_string().replace("\\", "/");
            let child_name_str = name.to_string_lossy();
            let remote_child_str = format!("{}/{}", remote_parent_str.trim_end_matches('/'), child_name_str);
            let remote_child_path = Path::new(&remote_child_str);
            upload_recursive(app_handle, sftp, &path, remote_child_path)?;
        }
    } else {
        let mut local_file = fs::File::open(local_path).map_err(|e| e.to_string())?;
        let mut remote_file = sftp.create(remote_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut local_file, &mut remote_file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn upload_with_progress<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    sftp: &ssh2::Sftp,
    local_path: &Path,
    remote_path: &Path,
    total_size: u64,
    copied_bytes: &mut u64,
    start_time: Instant,
    last_emit_time: &mut Instant
) -> Result<(), String> {
    if local_path.is_dir() {
        let _ = sftp.mkdir(remote_path, 0o755);
        for entry in fs::read_dir(local_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            let remote_parent_str = remote_path.to_string_lossy().to_string().replace("\\", "/");
            let child_name_str = name.to_string_lossy();
            let remote_child_str = format!("{}/{}", remote_parent_str.trim_end_matches('/'), child_name_str);
            let remote_child_path = Path::new(&remote_child_str);
            
            upload_with_progress(app_handle, sftp, &path, remote_child_path, total_size, copied_bytes, start_time, last_emit_time)?;
        }
    } else {
        let mut local_file = fs::File::open(local_path).map_err(|e| e.to_string())?;
        let mut remote_file = sftp.create(remote_path).map_err(|e| e.to_string())?;
        
        let mut buffer = [0u8; 64 * 1024]; // 64KB buffer
        loop {
            let n = local_file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            remote_file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            
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
                    &remote_path.to_string_lossy(), // Use remote path as "folder" name or task name
                    *copied_bytes, 
                    total_size, 
                    speed, 
                    eta
                );
                *last_emit_time = now;
            }
        }
    }
    Ok(())
}
