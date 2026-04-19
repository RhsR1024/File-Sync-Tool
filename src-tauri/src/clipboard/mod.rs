//! Clipboard manager module (spec §5).
//! See docs/superpowers/specs/2026-04-19-clipboard-manager-design.md

pub mod models;
pub mod db;
pub mod retention;
pub mod watcher;
pub mod image_store;
pub mod hotkey;
pub mod paste;
pub mod win_v;
pub mod admin;
pub mod commands;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use parking_lot::{Mutex, RwLock};

use models::ClipboardSettings;

pub struct ClipboardState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub db_path: PathBuf,
    pub image_dir: PathBuf,
    pub is_enabled: AtomicBool,
    pub last_hash: Mutex<Option<[u8; 32]>>,
    pub settings: Arc<RwLock<ClipboardSettings>>,
    pub watcher_handle: Mutex<Option<watcher::WatcherHandle>>,
    pub hotkey_handle: Mutex<Option<hotkey::HotkeyHandle>>,
}

impl ClipboardState {
    pub fn init(
        app_data_dir: &std::path::Path,
        settings: ClipboardSettings,
    ) -> Result<std::sync::Arc<Self>, String> {
        let db_path = app_data_dir.join("clipboard.db");
        let image_dir = app_data_dir.join("clipboard_images");
        std::fs::create_dir_all(&image_dir)
            .map_err(|e| format!("create image dir: {e}"))?;

        let conn = db::open(&db_path).map_err(|e| format!("open db: {e}"))?;

        Ok(std::sync::Arc::new(Self {
            db: std::sync::Arc::new(parking_lot::Mutex::new(conn)),
            db_path,
            image_dir,
            is_enabled: std::sync::atomic::AtomicBool::new(settings.enabled),
            last_hash: parking_lot::Mutex::new(None),
            settings: std::sync::Arc::new(parking_lot::RwLock::new(settings)),
            watcher_handle: parking_lot::Mutex::new(None),
            hotkey_handle: parking_lot::Mutex::new(None),
        }))
    }

    pub fn shutdown(&self) {
        if let Some(h) = self.watcher_handle.lock().take() {
            h.stop();
        }
        if let Some(h) = self.hotkey_handle.lock().take() {
            h.unregister();
        }
    }

    pub fn enable(self: &std::sync::Arc<Self>, app: tauri::AppHandle) {
        if self.is_enabled.swap(true, std::sync::atomic::Ordering::AcqRel) {
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
}
