use std::path::Path;

pub(crate) const TASK_NAME: &str = "FileSyncTool_ClipboardAdmin";

#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminTaskStatus {
    pub installed: bool,
    pub path_valid: bool,
    pub last_error: Option<String>,
}

pub(crate) fn build_create_task_args(exe_path: &Path) -> Vec<String> {
    vec![
        "/Create".to_string(),
        "/TN".to_string(),
        TASK_NAME.to_string(),
        "/TR".to_string(),
        format!("\"{}\" --admin-from-task", exe_path.to_string_lossy()),
        "/SC".to_string(),
        "ONLOGON".to_string(),
        "/RL".to_string(),
        "HIGHEST".to_string(),
        "/IT".to_string(),
        "/F".to_string(),
    ]
}

pub(crate) fn build_remove_task_args() -> Vec<String> {
    vec![
        "/Delete".to_string(),
        "/TN".to_string(),
        TASK_NAME.to_string(),
        "/F".to_string(),
    ]
}

pub(crate) fn build_query_task_args(verbose: bool) -> Vec<String> {
    let mut args = vec![
        "/Query".to_string(),
        "/TN".to_string(),
        TASK_NAME.to_string(),
    ];
    if verbose {
        args.extend(["/FO", "LIST", "/V"].into_iter().map(str::to_string));
    }
    args
}

#[allow(dead_code)]
pub(crate) fn build_run_task_args() -> Vec<String> {
    vec!["/Run".to_string(), "/TN".to_string(), TASK_NAME.to_string()]
}

pub(crate) fn build_run_task_command() -> String {
    format!("schtasks /Run /TN \"{TASK_NAME}\"")
}

pub(crate) fn query_output_matches_exe_path(output: &str, exe_path: &Path) -> bool {
    let expected = exe_path.to_string_lossy().to_lowercase();
    output.to_lowercase().contains(&expected)
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    use super::{
        build_create_task_args, build_query_task_args, build_remove_task_args, build_run_task_args,
        query_output_matches_exe_path, AdminTaskStatus,
    };

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    fn run_schtasks(args: &[String]) -> Result<Output, String> {
        Command::new("schtasks")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| decode_console_bytes(e.to_string().as_bytes()))
    }

    /// Decode bytes emitted by Windows console tools (e.g. `schtasks`) using the current
    /// OEM code page. These tools default to the OEM code page (936 / GBK on Chinese Windows),
    /// so naive UTF-8 decoding produces mojibake like `����: ϵͳ�Ҳ���ָ�����ļ���`.
    fn decode_console_bytes(bytes: &[u8]) -> String {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return text.to_string();
        }

        // `GetOEMCP` lives in kernel32.dll. Avoid a windows-rs feature just for one FFI entry.
        extern "system" {
            fn GetOEMCP() -> u32;
        }
        let code_page = unsafe { GetOEMCP() };
        let encoding = match code_page {
            936 => encoding_rs::GBK,
            950 => encoding_rs::BIG5,
            932 => encoding_rs::SHIFT_JIS,
            949 => encoding_rs::EUC_KR,
            65001 => encoding_rs::UTF_8,
            _ => encoding_rs::WINDOWS_1252,
        };
        let (decoded, _, _) = encoding.decode(bytes);
        decoded.into_owned()
    }

    fn stderr_string(output: &Output) -> String {
        let stderr = decode_console_bytes(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            return stderr;
        }
        decode_console_bytes(&output.stdout).trim().to_string()
    }

    fn is_missing_task_error(message: &str) -> bool {
        let lowered = message.to_lowercase();
        lowered.contains("cannot find")
            || lowered.contains("not exist")
            || message.contains("找不到")
            || message.contains("不存在")
    }

    pub fn create_task() -> Result<(), String> {
        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let _ = remove_task();
        let output = run_schtasks(&build_create_task_args(&exe_path))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(stderr_string(&output))
        }
    }

    #[allow(dead_code)]
    pub fn run_task() -> Result<(), String> {
        let output = run_schtasks(&build_run_task_args())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(stderr_string(&output))
        }
    }

    pub fn remove_task() -> Result<(), String> {
        let output = run_schtasks(&build_remove_task_args())?;
        if output.status.success() {
            return Ok(());
        }

        let message = stderr_string(&output);
        if is_missing_task_error(&message) {
            Ok(())
        } else {
            Err(message)
        }
    }

    pub fn task_status() -> AdminTaskStatus {
        let query_output = match run_schtasks(&build_query_task_args(false)) {
            Ok(output) => output,
            Err(error) => {
                return AdminTaskStatus {
                    installed: false,
                    path_valid: false,
                    last_error: Some(error),
                };
            }
        };

        if !query_output.status.success() {
            let message = stderr_string(&query_output);
            return if is_missing_task_error(&message) {
                AdminTaskStatus {
                    installed: false,
                    path_valid: false,
                    last_error: None,
                }
            } else {
                AdminTaskStatus {
                    installed: false,
                    path_valid: false,
                    last_error: Some(message),
                }
            };
        }

        let exe_path = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                return AdminTaskStatus {
                    installed: true,
                    path_valid: false,
                    last_error: Some(error.to_string()),
                };
            }
        };

        let verbose_output = match run_schtasks(&build_query_task_args(true)) {
            Ok(output) => output,
            Err(error) => {
                return AdminTaskStatus {
                    installed: true,
                    path_valid: false,
                    last_error: Some(error),
                };
            }
        };

        if !verbose_output.status.success() {
            return AdminTaskStatus {
                installed: true,
                path_valid: false,
                last_error: Some(stderr_string(&verbose_output)),
            };
        }

        let stdout = String::from_utf8_lossy(&verbose_output.stdout);
        let path_valid = query_output_matches_exe_path(&stdout, &exe_path);
        AdminTaskStatus {
            installed: true,
            path_valid,
            last_error: if path_valid {
                None
            } else {
                Some("task target does not match current executable".to_string())
            },
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod windows_impl {
    use super::AdminTaskStatus;

    pub fn create_task() -> Result<(), String> {
        Err("Task Scheduler is Windows-only".to_string())
    }

    pub fn run_task() -> Result<(), String> {
        Err("Task Scheduler is Windows-only".to_string())
    }

    pub fn remove_task() -> Result<(), String> {
        Ok(())
    }

    pub fn task_status() -> AdminTaskStatus {
        AdminTaskStatus {
            installed: false,
            path_valid: false,
            last_error: None,
        }
    }
}

pub use windows_impl::{create_task, remove_task, task_status};

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        build_create_task_args, build_run_task_command, query_output_matches_exe_path, TASK_NAME,
    };

    #[test]
    fn build_create_task_args_targets_current_exe_and_admin_switch() {
        let args = build_create_task_args(Path::new(r"C:\Program Files\File Sync Tool\app.exe"));

        assert_eq!(
            args,
            vec![
                "/Create".to_string(),
                "/TN".to_string(),
                TASK_NAME.to_string(),
                "/TR".to_string(),
                "\"C:\\Program Files\\File Sync Tool\\app.exe\" --admin-from-task".to_string(),
                "/SC".to_string(),
                "ONLOGON".to_string(),
                "/RL".to_string(),
                "HIGHEST".to_string(),
                "/IT".to_string(),
                "/F".to_string(),
            ],
        );
    }

    #[test]
    fn build_run_task_command_uses_fixed_task_name() {
        assert_eq!(
            build_run_task_command(),
            format!("schtasks /Run /TN \"{TASK_NAME}\""),
        );
    }

    #[test]
    fn query_output_matches_exe_path_ignores_case_and_quotes() {
        let query_output = r#"
Task To Run: "C:\Program Files\File Sync Tool\app.exe" --admin-from-task
"#;

        assert!(query_output_matches_exe_path(
            query_output,
            Path::new(r"c:\program files\file sync tool\app.exe"),
        ));
        assert!(!query_output_matches_exe_path(
            query_output,
            Path::new(r"c:\program files\file sync tool\other.exe"),
        ));
    }
}
