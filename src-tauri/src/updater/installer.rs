//! Helper-bat installation orchestration for the updater feature.

use crate::updater::{UpdaterError, HELPER_BAT};
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

pub fn build_helper_args(bat_path: &Path, pid: u32, src: &Path, dst: &Path) -> Vec<String> {
    vec![
        "/c".to_string(),
        "start".to_string(),
        "".to_string(),
        "/min".to_string(),
        bat_path.display().to_string(),
        pid.to_string(),
        src.display().to_string(),
        dst.display().to_string(),
    ]
}

pub fn spawn_helper(src: &Path, dst: &Path) -> Result<(), UpdaterError> {
    let bat_path = write_helper()?;
    let args = build_helper_args(&bat_path, std::process::id(), src, dst);
    std::process::Command::new("cmd.exe")
        .args(args)
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
        assert!(bat.contains("%~1"));
        assert!(bat.contains("%~2"));
        assert!(bat.contains("%~3"));
        assert!(bat.contains("move /y \"%~2\" \"%~3\""));
        assert!(bat.contains("start \"\" \"%~3\""));
        assert!(bat.contains("del \"%~f0\""));
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
    fn build_helper_args_quotes_paths() {
        let bat_path = std::path::Path::new(r"C:\Temp\fst-update.bat");
        let args = build_helper_args(
            bat_path,
            12345,
            std::path::Path::new(r"C:\Temp\with space\new.exe"),
            std::path::Path::new(r"C:\Program Files\app.exe"),
        );
        assert_eq!(args[0], "/c");
        assert_eq!(args[1], "start");
        assert_eq!(args[2], "");
        assert_eq!(args[3], "/min");
        assert_eq!(args[4], r"C:\Temp\fst-update.bat");
        assert_eq!(args[5], "12345");
        assert_eq!(args[6], r"C:\Temp\with space\new.exe");
        assert_eq!(args[7], r"C:\Program Files\app.exe");
    }
}
