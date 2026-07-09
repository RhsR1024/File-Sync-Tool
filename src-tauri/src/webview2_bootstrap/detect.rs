//! Registry-based WebView2 Runtime detection. Microsoft distribution docs
//! recommend checking the Evergreen Runtime client `pv` value.

const CLIENT_GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

pub fn version_indicates_present(pv: Option<&str>) -> bool {
    let Some(pv) = pv else {
        return false;
    };
    let trimmed = pv.trim();
    if trimmed.is_empty() {
        return false;
    }

    let mut any_nonzero = false;
    for part in trimmed.split('.') {
        match part.parse::<u64>() {
            Ok(value) => any_nonzero |= value != 0,
            Err(_) => return false,
        }
    }
    any_nonzero
}

#[cfg(target_os = "windows")]
pub fn detect_webview2_runtime() -> Option<String> {
    let hklm_subkey = format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT_GUID}");
    let hkcu_subkey = format!(r"Software\Microsoft\EdgeUpdate\Clients\{CLIENT_GUID}");

    [
        read_pv(
            windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            &hklm_subkey,
        ),
        read_pv(
            windows::Win32::System::Registry::HKEY_CURRENT_USER,
            &hkcu_subkey,
        ),
    ]
    .into_iter()
    .flatten()
    .find(|pv| version_indicates_present(Some(pv)))
}

#[cfg(not(target_os = "windows"))]
pub fn detect_webview2_runtime() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
fn read_pv(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let value_w: Vec<u16> = "pv".encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = [0u16; 64];
    let mut size = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR(value_w.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
    };
    if result != ERROR_SUCCESS {
        return None;
    }

    let chars = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..chars.min(buffer.len())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_empty_versions_are_absent() {
        assert!(!version_indicates_present(None));
        assert!(!version_indicates_present(Some("")));
        assert!(!version_indicates_present(Some("   ")));
    }

    #[test]
    fn zero_version_is_absent() {
        assert!(!version_indicates_present(Some("0.0.0.0")));
    }

    #[test]
    fn real_version_is_present() {
        assert!(version_indicates_present(Some("109.0.1518.78")));
    }

    #[test]
    fn garbage_version_is_absent() {
        assert!(!version_indicates_present(Some("abc")));
        assert!(!version_indicates_present(Some("1.2.x.4")));
    }
}
