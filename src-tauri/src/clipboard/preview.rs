use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Size, WebviewWindow,
    WebviewWindowBuilder, WebviewUrl,
};

use crate::clipboard::models::{ClipboardItem, ClipboardPreviewPosition, ClipboardSettings, ContentKind};

pub const IMAGE_PREVIEW_WINDOW_LABEL: &str = "clipboard-image-preview";
pub const TEXT_PREVIEW_WINDOW_LABEL: &str = "clipboard-text-preview";
pub const IMAGE_PREVIEW_UPDATE_EVENT: &str = "clipboard-image-preview-update";
pub const TEXT_PREVIEW_UPDATE_EVENT: &str = "clipboard-text-preview-update";
pub const PREVIEW_GAP_PX: i32 = 12;

const IMAGE_PREVIEW_ROUTE: &str = "index.html#/clipboard-preview/image";
const TEXT_PREVIEW_ROUTE: &str = "index.html#/clipboard-preview/text";
const IMAGE_PREVIEW_TITLE: &str = "Clipboard Image Preview";
const TEXT_PREVIEW_TITLE: &str = "Clipboard Text Preview";
const DEFAULT_IMAGE_PREVIEW_WIDTH: u32 = 420;
const DEFAULT_IMAGE_PREVIEW_HEIGHT: u32 = 320;
const DEFAULT_TEXT_PREVIEW_WIDTH: u32 = 420;
const DEFAULT_TEXT_PREVIEW_HEIGHT: u32 = 420;
const MIN_PREVIEW_WIDTH: u32 = 280;
const MIN_PREVIEW_HEIGHT: u32 = 220;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowRect {
    fn right(self) -> i32 {
        self.x + self.width as i32
    }

    fn bottom(self) -> i32 {
        self.y + self.height as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewWindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewPlacement {
    pub side: PreviewSide,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewKind {
    Image,
    Text,
}

#[derive(Debug, Clone, Serialize)]
struct ImagePreviewPayload {
    id: i64,
    image_path: String,
    zoom_step: u8,
    source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TextPreviewPayload {
    id: i64,
    kind: &'static str,
    content: String,
    source_app: Option<String>,
}

pub fn ensure_preview_windows<R: tauri::Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<()> {
    ensure_preview_window(
        manager,
        IMAGE_PREVIEW_WINDOW_LABEL,
        IMAGE_PREVIEW_ROUTE,
        IMAGE_PREVIEW_TITLE,
        PreviewWindowSize {
            width: DEFAULT_IMAGE_PREVIEW_WIDTH,
            height: DEFAULT_IMAGE_PREVIEW_HEIGHT,
        },
    )?;
    ensure_preview_window(
        manager,
        TEXT_PREVIEW_WINDOW_LABEL,
        TEXT_PREVIEW_ROUTE,
        TEXT_PREVIEW_TITLE,
        PreviewWindowSize {
            width: DEFAULT_TEXT_PREVIEW_WIDTH,
            height: DEFAULT_TEXT_PREVIEW_HEIGHT,
        },
    )?;
    Ok(())
}

pub fn hide_preview_windows<R: tauri::Runtime>(app: &AppHandle<R>) {
    for label in [IMAGE_PREVIEW_WINDOW_LABEL, TEXT_PREVIEW_WINDOW_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
}

pub fn preview_window_is_focused<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    [IMAGE_PREVIEW_WINDOW_LABEL, TEXT_PREVIEW_WINDOW_LABEL]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|window| window.is_focused().unwrap_or(false))
}

pub fn show_image_preview<R: tauri::Runtime>(
    app: &AppHandle<R>,
    settings: &ClipboardSettings,
    item: &ClipboardItem,
) -> Result<(), String> {
    if item.kind != ContentKind::Image {
        return Err("clipboard item is not an image kind".to_string());
    }

    if !settings.preview.image_enabled {
        hide_preview_windows(app);
        return Ok(());
    }

    let image_path = item
        .image_path
        .clone()
        .ok_or_else(|| "clipboard image preview is missing image_path".to_string())?;

    let payload = ImagePreviewPayload {
        id: item.id,
        image_path,
        zoom_step: settings.preview.zoom_step.max(1),
        source_app: item.source_app.clone(),
    };

    let desired_size = desired_image_preview_size(item);
    show_preview_window(app, settings, PreviewKind::Image, desired_size, &payload)
}

pub fn show_text_preview<R: tauri::Runtime>(
    app: &AppHandle<R>,
    settings: &ClipboardSettings,
    item: &ClipboardItem,
) -> Result<(), String> {
    if !matches!(item.kind, ContentKind::Text | ContentKind::Html | ContentKind::Rtf) {
        return Err("clipboard item is not text-previewable".to_string());
    }

    if !text_preview_enabled(settings) {
        hide_preview_windows(app);
        return Ok(());
    }

    let content = item
        .content_full
        .clone()
        .unwrap_or_else(|| item.content_preview.clone());
    if content.trim().is_empty() {
        hide_preview_windows(app);
        return Ok(());
    }

    let payload = TextPreviewPayload {
        id: item.id,
        kind: item.kind.as_sql(),
        content,
        source_app: item.source_app.clone(),
    };

    let desired_size = PreviewWindowSize {
        width: DEFAULT_TEXT_PREVIEW_WIDTH,
        height: DEFAULT_TEXT_PREVIEW_HEIGHT,
    };

    show_preview_window(app, settings, PreviewKind::Text, desired_size, &payload)
}

pub fn calculate_preview_placement(
    panel_rect: WindowRect,
    preview_size: PreviewWindowSize,
    monitor_rect: WindowRect,
    preference: ClipboardPreviewPosition,
) -> PreviewPlacement {
    let width = preview_size.width.min(monitor_rect.width).max(MIN_PREVIEW_WIDTH.min(monitor_rect.width));
    let height = preview_size.height.min(monitor_rect.height).max(MIN_PREVIEW_HEIGHT.min(monitor_rect.height));

    let desired_side = match preference {
        ClipboardPreviewPosition::Left => PreviewSide::Left,
        ClipboardPreviewPosition::Right => PreviewSide::Right,
        ClipboardPreviewPosition::Auto => {
            let left_space = panel_rect.x - monitor_rect.x;
            let right_space = monitor_rect.right() - panel_rect.right();
            if right_space >= left_space {
                PreviewSide::Right
            } else {
                PreviewSide::Left
            }
        }
    };

    let left_x = panel_rect.x - PREVIEW_GAP_PX - width as i32;
    let right_x = panel_rect.right() + PREVIEW_GAP_PX;
    let left_fits = left_x >= monitor_rect.x;
    let right_fits = right_x + width as i32 <= monitor_rect.right();

    let side = match desired_side {
        PreviewSide::Left if left_fits || !right_fits => PreviewSide::Left,
        PreviewSide::Left => PreviewSide::Right,
        PreviewSide::Right if right_fits || !left_fits => PreviewSide::Right,
        PreviewSide::Right => PreviewSide::Left,
    };

    let unclamped_x = match side {
        PreviewSide::Left => left_x,
        PreviewSide::Right => right_x,
    };
    let max_x = monitor_rect.right() - width as i32;
    let max_y = monitor_rect.bottom() - height as i32;

    PreviewPlacement {
        side,
        x: unclamped_x.clamp(monitor_rect.x, max_x.max(monitor_rect.x)),
        y: panel_rect.y.clamp(monitor_rect.y, max_y.max(monitor_rect.y)),
        width,
        height,
    }
}

fn ensure_preview_window<R: tauri::Runtime, M: Manager<R>>(
    manager: &M,
    label: &str,
    route: &str,
    title: &str,
    size: PreviewWindowSize,
) -> tauri::Result<()> {
    if manager.get_webview_window(label).is_some() {
        return Ok(());
    }

    WebviewWindowBuilder::new(manager, label, WebviewUrl::App(route.into()))
        .title(title)
        .inner_size(size.width as f64, size.height as f64)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .focused(false)
        .visible(false)
        .build()?;

    Ok(())
}

fn desired_image_preview_size(item: &ClipboardItem) -> PreviewWindowSize {
    let width = item
        .image_width
        .unwrap_or(DEFAULT_IMAGE_PREVIEW_WIDTH.saturating_sub(32))
        .saturating_add(32)
        .clamp(MIN_PREVIEW_WIDTH, 960);
    let height = item
        .image_height
        .unwrap_or(DEFAULT_IMAGE_PREVIEW_HEIGHT.saturating_sub(32))
        .saturating_add(32)
        .clamp(MIN_PREVIEW_HEIGHT, 720);

    PreviewWindowSize { width, height }
}

fn text_preview_enabled(settings: &ClipboardSettings) -> bool {
    settings.enable_text_preview || settings.preview.text_enabled
}

fn show_preview_window<R: tauri::Runtime, T: Serialize>(
    app: &AppHandle<R>,
    settings: &ClipboardSettings,
    kind: PreviewKind,
    desired_size: PreviewWindowSize,
    payload: &T,
) -> Result<(), String> {
    ensure_preview_windows(app).map_err(|error| error.to_string())?;

    let panel = app
        .get_webview_window("clipboard-panel")
        .ok_or_else(|| "clipboard-panel window not found".to_string())?;
    let panel_rect = window_rect(&panel)?;
    let monitor_rect = monitor_rect(&panel)?;
    let placement = calculate_preview_placement(
        panel_rect,
        desired_size,
        monitor_rect,
        settings.preview.position.clone(),
    );

    let (target_label, update_event, opposite_label) = match kind {
        PreviewKind::Image => (
            IMAGE_PREVIEW_WINDOW_LABEL,
            IMAGE_PREVIEW_UPDATE_EVENT,
            TEXT_PREVIEW_WINDOW_LABEL,
        ),
        PreviewKind::Text => (
            TEXT_PREVIEW_WINDOW_LABEL,
            TEXT_PREVIEW_UPDATE_EVENT,
            IMAGE_PREVIEW_WINDOW_LABEL,
        ),
    };

    if let Some(window) = app.get_webview_window(opposite_label) {
        let _ = window.hide();
    }

    let window = app
        .get_webview_window(target_label)
        .ok_or_else(|| format!("{target_label} window not found"))?;

    window
        .set_size(Size::Physical(PhysicalSize::new(
            placement.width,
            placement.height,
        )))
        .map_err(|error| error.to_string())?;
    window
        .set_position(PhysicalPosition::new(placement.x, placement.y))
        .map_err(|error| error.to_string())?;
    window
        .emit(update_event, payload)
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;

    Ok(())
}

fn window_rect<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<WindowRect, String> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;

    Ok(WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn monitor_rect<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<WindowRect, String> {
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "current monitor unavailable".to_string())?;
    let position = monitor.position();
    let size = monitor.size();

    Ok(WindowRect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> WindowRect {
        WindowRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn calculate_preview_placement_prefers_requested_right_side_when_space_exists() {
        let placement = calculate_preview_placement(
            rect(600, 200, 420, 720),
            PreviewWindowSize {
                width: 360,
                height: 280,
            },
            rect(0, 0, 1920, 1080),
            crate::clipboard::models::ClipboardPreviewPosition::Right,
        );

        assert_eq!(placement.side, PreviewSide::Right);
        assert_eq!(placement.x, 600 + 420_i32 + PREVIEW_GAP_PX);
        assert_eq!(placement.y, 200);
    }

    #[test]
    fn calculate_preview_placement_flips_to_left_when_requested_right_would_overflow() {
        let placement = calculate_preview_placement(
            rect(1480, 160, 420, 720),
            PreviewWindowSize {
                width: 360,
                height: 280,
            },
            rect(0, 0, 1920, 1080),
            crate::clipboard::models::ClipboardPreviewPosition::Right,
        );

        assert_eq!(placement.side, PreviewSide::Left);
        assert_eq!(placement.x, 1480 - PREVIEW_GAP_PX - 360);
        assert_eq!(placement.y, 160);
    }

    #[test]
    fn calculate_preview_placement_auto_prefers_the_side_with_more_horizontal_space() {
        let placement = calculate_preview_placement(
            rect(160, 120, 420, 720),
            PreviewWindowSize {
                width: 360,
                height: 280,
            },
            rect(0, 0, 1920, 1080),
            crate::clipboard::models::ClipboardPreviewPosition::Auto,
        );

        assert_eq!(placement.side, PreviewSide::Right);
        assert_eq!(placement.x, 160 + 420_i32 + PREVIEW_GAP_PX);
    }

    #[test]
    fn calculate_preview_placement_clamps_vertical_position_inside_monitor_bounds() {
        let placement = calculate_preview_placement(
            rect(720, 920, 420, 720),
            PreviewWindowSize {
                width: 360,
                height: 320,
            },
            rect(0, 0, 1920, 1080),
            crate::clipboard::models::ClipboardPreviewPosition::Left,
        );

        assert_eq!(placement.side, PreviewSide::Left);
        assert_eq!(placement.y, 1080 - 320);
    }
}
