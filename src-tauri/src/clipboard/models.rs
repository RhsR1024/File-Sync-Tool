//! Clipboard data models (spec 搂7.1).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    #[default]
    Text,
    Html,
    Rtf,
    Image,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardDedupStrategy {
    #[default]
    MoveToTop,
    Ignore,
    AlwaysNew,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardCardDensity {
    Compact,
    #[default]
    Standard,
    Spacious,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardTimeFormat {
    #[default]
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardSourceAppDisplay {
    None,
    #[default]
    Name,
    Icon,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardPreviewPosition {
    #[default]
    Auto,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardAppFilterMode {
    #[default]
    Blacklist,
    Whitelist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardDisplaySettings {
    pub density: ClipboardCardDensity,
    pub preview_lines: u8,
    pub time_format: ClipboardTimeFormat,
    pub show_char_count: bool,
    pub show_byte_size: bool,
    pub show_source_app: ClipboardSourceAppDisplay,
    pub image_max_height: u32,
    pub image_auto_height: bool,
    pub drag_indicator: bool,
}

impl Default for ClipboardDisplaySettings {
    fn default() -> Self {
        Self {
            density: ClipboardCardDensity::Standard,
            preview_lines: 3,
            time_format: ClipboardTimeFormat::Relative,
            show_char_count: false,
            show_byte_size: true,
            show_source_app: ClipboardSourceAppDisplay::Name,
            image_max_height: 120,
            image_auto_height: true,
            drag_indicator: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardPreviewSettings {
    pub image_enabled: bool,
    pub text_enabled: bool,
    pub delay_ms: u32,
    pub zoom_step: u8,
    pub position: ClipboardPreviewPosition,
}

impl Default for ClipboardPreviewSettings {
    fn default() -> Self {
        Self {
            image_enabled: true,
            text_enabled: true,
            delay_ms: 500,
            zoom_step: 10,
            position: ClipboardPreviewPosition::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardShortcutsSettings {
    pub quick_paste: Vec<String>,
    pub paste: String,
    pub plain_paste: String,
    pub delete: String,
    pub favorite: String,
    pub edit: String,
    pub focus_search: Vec<String>,
    pub close: String,
}

impl Default for ClipboardShortcutsSettings {
    fn default() -> Self {
        Self {
            quick_paste: vec![],
            paste: "Enter".to_string(),
            plain_paste: "Shift+Enter".to_string(),
            delete: "Delete".to_string(),
            favorite: "Ctrl+D".to_string(),
            edit: "Ctrl+E".to_string(),
            focus_search: vec!["Ctrl+F".to_string(), "/".to_string()],
            close: "Escape".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardToolbarSettings {
    pub items: Vec<String>,
}

impl Default for ClipboardToolbarSettings {
    fn default() -> Self {
        Self {
            items: vec![
                "search".to_string(),
                "filter".to_string(),
                "batch".to_string(),
                "settings".to_string(),
                "lock".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardDataSettings {
    pub max_items: u32,
    pub retain_days: u32,
    pub max_item_bytes: u64,
}

impl Default for ClipboardDataSettings {
    fn default() -> Self {
        Self {
            max_items: 1000,
            retain_days: 30,
            max_item_bytes: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardAudioSettings {
    pub enabled: bool,
    pub volume: u8,
    pub on_copy: bool,
    pub on_paste: bool,
}

impl Default for ClipboardAudioSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: 100,
            on_copy: false,
            on_paste: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardAppFilterSettings {
    pub enabled: bool,
    pub mode: ClipboardAppFilterMode,
    pub patterns: Vec<String>,
}

impl Default for ClipboardAppFilterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ClipboardAppFilterMode::Blacklist,
            patterns: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    pub dedup_strategy: ClipboardDedupStrategy,
    #[serde(default)]
    pub display: ClipboardDisplaySettings,
    #[serde(default)]
    pub preview: ClipboardPreviewSettings,
    #[serde(default)]
    pub shortcuts: ClipboardShortcutsSettings,
    #[serde(default)]
    pub toolbar: ClipboardToolbarSettings,
    #[serde(default)]
    pub data: ClipboardDataSettings,
    #[serde(default)]
    pub audio: ClipboardAudioSettings,
    #[serde(default)]
    pub app_filter: ClipboardAppFilterSettings,
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
            dedup_strategy: ClipboardDedupStrategy::MoveToTop,
            display: ClipboardDisplaySettings::default(),
            preview: ClipboardPreviewSettings::default(),
            shortcuts: ClipboardShortcutsSettings::default(),
            toolbar: ClipboardToolbarSettings::default(),
            data: ClipboardDataSettings::default(),
            audio: ClipboardAudioSettings::default(),
            app_filter: ClipboardAppFilterSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    pub id: i64,
    pub kind: ContentKind,
    pub content_preview: String,
    pub content_full: Option<String>,
    pub rtf_content: Option<String>,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_paths: Option<Vec<String>>,
    pub byte_size: i64,
    pub char_count: i64,
    pub hash: String,
    pub source_app: Option<String>,
    pub source_app_icon: Option<String>,
    pub group_id: Option<i64>,
    pub is_favorite: bool,
    pub is_pinned: bool,
    pub favorite_sort_index: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardFilter {
    #[default]
    All,
    Text,
    Image,
    File,
    Favorite,
    Pinned,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClipboardSearchFilters {
    #[serde(default)]
    pub kind: Option<ContentKind>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub fav: bool,
    #[serde(default)]
    pub size_gt: Option<i64>,
    #[serde(default)]
    pub size_lt: Option<i64>,
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub pinned_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClipboardSearchPayload {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub filters: ClipboardSearchFilters,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClipboardListQuery {
    pub filter: ClipboardFilter,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub search_payload: Option<ClipboardSearchPayload>,
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub pinned_only: bool,
    #[serde(default)]
    pub op_type: Option<String>,
    #[serde(default)]
    pub op_from_ms: Option<i64>,
    #[serde(default)]
    pub op_to_ms: Option<i64>,
    #[serde(default)]
    pub op_app: Option<String>,
    #[serde(default)]
    pub op_fav_only: bool,
    #[serde(default)]
    pub op_size_gt: Option<i64>,
    #[serde(default)]
    pub op_size_lt: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardGroup {
    pub id: i64,
    pub name: String,
    pub sort_index: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePathStatus {
    pub path: String,
    pub exists: bool,
    pub size: Option<u64>,
}

impl ContentKind {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
            Self::Rtf => "rtf",
            Self::Image => "image",
            Self::File => "file",
        }
    }

    pub fn from_sql(s: &str) -> Self {
        match s {
            "html" => Self::Html,
            "rtf" => Self::Rtf,
            "image" => Self::Image,
            "file" => Self::File,
            _ => Self::Text,
        }
    }
}
