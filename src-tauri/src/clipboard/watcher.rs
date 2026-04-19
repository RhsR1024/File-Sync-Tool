//! Clipboard listener thread (spec §5.1, §8.2).
//!
//! Uses `clipboard-master` to listen for Win32 clipboard events, then reads the payload with
//! `arboard`. Each captured payload is hashed with BLAKE3; duplicates are treated as "updated_at"
//! touches (no new row). New items trigger a Tauri event `clipboard-item-added`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

use arboard::Clipboard;
use clipboard_master::{CallbackResult, ClipboardHandler, Master, Shutdown};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::clipboard::ClipboardState;
use crate::clipboard::db::{self, NewItem};
use crate::clipboard::image_store;
use crate::clipboard::models::ContentKind;

pub struct WatcherHandle {
    stop_flag: Arc<AtomicBool>,
    shutdown: Mutex<Option<Shutdown>>,
}

impl WatcherHandle {
    pub fn stop(self) {
        self.stop_flag.store(true, Ordering::Release);
        // Actively wake the Win32 message loop so it breaks immediately instead of waiting for
        // the next clipboard change.
        if let Some(s) = self.shutdown.lock().take() {
            s.signal();
        }
    }
}

struct Handler {
    app: AppHandle,
    state: Arc<ClipboardState>,
    stop_flag: Arc<AtomicBool>,
}

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        if self.stop_flag.load(Ordering::Acquire) {
            return CallbackResult::Stop;
        }
        if !self.state.is_enabled.load(Ordering::Acquire) {
            return CallbackResult::Next;
        }
        if let Err(e) = try_capture(&self.app, &self.state) {
            eprintln!("[clipboard] capture failed: {e}");
        }
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, err: std::io::Error) -> CallbackResult {
        eprintln!("[clipboard] error: {err}");
        CallbackResult::Next
    }
}

fn try_capture(app: &AppHandle, state: &ClipboardState) -> Result<(), String> {
    let mut cb = Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;

    // Priority: image > text (files/html support ships in later iterations; arboard has limited
    // file-list support on Windows).
    if let Ok(img) = cb.get_image() {
        let hash = compute_hash(b"image", &img.bytes);
        if skip_duplicate(state, hash) {
            return Ok(());
        }
        let hex_str = hex(&hash);
        let saved_path = image_store::save_image_png(
            &state.image_dir,
            &hex_str,
            img.width as u32,
            img.height as u32,
            &img.bytes,
        )?;
        insert_or_touch(
            state,
            NewItem {
                kind: ContentKind::Image,
                content_preview: format!("[Image {}x{}]", img.width, img.height),
                content_full: None,
                html: None,
                image_path: Some(saved_path.to_string_lossy().to_string()),
                image_width: Some(img.width as u32),
                image_height: Some(img.height as u32),
                file_paths: None,
                byte_size: img.bytes.len() as i64,
                hash: hex_str,
                source_app: None,
            },
        )?;
        notify_added(app);
        return Ok(());
    }

    if let Ok(text) = cb.get_text() {
        if text.trim().is_empty() {
            return Ok(());
        }
        let hash = compute_hash(b"text", text.as_bytes());
        if skip_duplicate(state, hash) {
            return Ok(());
        }
        let preview: String = text.chars().take(200).collect();
        let byte_size = text.len() as i64;
        insert_or_touch(
            state,
            NewItem {
                kind: ContentKind::Text,
                content_preview: preview,
                content_full: Some(text),
                html: None,
                image_path: None,
                image_width: None,
                image_height: None,
                file_paths: None,
                byte_size,
                hash: hex(&hash),
                source_app: None,
            },
        )?;
        notify_added(app);
    }
    Ok(())
}

fn compute_hash(prefix: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(prefix);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(64);
    for b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn skip_duplicate(state: &ClipboardState, hash: [u8; 32]) -> bool {
    let mut last = state.last_hash.lock();
    if Some(hash) == *last {
        return true;
    }
    *last = Some(hash);
    false
}

fn insert_or_touch(state: &ClipboardState, item: NewItem) -> Result<(), String> {
    let conn = state.db.lock();
    if db::touch_item_by_hash(&conn, &item.hash).map_err(|e| e.to_string())? {
        return Ok(());
    }
    db::insert_item(&conn, &item)
        .map(|_| ())
        .map_err(|e| e.to_string())
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
            Ok(m) => m,
            Err(e) => {
                eprintln!("[clipboard] master init failed: {e}");
                return;
            }
        };
        // Send the Shutdown back to the caller before entering the blocking message loop.
        let _ = shutdown_tx.send(master.shutdown_channel());
        if let Err(e) = master.run() {
            eprintln!("[clipboard] watcher exit: {e}");
        }
    });

    // Wait briefly for the worker to report its Shutdown handle. If it times out (e.g. Master
    // creation failed), `shutdown` stays None and stop() falls back to the stop_flag only.
    let shutdown = shutdown_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .ok();

    WatcherHandle {
        stop_flag,
        shutdown: Mutex::new(shutdown),
    }
}
