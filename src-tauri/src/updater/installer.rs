//! Helper-bat installation orchestration for the updater feature.

use crate::updater::{UpdaterError, HELPER_BAT, HELPER_RENAME_BAT};
use std::path::{Path, PathBuf};

pub fn write_helper() -> Result<PathBuf, UpdaterError> {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "fst-update-{}-{}.bat",
        std::process::id(),
        random_suffix()
    ));
    std::fs::write(&path, HELPER_BAT).map_err(|error| UpdaterError::Io(error.to_string()))?;
    Ok(path)
}

pub fn build_helper_args(
    bat_path: &Path,
    pid: u32,
    src: &Path,
    current_path: &Path,
    target_path: &Path,
) -> Vec<String> {
    vec![
        "/d".to_string(),
        "/s".to_string(),
        "/c".to_string(),
        format!(
            "call \"{}\" {} \"{}\" \"{}\" \"{}\"",
            bat_path.display(),
            pid,
            src.display(),
            current_path.display(),
            target_path.display()
        ),
    ]
}

pub fn spawn_helper(
    src: &Path,
    current_path: &Path,
    target_path: &Path,
) -> Result<(), UpdaterError> {
    let bat_path = write_helper()?;
    let args = build_helper_args(
        &bat_path,
        std::process::id(),
        src,
        current_path,
        target_path,
    );
    spawn_cmd_helper(args)?;
    Ok(())
}

pub fn write_rename_helper() -> Result<PathBuf, UpdaterError> {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "fst-rename-{}-{}.bat",
        std::process::id(),
        random_suffix()
    ));
    std::fs::write(&path, HELPER_RENAME_BAT)
        .map_err(|error| UpdaterError::Io(error.to_string()))?;
    Ok(path)
}

pub fn build_rename_helper_args(
    bat_path: &Path,
    pid: u32,
    src: &Path,
    target: &Path,
) -> Vec<String> {
    vec![
        "/d".to_string(),
        "/s".to_string(),
        "/c".to_string(),
        format!(
            "call \"{}\" {} \"{}\" \"{}\"",
            bat_path.display(),
            pid,
            src.display(),
            target.display()
        ),
    ]
}

pub fn spawn_rename_helper(src: &Path, target: &Path) -> Result<(), UpdaterError> {
    let bat_path = write_rename_helper()?;
    let args = build_rename_helper_args(&bat_path, std::process::id(), src, target);
    spawn_cmd_helper(args)?;
    Ok(())
}

fn spawn_cmd_helper(args: Vec<String>) -> Result<(), UpdaterError> {
    let mut command = std::process::Command::new("cmd.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let (command_line, switches) = args
            .split_last()
            .ok_or_else(|| UpdaterError::Io("missing helper command line".to_string()))?;
        command.args(switches);
        // cmd.exe has different quoting rules from CommandLineToArgvW. Passing
        // its command text verbatim keeps quoted paths (including spaces) intact.
        command.raw_arg(command_line);
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    command.args(args);
    command
        .spawn()
        .map_err(|error| UpdaterError::Io(error.to_string()))?;
    Ok(())
}

fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_bat_template_is_present_and_uses_positional_args() {
        let bat = crate::updater::HELPER_BAT;
        assert!(bat.contains("tasklist"));
        assert!(bat.contains("WAIT_COUNT"));
        assert!(bat.contains("process_still_running"));
        assert!(!bat.contains("taskkill"));
        assert!(bat.contains("file-sync-tool-updater.log"));
        assert!(bat.contains("%~1"));
        assert!(bat.contains("%~2"));
        assert!(bat.contains("%~3"));
        assert!(bat.contains("%~4"));
        assert!(bat.contains("move /y \"%~2\" \"%~4\""));
        assert!(bat.contains("start \"\" \"%~4\""));
        assert!(bat.contains("del \"%~f0\""));
        assert!(
            !bat.contains("move /y \"%~3\""),
            "the running executable must remain available as a rollback path"
        );
    }

    #[test]
    fn write_helper_creates_a_unique_bat_under_temp() {
        let p1 = write_helper().expect("write");
        let p2 = write_helper().expect("write");
        assert_ne!(p1, p2);
        assert!(p1.exists());
        assert!(p2.exists());
        assert_eq!(p1.extension().unwrap(), "bat");
        let written = std::fs::read_to_string(&p1).unwrap();
        assert!(written.contains("tasklist"));
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn helper_bat_skips_move_when_src_equals_target() {
        let bat = crate::updater::HELPER_BAT;
        assert!(
            bat.contains("if /I \"%~2\"==\"%~4\""),
            "bat must short-circuit when src and target paths match"
        );
    }

    #[test]
    fn rename_helper_bat_renames_then_launches() {
        let bat = crate::updater::HELPER_RENAME_BAT;
        assert!(bat.contains("tasklist"));
        assert!(bat.contains("WAIT_COUNT"));
        assert!(bat.contains("process_still_running"));
        assert!(!bat.contains("taskkill"));
        assert!(bat.contains("file-sync-tool-updater.log"));
        assert!(bat.contains("%~1"));
        assert!(bat.contains("%~2"));
        assert!(bat.contains("%~3"));
        assert!(bat.contains("move /y \"%~2\" \"%~3\""));
        assert!(bat.contains("start \"\" \"%~3\""));
        assert!(bat.contains("start \"\" \"%~2\""));
        assert!(bat.contains("del \"%~f0\""));
    }

    #[test]
    fn write_rename_helper_creates_unique_bat_under_temp() {
        let p1 = write_rename_helper().expect("write");
        let p2 = write_rename_helper().expect("write");
        assert_ne!(p1, p2);
        assert!(p1.exists());
        assert!(p2.exists());
        assert_eq!(p1.extension().unwrap(), "bat");
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn build_rename_helper_args_positions_src_and_target() {
        let bat_path = std::path::Path::new(r"C:\Temp\fst-rename.bat");
        let args = build_rename_helper_args(
            bat_path,
            12345,
            std::path::Path::new(r"C:\app\file-sync-tool-1.1.0-202604271707.exe"),
            std::path::Path::new(r"C:\app\file-sync-tool-1.1.1-202605181737.exe"),
        );
        assert_eq!(args[0], "/d");
        assert_eq!(args[1], "/s");
        assert_eq!(args[2], "/c");
        assert_eq!(
            args[3],
            r#"call "C:\Temp\fst-rename.bat" 12345 "C:\app\file-sync-tool-1.1.0-202604271707.exe" "C:\app\file-sync-tool-1.1.1-202605181737.exe""#
        );
    }

    #[test]
    fn build_helper_args_quotes_paths() {
        let bat_path = std::path::Path::new(r"C:\Temp\fst-update.bat");
        let args = build_helper_args(
            bat_path,
            12345,
            std::path::Path::new(r"C:\Temp\with space\new.exe"),
            std::path::Path::new(r"C:\Program Files\file-sync-tool-1.0.7.exe"),
            std::path::Path::new(r"C:\Program Files\file-sync-tool-1.1.0.exe"),
        );
        assert_eq!(args[0], "/d");
        assert_eq!(args[1], "/s");
        assert_eq!(args[2], "/c");
        assert_eq!(
            args[3],
            r#"call "C:\Temp\fst-update.bat" 12345 "C:\Temp\with space\new.exe" "C:\Program Files\file-sync-tool-1.0.7.exe" "C:\Program Files\file-sync-tool-1.1.0.exe""#
        );
    }

    #[cfg(windows)]
    #[test]
    fn helper_launch_preserves_paths_with_spaces() {
        use std::time::{Duration, Instant};

        let unique = format!("fst helper args {} {}", std::process::id(), random_suffix());
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir(&root).expect("create spaced helper directory");
        let bat_path = root.join("update helper.bat");
        std::fs::write(&bat_path, HELPER_BAT).expect("write helper");

        let source = root.join("missing update source.exe");
        let current = root.join("missing current executable.exe");
        let target = root.join("missing update target.exe");
        let args = build_helper_args(&bat_path, u32::MAX, &source, &current, &target);
        spawn_cmd_helper(args).expect("spawn helper through cmd.exe");

        let deadline = Instant::now() + Duration::from_secs(5);
        while bat_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !bat_path.exists(),
            "the helper must receive its spaced path intact and self-delete"
        );
        std::fs::remove_dir(&root).expect("remove spaced helper directory");
    }
}
