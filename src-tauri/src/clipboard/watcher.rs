//! Clipboard listener thread (spec 搂5.1, 搂8.2).
//!
//! Uses `clipboard-master` to listen for Win32 clipboard events, then reads the payload with
//! `arboard` plus small Win32 helpers for RTF/source-app metadata.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;

use arboard::Clipboard;
use clipboard_master::{CallbackResult, ClipboardHandler, Master, Shutdown};
use parking_lot::Mutex;
use regex::Regex;
use tauri::{AppHandle, Emitter};

use crate::clipboard::db::{self, NewItem};
use crate::clipboard::models::ContentKind;
use crate::clipboard::{icon_store, image_store, source, ClipboardState};

const PREVIEW_LIMIT: usize = 200;

pub struct WatcherHandle {
    stop_flag: Arc<AtomicBool>,
    shutdown: Mutex<Option<Shutdown>>,
}

impl WatcherHandle {
    pub fn stop(self) {
        self.stop_flag.store(true, Ordering::Release);
        // Actively wake the Win32 message loop so it breaks immediately instead of waiting for
        // the next clipboard change.
        if let Some(shutdown) = self.shutdown.lock().take() {
            shutdown.signal();
        }
    }
}

struct Handler {
    app: AppHandle,
    state: Arc<ClipboardState>,
    stop_flag: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureSource {
    source_app: Option<String>,
    source_app_icon: Option<String>,
}

#[derive(Debug, Clone)]
struct CapturedImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageSnapshot {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CaptureSnapshot {
    rtf: Option<String>,
    html: Option<String>,
    text: Option<String>,
    files: Option<Vec<String>>,
    image: Option<ImageSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureKind {
    Rtf,
    Html,
    File,
    Image,
    Text,
}

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        if self.stop_flag.load(Ordering::Acquire) {
            return CallbackResult::Stop;
        }
        if !self.state.is_enabled.load(Ordering::Acquire) {
            return CallbackResult::Next;
        }
        if let Err(err) = try_capture(&self.app, &self.state) {
            eprintln!("[clipboard] capture failed: {err}");
        }
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, err: std::io::Error) -> CallbackResult {
        eprintln!("[clipboard] error: {err}");
        CallbackResult::Next
    }
}

fn try_capture(app: &AppHandle, state: &ClipboardState) -> Result<(), String> {
    let source_info = source::get_clipboard_source_app();
    let app_filter = state.settings.read().app_filter.clone();
    if !source::should_capture_source_app(source_info.as_ref(), &app_filter) {
        // Skip before touching heavier clipboard payload APIs when the source app is excluded.
        return Ok(());
    }

    let rtf = source::read_clipboard_rtf().and_then(normalize_optional_text);

    let mut clipboard = Clipboard::new().map_err(|err| format!("clipboard init: {err}"))?;
    let text = clipboard.get_text().ok().and_then(normalize_optional_text);
    let html = clipboard.get().html().ok().and_then(normalize_optional_text);
    let files = clipboard
        .get()
        .file_list()
        .ok()
        .map(|paths| {
            paths.into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|paths| !paths.is_empty());
    let image = clipboard.get_image().ok().map(|image| CapturedImage {
        width: image.width as u32,
        height: image.height as u32,
        rgba: image.bytes.to_vec(),
    });

    let snapshot = CaptureSnapshot {
        rtf: rtf.clone(),
        html: html.clone(),
        text: text.clone(),
        files: files.clone(),
        image: image.as_ref().map(|value| ImageSnapshot {
            width: value.width,
            height: value.height,
        }),
    };
    let Some(kind) = choose_capture_kind(&snapshot) else {
        return Ok(());
    };

    let hash_hex = match kind {
        CaptureKind::Rtf => hex(&compute_hash(b"rtf", rtf.as_deref().unwrap().as_bytes())),
        CaptureKind::Html => hex(&compute_hash(
            b"html",
            html.as_deref().unwrap().as_bytes(),
        )),
        CaptureKind::File => hex(&compute_hash(
            b"files",
            files.as_ref().unwrap().join("\0").as_bytes(),
        )),
        CaptureKind::Image => hex(&compute_hash(
            b"image",
            image.as_ref().unwrap().rgba.as_slice(),
        )),
        CaptureKind::Text => hex(&compute_hash(
            b"text",
            text.as_deref().unwrap().as_bytes(),
        )),
    };

    let source_capture = build_source_capture(state, source_info);
    let item = match kind {
        CaptureKind::Rtf => build_rtf_item(
            rtf.unwrap(),
            text.clone(),
            hash_hex,
            &source_capture,
        ),
        CaptureKind::Html => build_html_item(
            html.unwrap(),
            text.clone(),
            hash_hex,
            &source_capture,
        ),
        CaptureKind::File => build_file_item(files.unwrap(), hash_hex, &source_capture),
        CaptureKind::Image => {
            let image = image.unwrap();
            let saved_path = image_store::save_image_png(
                &state.image_dir,
                &hash_hex,
                image.width,
                image.height,
                &image.rgba,
            )?;
            build_image_item(
                saved_path.to_string_lossy().to_string(),
                image.width,
                image.height,
                image.rgba.len() as i64,
                hash_hex,
                &source_capture,
            )
        }
        CaptureKind::Text => build_text_item(text.unwrap(), hash_hex, &source_capture),
    };

    upsert_item(state, item)?;
    notify_added(app);
    Ok(())
}

fn choose_capture_kind(snapshot: &CaptureSnapshot) -> Option<CaptureKind> {
    if snapshot.rtf.is_some() {
        Some(CaptureKind::Rtf)
    } else if snapshot.html.is_some() {
        Some(CaptureKind::Html)
    } else if snapshot.files.is_some() {
        Some(CaptureKind::File)
    } else if snapshot.image.is_some() {
        Some(CaptureKind::Image)
    } else if snapshot.text.is_some() {
        Some(CaptureKind::Text)
    } else {
        None
    }
}

fn build_source_capture(
    state: &ClipboardState,
    info: Option<source::SourceAppInfo>,
) -> CaptureSource {
    let Some(info) = info else {
        return CaptureSource {
            source_app: None,
            source_app_icon: None,
        };
    };

    let icon_path = icon_store::ensure_icon_cached(&info.exe_path, &state.icon_dir, &info.icon_cache_key);
    CaptureSource {
        source_app: Some(info.app_name),
        source_app_icon: icon_path,
    }
}

fn build_text_item(text: String, hash: String, source: &CaptureSource) -> NewItem {
    NewItem {
        kind: ContentKind::Text,
        content_preview: clip_preview(&text, PREVIEW_LIMIT),
        content_full: Some(text.clone()),
        rtf_content: None,
        html: None,
        image_path: None,
        image_width: None,
        image_height: None,
        file_paths: None,
        byte_size: text.len() as i64,
        hash,
        source_app: source.source_app.clone(),
        source_app_icon: source.source_app_icon.clone(),
    }
}

fn build_html_item(
    html: String,
    plain_text: Option<String>,
    hash: String,
    source: &CaptureSource,
) -> NewItem {
    let fallback_text = plain_text
        .clone()
        .unwrap_or_else(|| strip_html_to_text(&html));
    NewItem {
        kind: ContentKind::Html,
        content_preview: clip_preview(&fallback_text, PREVIEW_LIMIT),
        content_full: Some(fallback_text),
        rtf_content: None,
        html: Some(html.clone()),
        image_path: None,
        image_width: None,
        image_height: None,
        file_paths: None,
        byte_size: html.len() as i64,
        hash,
        source_app: source.source_app.clone(),
        source_app_icon: source.source_app_icon.clone(),
    }
}

fn build_rtf_item(
    rtf: String,
    plain_text: Option<String>,
    hash: String,
    source: &CaptureSource,
) -> NewItem {
    let preview_text = plain_text
        .clone()
        .unwrap_or_else(|| "[RTF Content]".to_string());
    NewItem {
        kind: ContentKind::Rtf,
        content_preview: clip_preview(&preview_text, PREVIEW_LIMIT),
        content_full: plain_text,
        rtf_content: Some(rtf.clone()),
        html: None,
        image_path: None,
        image_width: None,
        image_height: None,
        file_paths: None,
        byte_size: rtf.len() as i64,
        hash,
        source_app: source.source_app.clone(),
        source_app_icon: source.source_app_icon.clone(),
    }
}

fn build_file_item(paths: Vec<String>, hash: String, source: &CaptureSource) -> NewItem {
    let preview = if paths.len() == 1 {
        paths[0].clone()
    } else {
        format!("{} files", paths.len())
    };
    let content_full = paths.join("\n");
    NewItem {
        kind: ContentKind::File,
        content_preview: preview,
        content_full: Some(content_full),
        rtf_content: None,
        html: None,
        image_path: None,
        image_width: None,
        image_height: None,
        file_paths: Some(paths.clone()),
        byte_size: total_file_bytes(&paths),
        hash,
        source_app: source.source_app.clone(),
        source_app_icon: source.source_app_icon.clone(),
    }
}

fn build_image_item(
    image_path: String,
    width: u32,
    height: u32,
    byte_size: i64,
    hash: String,
    source: &CaptureSource,
) -> NewItem {
    NewItem {
        kind: ContentKind::Image,
        content_preview: format!("[Image {width}x{height}]"),
        content_full: None,
        rtf_content: None,
        html: None,
        image_path: Some(image_path),
        image_width: Some(width),
        image_height: Some(height),
        file_paths: None,
        byte_size,
        hash,
        source_app: source.source_app.clone(),
        source_app_icon: source.source_app_icon.clone(),
    }
}

fn total_file_bytes(paths: &[String]) -> i64 {
    paths.iter()
        .filter_map(|path| {
            let path = std::path::Path::new(path);
            if path.is_file() {
                std::fs::metadata(path).ok().map(|meta| meta.len() as i64)
            } else {
                None
            }
        })
        .sum()
}

fn clip_preview(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if let Some((index, _)) = trimmed.char_indices().nth(limit) {
        format!("{}...", &trimmed[..index])
    } else {
        trimmed.to_string()
    }
}

fn normalize_optional_text(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn strip_html_to_text(html: &str) -> String {
    static HTML_TAG_RE: OnceLock<Regex> = OnceLock::new();
    let regex = HTML_TAG_RE.get_or_init(|| Regex::new(r"(?is)<[^>]+>").unwrap());
    let without_tags = regex.replace_all(html, " ");
    without_tags
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compute_hash(prefix: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(prefix);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;

    let mut text = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn upsert_item(state: &ClipboardState, item: NewItem) -> Result<(), String> {
    let settings = state.settings.read().clone();
    let needs_asset_cleanup = {
        let conn = state.write_db.lock();
        let duplicate_asset_candidate = (item.image_path.is_some() || item.source_app_icon.is_some())
            && db::item_exists_by_hash(&conn, &item.hash).map_err(|err| err.to_string())?;
        db::upsert_item_with_dedup(&conn, &item, settings.dedup_strategy.clone())
            .map_err(|err| err.to_string())?;
        let cleanup_stats =
            crate::clipboard::retention::run_cleanup(&conn, &settings).map_err(|err| err.to_string())?;
        duplicate_asset_candidate || cleanup_stats.0 > 0 || cleanup_stats.1 > 0
    };

    if needs_asset_cleanup {
        state.cleanup_orphan_assets();
    }
    Ok(())
}

fn notify_added(app: &AppHandle) {
    let _ = app.emit("clipboard-item-added", ());
}

pub fn start(app: AppHandle, state: Arc<ClipboardState>) -> WatcherHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<Shutdown>(1);
    let app_clone = app;
    let state_clone = state;

    thread::spawn(move || {
        let handler = Handler {
            app: app_clone,
            state: state_clone,
            stop_flag: stop_flag_clone,
        };
        let mut master = match Master::new(handler) {
            Ok(master) => master,
            Err(err) => {
                eprintln!("[clipboard] master init failed: {err}");
                return;
            }
        };
        let _ = shutdown_tx.send(master.shutdown_channel());
        if let Err(err) = master.run() {
            eprintln!("[clipboard] watcher exit: {err}");
        }
    });

    let shutdown = shutdown_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .ok();

    WatcherHandle {
        stop_flag,
        shutdown: Mutex::new(shutdown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_source() -> CaptureSource {
        CaptureSource {
            source_app: Some("Word".to_string()),
            source_app_icon: Some("C:\\icons\\word.png".to_string()),
        }
    }

    #[test]
    fn prioritize_rtf_over_html_files_image_and_text() {
        let snapshot = CaptureSnapshot {
            rtf: Some("{\\rtf1 hello}".to_string()),
            html: Some("<b>hello</b>".to_string()),
            text: Some("hello".to_string()),
            files: Some(vec!["C:\\temp\\a.txt".to_string()]),
            image: Some(ImageSnapshot {
                width: 10,
                height: 10,
            }),
        };

        assert_eq!(choose_capture_kind(&snapshot), Some(CaptureKind::Rtf));
    }

    #[test]
    fn build_html_item_prefers_plain_text_and_strips_markup_when_missing() {
        let source = sample_source();
        let with_text = build_html_item(
            "<div>Hello <b>World</b></div>".to_string(),
            Some("Hello World".to_string()),
            "hash".to_string(),
            &source,
        );
        assert_eq!(with_text.content_preview, "Hello World");
        assert_eq!(with_text.content_full.as_deref(), Some("Hello World"));

        let stripped = build_html_item(
            "<p>Hello&nbsp;<b>World</b></p>".to_string(),
            None,
            "hash".to_string(),
            &source,
        );
        assert_eq!(stripped.content_preview, "Hello World");
        assert_eq!(stripped.content_full.as_deref(), Some("Hello World"));
    }

    #[test]
    fn build_rtf_item_prefers_plain_text_and_keeps_rtf_payload() {
        let source = sample_source();
        let with_text = build_rtf_item(
            "{\\rtf1\\ansi Hello}".to_string(),
            Some("Hello".to_string()),
            "hash".to_string(),
            &source,
        );
        assert_eq!(with_text.kind, ContentKind::Rtf);
        assert_eq!(with_text.content_preview, "Hello");
        assert_eq!(with_text.content_full.as_deref(), Some("Hello"));
        assert_eq!(with_text.rtf_content.as_deref(), Some("{\\rtf1\\ansi Hello}"));
        assert_eq!(with_text.byte_size, "{\\rtf1\\ansi Hello}".len() as i64);

        let fallback = build_rtf_item(
            "{\\rtf1\\ansi}".to_string(),
            None,
            "hash".to_string(),
            &source,
        );
        assert_eq!(fallback.content_preview, "[RTF Content]");
        assert_eq!(fallback.content_full, None);
    }

    #[test]
    fn build_file_item_keeps_paths_and_counts_file_bytes() {
        let temp_dir = TempDir::new().unwrap();
        let first = temp_dir.path().join("a.txt");
        let second = temp_dir.path().join("b.txt");
        std::fs::write(&first, b"abc").unwrap();
        std::fs::write(&second, b"hello").unwrap();

        let paths = vec![
            first.to_string_lossy().to_string(),
            second.to_string_lossy().to_string(),
        ];
        let item = build_file_item(paths.clone(), "hash".to_string(), &sample_source());
        let expected_full = paths.join("\n");

        assert_eq!(item.kind, ContentKind::File);
        assert_eq!(item.file_paths, Some(paths.clone()));
        assert_eq!(item.content_full.as_deref(), Some(expected_full.as_str()));
        assert_eq!(item.byte_size, 8);
    }

    #[test]
    fn normalize_optional_text_preserves_non_empty_whitespace() {
        assert_eq!(
            normalize_optional_text("  keep me \n".to_string()),
            Some("  keep me \n".to_string())
        );
        assert_eq!(normalize_optional_text(" \r\n\t ".to_string()), None);
    }

    #[test]
    fn upsert_item_cleans_replaced_icon_assets() {
        let temp_dir = TempDir::new().unwrap();
        let state = ClipboardState::init(temp_dir.path(), crate::clipboard::models::ClipboardSettings::default())
            .unwrap();

        let old_icon = state.icon_dir.join("old.png");
        let new_icon = state.icon_dir.join("new.png");
        std::fs::write(&old_icon, b"png").unwrap();
        std::fs::write(&new_icon, b"png").unwrap();

        let old_source = CaptureSource {
            source_app: Some("Word".to_string()),
            source_app_icon: Some(old_icon.to_string_lossy().to_string()),
        };
        let new_source = CaptureSource {
            source_app: Some("Excel".to_string()),
            source_app_icon: Some(new_icon.to_string_lossy().to_string()),
        };

        upsert_item(
            &state,
            build_text_item("same text".to_string(), "same-hash".to_string(), &old_source),
        )
        .unwrap();
        upsert_item(
            &state,
            build_text_item("same text".to_string(), "same-hash".to_string(), &new_source),
        )
        .unwrap();

        assert!(!old_icon.exists());
        assert!(new_icon.exists());
    }
}
