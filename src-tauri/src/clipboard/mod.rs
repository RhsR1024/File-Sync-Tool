//! Clipboard manager module (spec §5).
//! See docs/superpowers/specs/2026-04-19-clipboard-manager-design.md

pub mod models;
pub mod db;
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
    pub image_dir: PathBuf,
    pub is_enabled: AtomicBool,
    pub last_hash: Mutex<Option<[u8; 32]>>,
    pub settings: Arc<RwLock<ClipboardSettings>>,
    pub watcher_handle: Mutex<Option<watcher::WatcherHandle>>,
    pub hotkey_handle: Mutex<Option<hotkey::HotkeyHandle>>,
}

impl ClipboardState {
    pub fn shutdown(&self) {
        if let Some(h) = self.watcher_handle.lock().take() {
            h.stop();
        }
        if let Some(h) = self.hotkey_handle.lock().take() {
            h.unregister();
        }
    }
}
