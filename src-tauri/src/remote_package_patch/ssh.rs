use super::{RemoteAuth, RemoteDirEntry, RemoteDirListing, RemoteSshConfig};
use ssh2::{FileStat, PtyModeOpcode, PtyModes, Session};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, Instant};

const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFDIR: u32 = 0o040000;
const S_IFLNK: u32 = 0o120000;

pub fn connect(config: &RemoteSshConfig) -> Result<Session, String> {
    super::validate_config(config)?;
    let addr = format!("{}:{}", config.host.trim(), config.port)
        .to_socket_addrs()
        .map_err(|error| format!("Address resolution failed: {error}"))?
        .next()
        .ok_or_else(|| "Address resolution returned no address".to_string())?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|error| format!("TCP connect failed: {error}"))?;

    let mut session =
        Session::new().map_err(|error| format!("SSH session init failed: {error}"))?;
    // Fail fast instead of hanging forever on a dead peer; exec_stream lifts
    // this for long-running remote scripts whose stages can be silent.
    session.set_timeout(15_000);
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|error| format!("SSH handshake failed: {error}"))?;
    match &config.auth {
        RemoteAuth::Password { password } => session
            .userauth_password(config.username.trim(), password)
            .map_err(|error| format!("SSH password authentication failed: {error}"))?,
        RemoteAuth::KeyFile {
            key_path,
            passphrase,
        } => session
            .userauth_pubkey_file(
                config.username.trim(),
                None,
                Path::new(key_path),
                passphrase.as_deref(),
            )
            .map_err(|error| format!("SSH private-key authentication failed: {error}"))?,
    }
    if !session.authenticated() {
        return Err("SSH authentication failed".into());
    }
    Ok(session)
}

pub fn exec_capture(session: &Session, command: &str) -> Result<String, String> {
    match exec_capture_mode(session, command, CommandMode::Exec) {
        Ok(output) => Ok(output),
        Err(exec_error) if is_exec_restriction(&exec_error) => {
            match exec_capture_mode(session, command, CommandMode::ExecPty) {
                Ok(output) => Ok(output),
                Err(pty_error) if is_exec_restriction(&pty_error) => {
                    exec_capture_mode(session, command, CommandMode::Shell).map_err(|shell_error| {
                        format!(
                            "exec failed: {exec_error}; exec+pty failed: {pty_error}; shell failed: {shell_error}"
                        )
                    })
                }
                Err(pty_error) => Err(format!(
                    "exec failed: {exec_error}; exec+pty failed: {pty_error}"
                )),
            }
        }
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy)]
enum CommandMode {
    Exec,
    ExecPty,
    Shell,
}

fn is_exec_restriction(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("not allowed")
        || message.contains("not permitted")
        || message.contains("forbidden")
        || message.contains("restricted")
}

fn prepare_command_channel(
    session: &Session,
    command: &str,
    mode: CommandMode,
) -> Result<ssh2::Channel, String> {
    let mut channel = session
        .channel_session()
        .map_err(|error| format!("channel_session failed: {error}"))?;
    channel
        .handle_extended_data(ssh2::ExtendedData::Merge)
        .map_err(|error| error.to_string())?;

    match mode {
        CommandMode::Exec => channel.exec(command).map_err(|error| error.to_string())?,
        CommandMode::ExecPty => {
            channel
                .request_pty("xterm", None, None)
                .map_err(|error| format!("PTY allocation failed: {error}"))?;
            channel.exec(command).map_err(|error| error.to_string())?;
        }
        CommandMode::Shell => {
            // A few hardened appliances reject SSH exec requests but allow an
            // interactive shell. Disable terminal echo so the submitted scan
            // script cannot be mistaken for script protocol output.
            let mut modes = PtyModes::new();
            modes.set_boolean(PtyModeOpcode::ECHO, false);
            modes.set_boolean(PtyModeOpcode::ECHONL, false);
            channel
                .request_pty("xterm", Some(modes), None)
                .map_err(|error| format!("PTY allocation failed: {error}"))?;
            channel
                .shell()
                .map_err(|error| format!("Shell start failed: {error}"))?;
            channel
                .write_all(format!("printf '\\n'\n{command}\nexit $?\n").as_bytes())
                .map_err(|error| format!("Shell command write failed: {error}"))?;
        }
    }
    let _ = channel.send_eof();
    Ok(channel)
}

fn exec_capture_mode(
    session: &Session,
    command: &str,
    mode: CommandMode,
) -> Result<String, String> {
    let mut channel = prepare_command_channel(session, command, mode)?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|error| error.to_string())?;
    channel.wait_close().map_err(|error| error.to_string())?;
    let code = channel.exit_status().unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "Remote command exited with {code}: {}",
            output.trim()
        ));
    }
    Ok(output)
}

pub fn exec_stream<F>(session: &Session, command: &str, mut on_line: F) -> Result<i32, String>
where
    F: FnMut(&str),
{
    // Remote scan/patch scripts may stay silent for minutes (gzip/zstd on
    // large archives), so streaming reads must not hit the session timeout.
    session.set_timeout(0);
    match exec_stream_mode(session, command, CommandMode::Exec, &mut on_line) {
        Ok((exit_code, false)) | Ok((exit_code @ 0, true)) => Ok(exit_code),
        Ok((_exit_code, true)) => {
            match exec_stream_mode(session, command, CommandMode::ExecPty, &mut on_line) {
                Ok((exit_code, false)) | Ok((exit_code @ 0, true)) => Ok(exit_code),
                Ok((_exit_code, true)) => {
                    exec_stream_mode(session, command, CommandMode::Shell, &mut on_line)
                        .map(|(exit_code, _)| exit_code)
                }
                Err(error) if is_exec_restriction(&error) => {
                    exec_stream_mode(session, command, CommandMode::Shell, &mut on_line)
                        .map(|(exit_code, _)| exit_code)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) if is_exec_restriction(&error) => {
            match exec_stream_mode(session, command, CommandMode::ExecPty, &mut on_line) {
                Ok((exit_code, false)) | Ok((exit_code @ 0, true)) => Ok(exit_code),
                Ok((_exit_code, true)) => {
                    exec_stream_mode(session, command, CommandMode::Shell, &mut on_line)
                        .map(|(exit_code, _)| exit_code)
                }
                Err(pty_error) if is_exec_restriction(&pty_error) => {
                    exec_stream_mode(session, command, CommandMode::Shell, &mut on_line)
                        .map(|(exit_code, _)| exit_code)
                }
                Err(pty_error) => Err(pty_error),
            }
        }
        Err(error) => Err(error),
    }
}

fn exec_stream_mode<F>(
    session: &Session,
    command: &str,
    mode: CommandMode,
    on_line: &mut F,
) -> Result<(i32, bool), String>
where
    F: FnMut(&str),
{
    let mut channel = prepare_command_channel(session, command, mode)?;

    let mut pending = String::new();
    let mut buffer = [0_u8; 8192];
    let mut restriction_seen = false;
    loop {
        match channel.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                pending.push_str(&String::from_utf8_lossy(&buffer[..count]));
                while let Some(index) = pending.find('\n') {
                    let line = pending[..index].trim_end_matches('\r').to_string();
                    restriction_seen |= is_exec_restriction(&line);
                    on_line(&line);
                    pending = pending[index + 1..].to_string();
                }
            }
            Err(error) => return Err(format!("Remote command read failed: {error}")),
        }
    }
    if !pending.trim().is_empty() {
        let line = pending.trim_end_matches('\r');
        restriction_seen |= is_exec_restriction(line);
        on_line(line);
    }

    channel.wait_close().map_err(|error| error.to_string())?;
    Ok((channel.exit_status().unwrap_or(-1), restriction_seen))
}

pub fn list_dir(session: &Session, path: &str) -> Result<RemoteDirListing, String> {
    let sftp = session
        .sftp()
        .map_err(|error| format!("SFTP init failed: {error}"))?;
    let remote_path = if path.trim().is_empty() { "." } else { path };
    let mut entries = Vec::new();
    for (entry_path, stat) in sftp
        .readdir(Path::new(remote_path))
        .map_err(|error| format!("SFTP readdir failed for {remote_path}: {error}"))?
    {
        let name = entry_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if name == "." || name == ".." || name.is_empty() {
            continue;
        }
        let entry_remote_path = if remote_path == "/" {
            format!("/{name}")
        } else {
            format!("{}/{}", remote_path.trim_end_matches('/'), name)
        };
        entries.push(RemoteDirEntry {
            name,
            path: entry_remote_path,
            kind: stat_kind(&stat).to_string(),
            size: stat.size.unwrap_or(0),
            modified_ms: stat.mtime.map(|mtime| (mtime as i64) * 1000),
        });
    }

    entries.sort_by(|left, right| {
        let left_dir = left.kind == "dir";
        let right_dir = right.kind == "dir";
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(RemoteDirListing {
        path: remote_path.to_string(),
        entries,
    })
}

fn stat_kind(stat: &FileStat) -> &'static str {
    let Some(perm) = stat.perm else {
        return "other";
    };
    match perm & S_IFMT {
        S_IFDIR => "dir",
        S_IFREG => "file",
        S_IFLNK => "symlink",
        _ => "other",
    }
}

pub fn upload_file_with_progress<F>(
    sftp: &ssh2::Sftp,
    local_path: &Path,
    remote_path: &Path,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64),
{
    let mut local_file = std::fs::File::open(local_path).map_err(|error| {
        format!(
            "Failed to open local file {}: {error}",
            local_path.display()
        )
    })?;
    let mut remote_file = sftp.create(remote_path).map_err(|error| {
        format!(
            "Failed to create remote file {}: {error}",
            remote_path.display()
        )
    })?;
    let mut sent = 0_u64;
    let mut last_emit = Instant::now();
    let mut buffer = [0_u8; 64 * 1024];
    on_progress(0);
    loop {
        let count = local_file
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        remote_file
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        sent += count as u64;
        if last_emit.elapsed().as_millis() >= 200 {
            on_progress(sent);
            last_emit = Instant::now();
        }
    }
    on_progress(sent);
    Ok(())
}

pub fn write_remote_file(
    sftp: &ssh2::Sftp,
    remote_path: &Path,
    contents: &[u8],
    mode: u32,
) -> Result<(), String> {
    let mut remote_file = sftp.create(remote_path).map_err(|error| {
        format!(
            "Failed to create remote file {}: {error}",
            remote_path.display()
        )
    })?;
    remote_file
        .write_all(contents)
        .map_err(|error| error.to_string())?;
    sftp.setstat(
        remote_path,
        FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: Some(mode),
            atime: None,
            mtime: None,
        },
    )
    .map_err(|error| {
        format!(
            "Failed to chmod remote file {}: {error}",
            remote_path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_exec_restriction;

    #[test]
    fn detects_restricted_exec_messages() {
        assert!(is_exec_restriction(
            "Remote command execution is not allowed."
        ));
        assert!(is_exec_restriction("operation is FORBIDDEN"));
        assert!(!is_exec_restriction("tar: package not found"));
    }
}
