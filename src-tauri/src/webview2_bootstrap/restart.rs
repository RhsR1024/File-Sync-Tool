//! Relaunch the current exe with original args plus the loop-prevention env.

use std::path::Path;
use std::process::Command;

pub const RESTARTED_ENV: &str = "FST_WEBVIEW2_BOOTSTRAP_RESTARTED";

pub fn restart_command(exe: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    cmd.env(RESTARTED_ENV, "1");
    cmd
}

pub fn restart_and_exit() -> ! {
    let exe = std::env::current_exe().unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match restart_command(&exe, &args).spawn() {
        Ok(_) => crate::startup_log(
            "info",
            "webview2 bootstrap: installation complete, spawned restarted instance",
        ),
        Err(error) => crate::startup_log(
            "error",
            &format!("webview2 bootstrap: restart failed; please launch manually: {error}"),
        ),
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn preserves_original_args() {
        let cmd = restart_command(
            Path::new(r"C:\app\file-sync-tool.exe"),
            &["--minimized".to_string(), "--from-autostart".to_string()],
        );
        assert_eq!(cmd.get_program(), OsStr::new(r"C:\app\file-sync-tool.exe"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(
            args,
            [OsStr::new("--minimized"), OsStr::new("--from-autostart")]
        );
    }

    #[test]
    fn sets_restarted_env_flag() {
        let cmd = restart_command(Path::new("app.exe"), &[]);
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.contains(&(OsStr::new(RESTARTED_ENV), Some(OsStr::new("1")))));
    }
}
