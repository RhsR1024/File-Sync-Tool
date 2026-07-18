//! Clipboard source-app discovery and RTF clipboard helpers.

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

use crate::clipboard::models::{ClipboardAppFilterMode, ClipboardAppFilterSettings};

const RTF_BASE64_PREFIX: &str = "base64:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAppInfo {
    pub app_name: String,
    pub exe_path: PathBuf,
    pub icon_cache_key: String,
}

#[cfg(target_os = "windows")]
pub fn clipboard_sequence_number() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}

#[cfg(not(target_os = "windows"))]
pub fn clipboard_sequence_number() -> u32 {
    0
}

pub fn compute_icon_cache_key(exe_path: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(exe_path.to_string_lossy().to_lowercase().as_bytes());
    hasher.finalize().to_hex()[..12].to_string()
}

pub fn resolve_display_name(exe_path: &Path, file_description: Option<&str>) -> String {
    file_description
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            exe_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().flat_map(|ch| ch.to_lowercase()).collect();
    let candidate: Vec<char> = candidate.chars().flat_map(|ch| ch.to_lowercase()).collect();

    let mut pattern_index = 0usize;
    let mut candidate_index = 0usize;
    let mut last_star = None;
    let mut last_star_match = 0usize;

    while candidate_index < candidate.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?'
                || pattern[pattern_index] == candidate[candidate_index])
        {
            pattern_index += 1;
            candidate_index += 1;
            continue;
        }

        if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            last_star = Some(pattern_index);
            pattern_index += 1;
            last_star_match = candidate_index;
            continue;
        }

        if let Some(star_index) = last_star {
            pattern_index = star_index + 1;
            last_star_match += 1;
            candidate_index = last_star_match;
            continue;
        }

        return false;
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

pub fn matches_app_pattern(info: &SourceAppInfo, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }

    wildcard_match(pattern, &info.app_name)
        || info
            .exe_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| wildcard_match(pattern, name))
        || info
            .exe_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| wildcard_match(pattern, stem))
        || wildcard_match(pattern, &info.exe_path.to_string_lossy())
}

pub fn should_capture_source_app(
    info: Option<&SourceAppInfo>,
    settings: &ClipboardAppFilterSettings,
) -> bool {
    if !settings.enabled {
        return true;
    }

    let mut patterns = settings
        .patterns
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .peekable();
    if patterns.peek().is_none() {
        return true;
    }

    let matched =
        info.is_some_and(|info| patterns.any(|pattern| matches_app_pattern(info, pattern)));
    match settings.mode {
        ClipboardAppFilterMode::Blacklist => !matched,
        ClipboardAppFilterMode::Whitelist => matched,
    }
}

fn decode_clipboard_text_bytes(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let payload = &bytes[..end];
    if payload.is_empty() {
        return None;
    }

    match std::str::from_utf8(payload) {
        Ok(text) => Some(text.to_string()),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::WINDOWS_1252.decode(payload);
            Some(decoded.into_owned())
        }
    }
}

pub(crate) fn encode_rtf_storage(bytes: &[u8]) -> String {
    format!("{RTF_BASE64_PREFIX}{}", BASE64_STANDARD.encode(bytes))
}

pub(crate) fn decode_rtf_storage(value: &str) -> Vec<u8> {
    if let Some(encoded) = value.strip_prefix(RTF_BASE64_PREFIX) {
        if let Ok(bytes) = BASE64_STANDARD.decode(encoded) {
            return bytes;
        }
    }

    let (encoded, _, _) = encoding_rs::WINDOWS_1252.encode(value);
    encoded.into_owned()
}

pub(crate) fn rtf_storage_byte_len(value: &str) -> usize {
    decode_rtf_storage(value).len()
}

fn is_application_frame_host_exe(exe_path: &Path) -> bool {
    exe_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("ApplicationFrameHost.exe"))
        .unwrap_or(false)
}

fn resolve_uwp_child_exe_path(
    exe_path: &Path,
    child_exe_paths: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    if !is_application_frame_host_exe(exe_path) {
        return None;
    }

    child_exe_paths
        .into_iter()
        .find(|path| !is_application_frame_host_exe(path))
}

#[cfg(target_os = "windows")]
pub fn get_clipboard_source_app() -> Option<SourceAppInfo> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::DataExchange::GetClipboardOwner;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let self_pid = std::process::id();

    unsafe fn try_resolve(hwnd: HWND, self_pid: u32) -> Option<SourceAppInfo> {
        if hwnd.0.is_null() {
            return None;
        }

        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == 0 || pid == self_pid {
            return None;
        }

        let exe_path = unsafe { get_exe_path_from_pid(pid) }?;
        let exe_path = unsafe { resolve_uwp_app(hwnd, &exe_path) }.unwrap_or(exe_path);
        let app_name = get_app_display_name(&exe_path);
        let icon_cache_key = compute_icon_cache_key(&exe_path);

        Some(SourceAppInfo {
            app_name,
            exe_path,
            icon_cache_key,
        })
    }

    unsafe {
        if let Ok(owner) = GetClipboardOwner() {
            if let Some(info) = try_resolve(owner, self_pid) {
                return Some(info);
            }
        }

        let foreground = GetForegroundWindow();
        try_resolve(foreground, self_pid)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_clipboard_source_app() -> Option<SourceAppInfo> {
    None
}

#[cfg(target_os = "windows")]
unsafe fn resolve_uwp_app(
    owner_hwnd: windows::Win32::Foundation::HWND,
    exe_path: &Path,
) -> Option<PathBuf> {
    if !is_application_frame_host_exe(exe_path) {
        return None;
    }

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetWindowThreadProcessId};

    struct CallbackData {
        host_pid: u32,
        child_exe_paths: Vec<PathBuf>,
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = unsafe { &mut *(lparam.0 as *mut CallbackData) };
        let mut child_pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut child_pid)) };
        if child_pid != 0 && child_pid != data.host_pid {
            if let Some(path) = unsafe { get_exe_path_from_pid(child_pid) } {
                if !is_application_frame_host_exe(&path) {
                    data.child_exe_paths.push(path);
                }
            }
        }
        BOOL::from(true)
    }

    let mut host_pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(owner_hwnd, Some(&mut host_pid)) };
    if host_pid == 0 {
        return None;
    }

    let mut data = CallbackData {
        host_pid,
        child_exe_paths: Vec::new(),
    };
    let _ = unsafe {
        EnumChildWindows(
            owner_hwnd,
            Some(enum_callback),
            LPARAM(&mut data as *mut _ as isize),
        )
    };

    resolve_uwp_child_exe_path(exe_path, data.child_exe_paths)
}

#[cfg(target_os = "windows")]
pub fn read_clipboard_rtf() -> Option<String> {
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    unsafe {
        let format = RegisterClipboardFormatW(w!("Rich Text Format"));
        if format == 0 {
            return None;
        }

        OpenClipboard(None).ok()?;
        let _guard = ClipboardGuard;

        let handle = GetClipboardData(format).ok()?;
        let memory = HGLOBAL(handle.0);
        let size = GlobalSize(memory);
        if size == 0 {
            return None;
        }

        let ptr = GlobalLock(memory);
        if ptr.is_null() {
            return None;
        }

        let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size);
        let payload_end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        let payload = &bytes[..payload_end];
        let stored = (!payload.is_empty()).then(|| encode_rtf_storage(payload));
        let _ = GlobalUnlock(memory);
        stored
    }
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_rtf() -> Option<String> {
    None
}

/// Raw clipboard image read result (width, height, RGBA8 bytes).
pub type RawClipboardImage = (u32, u32, Vec<u8>);

/// Read clipboard image data directly via Win32 API when `arboard` fails.
///
/// Some screenshot tools (PixPin, Snipaste, ShareX...) write image data in ways that
/// confuse `arboard`'s `get_image()` — e.g. only the `"PNG"` private format, a DIBv5
/// with non-standard masks, or a top-down DIB. This fallback tries the common formats
/// in priority order: PNG → CF_DIBV5 → CF_DIB. Returns `None` if no image is present
/// or decoding fails.
#[cfg(target_os = "windows")]
pub fn read_clipboard_image_raw() -> Option<RawClipboardImage> {
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    unsafe fn read_format_bytes(format: u32) -> Option<Vec<u8>> {
        unsafe {
            if IsClipboardFormatAvailable(format).is_err() {
                return None;
            }
            let handle = GetClipboardData(format).ok()?;
            let memory = HGLOBAL(handle.0);
            let size = GlobalSize(memory);
            if size == 0 {
                return None;
            }
            let ptr = GlobalLock(memory);
            if ptr.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
            let _ = GlobalUnlock(memory);
            Some(bytes)
        }
    }

    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let _guard = ClipboardGuard;

        let png_format = RegisterClipboardFormatW(w!("PNG"));
        if png_format != 0 {
            if let Some(bytes) = read_format_bytes(png_format) {
                if let Some(image) = decode_png_to_rgba(&bytes) {
                    return Some(image);
                }
            }
        }

        for format in [CF_DIBV5, CF_DIB] {
            if let Some(dib) = read_format_bytes(format) {
                if let Some(image) = decode_dib_to_rgba(&dib) {
                    return Some(image);
                }
            }
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
pub fn read_clipboard_image_raw() -> Option<RawClipboardImage> {
    None
}

#[cfg(target_os = "windows")]
fn decode_png_to_rgba(bytes: &[u8]) -> Option<RawClipboardImage> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// Decode a Windows Device-Independent Bitmap (starts with `BITMAPINFOHEADER` /
/// `BITMAPV5HEADER`, no `BITMAPFILEHEADER`) into RGBA8 by synthesising a
/// BMP file-header and feeding it to the `image` crate.
#[cfg(target_os = "windows")]
fn decode_dib_to_rgba(dib: &[u8]) -> Option<RawClipboardImage> {
    if dib.len() < 4 {
        return None;
    }
    let header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if header_size < 12 || header_size > dib.len() {
        return None;
    }

    // Compute where the pixel data starts. BI_BITFIELDS/BI_ALPHABITFIELDS add 3/4 DWORD masks
    // after the info header, colour tables add bits_per_pixel-dependent entries.
    let bits_per_pixel = if header_size >= 16 {
        u16::from_le_bytes([dib[14], dib[15]]) as u32
    } else {
        0
    };
    let compression = if header_size >= 20 {
        u32::from_le_bytes([dib[16], dib[17], dib[18], dib[19]])
    } else {
        0
    };

    const BI_BITFIELDS: u32 = 3;
    const BI_ALPHABITFIELDS: u32 = 6;

    let mask_bytes = match compression {
        BI_BITFIELDS if header_size == 40 => 12,
        BI_ALPHABITFIELDS if header_size == 40 => 16,
        _ => 0,
    };

    let colour_table_bytes = if bits_per_pixel <= 8 {
        let colours_used = if header_size >= 36 {
            u32::from_le_bytes([dib[32], dib[33], dib[34], dib[35]])
        } else {
            0
        };
        let colours = if colours_used == 0 {
            1u32 << bits_per_pixel
        } else {
            colours_used
        };
        (colours as usize) * 4
    } else {
        0
    };

    let pixel_offset = 14 + header_size + mask_bytes + colour_table_bytes;
    let file_size = 14 + dib.len();

    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(file_size as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&(pixel_offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);

    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?;
    let rgba = img.to_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

#[cfg(target_os = "windows")]
fn get_app_display_name(exe_path: &Path) -> String {
    resolve_display_name(exe_path, get_file_description(exe_path).as_deref())
}

#[cfg(target_os = "windows")]
unsafe fn get_exe_path_from_pid(pid: u32) -> Option<PathBuf> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR::from_raw(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    result.ok()?;

    if size == 0 {
        return None;
    }

    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

#[cfg(target_os = "windows")]
fn get_file_description(exe_path: &Path) -> Option<String> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    unsafe fn query_string(buffer: &[u8], sub_path: &str) -> Option<String> {
        let wide: Vec<u16> = sub_path.encode_utf16().collect();
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        if !unsafe {
            VerQueryValueW(
                buffer.as_ptr().cast::<c_void>(),
                PCWSTR::from_raw(wide.as_ptr()),
                &mut ptr,
                &mut len,
            )
        }
        .as_bool()
            || ptr.is_null()
            || len == 0
        {
            return None;
        }

        let slice = unsafe { std::slice::from_raw_parts(ptr.cast::<u16>(), len as usize) };
        let end = slice.iter().position(|ch| *ch == 0).unwrap_or(slice.len());
        let value = String::from_utf16_lossy(&slice[..end]).trim().to_string();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }

    unsafe {
        let wide_path: Vec<u16> = exe_path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let pcwstr = PCWSTR::from_raw(wide_path.as_ptr());
        let size = GetFileVersionInfoSizeW(pcwstr, None);
        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        GetFileVersionInfoW(pcwstr, 0, size, buffer.as_mut_ptr().cast::<c_void>()).ok()?;

        let mut translation_ptr: *mut c_void = std::ptr::null_mut();
        let mut translation_len: u32 = 0;
        let translation_path: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
        if VerQueryValueW(
            buffer.as_ptr().cast::<c_void>(),
            PCWSTR::from_raw(translation_path.as_ptr()),
            &mut translation_ptr,
            &mut translation_len,
        )
        .as_bool()
            && !translation_ptr.is_null()
            && translation_len >= 4
        {
            let lang = *(translation_ptr.cast::<u16>());
            let code_page = *(translation_ptr.cast::<u16>().add(1));
            let description_path = format!(
                "\\StringFileInfo\\{:04x}{:04x}\\FileDescription\0",
                lang, code_page
            );
            if let Some(description) = query_string(&buffer, &description_path) {
                return Some(description);
            }
        }

        query_string(&buffer, "\\StringFileInfo\\040904B0\\FileDescription\0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::models::{ClipboardAppFilterMode, ClipboardAppFilterSettings};

    fn sample_info(app_name: &str, exe_path: &str) -> SourceAppInfo {
        SourceAppInfo {
            app_name: app_name.to_string(),
            exe_path: PathBuf::from(exe_path),
            icon_cache_key: "icon-key".to_string(),
        }
    }

    #[test]
    fn icon_cache_key_is_case_insensitive_and_short() {
        let upper = Path::new("C:\\Program Files\\FancyApp\\FANCY.EXE");
        let lower = Path::new("c:\\program files\\fancyapp\\fancy.exe");

        let upper_hash = compute_icon_cache_key(upper);
        let lower_hash = compute_icon_cache_key(lower);

        assert_eq!(upper_hash, lower_hash);
        assert_eq!(upper_hash.len(), 12);
    }

    #[test]
    fn resolve_display_name_prefers_description_then_file_stem() {
        let exe_path = Path::new("C:\\Program Files\\FancyApp\\fancy.exe");

        assert_eq!(
            resolve_display_name(exe_path, Some("Fancy Application")),
            "Fancy Application"
        );
        assert_eq!(resolve_display_name(exe_path, Some("   ")), "fancy");
        assert_eq!(resolve_display_name(exe_path, None), "fancy");
        assert_eq!(resolve_display_name(Path::new(""), None), "Unknown");
    }

    #[test]
    fn resolve_uwp_child_exe_path_prefers_real_child_process() {
        let host = Path::new("C:\\Windows\\System32\\ApplicationFrameHost.exe");
        let child = PathBuf::from("C:\\Program Files\\WindowsApps\\Contoso.App\\App.exe");

        let resolved = resolve_uwp_child_exe_path(
            host,
            vec![
                PathBuf::from("C:\\Windows\\System32\\ApplicationFrameHost.exe"),
                child.clone(),
            ],
        );

        assert_eq!(resolved, Some(child));
    }

    #[test]
    fn resolve_uwp_child_exe_path_ignores_non_host_parents() {
        let parent = Path::new("C:\\Program Files\\Contoso\\app.exe");
        let child = PathBuf::from("C:\\Program Files\\WindowsApps\\Contoso.App\\App.exe");

        let resolved = resolve_uwp_child_exe_path(parent, vec![child]);
        assert_eq!(resolved, None);
    }

    #[test]
    fn decode_clipboard_text_bytes_preserves_non_utf8_rtf_payload() {
        let decoded = decode_clipboard_text_bytes(b"{\\rtf1\\ansi caf\xe9}\0");
        assert_eq!(decoded.as_deref(), Some("{\\rtf1\\ansi caf\u{00e9}}"));
    }

    #[test]
    fn rtf_storage_roundtrip_preserves_original_bytes() {
        let original = b"{\\rtf1\\ansi caf\xe9}";
        let stored = encode_rtf_storage(original);

        assert!(stored.starts_with(RTF_BASE64_PREFIX));
        assert_eq!(decode_rtf_storage(&stored), original);
        assert_eq!(rtf_storage_byte_len(&stored), original.len());
    }

    #[test]
    fn rtf_storage_keeps_legacy_plain_text_compatible() {
        assert_eq!(
            decode_rtf_storage("{\\rtf1\\ansi caf\u{00e9}}"),
            b"{\\rtf1\\ansi caf\xe9}"
        );
    }

    #[test]
    fn matches_app_pattern_checks_display_name_and_executable_candidates() {
        let info = sample_info(
            "Visual Studio Code",
            "C:\\Users\\Admin\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe",
        );

        assert!(matches_app_pattern(&info, "Visual Studio Code"));
        assert!(matches_app_pattern(&info, "visual*code"));
        assert!(matches_app_pattern(&info, "Code.exe"));
        assert!(matches_app_pattern(&info, "code"));
        assert!(matches_app_pattern(&info, "*vs code\\Code.exe"));
        assert!(!matches_app_pattern(&info, "SnippingTool.exe"));
    }

    #[test]
    fn should_capture_source_app_applies_blacklist_and_whitelist_rules() {
        let chrome = sample_info(
            "Google Chrome",
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        );
        let code = sample_info(
            "Visual Studio Code",
            "C:\\Users\\Admin\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe",
        );
        let blacklist = ClipboardAppFilterSettings {
            enabled: true,
            mode: ClipboardAppFilterMode::Blacklist,
            patterns: vec!["*chrome*".to_string(), "SnippingTool.exe".to_string()],
        };
        let whitelist = ClipboardAppFilterSettings {
            enabled: true,
            mode: ClipboardAppFilterMode::Whitelist,
            patterns: vec!["Code.exe".to_string(), "Windows Terminal".to_string()],
        };

        assert!(!should_capture_source_app(Some(&chrome), &blacklist));
        assert!(should_capture_source_app(Some(&code), &blacklist));
        assert!(should_capture_source_app(Some(&code), &whitelist));
        assert!(!should_capture_source_app(Some(&chrome), &whitelist));
        assert!(should_capture_source_app(None, &blacklist));
        assert!(!should_capture_source_app(None, &whitelist));
    }

    #[test]
    fn should_capture_source_app_ignores_empty_patterns() {
        let info = sample_info("Notepad", "C:\\Windows\\System32\\notepad.exe");
        let settings = ClipboardAppFilterSettings {
            enabled: true,
            mode: ClipboardAppFilterMode::Whitelist,
            patterns: vec!["   ".to_string()],
        };

        assert!(should_capture_source_app(Some(&info), &settings));
        assert!(should_capture_source_app(None, &settings));
    }
}
