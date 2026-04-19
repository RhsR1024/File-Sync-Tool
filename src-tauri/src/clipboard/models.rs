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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub hash: String,
    pub source_app: Option<String>,
    pub is_favorite: bool,
    pub favorite_sort_index: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardFilter {
    All,
    Text,
    Image,
    File,
    Favorite,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClipboardListQuery {
    pub filter: ClipboardFilter,
    #[serde(default)]
    pub search: String,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardListResult {
    pub items: Vec<ClipboardItem>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardStats {
    pub total: i64,
    pub db_bytes: i64,
    pub image_count: i64,
    pub images_bytes: i64,
}

impl ContentKind {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
            Self::Image => "image",
            Self::File => "file",
        }
    }

    pub fn from_sql(s: &str) -> Self {
        match s {
            "html" => Self::Html,
            "image" => Self::Image,
            "file" => Self::File,
            _ => Self::Text,
        }
    }
}
