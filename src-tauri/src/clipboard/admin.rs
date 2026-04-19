//! Admin elevation detection + auto-start configuration (spec §8.4).

#[cfg(target_os = "windows")]
pub use windows_impl::*;

#[cfg(not(target_os = "windows"))]
mod stub {
    pub fn is_elevated() -> bool {
        false
    }
    pub fn set_autostart_as_admin(_exe_path: &str, _enable: bool) -> Result<(), String> {
        Err("admin auto-start is Windows-only".into())
    }
    pub fn is_autostart_as_admin_enabled() -> bool {
        false
    }
}
#[cfg(not(target_os = "windows"))]
pub use stub::*;

#[cfg(target_os = "windows")]
mod windows_impl {
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
    use windows::core::PCWSTR;

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

    pub fn set_autostart_as_admin(exe_path: &str, enable: bool) -> Result<(), String> {
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

            if enable {
                // Wrap the exe path in single quotes inside the PowerShell command so
                // paths containing spaces survive the registry string round-trip.
                let cmd = format!(
                    "powershell -WindowStyle Hidden -Command \"Start-Process -FilePath '{}' -Verb RunAs\"",
                    exe_path
                );
                let wide = to_wide(&cmd);
                let bytes = std::slice::from_raw_parts(
                    wide.as_ptr() as *const u8,
                    wide.len() * std::mem::size_of::<u16>(),
                );
                let set_status = RegSetValueExW(
                    key,
                    PCWSTR(value_name.as_ptr()),
                    0,
                    REG_SZ,
                    Some(bytes),
                );
                let _ = RegCloseKey(key);
                if set_status.is_err() {
                    return Err(format!("RegSetValueExW: {:?}", set_status));
                }
            } else {
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
