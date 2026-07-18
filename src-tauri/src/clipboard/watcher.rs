//! Clipboard listener thread (spec 搂5.1, 搂8.2).
//!
//! Uses `clipboard-master` to listen for Win32 clipboard events, then reads the payload with
//! `arboard` plus small Win32 helpers for RTF/source-app metadata.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use clipboard_master::{CallbackResult, ClipboardHandler, Master, Shutdown};
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::clipboard::db::{self, NewItem};
use crate::clipboard::models::{ClipboardItem, ContentKind};
use crate::clipboard::{icon_store, image_store, source, ClipboardState};

const PREVIEW_LIMIT: usize = 200;
const SELF_WRITE_WINDOW_MS: u64 = 500;
const CAPTURE_DEBOUNCE_MS: u64 = 30;
const WATCHER_MAX_BACKOFF_MS: u64 = 5_000;
const CAPTURE_RETRY_DELAYS_MS: [u64; 5] = [0, 20, 50, 100, 200];

pub struct WatcherHandle {
    stop_flag: Arc<AtomicBool>,
    shutdown: Arc<Mutex<Option<Shutdown>>>,
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
    state: Arc<ClipboardState>,
    stop_flag: Arc<AtomicBool>,
    work_tx: mpsc::Sender<CaptureSignal>,
}

#[derive(Debug, Clone)]
struct CaptureSignal {
    source_info: Option<source::SourceAppInfo>,
    sequence: u32,
}

#[derive(Debug, Clone)]
struct CaptureWorkItem {
    source_info: Option<source::SourceAppInfo>,
    rtf: Option<String>,
    html: Option<String>,
    text: Option<String>,
    files: Option<Vec<String>>,
    image: Option<CapturedImage>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ClipboardItemAddedEvent {
    id: i64,
    is_new: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelfWriteDecision {
    None,
    Skip,
    CaptureAsSelf,
}

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        if self.stop_flag.load(Ordering::Acquire) {
            return CallbackResult::Stop;
        }
        if !self.state.is_enabled.load(Ordering::Acquire) {
            return CallbackResult::Next;
        }
        let source_info = source::get_clipboard_source_app();
        let has_pending_self_write = self.state.pending_self_write.lock().is_some();
        if !has_pending_self_write {
            let app_filter = self.state.settings.read().app_filter.clone();
            if !source::should_capture_source_app(source_info.as_ref(), &app_filter) {
                return CallbackResult::Next;
            }
        }

        if self
            .work_tx
            .send(CaptureSignal {
                source_info,
                sequence: source::clipboard_sequence_number(),
            })
            .is_err()
        {
            return CallbackResult::Stop;
        }
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, err: std::io::Error) -> CallbackResult {
        eprintln!("[clipboard] error: {err}");
        CallbackResult::Next
    }
}

fn read_capture_work_item(
    mut source_info: Option<source::SourceAppInfo>,
    expected_sequence: u32,
) -> Result<Option<CaptureWorkItem>, String> {
    let mut last_error = None;

    for (attempt, delay_ms) in CAPTURE_RETRY_DELAYS_MS.iter().copied().enumerate() {
        if delay_ms > 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }

        let sequence_before = source::clipboard_sequence_number();
        if expected_sequence != 0 && sequence_before != expected_sequence {
            source_info = source::get_clipboard_source_app();
        }

        match read_capture_work_item_once(source_info.clone()) {
            Ok(item) => {
                let sequence_after = source::clipboard_sequence_number();
                if sequence_before == 0 || sequence_before == sequence_after {
                    if item.is_none() && attempt + 1 < CAPTURE_RETRY_DELAYS_MS.len() {
                        last_error = Some(format!(
                            "clipboard formats were temporarily unavailable (attempt {}/{})",
                            attempt + 1,
                            CAPTURE_RETRY_DELAYS_MS.len()
                        ));
                        continue;
                    }
                    return Ok(item);
                }
                last_error = Some(format!(
                    "clipboard changed during read (attempt {}/{})",
                    attempt + 1,
                    CAPTURE_RETRY_DELAYS_MS.len()
                ));
            }
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| "clipboard read failed".to_string()))
}

fn read_capture_work_item_once(
    source_info: Option<source::SourceAppInfo>,
) -> Result<Option<CaptureWorkItem>, String> {
    let rtf = source::read_clipboard_rtf().and_then(normalize_optional_text);

    let mut clipboard = Clipboard::new().map_err(|err| format!("clipboard init: {err}"))?;
    let text = clipboard.get_text().ok().and_then(normalize_optional_text);
    let html = clipboard
        .get()
        .html()
        .ok()
        .and_then(normalize_optional_text);
    let files = clipboard
        .get()
        .file_list()
        .ok()
        .map(|paths| {
            paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|paths| !paths.is_empty());
    let image = clipboard
        .get_image()
        .ok()
        .map(|image| CapturedImage {
            width: image.width as u32,
            height: image.height as u32,
            rgba: image.bytes.to_vec(),
        })
        .or_else(|| {
            // arboard's Windows reader can miss images from tools like PixPin / Snipaste that
            // only set private `"PNG"` data or an unusual CF_DIBV5 layout. Fall back to a raw
            // Win32 reader that also handles those formats.
            source::read_clipboard_image_raw().map(|(width, height, rgba)| CapturedImage {
                width,
                height,
                rgba,
            })
        });

    if rtf.is_none() && text.is_none() && html.is_none() && files.is_none() && image.is_none() {
        return Ok(None);
    }

    Ok(Some(CaptureWorkItem {
        source_info,
        rtf,
        html,
        text,
        files,
        image,
    }))
}

fn process_capture(
    app: &AppHandle,
    state: &ClipboardState,
    work: CaptureWorkItem,
) -> Result<(), String> {
    let CaptureWorkItem {
        source_info,
        rtf,
        html,
        text,
        files,
        image,
    } = work;

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

    let max_item_bytes = state.settings.read().max_item_bytes;
    if !capture_kind_fits_limit(kind, &rtf, &html, &text, image.as_ref(), max_item_bytes) {
        eprintln!(
            "[clipboard] capture skipped: payload exceeds configured max item size ({max_item_bytes} bytes)"
        );
        return Ok(());
    }

    let hash_hex = match kind {
        CaptureKind::Rtf => {
            let bytes = source::decode_rtf_storage(rtf.as_deref().unwrap());
            crate::clipboard::capture_hash(b"rtf", &bytes)
        }
        CaptureKind::Html => {
            crate::clipboard::capture_hash(b"html", html.as_deref().unwrap().as_bytes())
        }
        CaptureKind::File => {
            crate::clipboard::capture_hash(b"files", files.as_ref().unwrap().join("\0").as_bytes())
        }
        CaptureKind::Image => {
            crate::clipboard::capture_hash(b"image", image.as_ref().unwrap().rgba.as_slice())
        }
        CaptureKind::Text => {
            crate::clipboard::capture_hash(b"text", text.as_deref().unwrap().as_bytes())
        }
    };

    let self_write_decision = {
        let pending = state.pending_self_write.lock().take();
        let settings = state.settings.read();
        resolve_self_write_match(
            pending,
            &hash_hex,
            settings.reinsert_on_self_copy,
            std::time::Instant::now(),
        )
    };

    if matches!(self_write_decision, SelfWriteDecision::Skip) {
        return Ok(());
    }

    if matches!(self_write_decision, SelfWriteDecision::None) {
        let app_filter = state.settings.read().app_filter.clone();
        if !source::should_capture_source_app(source_info.as_ref(), &app_filter) {
            return Ok(());
        }
    }

    let source_capture = if matches!(self_write_decision, SelfWriteDecision::CaptureAsSelf) {
        CaptureSource {
            source_app: None,
            source_app_icon: None,
        }
    } else {
        build_source_capture(state, source_info)
    };
    let mut item = match kind {
        CaptureKind::Rtf => build_rtf_item(
            rtf.unwrap(),
            html.clone(),
            text.clone(),
            hash_hex,
            &source_capture,
        ),
        CaptureKind::Html => {
            build_html_item(html.unwrap(), text.clone(), hash_hex, &source_capture)
        }
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
    if matches!(self_write_decision, SelfWriteDecision::CaptureAsSelf) {
        item.from_self = true;
    }

    let group_id = *state.active_group_id.lock();
    let (stored, is_new) = upsert_item(state, item, group_id)?;
    notify_added(app, stored.id, is_new);
    Ok(())
}

fn capture_kind_fits_limit(
    kind: CaptureKind,
    rtf: &Option<String>,
    html: &Option<String>,
    text: &Option<String>,
    image: Option<&CapturedImage>,
    max_item_bytes: u64,
) -> bool {
    if max_item_bytes == 0 {
        return true;
    }

    let payload_bytes = match kind {
        CaptureKind::Rtf => rtf.as_deref().map_or(0, source::rtf_storage_byte_len) as u64,
        CaptureKind::Html => html.as_ref().map_or(0, String::len) as u64,
        CaptureKind::Image => image
            .map(|value| {
                u64::from(value.width)
                    .saturating_mul(u64::from(value.height))
                    .saturating_mul(4)
                    .max(value.rgba.len() as u64)
            })
            .unwrap_or(0),
        CaptureKind::Text => text.as_ref().map_or(0, String::len) as u64,
        // File entries store metadata and paths. File contents use a separate staging quota.
        CaptureKind::File => return true,
    };

    payload_bytes <= max_item_bytes
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

    let icon_path =
        icon_store::ensure_icon_cached(&info.exe_path, &state.icon_dir, &info.icon_cache_key);
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
        from_self: false,
    }
}

fn resolve_self_write_match(
    pending: Option<(String, std::time::Instant)>,
    captured_hash: &str,
    reinsert_on_self_copy: bool,
    now: std::time::Instant,
) -> SelfWriteDecision {
    let Some((pending_hash, created_at)) = pending else {
        return SelfWriteDecision::None;
    };
    if now.duration_since(created_at) > std::time::Duration::from_millis(SELF_WRITE_WINDOW_MS) {
        return SelfWriteDecision::None;
    }
    if pending_hash != captured_hash {
        return SelfWriteDecision::None;
    }
    if reinsert_on_self_copy {
        SelfWriteDecision::CaptureAsSelf
    } else {
        SelfWriteDecision::Skip
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
        from_self: false,
    }
}

fn build_rtf_item(
    rtf: String,
    html: Option<String>,
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
        html,
        image_path: None,
        image_width: None,
        image_height: None,
        file_paths: None,
        byte_size: source::rtf_storage_byte_len(&rtf) as i64,
        hash,
        source_app: source.source_app.clone(),
        source_app_icon: source.source_app_icon.clone(),
        from_self: false,
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
        from_self: false,
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
        from_self: false,
    }
}

fn total_file_bytes(paths: &[String]) -> i64 {
    paths
        .iter()
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

fn upsert_item(
    state: &ClipboardState,
    item: NewItem,
    group_id: Option<i64>,
) -> Result<(ClipboardItem, bool), String> {
    let settings = state.settings.read().clone();
    let candidate_image_path = item.image_path.clone();
    let (stored, is_new, duplicate_asset_candidate, needs_asset_cleanup) = {
        let conn = state.write_db.lock();
        let existed =
            db::item_exists_for_dedup(&conn, &item, group_id).map_err(|err| err.to_string())?;
        let previous_icon = if existed {
            db::get_item_for_dedup(&conn, &item, group_id)
                .ok()
                .and_then(|item| item.source_app_icon)
        } else {
            None
        };
        let is_new = matches!(
            settings.dedup_strategy,
            crate::clipboard::models::ClipboardDedupStrategy::AlwaysNew
        ) || !existed;
        let duplicate_asset_candidate =
            (item.image_path.is_some() || item.source_app_icon.is_some()) && existed;
        let stored = db::upsert_item_with_dedup_in_group(
            &conn,
            &item,
            settings.dedup_strategy.clone(),
            group_id,
        )
        .map_err(|err| err.to_string())?;
        let cleanup_stats = crate::clipboard::retention::run_cleanup(&conn, &settings)
            .map_err(|err| err.to_string())?;
        let replaced_icon = previous_icon.is_some() && previous_icon != stored.source_app_icon;
        (
            stored,
            is_new,
            duplicate_asset_candidate,
            replaced_icon || cleanup_stats.0 > 0 || cleanup_stats.1 > 0,
        )
    };

    if duplicate_asset_candidate && candidate_image_path != stored.image_path {
        remove_generated_candidate(&state.image_dir, candidate_image_path.as_deref());
    }
    if needs_asset_cleanup {
        state.cleanup_orphan_assets();
    }
    Ok((stored, is_new))
}

fn remove_generated_candidate(root: &std::path::Path, candidate: Option<&str>) {
    let Some(candidate) = candidate.map(std::path::Path::new) else {
        return;
    };
    if candidate.strip_prefix(root).is_ok() {
        if let Err(err) = std::fs::remove_file(candidate) {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "[clipboard] remove duplicate image candidate {}: {err}",
                    candidate.display()
                );
            }
        }
    }
}

fn notify_added(app: &AppHandle, id: i64, is_new: bool) {
    let _ = app.emit(
        "clipboard-item-added",
        ClipboardItemAddedEvent { id, is_new },
    );
}

pub fn start(app: AppHandle, state: Arc<ClipboardState>) -> WatcherHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(Mutex::new(None));
    let (work_tx, work_rx) = mpsc::channel::<CaptureSignal>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<()>(1);

    let worker_stop = stop_flag.clone();
    let worker_app = app.clone();
    let worker_state = state.clone();
    let worker = thread::Builder::new()
        .name("clipboard-worker".to_string())
        .spawn(move || run_capture_worker(work_rx, worker_app, worker_state, worker_stop));
    if let Err(err) = worker {
        eprintln!("[clipboard] worker init failed: {err}");
        stop_flag.store(true, Ordering::Release);
        return WatcherHandle {
            stop_flag,
            shutdown,
        };
    }

    let watcher_stop = stop_flag.clone();
    let watcher_shutdown = shutdown.clone();
    let watcher_state = state;
    let watcher = thread::Builder::new()
        .name("clipboard-watcher".to_string())
        .spawn(move || {
            let mut failures = 0u32;
            let mut ready_tx = Some(ready_tx);

            while !watcher_stop.load(Ordering::Acquire) {
                let handler = Handler {
                    state: watcher_state.clone(),
                    stop_flag: watcher_stop.clone(),
                    work_tx: work_tx.clone(),
                };
                let mut master = match Master::new(handler) {
                    Ok(master) => master,
                    Err(err) => {
                        failures = failures.saturating_add(1);
                        eprintln!("[clipboard] master init failed: {err}");
                        sleep_with_stop(&watcher_stop, watcher_backoff_ms(failures));
                        continue;
                    }
                };

                *watcher_shutdown.lock() = Some(master.shutdown_channel());
                if let Some(sender) = ready_tx.take() {
                    let _ = sender.send(());
                }

                let started_at = Instant::now();
                let result = master.run();
                watcher_shutdown.lock().take();
                if watcher_stop.load(Ordering::Acquire) {
                    break;
                }

                if started_at.elapsed() >= Duration::from_secs(30) {
                    failures = 0;
                }
                failures = failures.saturating_add(1);
                match result {
                    Ok(()) => eprintln!("[clipboard] watcher exited unexpectedly"),
                    Err(err) => eprintln!("[clipboard] watcher exit: {err}"),
                }
                sleep_with_stop(&watcher_stop, watcher_backoff_ms(failures));
            }

            drop(work_tx);
        });

    if let Err(err) = watcher {
        eprintln!("[clipboard] watcher init failed: {err}");
        stop_flag.store(true, Ordering::Release);
        return WatcherHandle {
            stop_flag,
            shutdown,
        };
    }

    let _ = ready_rx.recv_timeout(Duration::from_secs(2));

    WatcherHandle {
        stop_flag,
        shutdown,
    }
}

fn run_capture_worker(
    work_rx: mpsc::Receiver<CaptureSignal>,
    app: AppHandle,
    state: Arc<ClipboardState>,
    stop_flag: Arc<AtomicBool>,
) {
    while !stop_flag.load(Ordering::Acquire) {
        let mut signal = match work_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(signal) => signal,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        thread::sleep(Duration::from_millis(CAPTURE_DEBOUNCE_MS));
        while let Ok(newer) = work_rx.try_recv() {
            signal = newer;
        }

        if stop_flag.load(Ordering::Acquire) || !state.is_enabled.load(Ordering::Acquire) {
            continue;
        }

        match read_capture_work_item(signal.source_info, signal.sequence) {
            Ok(Some(work)) => {
                if let Err(err) = process_capture(&app, state.as_ref(), work) {
                    eprintln!("[clipboard] capture failed: {err}");
                }
            }
            Ok(None) => {}
            Err(err) => eprintln!("[clipboard] read failed: {err}"),
        }
    }
}

fn watcher_backoff_ms(failures: u32) -> u64 {
    100u64
        .saturating_mul(2u64.saturating_pow(failures.min(6)))
        .min(WATCHER_MAX_BACKOFF_MS)
}

fn sleep_with_stop(stop_flag: &AtomicBool, total_ms: u64) {
    let mut remaining = total_ms;
    while remaining > 0 && !stop_flag.load(Ordering::Acquire) {
        let slice = remaining.min(50);
        thread::sleep(Duration::from_millis(slice));
        remaining -= slice;
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
    fn capture_size_limit_uses_decoded_rtf_and_rgba_bytes() {
        let rtf = Some(source::encode_rtf_storage(b"12345"));
        assert!(capture_kind_fits_limit(
            CaptureKind::Rtf,
            &rtf,
            &None,
            &None,
            None,
            5,
        ));
        assert!(!capture_kind_fits_limit(
            CaptureKind::Rtf,
            &rtf,
            &None,
            &None,
            None,
            4,
        ));

        let image = CapturedImage {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
        };
        assert!(!capture_kind_fits_limit(
            CaptureKind::Image,
            &None,
            &None,
            &None,
            Some(&image),
            15,
        ));
    }

    #[test]
    fn watcher_restart_backoff_is_exponential_and_capped() {
        assert_eq!(watcher_backoff_ms(0), 100);
        assert_eq!(watcher_backoff_ms(1), 200);
        assert_eq!(watcher_backoff_ms(2), 400);
        assert_eq!(watcher_backoff_ms(6), WATCHER_MAX_BACKOFF_MS);
        assert_eq!(watcher_backoff_ms(u32::MAX), WATCHER_MAX_BACKOFF_MS);
    }

    #[test]
    fn duplicate_candidate_cleanup_stays_inside_asset_root() {
        let root = TempDir::new().unwrap();
        let candidate = root.path().join("candidate.png");
        std::fs::write(&candidate, b"candidate").unwrap();

        remove_generated_candidate(root.path(), candidate.to_str());

        assert!(!candidate.exists());
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
    fn resolve_self_write_match_skips_capture_when_setting_is_disabled() {
        let now = std::time::Instant::now();

        let decision = resolve_self_write_match(
            Some(("same-hash".to_string(), now)),
            "same-hash",
            false,
            now,
        );

        assert_eq!(decision, SelfWriteDecision::Skip);
    }

    #[test]
    fn resolve_self_write_match_marks_capture_as_self_when_setting_is_enabled() {
        let now = std::time::Instant::now();

        let decision =
            resolve_self_write_match(Some(("same-hash".to_string(), now)), "same-hash", true, now);

        assert_eq!(decision, SelfWriteDecision::CaptureAsSelf);
    }

    #[test]
    fn resolve_self_write_match_ignores_stale_marker() {
        let now = std::time::Instant::now();

        let decision = resolve_self_write_match(
            Some((
                "same-hash".to_string(),
                now - std::time::Duration::from_millis(900),
            )),
            "same-hash",
            true,
            now,
        );

        assert_eq!(decision, SelfWriteDecision::None);
    }

    #[test]
    fn build_rtf_item_prefers_plain_text_and_keeps_rtf_payload() {
        let source = sample_source();
        let with_text = build_rtf_item(
            "{\\rtf1\\ansi Hello}".to_string(),
            Some("<b>Hello</b>".to_string()),
            Some("Hello".to_string()),
            "hash".to_string(),
            &source,
        );
        assert_eq!(with_text.kind, ContentKind::Rtf);
        assert_eq!(with_text.content_preview, "Hello");
        assert_eq!(with_text.content_full.as_deref(), Some("Hello"));
        assert_eq!(with_text.html.as_deref(), Some("<b>Hello</b>"));
        assert_eq!(
            with_text.rtf_content.as_deref(),
            Some("{\\rtf1\\ansi Hello}")
        );
        assert_eq!(with_text.byte_size, "{\\rtf1\\ansi Hello}".len() as i64);

        let fallback = build_rtf_item(
            "{\\rtf1\\ansi}".to_string(),
            None,
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
        let state = ClipboardState::init(
            temp_dir.path(),
            crate::clipboard::models::ClipboardSettings::default(),
        )
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
            build_text_item(
                "same text".to_string(),
                "same-hash".to_string(),
                &old_source,
            ),
            None,
        )
        .unwrap();
        upsert_item(
            &state,
            build_text_item(
                "same text".to_string(),
                "same-hash".to_string(),
                &new_source,
            ),
            None,
        )
        .unwrap();

        assert!(!old_icon.exists());
        assert!(new_icon.exists());
    }
}
