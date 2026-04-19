//! Foreground-window process name capture for clipboard source attribution.
//!
//! Called from the watcher right after a clipboard event fires; at that point
//! the Win32 foreground window is still the app that performed the copy.

#[cfg(target_os = "windows")]
pub fn foreground_process_name() -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let handle: HANDLE = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return None,
        };
        if handle.0.is_null() {
            return None;
        }

        let mut buf: Vec<u16> = vec![0; 1024];
        let mut size: u32 = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        if ok.is_err() || size == 0 {
            return None;
        }
        buf.truncate(size as usize);
        let full = std::ffi::OsString::from_wide(&buf).to_string_lossy().into_owned();
        // Extract the executable file stem (e.g., "C:\\...\\chrome.exe" -> "chrome").
        let path = std::path::Path::new(&full);
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_process_name() -> Option<String> {
    None
}
