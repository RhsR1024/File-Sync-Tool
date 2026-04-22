//! Source application icon extraction and cache maintenance.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn icon_file_path(icon_dir: &Path, cache_key: &str) -> PathBuf {
    icon_dir.join(format!("{cache_key}.png"))
}

#[cfg(target_os = "windows")]
pub fn ensure_icon_cached(exe_path: &Path, icon_dir: &Path, cache_key: &str) -> Option<String> {
    let icon_path = icon_file_path(icon_dir, cache_key);
    if icon_path.exists() {
        return Some(icon_path.to_string_lossy().to_string());
    }

    std::fs::create_dir_all(icon_dir).ok()?;
    let png_data = extract_icon_png(exe_path)?;
    std::fs::write(&icon_path, png_data).ok()?;
    Some(icon_path.to_string_lossy().to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_icon_cached(_exe_path: &Path, _icon_dir: &Path, _cache_key: &str) -> Option<String> {
    None
}

pub fn gc_orphan_icons(icon_dir: &Path, referenced_paths: &HashSet<String>) -> Result<u64, String> {
    use rayon::prelude::*;

    if !icon_dir.exists() {
        return Ok(0);
    }

    let files: Vec<PathBuf> = std::fs::read_dir(icon_dir)
        .map_err(|err| format!("read icon dir: {err}"))?
        .filter_map(|entry| entry.ok().map(|value| value.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
        .collect();

    let deleted = files
        .par_iter()
        .filter(|path| !referenced_paths.contains(&path.to_string_lossy().to_string()))
        .map(|path| u64::from(std::fs::remove_file(path).is_ok()))
        .sum();

    Ok(deleted)
}

#[cfg(target_os = "windows")]
fn extract_icon_png(exe_path: &Path) -> Option<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, DrawIconEx, GetIconInfo, DI_NORMAL, ICONINFO,
    };

    unsafe {
        let wide_path: Vec<u16> = exe_path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut file_info = SHFILEINFOW::default();
        let result = SHGetFileInfoW(
            PCWSTR::from_raw(wide_path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || file_info.hIcon.0.is_null() {
            return None;
        }

        let icon = file_info.hIcon;
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(icon, &mut icon_info).is_err() {
            let _ = DestroyIcon(icon);
            return None;
        }

        let width = 32;
        let height = 32;
        let screen_dc = GetDC(None);
        let memory_dc = CreateCompatibleDC(screen_dc);
        let memory_bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        let old_bitmap = SelectObject(memory_dc, memory_bitmap);
        let _ = DrawIconEx(memory_dc, 0, 0, icon, width, height, 0, None, DI_NORMAL);

        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        let lines = GetDIBits(
            memory_dc,
            memory_bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );

        let _ = SelectObject(memory_dc, old_bitmap);
        let _ = DeleteObject(memory_bitmap);
        let _ = DeleteDC(memory_dc);
        let _ = ReleaseDC(None, screen_dc);
        if !icon_info.hbmColor.0.is_null() {
            let _ = DeleteObject(icon_info.hbmColor);
        }
        if !icon_info.hbmMask.0.is_null() {
            let _ = DeleteObject(icon_info.hbmMask);
        }
        let _ = DestroyIcon(icon);

        if lines == 0 {
            return None;
        }

        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        let image = image::RgbaImage::from_raw(width as u32, height as u32, pixels)?;
        let mut buffer = std::io::Cursor::new(Vec::new());
        image.write_to(&mut buffer, image::ImageFormat::Png).ok()?;
        Some(buffer.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn icon_path_uses_cache_key_png_name() {
        let temp_dir = TempDir::new().unwrap();
        let icon_path = icon_file_path(temp_dir.path(), "abc123");
        assert!(icon_path.ends_with(Path::new("abc123.png")));
    }

    #[test]
    fn gc_orphan_icons_removes_unreferenced_pngs() {
        let temp_dir = TempDir::new().unwrap();
        let kept = icon_file_path(temp_dir.path(), "keep");
        let orphan = icon_file_path(temp_dir.path(), "orphan");
        std::fs::write(&kept, b"png").unwrap();
        std::fs::write(&orphan, b"png").unwrap();

        let mut referenced = HashSet::new();
        referenced.insert(kept.to_string_lossy().to_string());

        let deleted = gc_orphan_icons(temp_dir.path(), &referenced).unwrap();
        assert_eq!(deleted, 1);
        assert!(kept.exists());
        assert!(!orphan.exists());
    }
}
