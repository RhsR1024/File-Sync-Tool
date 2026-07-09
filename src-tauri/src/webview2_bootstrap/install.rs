//! Spawn the Evergreen Standalone Installer silently and poll detection.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub const INSTALL_ARGS: [&str; 2] = ["/silent", "/install"];

pub fn install_command(installer: &Path) -> Command {
    let mut cmd = Command::new(installer);
    cmd.args(INSTALL_ARGS);
    cmd
}

pub fn run_silent_install(installer: &Path) -> Result<(), String> {
    let status = install_command(installer)
        .status()
        .map_err(|error| format!("failed to spawn WebView2 installer: {error}"))?;
    if status.success() {
        return Ok(());
    }

    let code = status.code().unwrap_or(-1);
    let hint = if code as u32 == 0x8007_0005 {
        " (may require administrator rights)"
    } else {
        ""
    };
    Err(format!("WebView2 installer exit code {code}{hint}"))
}

pub fn wait_for_runtime(timeout: Duration) -> Option<String> {
    wait_with(
        super::detect::detect_webview2_runtime,
        timeout,
        Duration::from_secs(2),
    )
}

fn wait_with(
    probe: impl Fn() -> Option<String>,
    timeout: Duration,
    interval: Duration,
) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(version) = probe() {
            return Some(version);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn install_command_uses_exact_silent_args() {
        let cmd = install_command(Path::new(r"C:\tmp\installer.exe"));
        assert_eq!(cmd.get_program(), OsStr::new(r"C:\tmp\installer.exe"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, [OsStr::new("/silent"), OsStr::new("/install")]);
    }

    #[test]
    fn wait_returns_immediately_when_probe_succeeds() {
        let hit = wait_with(
            || Some("109.0.1518.78".to_string()),
            Duration::from_secs(60),
            Duration::from_millis(1),
        );
        assert_eq!(hit.as_deref(), Some("109.0.1518.78"));
    }

    #[test]
    fn wait_retries_until_probe_succeeds() {
        let calls = std::cell::Cell::new(0);
        let hit = wait_with(
            || {
                calls.set(calls.get() + 1);
                (calls.get() >= 3).then(|| "1.0.0.1".to_string())
            },
            Duration::from_secs(5),
            Duration::from_millis(1),
        );
        assert_eq!(hit.as_deref(), Some("1.0.0.1"));
        assert!(calls.get() >= 3);
    }

    #[test]
    fn wait_times_out_to_none() {
        let hit = wait_with(|| None, Duration::from_millis(10), Duration::from_millis(2));
        assert!(hit.is_none());
    }
}
