//! Paste simulation via enigo (spec §8.3).
//!
//! 1. Write the target item back into the system clipboard (multi-type: text/html/image/files).
//! 2. Hide the clipboard panel so focus returns to the target window.
//! 3. Sleep ~30ms to let Windows complete the focus switch.
//! 4. Simulate Ctrl+V with enigo.

use std::borrow::Cow;
use std::thread;
use std::time::Duration;

use arboard::{Clipboard, ImageData};
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use tauri::{AppHandle, Manager};

use crate::clipboard::models::{ClipboardItem, ContentKind};

/// Paste an item. If `plain_text` is true, HTML items are written as plain text (no rich
/// formatting) and will paste as plain text in the target app.
pub fn paste_item(
    app: &AppHandle,
    item: &ClipboardItem,
    plain_text: bool,
) -> Result<(), String> {
    write_to_clipboard(item, plain_text)?;

    if let Some(panel) = app.get_webview_window("clipboard-panel") {
        let _ = panel.hide();
    }

    thread::sleep(Duration::from_millis(30));

    simulate_paste()
}

fn write_to_clipboard(item: &ClipboardItem, plain_text: bool) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;

    match item.kind {
        ContentKind::Text => {
            let text = item.content_full.as_deref().unwrap_or(&item.content_preview);
            cb.set_text(text).map_err(|e| format!("set text: {e}"))?;
        }
        ContentKind::Html => {
            let text = item.content_full.as_deref().unwrap_or(&item.content_preview);
            if plain_text {
                cb.set_text(text).map_err(|e| format!("set text: {e}"))?;
            } else {
                let html = item.html.as_deref().unwrap_or(text);
                cb.set_html(html, Some(text))
                    .map_err(|e| format!("set html: {e}"))?;
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
            cb.set_image(data).map_err(|e| format!("set image: {e}"))?;
        }
        ContentKind::File => {
            // arboard's cross-platform API doesn't expose file lists on Windows.
            // Fall back to newline-joined paths as text — useful for terminals and Explorer search.
            let paths = item
                .file_paths
                .as_ref()
                .ok_or_else(|| "file paths missing".to_string())?;
            cb.set_text(paths.join("\n"))
                .map_err(|e| format!("set text (files): {e}"))?;
        }
    }
    Ok(())
}

fn simulate_paste() -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("enigo init: {e}"))?;
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
