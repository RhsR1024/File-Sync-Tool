//! Win+V replacement (spec §8.5). Windows-only — no-ops on other platforms.

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(not(target_os = "windows"))]
mod stub {
    pub fn enable_win_v_replacement() -> Result<(), String> {
        Err("Win+V replacement is Windows-only".into())
    }
    pub fn disable_win_v_replacement() -> Result<(), String> {
        Err("Win+V replacement is Windows-only".into())
    }
    pub fn is_win_v_replacement_enabled() -> bool {
        false
    }
}
#[cfg(not(target_os = "windows"))]
pub use stub::*;

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_DWORD, REG_VALUE_TYPE,
    };

    const EXPLORER_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer";
    const ALLOW_CLIPBOARD_HISTORY: &str = "AllowClipboardHistory";

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn write_dword(value: u32) -> Result<(), String> {
        unsafe {
            let mut key = HKEY::default();
            let subkey = to_wide(EXPLORER_SUBKEY);
            let create_status = RegCreateKeyExW(
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
            if create_status.is_err() {
                return Err(format!("RegCreateKeyExW: {:?}", create_status));
            }
            let name = to_wide(ALLOW_CLIPBOARD_HISTORY);
            let bytes = value.to_le_bytes();
            let set_status =
                RegSetValueExW(key, PCWSTR(name.as_ptr()), 0, REG_DWORD, Some(&bytes));
            let _ = RegCloseKey(key);
            if set_status.is_err() {
                return Err(format!("RegSetValueExW: {:?}", set_status));
            }
        }
        Ok(())
    }

    fn delete_value() -> Result<(), String> {
        unsafe {
            let mut key = HKEY::default();
            let subkey = to_wide(EXPLORER_SUBKEY);
            let open_status = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                KEY_SET_VALUE,
                &mut key,
            );
            if open_status.is_err() {
                return Err(format!("RegOpenKeyExW: {:?}", open_status));
            }
            let name = to_wide(ALLOW_CLIPBOARD_HISTORY);
            let del_status = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            let _ = RegCloseKey(key);
            // Deleting a non-existent value is fine.
            if del_status.is_err() {
                let err_str = format!("{:?}", del_status);
                if !err_str.contains("FILE_NOT_FOUND") && !err_str.contains("0x80070002") {
                    return Err(format!("RegDeleteValueW: {err_str}"));
                }
            }
        }
        Ok(())
    }

    fn read_dword() -> Option<u32> {
        unsafe {
            let mut key = HKEY::default();
            let subkey = to_wide(EXPLORER_SUBKEY);
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                KEY_QUERY_VALUE,
                &mut key,
            )
            .is_err()
            {
                return None;
            }
            let name = to_wide(ALLOW_CLIPBOARD_HISTORY);
            let mut value: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            let mut ty: REG_VALUE_TYPE = REG_DWORD;
            let status = RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut ty),
                Some(&mut value as *mut _ as *mut u8),
                Some(&mut size),
            );
            let _ = RegCloseKey(key);
            if status.is_ok() {
                Some(value)
            } else {
                None
            }
        }
    }

    fn restart_explorer() -> Result<(), String> {
        // 1. Kill explorer.exe (taskkill returns non-zero if no process matched; treat as OK).
        let _ = Command::new("taskkill")
            .args(["/IM", "explorer.exe", "/F"])
            .status()
            .map_err(|e| format!("taskkill: {e}"))?;

        // 2. Brief pause so Windows can clean up; then start explorer.
        thread::sleep(Duration::from_millis(500));
        Command::new("explorer.exe")
            .spawn()
            .map_err(|e| format!("start explorer: {e}"))?;
        Ok(())
    }

    pub fn enable_win_v_replacement() -> Result<(), String> {
        // Write the registry DWORD=0 first.
        write_dword(0).map_err(|e| format!("write DWORD: {e}"))?;

        // Try to restart explorer; on failure, roll back the registry change.
        if let Err(e) = restart_explorer() {
            let _ = delete_value();
            return Err(format!("restart explorer failed, rolled back: {e}"));
        }
        Ok(())
    }

    pub fn disable_win_v_replacement() -> Result<(), String> {
        delete_value().map_err(|e| format!("delete value: {e}"))?;
        // Explorer restart required for the change to take effect.
        if let Err(e) = restart_explorer() {
            return Err(format!("restart explorer failed: {e}"));
        }
        Ok(())
    }

    pub fn is_win_v_replacement_enabled() -> bool {
        read_dword().map(|v| v == 0).unwrap_or(false)
    }
}
