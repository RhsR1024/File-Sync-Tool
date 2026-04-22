//! Admin elevation detection + auto-start configuration (spec 搂8.4).

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(not(target_os = "windows"))]
mod stub {
    use crate::clipboard::task_scheduler::AdminTaskStatus;

    pub fn is_elevated() -> bool {
        false
    }

    pub fn set_autostart_as_admin(
        _exe_path: &str,
        _enable: bool,
    ) -> Result<AdminTaskStatus, String> {
        Err("admin auto-start is Windows-only".into())
    }

    pub fn is_autostart_as_admin_enabled() -> bool {
        false
    }

    pub fn create_admin_task(_exe_path: &str) -> Result<AdminTaskStatus, String> {
        Err("Task Scheduler is Windows-only".into())
    }

    pub fn remove_admin_task(_exe_path: &str) -> Result<AdminTaskStatus, String> {
        Err("Task Scheduler is Windows-only".into())
    }

    pub fn admin_task_status() -> AdminTaskStatus {
        AdminTaskStatus {
            installed: false,
            path_valid: false,
            last_error: None,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub use stub::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use crate::clipboard::task_scheduler::{self, AdminTaskStatus};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const RUN_VALUE_NAME: &str = "FileSyncToolClipboardAdmin";

    pub fn is_elevated() -> bool {
        unsafe {
            let mut token: HANDLE = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
                return false;
            }
            let mut elev = TOKEN_ELEVATION::default();
            let mut ret_len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elev as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            )
            .is_ok()
                && elev.TokenIsElevated != 0;
            let _ = CloseHandle(token);
            ok
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set_run_value(command: &str) -> Result<(), String> {
        unsafe {
            let mut key = HKEY::default();
            let subkey = to_wide(RUN_SUBKEY);
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &mut key,
                None,
            );
            if status.is_err() {
                return Err(format!("RegCreateKeyExW: {:?}", status));
            }

            let value_name = to_wide(RUN_VALUE_NAME);
            let wide = to_wide(command);
            let bytes = std::slice::from_raw_parts(
                wide.as_ptr() as *const u8,
                wide.len() * std::mem::size_of::<u16>(),
            );
            let set_status = RegSetValueExW(key, PCWSTR(value_name.as_ptr()), 0, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
            if set_status.is_err() {
                return Err(format!("RegSetValueExW: {:?}", set_status));
            }
        }

        Ok(())
    }

    fn clear_run_value() -> Result<(), String> {
        unsafe {
            let mut key = HKEY::default();
            let subkey = to_wide(RUN_SUBKEY);
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &mut key,
                None,
            );
            if status.is_err() {
                return Err(format!("RegCreateKeyExW: {:?}", status));
            }

            let value_name = to_wide(RUN_VALUE_NAME);
            let del_status = RegDeleteValueW(key, PCWSTR(value_name.as_ptr()));
            let _ = RegCloseKey(key);
            let err_str = format!("{:?}", del_status);
            if del_status.is_err()
                && !err_str.contains("FILE_NOT_FOUND")
                && !err_str.contains("0x80070002")
            {
                return Err(format!("RegDeleteValueW: {err_str}"));
            }
        }

        Ok(())
    }

    fn fallback_run_command(exe_path: &str) -> String {
        format!(
            "powershell -WindowStyle Hidden -Command \"Start-Process -FilePath '{}' -Verb RunAs\"",
            exe_path
        )
    }

    fn status_with_last_error(last_error: Option<String>) -> AdminTaskStatus {
        let mut status = task_scheduler::task_status();
        if last_error.is_some() {
            status.last_error = last_error;
        }
        status
    }

    pub fn set_autostart_as_admin(
        exe_path: &str,
        enable: bool,
    ) -> Result<AdminTaskStatus, String> {
        if enable {
            match task_scheduler::create_task() {
                Ok(()) => {
                    set_run_value(&task_scheduler::build_run_task_command())?;
                    Ok(task_scheduler::task_status())
                }
                Err(error) => {
                    set_run_value(&fallback_run_command(exe_path))?;
                    Ok(AdminTaskStatus {
                        installed: false,
                        path_valid: false,
                        last_error: Some(error),
                    })
                }
            }
        } else {
            clear_run_value()?;
            let cleanup_error = task_scheduler::remove_task().err();
            Ok(status_with_last_error(cleanup_error))
        }
    }

    pub fn create_admin_task(_exe_path: &str) -> Result<AdminTaskStatus, String> {
        task_scheduler::create_task()?;
        if is_autostart_as_admin_enabled() {
            set_run_value(&task_scheduler::build_run_task_command())?;
        }
        Ok(task_scheduler::task_status())
    }

    pub fn remove_admin_task(exe_path: &str) -> Result<AdminTaskStatus, String> {
        if is_autostart_as_admin_enabled() {
            set_run_value(&fallback_run_command(exe_path))?;
        }
        task_scheduler::remove_task()?;
        Ok(task_scheduler::task_status())
    }

    pub fn admin_task_status() -> AdminTaskStatus {
        task_scheduler::task_status()
    }

    pub fn is_autostart_as_admin_enabled() -> bool {
        unsafe {
            let mut key = HKEY::default();
            let subkey = to_wide(RUN_SUBKEY);
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                KEY_QUERY_VALUE,
                &mut key,
            )
            .is_err()
            {
                return false;
            }
            let value_name = to_wide(RUN_VALUE_NAME);
            let mut size: u32 = 0;
            let status = RegQueryValueExW(
                key,
                PCWSTR(value_name.as_ptr()),
                None,
                None,
                None,
                Some(&mut size),
            );
            let _ = RegCloseKey(key);
            status.is_ok()
        }
    }
}
