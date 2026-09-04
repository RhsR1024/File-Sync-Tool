//! Clipboard manager module (spec §5).
//! See docs/superpowers/specs/2026-04-19-clipboard-manager-design.md

pub mod admin;
pub mod commands;
pub mod data_transfer;
pub mod db;
pub mod explorer_menu;
pub mod groups;
pub mod hotkey;
pub mod icon_store;
pub mod image_copy;
pub mod image_store;
pub mod models;
pub mod paste;
pub mod preview;
pub mod retention;
pub mod source;
pub mod task_scheduler;
pub mod watcher;
pub mod win_v;

use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use models::ClipboardSettings;

pub struct ClipboardState {
    /// Compatibility alias for existing callers that still use the write path.
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub read_db: Arc<Mutex<rusqlite::Connection>>,
    pub write_db: Arc<Mutex<rusqlite::Connection>>,
    pub db_path: PathBuf,
    pub image_dir: PathBuf,
    pub icon_dir: PathBuf,
    pub is_enabled: AtomicBool,
    /// When true the popup panel does NOT auto-hide on focus loss. Toggled by
    /// the lock-window toolbar button on the panel.
    pub panel_pinned: AtomicBool,
    /// Prevents the popup panel from hiding its owned native file dialog when
    /// focus moves from the WebView to that dialog.
    pub panel_native_dialog_open: AtomicBool,
    pub settings: Arc<RwLock<ClipboardSettings>>,
    /// Current custom group selected in the clipboard panel. `None` is the
    /// default ungrouped bucket; new captures are stored into this group.
    pub active_group_id: Mutex<Option<i64>>,
    /// Native window that owned focus immediately before the clipboard panel opened.
    /// The paste path restores it explicitly before sending Ctrl+V.
    pub paste_target_window: Mutex<Option<isize>>,
    pub pending_self_write: Mutex<Option<(String, std::time::Instant)>>,
    pub watcher_handle: Mutex<Option<watcher::WatcherHandle>>,
    pub hotkey_handle: Mutex<Option<hotkey::HotkeyHandle>>,
    pub image_copy_hotkey_handle: Mutex<Option<hotkey::HotkeyHandle>>,
}

impl ClipboardState {
    pub fn init(
        app_data_dir: &std::path::Path,
        settings: ClipboardSettings,
    ) -> Result<std::sync::Arc<Self>, String> {
        let db_path = app_data_dir.join("clipboard.db");
        let image_dir = app_data_dir.join("clipboard_images");
        let icon_dir = app_data_dir.join("clipboard_icons");
        std::fs::create_dir_all(&image_dir).map_err(|e| format!("create image dir: {e}"))?;
        std::fs::create_dir_all(&icon_dir).map_err(|e| format!("create icon dir: {e}"))?;

        let write_conn = db::open(&db_path).map_err(|e| format!("open db: {e}"))?;
        let read_conn = db::open_read(&db_path).map_err(|e| format!("open read db: {e}"))?;
        let write_db = std::sync::Arc::new(parking_lot::Mutex::new(write_conn));
        let read_db = std::sync::Arc::new(parking_lot::Mutex::new(read_conn));

        let state = std::sync::Arc::new(Self {
            db: write_db.clone(),
            read_db,
            write_db,
            db_path,
            image_dir,
            icon_dir,
            is_enabled: std::sync::atomic::AtomicBool::new(settings.enabled),
            panel_pinned: std::sync::atomic::AtomicBool::new(false),
            panel_native_dialog_open: std::sync::atomic::AtomicBool::new(false),
            settings: std::sync::Arc::new(parking_lot::RwLock::new(settings)),
            active_group_id: parking_lot::Mutex::new(None),
            paste_target_window: parking_lot::Mutex::new(None),
            pending_self_write: parking_lot::Mutex::new(None),
            watcher_handle: parking_lot::Mutex::new(None),
            hotkey_handle: parking_lot::Mutex::new(None),
            image_copy_hotkey_handle: parking_lot::Mutex::new(None),
        });

        state.cleanup_orphan_assets();
        Ok(state)
    }

    #[allow(dead_code)] // reserved for explicit shutdown; Tauri exit handler lets OS reclaim
    pub fn shutdown(&self) {
        if let Some(h) = self.watcher_handle.lock().take() {
            h.stop();
        }
        if let Some(h) = self.hotkey_handle.lock().take() {
            h.unregister();
        }
        if let Some(h) = self.image_copy_hotkey_handle.lock().take() {
            h.unregister();
        }
    }

    pub fn enable(self: &std::sync::Arc<Self>, app: tauri::AppHandle) {
        if self
            .is_enabled
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let handle = crate::clipboard::watcher::start(app, self.clone());
        *self.watcher_handle.lock() = Some(handle);
    }

    pub fn disable(&self) {
        self.is_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        if let Some(h) = self.watcher_handle.lock().take() {
            h.stop();
        }
    }

    pub fn cleanup_orphan_assets(&self) {
        let (image_paths, icon_paths) = {
            let conn = self.write_db.lock();
            let image_paths = match db::list_referenced_image_paths(&conn) {
                Ok(paths) => paths,
                Err(err) => {
                    eprintln!("[clipboard] list referenced images failed: {err}");
                    return;
                }
            };
            let icon_paths = match db::list_referenced_icon_paths(&conn) {
                Ok(paths) => paths,
                Err(err) => {
                    eprintln!("[clipboard] list referenced icons failed: {err}");
                    return;
                }
            };
            (image_paths, icon_paths)
        };

        if let Err(err) = image_store::gc_orphan_images(&self.image_dir, &image_paths) {
            eprintln!("[clipboard] orphan image cleanup failed: {err}");
        }
        if let Err(err) = icon_store::gc_orphan_icons(&self.icon_dir, &icon_paths) {
            eprintln!("[clipboard] orphan icon cleanup failed: {err}");
        }
    }
}

pub(crate) fn capture_hash(prefix: &[u8], data: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(prefix);
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}
