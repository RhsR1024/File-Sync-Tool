//! Paste simulation via enigo (spec §8.3).
//!
//! 1. Write the target item back into the system clipboard (multi-type: text/html/image/files).
//! 2. Hide the clipboard panel so focus returns to the target window.
//! 3. Sleep ~30ms to let Windows complete the focus switch.
//! 4. Simulate Ctrl+V with enigo.

use std::borrow::Cow;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use arboard::{Clipboard, ImageData};
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use tauri::{AppHandle, Manager};

use crate::clipboard::models::{ClipboardItem, ContentKind};

/// Paste an item. If `plain_text` is true, rich items are written as plain text.
pub fn paste_item(app: &AppHandle, item: &ClipboardItem, plain_text: bool) -> Result<(), String> {
    write_to_clipboard(item, plain_text)?;

    if let Some(panel) = app.get_webview_window("clipboard-panel") {
        let _ = panel.hide();
    }

    thread::sleep(Duration::from_millis(30));

    simulate_paste()
}

/// Explicit actual-files paste path for file items.
pub fn paste_file_item(app: &AppHandle, item: &ClipboardItem) -> Result<(), String> {
    if item.kind != ContentKind::File {
        return Err("clipboard item is not a file kind".to_string());
    }
    paste_item(app, item, false)
}

/// Copy an item into the system clipboard without simulating paste.
pub fn copy_item(item: &ClipboardItem) -> Result<(), String> {
    write_to_clipboard(item, false)
}

fn write_to_clipboard(item: &ClipboardItem, plain_text: bool) -> Result<(), String> {
    match item.kind {
        ContentKind::Text => {
            let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
            cb.set_text(preferred_text(item))
                .map_err(|e| format!("set text: {e}"))?;
        }
        ContentKind::Html => {
            let text = preferred_text(item);
            let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
            if plain_text {
                cb.set_text(text).map_err(|e| format!("set text: {e}"))?;
            } else {
                let html = item.html.as_deref().unwrap_or(text);
                cb.set_html(html, Some(text))
                    .map_err(|e| format!("set html: {e}"))?;
            }
        }
        ContentKind::Rtf => {
            let text = preferred_rich_text(item);
            let rich_text = item.rtf_content.as_deref().filter(|rtf| !rtf.is_empty());
            if plain_text || rich_text.is_none() {
                let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
                cb.set_text(text)
                    .map_err(|e| format!("set rtf text: {e}"))?;
            } else {
                write_rtf_to_clipboard(text, rich_text.unwrap())?;
            }
        }
        ContentKind::Image => {
            let path = item
                .image_path
                .as_deref()
                .ok_or_else(|| "image path missing".to_string())?;
            let img = image::open(path).map_err(|e| format!("open image: {e}"))?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let bytes = rgba.into_raw();
            let data = ImageData {
                width: w as usize,
                height: h as usize,
                bytes: Cow::Owned(bytes),
            };
            let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
            cb.set_image(data).map_err(|e| format!("set image: {e}"))?;
        }
        ContentKind::File => {
            let paths = item
                .file_paths
                .as_ref()
                .ok_or_else(|| "file paths missing".to_string())?;
            let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
            if plain_text {
                cb.set_text(paths_as_newline_text(paths))
                    .map_err(|e| format!("set text (files): {e}"))?;
            } else {
                let file_paths = resolve_file_list_paths(item)?;
                cb.set()
                    .file_list(&file_paths)
                    .map_err(|e| format!("set files: {e}"))?;
            }
        }
    }

    Ok(())
}

fn preferred_text(item: &ClipboardItem) -> &str {
    item.content_full
        .as_deref()
        .unwrap_or(&item.content_preview)
}

fn preferred_rich_text(item: &ClipboardItem) -> &str {
    item.content_full
        .as_deref()
        .unwrap_or(&item.content_preview)
}

fn paths_as_newline_text(paths: &[String]) -> String {
    paths.join("\n")
}

fn resolve_file_list_paths(item: &ClipboardItem) -> Result<Vec<PathBuf>, String> {
    if item.kind != ContentKind::File {
        return Err("clipboard item is not a file kind".to_string());
    }

    let paths = item
        .file_paths
        .as_ref()
        .filter(|paths| !paths.is_empty())
        .ok_or_else(|| "file paths missing".to_string())?;

    Ok(paths.iter().map(PathBuf::from).collect())
}

#[cfg(target_os = "windows")]
fn write_rtf_to_clipboard(text: &str, rtf: &str) -> Result<(), String> {
    use encoding_rs::WINDOWS_1252;
    use windows::core::w;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    const CF_UNICODETEXT_FORMAT: u32 = 13;

    struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    fn open_clipboard_with_retry() -> Result<(), String> {
        for _ in 0..5 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err("open clipboard: clipboard occupied".to_string())
    }

    unsafe fn set_text_data(text: &str) -> Result<(), String> {
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = utf16.len() * std::mem::size_of::<u16>();
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes)
            .map_err(|e| format!("alloc clipboard text: {e}"))?;
        if handle.is_invalid() {
            return Err("alloc clipboard text".to_string());
        }

        let ptr = GlobalLock(handle).cast::<u16>();
        if ptr.is_null() {
            return Err("lock clipboard text".to_string());
        }

        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
        let _ = GlobalUnlock(handle);

        SetClipboardData(CF_UNICODETEXT_FORMAT, HANDLE(handle.0))
            .map_err(|e| format!("set clipboard text: {e}"))?;

        Ok(())
    }

    unsafe fn set_rtf_data(rtf: &str) -> Result<(), String> {
        let format = RegisterClipboardFormatW(w!("Rich Text Format"));
        if format == 0 {
            return Err("register RTF format".to_string());
        }

        let (encoded, _, _) = WINDOWS_1252.encode(rtf);
        let mut bytes = encoded.into_owned();
        bytes.push(0);

        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|e| format!("alloc clipboard rtf: {e}"))?;
        if handle.is_invalid() {
            return Err("alloc clipboard rtf".to_string());
        }

        let ptr = GlobalLock(handle).cast::<u8>();
        if ptr.is_null() {
            return Err("lock clipboard rtf".to_string());
        }

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(handle);

        SetClipboardData(format, HANDLE(handle.0))
            .map_err(|e| format!("set clipboard rtf: {e}"))?;

        Ok(())
    }

    open_clipboard_with_retry()?;
    let _guard = ClipboardGuard;

    unsafe {
        EmptyClipboard().map_err(|e| format!("empty clipboard: {e}"))?;
        set_text_data(text)?;
        set_rtf_data(rtf)?;
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_rtf_to_clipboard(text: &str, _rtf: &str) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
    cb.set_text(text).map_err(|e| format!("set rtf text: {e}"))
}

fn simulate_paste() -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("enigo init: {e}"))?;
    enigo
        .key(Key::Control, Press)
        .map_err(|e| format!("ctrl press: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Click)
        .map_err(|e| format!("v click: {e}"))?;
    enigo
        .key(Key::Control, Release)
        .map_err(|e| format!("ctrl release: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item(kind: ContentKind) -> ClipboardItem {
        ClipboardItem {
            id: 1,
            kind,
            content_preview: "preview".into(),
            content_full: Some("full text".into()),
            rtf_content: Some("{\\rtf1\\ansi full text}".into()),
            html: Some("<b>full text</b>".into()),
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: Some(vec!["C:\\alpha.txt".into(), "D:\\beta.png".into()]),
            byte_size: 12,
            char_count: 9,
            hash: "hash-1".into(),
            source_app: Some("Explorer".into()),
            source_app_icon: None,
            group_id: None,
            is_favorite: false,
            is_pinned: false,
            favorite_sort_index: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn paths_as_newline_text_joins_paths_for_text_fallback() {
        assert_eq!(
            paths_as_newline_text(&["C:\\alpha.txt".into(), "D:\\beta.png".into()]),
            "C:\\alpha.txt\nD:\\beta.png"
        );
    }

    #[test]
    fn resolve_file_list_paths_preserves_each_file_path() {
        let item = sample_item(ContentKind::File);

        let paths = resolve_file_list_paths(&item).unwrap();

        assert_eq!(
            paths,
            vec![PathBuf::from("C:\\alpha.txt"), PathBuf::from("D:\\beta.png")]
        );
    }
}
