//! Clipboard data models (spec §7.1).

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Text,
    Html,
    Image,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardSettings {
    pub enabled: bool,
    pub hotkey: String,
    pub max_items: u32,
    pub retain_days: u32,
    pub max_item_bytes: u64,
    pub preview_delay_ms: u32,
    pub enable_text_preview: bool,
    pub use_win_v_replacement: bool,
    pub run_as_admin: bool,
    pub show_startup_notification: bool,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: "Alt+C".to_string(),
            max_items: 1000,
            retain_days: 30,
            max_item_bytes: 10 * 1024 * 1024,
            preview_delay_ms: 500,
            enable_text_preview: false,
            use_win_v_replacement: false,
            run_as_admin: false,
            show_startup_notification: true,
        }
    }
}
