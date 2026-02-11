use crate::config::{AppConfig, DeployServer};
use std::net::TcpStream;
use std::path::Path;
use ssh2::Session;
use std::io::Read;
use std::fs;
use tauri::Emitter;
use std::sync::Arc;

#[derive(Debug, serde::Serialize, Clone)]
struct LogEvent {
    msg: String,
    level: String,
}

fn emit_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, msg: String, level: &str) {
    let _ = app_handle.emit("log-message", LogEvent {
        msg,
        level: level.to_string(),
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
             let mut s = String::new();
             channel.read_to_string(&mut s).unwrap();
             channel.wait_close().unwrap();
             
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

pub fn deploy_manual<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    server: &DeployServer,
    post_commands: &[String],
    local_path: &str,
    remote_path: &str
) -> Result<(), String> {
    emit_log(app_handle, format!("Starting manual deployment: {} -> [{}] {}:{}", local_path, server.name, server.host, remote_path), "info");

    // 1. Connect
    let tcp = TcpStream::connect(format!("{}:{}", server.host, server.port))
        .map_err(|e| e.to_string())?;
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| e.to_string())?;
    sess.userauth_password(&server.user, &server.password).map_err(|e| e.to_string())?;

    emit_log(app_handle, "SSH Connected & Authenticated".to_string(), "success");

    let sftp = sess.sftp().map_err(|e| format!("SFTP init failed: {}", e))?;
    let local_p = Path::new(local_path);
    
    if !local_p.exists() {
        return Err(format!("Local path does not exist: {}", local_path));
    }

    // Determine target remote path logic (same as before)
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
            channel.wait_close().unwrap();
        }
    }

    upload_recursive(app_handle, &sftp, local_p, target_p)?;
    emit_log(app_handle, "Upload complete".to_string(), "success");

    // Exec commands
    if !post_commands.is_empty() {
        emit_log(app_handle, "Executing post-deployment commands...".to_string(), "info");
        for cmd in post_commands {
            emit_log(app_handle, format!("$ {}", cmd), "info");
            let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
            channel.exec(cmd).map_err(|e| e.to_string())?;
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
    // Ensure remote path uses forward slashes for Linux
    // Path::new might parse backslashes on Windows.
    // When passing to sftp, we should probably pass Path that looks like unix path.
    // However, sftp methods take &Path.
    // If we are on Windows, Path("foo/bar") is valid.
    
    if local_path.is_dir() {
        // Ensure remote dir exists (mkdir if not)
        // sftp.mkdir returns Err if exists, so ignore
        let _ = sftp.mkdir(remote_path, 0o755);
        
        for entry in fs::read_dir(local_path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            // remote_path is Path, join takes path. name is OsString.
            // On Windows, Path separators are \, on Linux /.
            // ssh2 sftp expects unix paths usually?
            // We should ensure remote path uses / separators.
            // But PathBuf logic might use \ on Windows.
            // Let's handle path string conversion carefully.
            
            // Just use Path join, but then convert separators for SFTP if needed?
            // Actually ssh2::Sftp::mkdir expects Path. It converts it internally?
            // If we run on Windows, Path::join produces backslashes.
            // Remote is Linux. It needs forward slashes.
            // We must manually construct string with forward slashes.
            let remote_parent_str = remote_path.to_string_lossy().to_string().replace("\\", "/");
            let child_name_str = name.to_string_lossy();
            let remote_child_str = format!("{}/{}", remote_parent_str.trim_end_matches('/'), child_name_str);
            let remote_child_path = Path::new(&remote_child_str);

            upload_recursive(app_handle, sftp, &path, remote_child_path)?;
        }
    } else {
        // File
        let mut local_file = fs::File::open(local_path).map_err(|e| e.to_string())?;
        let mut remote_file = sftp.create(remote_path).map_err(|e| e.to_string())?;
        
        // Copy content
        // For large files, stream?
        // std::io::copy(&mut local_file, &mut remote_file) works if SftpFile implements Write
        std::io::copy(&mut local_file, &mut remote_file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
