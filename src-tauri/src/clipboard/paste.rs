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
use crate::clipboard::ClipboardState;

/// Paste an item. If `plain_text` is true, rich items are written as plain text.
pub fn paste_item(
    app: &AppHandle,
    clipboard: &ClipboardState,
    item: &ClipboardItem,
    plain_text: bool,
) -> Result<(), String> {
    write_to_clipboard(clipboard, item, plain_text)?;
    finish_paste(app)
}

/// Explicit actual-files paste path for file items.
pub fn paste_file_item(
    app: &AppHandle,
    clipboard: &ClipboardState,
    item: &ClipboardItem,
) -> Result<(), String> {
    if item.kind != ContentKind::File {
        return Err("clipboard item is not a file kind".to_string());
    }
    paste_item(app, clipboard, item, false)
}

/// Copy an item into the system clipboard without simulating paste.
pub fn copy_item(clipboard: &ClipboardState, item: &ClipboardItem) -> Result<(), String> {
    write_to_clipboard(clipboard, item, false)
}

/// Paste file items as newline-joined paths.
pub fn paste_file_paths_as_text(
    app: &AppHandle,
    clipboard: &ClipboardState,
    item: &ClipboardItem,
) -> Result<(), String> {
    let text = file_paths_as_text(item)?;
    paste_text(app, clipboard, &text)
}

/// Paste plain text directly.
pub fn paste_text(app: &AppHandle, clipboard: &ClipboardState, text: &str) -> Result<(), String> {
    write_text_to_clipboard_with_marker(clipboard, text)?;
    finish_paste(app)
}

/// Convert a file item into the path text used by "paste as path".
pub fn file_paths_as_text(item: &ClipboardItem) -> Result<String, String> {
    if item.kind != ContentKind::File {
        return Err("clipboard item is not a file kind".to_string());
    }

    let paths = item
        .file_paths
        .as_ref()
        .filter(|paths| !paths.is_empty())
        .ok_or_else(|| "file paths missing".to_string())?;

    Ok(paths_as_newline_text(paths))
}

/// Merge text-like clipboard items into a single plain-text payload.
pub fn merge_items_text(
    items: &[ClipboardItem],
    separator: Option<&str>,
) -> Result<String, String> {
    if items.is_empty() {
        return Err("no clipboard items were provided".to_string());
    }

    let separator = separator.filter(|value| !value.is_empty()).unwrap_or("\n");
    let mut merged = Vec::with_capacity(items.len());
    for item in items {
        match item.kind {
            ContentKind::Text | ContentKind::Html | ContentKind::Rtf => {}
            _ => return Err("all selected clipboard items must be text-like".to_string()),
        }

        let text = item
            .content_full
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "selected clipboard item is missing full text".to_string())?;
        if text.is_empty() {
            return Err("selected clipboard item is empty".to_string());
        }
        merged.push(text);
    }

    Ok(merged.join(separator))
}

/// Copy an image item to a caller-selected path.
pub fn save_image_item_to_path(item: &ClipboardItem, target_path: &str) -> Result<(), String> {
    if item.kind != ContentKind::Image {
        return Err("clipboard item is not an image kind".to_string());
    }

    if target_path.trim().is_empty() {
        return Err("target path missing".to_string());
    }

    let source_path = item
        .image_path
        .as_deref()
        .ok_or_else(|| "image path missing".to_string())?;
    let source = std::path::Path::new(source_path);

    if !source.is_file() {
        return Err(format!("image file does not exist: {}", source.display()));
    }

    std::fs::copy(source, target_path).map_err(|e| format!("copy image: {e}"))?;
    Ok(())
}

fn write_to_clipboard(
    clipboard: &ClipboardState,
    item: &ClipboardItem,
    plain_text: bool,
) -> Result<(), String> {
    mark_pending_write(clipboard, clipboard_write_hash(item, plain_text)?);

    let result = match item.kind {
        ContentKind::Text => {
            write_text_to_clipboard(preferred_text(item))?;
            Ok(())
        }
        ContentKind::Html => {
            let text = preferred_text(item);
            if plain_text {
                write_text_to_clipboard(text)?;
                Ok(())
            } else {
                let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
                let html = item.html.as_deref().unwrap_or(text);
                cb.set_html(html, Some(text))
                    .map_err(|e| format!("set html: {e}"))
            }
        }
        ContentKind::Rtf => {
            let text = preferred_rich_text(item);
            if plain_text {
                write_text_to_clipboard(text)?;
                Ok(())
            } else if let Some(rich_text) =
                item.rtf_content.as_deref().filter(|rtf| !rtf.is_empty())
            {
                write_rtf_to_clipboard(text, rich_text, item.html.as_deref())
            } else {
                write_text_to_clipboard(text)?;
                Ok(())
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
            cb.set_image(data).map_err(|e| format!("set image: {e}"))
        }
        ContentKind::File => {
            if plain_text {
                write_text_to_clipboard(&file_paths_as_text(item)?)?;
                Ok(())
            } else {
                let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
                let file_paths = resolve_file_list_paths(item)?;
                cb.set()
                    .file_list(&file_paths)
                    .map_err(|e| format!("set files: {e}"))
            }
        }
    };

    if result.is_err() {
        clear_pending_write(clipboard);
    }

    result
}

fn write_text_to_clipboard(text: &str) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
    cb.set_text(text).map_err(|e| format!("set text: {e}"))
}

fn write_text_to_clipboard_with_marker(
    clipboard: &ClipboardState,
    text: &str,
) -> Result<(), String> {
    mark_pending_write(
        clipboard,
        crate::clipboard::capture_hash(b"text", text.as_bytes()),
    );

    let result = write_text_to_clipboard(text);
    if result.is_err() {
        clear_pending_write(clipboard);
    }
    result
}

fn clipboard_write_hash(item: &ClipboardItem, plain_text: bool) -> Result<String, String> {
    match item.kind {
        ContentKind::Text => Ok(crate::clipboard::capture_hash(
            b"text",
            preferred_text(item).as_bytes(),
        )),
        ContentKind::Html => {
            let text = preferred_text(item);
            if plain_text {
                Ok(crate::clipboard::capture_hash(b"text", text.as_bytes()))
            } else {
                Ok(crate::clipboard::capture_hash(
                    b"html",
                    item.html.as_deref().unwrap_or(text).as_bytes(),
                ))
            }
        }
        ContentKind::Rtf => {
            let text = preferred_rich_text(item);
            if plain_text {
                Ok(crate::clipboard::capture_hash(b"text", text.as_bytes()))
            } else if let Some(rich_text) =
                item.rtf_content.as_deref().filter(|rtf| !rtf.is_empty())
            {
                Ok(crate::clipboard::capture_hash(
                    b"rtf",
                    &crate::clipboard::source::decode_rtf_storage(rich_text),
                ))
            } else {
                Ok(crate::clipboard::capture_hash(b"text", text.as_bytes()))
            }
        }
        ContentKind::Image => {
            let path = item
                .image_path
                .as_deref()
                .ok_or_else(|| "image path missing".to_string())?;
            let rgba = image::open(path)
                .map_err(|e| format!("open image: {e}"))?
                .to_rgba8()
                .into_raw();
            Ok(crate::clipboard::capture_hash(b"image", rgba.as_slice()))
        }
        ContentKind::File => {
            if plain_text {
                let text = file_paths_as_text(item)?;
                Ok(crate::clipboard::capture_hash(b"text", text.as_bytes()))
            } else {
                let paths = item
                    .file_paths
                    .as_ref()
                    .filter(|paths| !paths.is_empty())
                    .ok_or_else(|| "file paths missing".to_string())?;
                Ok(crate::clipboard::capture_hash(
                    b"files",
                    paths.join("\0").as_bytes(),
                ))
            }
        }
    }
}

fn mark_pending_write(clipboard: &ClipboardState, hash: String) {
    *clipboard.pending_self_write.lock() = Some((hash, std::time::Instant::now()));
}

fn clear_pending_write(clipboard: &ClipboardState) {
    clipboard.pending_self_write.lock().take();
}

fn finish_paste(app: &AppHandle) -> Result<(), String> {
    crate::clipboard::preview::hide_preview_windows(app);
    if let Some(panel) = app.get_webview_window("clipboard-panel") {
        let _ = panel.hide();
    }

    thread::sleep(Duration::from_millis(30));
    simulate_paste()
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
fn write_rtf_to_clipboard(text: &str, rtf: &str, html: Option<&str>) -> Result<(), String> {
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
        let handle =
            GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| format!("alloc clipboard text: {e}"))?;
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

    unsafe fn set_bytes_data(format: u32, mut bytes: Vec<u8>, label: &str) -> Result<(), String> {
        bytes.push(0);
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|e| format!("alloc clipboard {label}: {e}"))?;
        if handle.is_invalid() {
            return Err(format!("alloc clipboard {label}"));
        }

        let ptr = GlobalLock(handle).cast::<u8>();
        if ptr.is_null() {
            return Err(format!("lock clipboard {label}"));
        }

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(handle);

        SetClipboardData(format, HANDLE(handle.0))
            .map_err(|e| format!("set clipboard {label}: {e}"))?;
        Ok(())
    }

    unsafe fn set_rtf_data(rtf: &str) -> Result<(), String> {
        let format = RegisterClipboardFormatW(w!("Rich Text Format"));
        if format == 0 {
            return Err("register RTF format".to_string());
        }
        set_bytes_data(
            format,
            crate::clipboard::source::decode_rtf_storage(rtf),
            "rtf",
        )
    }

    unsafe fn set_html_data(html: &str) -> Result<(), String> {
        let format = RegisterClipboardFormatW(w!("HTML Format"));
        if format == 0 {
            return Err("register HTML format".to_string());
        }
        set_bytes_data(format, build_cf_html(html).into_bytes(), "html")
    }

    open_clipboard_with_retry()?;
    let _guard = ClipboardGuard;

    unsafe {
        EmptyClipboard().map_err(|e| format!("empty clipboard: {e}"))?;
        set_text_data(text)?;
        set_rtf_data(rtf)?;
        if let Some(html) = html.filter(|value| !value.is_empty()) {
            set_html_data(html)?;
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_rtf_to_clipboard(text: &str, _rtf: &str, _html: Option<&str>) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
    cb.set_text(text).map_err(|e| format!("set rtf text: {e}"))
}

fn build_cf_html(fragment: &str) -> String {
    const HEADER_TEMPLATE: &str = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
    const HTML_PREFIX: &str = "<html><body><!--StartFragment-->";
    const HTML_SUFFIX: &str = "<!--EndFragment--></body></html>";

    let start_html = HEADER_TEMPLATE.len();
    let start_fragment = start_html + HTML_PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + HTML_SUFFIX.len();
    format!(
        "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n{HTML_PREFIX}{fragment}{HTML_SUFFIX}"
    )
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
    use tempfile::TempDir;

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
            from_self: false,
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
            vec![
                PathBuf::from("C:\\alpha.txt"),
                PathBuf::from("D:\\beta.png")
            ]
        );
    }

    #[test]
    fn file_paths_as_text_converts_file_items_for_path_paste() {
        let item = sample_item(ContentKind::File);

        let text = file_paths_as_text(&item).unwrap();

        assert_eq!(text, "C:\\alpha.txt\nD:\\beta.png");
    }

    #[test]
    fn clipboard_write_hash_matches_plain_text_payload_for_file_path_paste() {
        let item = sample_item(ContentKind::File);

        assert_eq!(
            clipboard_write_hash(&item, true).unwrap(),
            crate::clipboard::capture_hash(b"text", b"C:\\alpha.txt\nD:\\beta.png")
        );
    }

    #[test]
    fn cf_html_offsets_point_to_utf8_fragment_boundaries() {
        let fragment = "<b>中文</b>";
        let payload = build_cf_html(fragment);
        let read_offset = |label: &str| {
            payload
                .lines()
                .find_map(|line| line.strip_prefix(label))
                .unwrap()
                .parse::<usize>()
                .unwrap()
        };

        let start_fragment = read_offset("StartFragment:");
        let end_fragment = read_offset("EndFragment:");
        assert_eq!(
            &payload.as_bytes()[start_fragment..end_fragment],
            fragment.as_bytes()
        );
    }

    #[test]
    fn file_paths_as_text_rejects_non_file_items_or_missing_paths() {
        let err = file_paths_as_text(&sample_item(ContentKind::Text)).unwrap_err();
        assert!(err.contains("file kind"));

        let mut missing_paths = sample_item(ContentKind::File);
        missing_paths.file_paths = Some(vec![]);

        let err = file_paths_as_text(&missing_paths).unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn merge_items_text_defaults_to_newline_and_honors_custom_separator() {
        let mut first = sample_item(ContentKind::Text);
        first.content_full = Some("alpha".into());
        first.content_preview = "alpha".into();

        let mut second = sample_item(ContentKind::Html);
        second.content_full = Some("beta".into());
        second.content_preview = "beta".into();

        assert_eq!(
            merge_items_text(&[first.clone(), second.clone()], None).unwrap(),
            "alpha\nbeta"
        );
        assert_eq!(
            merge_items_text(&[first, second], Some(", ")).unwrap(),
            "alpha, beta"
        );
    }

    #[test]
    fn merge_items_text_treats_empty_separator_as_default_newline() {
        let mut first = sample_item(ContentKind::Text);
        first.content_full = Some("alpha".into());
        first.content_preview = "alpha".into();

        let mut second = sample_item(ContentKind::Rtf);
        second.content_full = Some("beta".into());
        second.content_preview = "beta".into();

        assert_eq!(
            merge_items_text(&[first.clone(), second.clone()], Some("")).unwrap(),
            "alpha\nbeta"
        );
        assert_eq!(
            merge_items_text(&[first, second], Some(" ")).unwrap(),
            "alpha beta"
        );
    }

    #[test]
    fn merge_items_text_rejects_non_text_like_items() {
        let err = merge_items_text(
            &[
                sample_item(ContentKind::Text),
                sample_item(ContentKind::File),
            ],
            None,
        )
        .unwrap_err();

        assert!(err.contains("text-like"));
    }

    #[test]
    fn merge_items_text_rejects_preview_only_text_like_items() {
        let mut item = sample_item(ContentKind::Text);
        item.content_full = None;
        item.content_preview = "preview only".into();

        let err = merge_items_text(&[item], None).unwrap_err();

        assert!(err.contains("full text"));
    }

    #[test]
    fn save_image_item_to_path_copies_existing_image() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.png");
        let target = temp_dir.path().join("target.png");

        image::RgbaImage::from_pixel(1, 1, image::Rgba([12, 34, 56, 255]))
            .save(&source)
            .unwrap();

        let mut item = sample_item(ContentKind::Image);
        item.image_path = Some(source.to_string_lossy().to_string());
        item.file_paths = None;

        save_image_item_to_path(&item, &target.to_string_lossy()).unwrap();

        assert_eq!(
            std::fs::read(&source).unwrap(),
            std::fs::read(&target).unwrap()
        );
    }

    #[test]
    fn save_image_item_to_path_rejects_non_image_or_missing_source() {
        let err =
            save_image_item_to_path(&sample_item(ContentKind::Text), "target.png").unwrap_err();
        assert!(err.contains("image kind"));

        let mut image = sample_item(ContentKind::Image);
        image.file_paths = None;
        image.image_path = None;

        let err = save_image_item_to_path(&image, "target.png").unwrap_err();
        assert!(err.contains("image path missing"));
    }
}
