use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
};
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Size, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::clipboard::models::{
    ClipboardItem, ClipboardPreviewPosition, ClipboardSettings, ContentKind,
};
use crate::clipboard::ClipboardState;

pub const IMAGE_PREVIEW_WINDOW_LABEL: &str = "clipboard-image-preview";
pub const TEXT_PREVIEW_WINDOW_LABEL: &str = "clipboard-text-preview";
pub const IMAGE_PREVIEW_UPDATE_EVENT: &str = "clipboard-image-preview-update";
pub const TEXT_PREVIEW_UPDATE_EVENT: &str = "clipboard-text-preview-update";
pub const IMAGE_PREVIEW_CLEAR_EVENT: &str = "clipboard-image-preview-clear";
pub const TEXT_PREVIEW_CLEAR_EVENT: &str = "clipboard-text-preview-clear";
pub const PREVIEW_GAP_PX: i32 = 12;

const IMAGE_PREVIEW_ROUTE: &str = "/clipboard-image-preview.html";
const TEXT_PREVIEW_ROUTE: &str = "/clipboard-text-preview.html";
const IMAGE_PREVIEW_TITLE: &str = "Clipboard Image Preview";
const TEXT_PREVIEW_TITLE: &str = "Clipboard Text Preview";
const DEFAULT_IMAGE_PREVIEW_WIDTH: u32 = 420;
const DEFAULT_IMAGE_PREVIEW_HEIGHT: u32 = 320;
const DEFAULT_TEXT_PREVIEW_WIDTH: u32 = 420;
const DEFAULT_TEXT_PREVIEW_HEIGHT: u32 = 420;
const MAX_IMAGE_PREVIEW_WIDTH: u32 = 720;
const MAX_IMAGE_PREVIEW_HEIGHT: u32 = 640;
const IMAGE_PREVIEW_HORIZONTAL_CHROME_PX: u32 = 32;
const IMAGE_PREVIEW_VERTICAL_CHROME_PX: u32 = 96;
const MIN_PREVIEW_WIDTH: u32 = 280;
const MIN_PREVIEW_HEIGHT: u32 = 220;
static PREVIEW_REQUEST_TOKEN: AtomicU64 = AtomicU64::new(0);

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImagePreviewPayload {
    id: i64,
    image_path: String,
    zoom_step: u8,
    source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextPreviewPayload {
    id: i64,
    kind: &'static str,
    content: String,
    source_app: Option<String>,
}

#[derive(Debug, Default)]
struct PreviewPayloadCache {
    image: Option<ImagePreviewPayload>,
    text: Option<TextPreviewPayload>,
}

fn preview_payload_cache() -> &'static Mutex<PreviewPayloadCache> {
    static CACHE: OnceLock<Mutex<PreviewPayloadCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(PreviewPayloadCache::default()))
}

fn promote_preview_token(token: u64) -> bool {
    if token == 0 {
        return true;
    }
    let active_token = token.saturating_mul(2);
    PREVIEW_REQUEST_TOKEN.fetch_max(active_token, Ordering::AcqRel);
    PREVIEW_REQUEST_TOKEN.load(Ordering::Acquire) == active_token
}

fn is_preview_token_current(token: u64) -> bool {
    token == 0 || PREVIEW_REQUEST_TOKEN.load(Ordering::Acquire) == token.saturating_mul(2)
}

fn cancel_all_preview_tokens() {
    loop {
        let current = PREVIEW_REQUEST_TOKEN.load(Ordering::Acquire);
        if current % 2 == 1 {
            return;
        }
        if PREVIEW_REQUEST_TOKEN
            .compare_exchange(
                current,
                current.saturating_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return;
        }
    }
}

fn cancel_preview_token(token: u64) -> bool {
    if token == 0 {
        cancel_all_preview_tokens();
        return true;
    }

    let canceled_token = token.saturating_mul(2).saturating_add(1);
    PREVIEW_REQUEST_TOKEN.fetch_max(canceled_token, Ordering::AcqRel);
    PREVIEW_REQUEST_TOKEN.load(Ordering::Acquire) == canceled_token
}

fn cache_image_preview_payload(payload: ImagePreviewPayload) {
    let mut cache = preview_payload_cache()
        .lock()
        .expect("preview payload cache lock poisoned");
    cache.image = Some(payload);
}

fn cache_text_preview_payload(payload: TextPreviewPayload) {
    let mut cache = preview_payload_cache()
        .lock()
        .expect("preview payload cache lock poisoned");
    cache.text = Some(payload);
}

pub fn current_image_preview_payload() -> Option<ImagePreviewPayload> {
    preview_payload_cache()
        .lock()
        .expect("preview payload cache lock poisoned")
        .image
        .clone()
}

pub fn current_text_preview_payload() -> Option<TextPreviewPayload> {
    preview_payload_cache()
        .lock()
        .expect("preview payload cache lock poisoned")
        .text
        .clone()
}

pub fn clear_cached_preview_payloads() {
    let mut cache = preview_payload_cache()
        .lock()
        .expect("preview payload cache lock poisoned");
    cache.image = None;
    cache.text = None;
}

#[derive(Debug, Default, Clone, Copy)]
struct SavedRect {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}

#[derive(Debug, Default)]
struct PreviewFullscreenState {
    image_saved: Option<SavedRect>,
    text_saved: Option<SavedRect>,
}

fn preview_fullscreen_state() -> &'static Mutex<PreviewFullscreenState> {
    static STATE: OnceLock<Mutex<PreviewFullscreenState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PreviewFullscreenState::default()))
}

fn fullscreen_slot_for_label<'a>(
    state: &'a mut PreviewFullscreenState,
    label: &str,
) -> Option<&'a mut Option<SavedRect>> {
    match label {
        IMAGE_PREVIEW_WINDOW_LABEL => Some(&mut state.image_saved),
        TEXT_PREVIEW_WINDOW_LABEL => Some(&mut state.text_saved),
        _ => None,
    }
}

fn clear_preview_fullscreen_saved(label: &str) {
    let mut state = preview_fullscreen_state()
        .lock()
        .expect("preview fullscreen state poisoned");
    if let Some(slot) = fullscreen_slot_for_label(&mut state, label) {
        *slot = None;
    }
}

pub fn toggle_preview_fullscreen<R: tauri::Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<bool, String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("preview window not found: {label}"))?;

    let mut state = preview_fullscreen_state()
        .lock()
        .map_err(|_| "preview fullscreen state poisoned".to_string())?;
    let slot = fullscreen_slot_for_label(&mut state, label)
        .ok_or_else(|| format!("invalid preview label: {label}"))?;

    if let Some(saved) = slot.take() {
        window
            .set_size(Size::Physical(PhysicalSize::new(saved.width, saved.height)))
            .map_err(|error| error.to_string())?;
        window
            .set_position(PhysicalPosition::new(saved.x, saved.y))
            .map_err(|error| error.to_string())?;
        Ok(false)
    } else {
        let pos = window.outer_position().map_err(|error| error.to_string())?;
        let size = window.outer_size().map_err(|error| error.to_string())?;
        *slot = Some(SavedRect {
            width: size.width,
            height: size.height,
            x: pos.x,
            y: pos.y,
        });

        let monitor = window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "current monitor unavailable".to_string())?;
        let monitor_size = monitor.size();
        let monitor_pos = monitor.position();
        window
            .set_size(Size::Physical(PhysicalSize::new(
                monitor_size.width,
                monitor_size.height,
            )))
            .map_err(|error| error.to_string())?;
        window
            .set_position(PhysicalPosition::new(monitor_pos.x, monitor_pos.y))
            .map_err(|error| error.to_string())?;
        Ok(true)
    }
}

pub fn schedule_dismiss_if_orphaned<R>(
    app: AppHandle<R>,
    panel: WebviewWindow<R>,
    state: Arc<ClipboardState>,
) where
    R: tauri::Runtime,
{
    if state
        .panel_pinned
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return;
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        let panel_focused = panel.is_focused().unwrap_or(false);
        let preview_focused = preview_window_is_focused(&app);
        if !panel_focused && !preview_focused {
            hide_preview_windows(&app);
            let _ = panel.hide();
        }
    });
}

pub fn attach_preview_dismiss_handlers<R>(
    app: &AppHandle<R>,
    panel: WebviewWindow<R>,
    state: Arc<ClipboardState>,
) -> tauri::Result<()>
where
    R: tauri::Runtime,
{
    for label in [IMAGE_PREVIEW_WINDOW_LABEL, TEXT_PREVIEW_WINDOW_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let app_handle = app.clone();
            let panel = panel.clone();
            let state = state.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    schedule_dismiss_if_orphaned(app_handle.clone(), panel.clone(), state.clone());
                }
            });
        }
    }
    Ok(())
}

pub fn ensure_preview_windows<R: tauri::Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<()> {
    let _ = ensure_preview_window(
        manager,
        IMAGE_PREVIEW_WINDOW_LABEL,
        IMAGE_PREVIEW_ROUTE,
        IMAGE_PREVIEW_TITLE,
        PreviewWindowSize {
            width: DEFAULT_IMAGE_PREVIEW_WIDTH,
            height: DEFAULT_IMAGE_PREVIEW_HEIGHT,
        },
    )?;
    let _ = ensure_preview_window(
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

fn hide_preview_window_handles<R: tauri::Runtime>(app: &AppHandle<R>) {
    debug_window_snapshot(app, "hide-preview-windows:start");
    for (label, clear_event) in [
        (IMAGE_PREVIEW_WINDOW_LABEL, IMAGE_PREVIEW_CLEAR_EVENT),
        (TEXT_PREVIEW_WINDOW_LABEL, TEXT_PREVIEW_CLEAR_EVENT),
    ] {
        if let Some(window) = app.get_webview_window(label) {
            clear_preview_fullscreen_saved(label);
            let _ = window.emit(clear_event, ());
            let _ = window.hide();
        }
    }
    debug_window_snapshot(app, "hide-preview-windows:done");
}

pub fn hide_preview_windows<R: tauri::Runtime>(app: &AppHandle<R>) {
    cancel_all_preview_tokens();
    hide_preview_window_handles(app);
}

pub fn hide_preview_windows_for_token<R: tauri::Runtime>(app: &AppHandle<R>, token: Option<u64>) {
    let should_hide = match token {
        Some(token) => cancel_preview_token(token),
        None => {
            cancel_all_preview_tokens();
            true
        }
    };

    if should_hide {
        hide_preview_window_handles(app);
    }
}

fn tauri_window_reports_focus<R: tauri::Runtime>(window: &WebviewWindow<R>) -> bool {
    window.is_focused().unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn native_window_handle<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
) -> Result<windows::Win32::Foundation::HWND, String> {
    use windows::Win32::Foundation::HWND;

    window
        .hwnd()
        .map(|hwnd| HWND(hwnd.0 as *mut _))
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn normalize_to_root_hwnd(
    hwnd: windows::Win32::Foundation::HWND,
) -> windows::Win32::Foundation::HWND {
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};

    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if root.0.is_null() {
        hwnd
    } else {
        root
    }
}

#[cfg(target_os = "windows")]
fn foreground_root_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        None
    } else {
        Some(normalize_to_root_hwnd(foreground))
    }
}

#[cfg(target_os = "windows")]
fn preview_window_matches_foreground_root<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
    foreground_root: windows::Win32::Foundation::HWND,
) -> bool {
    native_window_handle(window)
        .map(|hwnd| normalize_to_root_hwnd(hwnd) == foreground_root)
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn enumerate_child_window_handles(
    hwnd: windows::Win32::Foundation::HWND,
) -> Vec<windows::Win32::Foundation::HWND> {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    let mut child_windows = Vec::new();
    unsafe {
        let mut callback = |child_hwnd| {
            child_windows.push(child_hwnd);
            true
        };
        let mut trait_obj: &mut dyn FnMut(HWND) -> bool = &mut callback;
        let closure_pointer_pointer: *mut c_void = std::mem::transmute(&mut trait_obj);
        let lparam = LPARAM(closure_pointer_pointer as isize);
        unsafe extern "system" fn enumerate_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let closure = &mut *(lparam.0 as *mut c_void as *mut &mut dyn FnMut(HWND) -> bool);
            closure(hwnd).into()
        }
        let _ = EnumChildWindows(hwnd, Some(enumerate_callback), lparam);
    }
    child_windows
}

#[cfg(target_os = "windows")]
fn ex_style_summary(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongW, GWL_EXSTYLE};

    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 };
    format!("0x{:x}:0x{ex_style:08x}", hwnd.0 as usize)
}

#[cfg(target_os = "windows")]
fn window_style_tree_summary<R: tauri::Runtime>(window: &WebviewWindow<R>) -> String {
    let Ok(hwnd) = native_window_handle(window) else {
        return "hwnd=err".to_string();
    };

    let mut entries = vec![ex_style_summary(hwnd)];
    for child_hwnd in enumerate_child_window_handles(hwnd) {
        entries.push(ex_style_summary(child_hwnd));
    }
    entries.join(",")
}

#[cfg(target_os = "windows")]
fn debug_window_summary<R: tauri::Runtime>(
    label: &str,
    window: &WebviewWindow<R>,
    foreground_root: Option<windows::Win32::Foundation::HWND>,
) -> String {
    let rect_summary = match window_rect(window) {
        Ok(rect) => format!("rect={}x{}@{},{}", rect.width, rect.height, rect.x, rect.y),
        Err(error) => format!("rect=err({error})"),
    };
    let visible = window.is_visible().unwrap_or(false);
    let tauri_focused = tauri_window_reports_focus(window);
    let foreground_match = foreground_root
        .map(|root| preview_window_matches_foreground_root(window, root))
        .unwrap_or(false);

    format!(
        "{label} visible={visible} tauri_focused={tauri_focused} foreground_match={foreground_match} {rect_summary} styles=[{}]",
        window_style_tree_summary(window)
    )
}

#[cfg(target_os = "windows")]
pub fn log_preview_window_diagnostics<R: tauri::Runtime>(app: &AppHandle<R>, reason: &str) {
    let foreground_root = foreground_root_hwnd();
    let foreground_label = foreground_root
        .map(|hwnd| format!("0x{:x}", hwnd.0 as usize))
        .unwrap_or_else(|| "none".to_string());

    let mut summaries = Vec::new();
    for label in [
        "clipboard-panel",
        IMAGE_PREVIEW_WINDOW_LABEL,
        TEXT_PREVIEW_WINDOW_LABEL,
    ] {
        match app.get_webview_window(label) {
            Some(window) => summaries.push(debug_window_summary(label, &window, foreground_root)),
            None => summaries.push(format!("{label} missing")),
        }
    }

    eprintln!(
        "[clipboard-preview][{reason}] foreground_root={foreground_label} {}",
        summaries.join(" | ")
    );
}

#[cfg(not(target_os = "windows"))]
pub fn log_preview_window_diagnostics<R: tauri::Runtime>(_app: &AppHandle<R>, _reason: &str) {}

#[cfg(target_os = "windows")]
pub fn preview_window_is_focused<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    let Some(foreground_root) = foreground_root_hwnd() else {
        return false;
    };

    [IMAGE_PREVIEW_WINDOW_LABEL, TEXT_PREVIEW_WINDOW_LABEL]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|window| preview_window_matches_foreground_root(&window, foreground_root))
}

#[cfg(not(target_os = "windows"))]
pub fn preview_window_is_focused<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    [IMAGE_PREVIEW_WINDOW_LABEL, TEXT_PREVIEW_WINDOW_LABEL]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|window| tauri_window_reports_focus(&window))
}

pub fn show_image_preview<R: tauri::Runtime + 'static>(
    app: &AppHandle<R>,
    settings: &ClipboardSettings,
    item: &ClipboardItem,
    token: Option<u64>,
) -> Result<(), String> {
    let token = token.unwrap_or(0);
    eprintln!(
        "[clipboard-preview][request:image] id={} token={token} kind={} enabled={}",
        item.id,
        item.kind.as_sql(),
        settings.preview.image_enabled
    );
    if !promote_preview_token(token) {
        eprintln!(
            "[clipboard-preview][request:image] stale-token id={} token={token}",
            item.id
        );
        return Ok(());
    }

    if item.kind != ContentKind::Image {
        eprintln!(
            "[clipboard-preview][request:image] invalid-kind id={} actual={}",
            item.id,
            item.kind.as_sql()
        );
        return Err("clipboard item is not an image kind".to_string());
    }

    if !settings.preview.image_enabled {
        eprintln!(
            "[clipboard-preview][request:image] disabled id={} token={token}",
            item.id
        );
        clear_cached_preview_payloads();
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

    if !is_preview_token_current(token) {
        eprintln!(
            "[clipboard-preview][request:image] superseded-before-show id={} token={token}",
            item.id
        );
        return Ok(());
    }

    let desired_size = desired_image_preview_size(item);
    cache_image_preview_payload(payload);
    if let Err(error) = show_preview_window(
        app,
        settings,
        PreviewKind::Image,
        desired_size,
        token,
        &current_image_preview_payload()
            .ok_or_else(|| "clipboard image preview payload missing".to_string())?,
    ) {
        clear_cached_preview_payloads();
        return Err(error);
    }
    Ok(())
}

pub fn show_text_preview<R: tauri::Runtime + 'static>(
    app: &AppHandle<R>,
    settings: &ClipboardSettings,
    item: &ClipboardItem,
    token: Option<u64>,
) -> Result<(), String> {
    let token = token.unwrap_or(0);
    eprintln!(
        "[clipboard-preview][request:text] id={} token={token} kind={} enabled={}",
        item.id,
        item.kind.as_sql(),
        text_preview_enabled(settings)
    );
    if !promote_preview_token(token) {
        eprintln!(
            "[clipboard-preview][request:text] stale-token id={} token={token}",
            item.id
        );
        return Ok(());
    }

    if !matches!(
        item.kind,
        ContentKind::Text | ContentKind::Html | ContentKind::Rtf
    ) {
        eprintln!(
            "[clipboard-preview][request:text] invalid-kind id={} actual={}",
            item.id,
            item.kind.as_sql()
        );
        return Err("clipboard item is not text-previewable".to_string());
    }

    if !text_preview_enabled(settings) {
        eprintln!(
            "[clipboard-preview][request:text] disabled id={} token={token}",
            item.id
        );
        clear_cached_preview_payloads();
        hide_preview_windows(app);
        return Ok(());
    }

    let content = item
        .content_full
        .clone()
        .unwrap_or_else(|| item.content_preview.clone());
    if content.trim().is_empty() {
        eprintln!(
            "[clipboard-preview][request:text] empty-content id={} token={token}",
            item.id
        );
        hide_preview_windows(app);
        return Ok(());
    }

    let payload = TextPreviewPayload {
        id: item.id,
        kind: item.kind.as_sql(),
        content,
        source_app: item.source_app.clone(),
    };

    if !is_preview_token_current(token) {
        eprintln!(
            "[clipboard-preview][request:text] superseded-before-show id={} token={token}",
            item.id
        );
        return Ok(());
    }

    let desired_size = PreviewWindowSize {
        width: DEFAULT_TEXT_PREVIEW_WIDTH,
        height: DEFAULT_TEXT_PREVIEW_HEIGHT,
    };

    cache_text_preview_payload(payload);
    if let Err(error) = show_preview_window(
        app,
        settings,
        PreviewKind::Text,
        desired_size,
        token,
        &current_text_preview_payload()
            .ok_or_else(|| "clipboard text preview payload missing".to_string())?,
    ) {
        clear_cached_preview_payloads();
        return Err(error);
    }
    Ok(())
}

pub fn calculate_preview_placement(
    panel_rect: WindowRect,
    preview_size: PreviewWindowSize,
    monitor_rect: WindowRect,
    preference: ClipboardPreviewPosition,
) -> PreviewPlacement {
    let height = preview_size
        .height
        .min(monitor_rect.height)
        .max(MIN_PREVIEW_HEIGHT.min(monitor_rect.height));

    let left_available = side_available_width(panel_rect, monitor_rect, PreviewSide::Left);
    let right_available = side_available_width(panel_rect, monitor_rect, PreviewSide::Right);
    let preferred_side = match preference {
        ClipboardPreviewPosition::Left => PreviewSide::Left,
        ClipboardPreviewPosition::Right => PreviewSide::Right,
        ClipboardPreviewPosition::Auto => {
            if right_available >= left_available {
                PreviewSide::Right
            } else {
                PreviewSide::Left
            }
        }
    };

    let alternate_side = match preferred_side {
        PreviewSide::Left => PreviewSide::Right,
        PreviewSide::Right => PreviewSide::Left,
    };
    let preferred_available =
        available_width_for_side(preferred_side, left_available, right_available);
    let alternate_available =
        available_width_for_side(alternate_side, left_available, right_available);

    let side =
        if preferred_available >= MIN_PREVIEW_WIDTH || preferred_available >= alternate_available {
            preferred_side
        } else {
            alternate_side
        };
    let side_available = available_width_for_side(side, left_available, right_available);
    let width = preview_size.width.min(side_available.max(1));

    let x = match side {
        PreviewSide::Left => panel_rect.x - PREVIEW_GAP_PX - width as i32,
        PreviewSide::Right => panel_rect.right() + PREVIEW_GAP_PX,
    };
    let max_y = monitor_rect.bottom() - height as i32;

    PreviewPlacement {
        side,
        x,
        y: panel_rect
            .y
            .clamp(monitor_rect.y, max_y.max(monitor_rect.y)),
        width,
        height,
    }
}

fn available_width_for_side(side: PreviewSide, left: u32, right: u32) -> u32 {
    match side {
        PreviewSide::Left => left,
        PreviewSide::Right => right,
    }
}

fn side_available_width(
    panel_rect: WindowRect,
    monitor_rect: WindowRect,
    side: PreviewSide,
) -> u32 {
    let available = match side {
        PreviewSide::Left => panel_rect.x - monitor_rect.x - PREVIEW_GAP_PX,
        PreviewSide::Right => monitor_rect.right() - panel_rect.right() - PREVIEW_GAP_PX,
    };
    available.max(0) as u32
}

fn ensure_preview_window<R: tauri::Runtime, M: Manager<R>>(
    manager: &M,
    label: &str,
    route: &str,
    title: &str,
    size: PreviewWindowSize,
) -> tauri::Result<WebviewWindow<R>> {
    if let Some(window) = manager.get_webview_window(label) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(manager, label, WebviewUrl::App(route.into()))
        .title(title)
        .inner_size(size.width as f64, size.height as f64)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .focused(false)
        .visible(false)
        .build()
}

#[cfg(target_os = "windows")]
fn restack_preview_behind_panel<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
    panel: &WebviewWindow<R>,
) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    let hwnd = native_window_handle(window)?;
    let insert_after = panel
        .hwnd()
        .ok()
        .map(|hwnd| HWND(hwnd.0 as *mut _))
        .unwrap_or(HWND_TOPMOST);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn show_preview_without_focus<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
    panel: &WebviewWindow<R>,
) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())?;
    restack_preview_behind_panel(window, panel)
}

#[cfg(not(target_os = "windows"))]
fn show_preview_without_focus<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
    _panel: &WebviewWindow<R>,
) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())
}

fn desired_image_preview_size(item: &ClipboardItem) -> PreviewWindowSize {
    let content_width = item
        .image_width
        .unwrap_or(DEFAULT_IMAGE_PREVIEW_WIDTH.saturating_sub(IMAGE_PREVIEW_HORIZONTAL_CHROME_PX))
        .max(1);
    let content_height = item
        .image_height
        .unwrap_or(DEFAULT_IMAGE_PREVIEW_HEIGHT.saturating_sub(IMAGE_PREVIEW_VERTICAL_CHROME_PX))
        .max(1);
    let max_content_width = MAX_IMAGE_PREVIEW_WIDTH
        .saturating_sub(IMAGE_PREVIEW_HORIZONTAL_CHROME_PX)
        .max(1);
    let max_content_height = MAX_IMAGE_PREVIEW_HEIGHT
        .saturating_sub(IMAGE_PREVIEW_VERTICAL_CHROME_PX)
        .max(1);
    let width_scale = max_content_width as f64 / content_width as f64;
    let height_scale = max_content_height as f64 / content_height as f64;
    let scale = width_scale.min(height_scale).min(1.0);
    let width = ((content_width as f64 * scale).round() as u32)
        .max(1)
        .saturating_add(IMAGE_PREVIEW_HORIZONTAL_CHROME_PX)
        .clamp(MIN_PREVIEW_WIDTH, MAX_IMAGE_PREVIEW_WIDTH);
    let height = ((content_height as f64 * scale).round() as u32)
        .max(1)
        .saturating_add(IMAGE_PREVIEW_VERTICAL_CHROME_PX)
        .clamp(MIN_PREVIEW_HEIGHT, MAX_IMAGE_PREVIEW_HEIGHT);

    PreviewWindowSize { width, height }
}

fn text_preview_enabled(settings: &ClipboardSettings) -> bool {
    settings.enable_text_preview || settings.preview.text_enabled
}

fn preview_stage_error(target_label: &str, token: u64, stage: &str, error: &str) {
    eprintln!(
        "[clipboard-preview][prepare:{target_label}] error stage={stage} token={token} error={error}"
    );
}

fn preview_stage_ok(target_label: &str, token: u64, stage: &str) {
    eprintln!("[clipboard-preview][prepare:{target_label}] ok stage={stage} token={token}");
}

fn show_preview_window<R: tauri::Runtime + 'static, T: Serialize>(
    app: &AppHandle<R>,
    settings: &ClipboardSettings,
    kind: PreviewKind,
    desired_size: PreviewWindowSize,
    token: u64,
    payload: &T,
) -> Result<(), String> {
    if !is_preview_token_current(token) {
        eprintln!("[clipboard-preview][prepare:stale] label=pending token={token} stage=entry");
        return Ok(());
    }

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

    let (
        target_label,
        target_route,
        target_title,
        update_event,
        opposite_label,
        opposite_clear_event,
    ) = match kind {
        PreviewKind::Image => (
            IMAGE_PREVIEW_WINDOW_LABEL,
            IMAGE_PREVIEW_ROUTE,
            IMAGE_PREVIEW_TITLE,
            IMAGE_PREVIEW_UPDATE_EVENT,
            TEXT_PREVIEW_WINDOW_LABEL,
            TEXT_PREVIEW_CLEAR_EVENT,
        ),
        PreviewKind::Text => (
            TEXT_PREVIEW_WINDOW_LABEL,
            TEXT_PREVIEW_ROUTE,
            TEXT_PREVIEW_TITLE,
            TEXT_PREVIEW_UPDATE_EVENT,
            IMAGE_PREVIEW_WINDOW_LABEL,
            IMAGE_PREVIEW_CLEAR_EVENT,
        ),
    };

    eprintln!(
        "[clipboard-preview][prepare:{target_label}] token={token} panel={}x{}@{},{} monitor={}x{}@{},{} preview={}x{}@{},{} side={:?}",
        panel_rect.width,
        panel_rect.height,
        panel_rect.x,
        panel_rect.y,
        monitor_rect.width,
        monitor_rect.height,
        monitor_rect.x,
        monitor_rect.y,
        placement.width,
        placement.height,
        placement.x,
        placement.y,
        placement.side
    );

    if let Some(window) = app.get_webview_window(opposite_label) {
        let _ = window.emit(opposite_clear_event, ());
        let _ = window.hide();
    }

    if !is_preview_token_current(token) {
        eprintln!(
            "[clipboard-preview][prepare:{target_label}] stale token={token} stage=after-opposite-hide"
        );
        return Ok(());
    }

    let window = ensure_preview_window(app, target_label, target_route, target_title, desired_size)
        .map_err(|error| {
            let error = error.to_string();
            preview_stage_error(target_label, token, "ensure-window", &error);
            error
        })?;
    preview_stage_ok(target_label, token, "ensure-window");

    if !is_preview_token_current(token) {
        eprintln!(
            "[clipboard-preview][prepare:{target_label}] stale token={token} stage=after-ensure-window"
        );
        let _ = window.hide();
        return Ok(());
    }

    debug_window_snapshot(
        app,
        &format!("show-preview-window:{target_label}:before-resize"),
    );
    window
        .set_ignore_cursor_events(false)
        .map_err(|error| {
            let error = error.to_string();
            preview_stage_error(target_label, token, "set-ignore-cursor-events", &error);
            error
        })?;
    preview_stage_ok(target_label, token, "set-ignore-cursor-events");
    clear_preview_fullscreen_saved(target_label);
    window
        .set_size(Size::Physical(PhysicalSize::new(
            placement.width,
            placement.height,
        )))
        .map_err(|error| {
            let error = error.to_string();
            preview_stage_error(target_label, token, "set-size", &error);
            error
        })?;
    preview_stage_ok(target_label, token, "set-size");
    window
        .set_position(PhysicalPosition::new(placement.x, placement.y))
        .map_err(|error| {
            let error = error.to_string();
            preview_stage_error(target_label, token, "set-position", &error);
            error
        })?;
    preview_stage_ok(target_label, token, "set-position");
    window
        .emit(update_event, payload)
        .map_err(|error| {
            let error = error.to_string();
            preview_stage_error(target_label, token, "emit-update", &error);
            error
        })?;
    preview_stage_ok(target_label, token, "emit-update");

    if !is_preview_token_current(token) {
        eprintln!(
            "[clipboard-preview][prepare:{target_label}] stale token={token} stage=after-emit"
        );
        let _ = window.hide();
        return Ok(());
    }

    show_preview_without_focus(&window, &panel).map_err(|error| {
        preview_stage_error(target_label, token, "show-preview-without-focus", &error);
        error
    })?;
    preview_stage_ok(target_label, token, "show-preview-without-focus");
    debug_window_snapshot(
        app,
        &format!("show-preview-window:{target_label}:after-show"),
    );
    log_preview_window_diagnostics(app, &format!("show:{target_label}"));
    schedule_debug_snapshots(
        app,
        format!("show-preview-window:{target_label}:delayed-after-show"),
    );

    if !is_preview_token_current(token) {
        eprintln!(
            "[clipboard-preview][prepare:{target_label}] stale token={token} stage=after-show"
        );
        let _ = window.hide();
        return Ok(());
    }

    preview_stage_ok(target_label, token, "after-show");
    Ok(())
}

fn schedule_debug_snapshots<R: tauri::Runtime + 'static>(app: &AppHandle<R>, context: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for delay_ms in [50_u64, 300, 1000] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            debug_window_snapshot(&app, &format!("{context}:{delay_ms}ms"));
        }
    });
}

pub fn debug_window_snapshot<R: tauri::Runtime>(app: &AppHandle<R>, context: &str) {
    let mut summaries = Vec::new();
    for label in [
        "clipboard-panel",
        IMAGE_PREVIEW_WINDOW_LABEL,
        TEXT_PREVIEW_WINDOW_LABEL,
    ] {
        match app.get_webview_window(label) {
            Some(window) => {
                let visible = window.is_visible().unwrap_or(false);
                let focused = tauri_window_reports_focus(&window);
                let rect = window_rect(&window).ok();
                summaries.push(format!(
                    "{label} visible={visible} focused={focused} rect={rect:?}"
                ));
            }
            None => summaries.push(format!("{label} missing")),
        }
    }
    eprintln!("[clipboard-debug] {context} {}", summaries.join(" | "));
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

    fn preview_cache_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn reset_preview_token_for_test() {
        PREVIEW_REQUEST_TOKEN.store(0, Ordering::SeqCst);
    }

    fn image_item(width: u32, height: u32) -> ClipboardItem {
        ClipboardItem {
            id: 1,
            kind: ContentKind::Image,
            content_preview: String::new(),
            content_full: None,
            rtf_content: None,
            html: None,
            image_path: Some("C:/preview.png".into()),
            image_width: Some(width),
            image_height: Some(height),
            file_paths: None,
            byte_size: 0,
            char_count: 0,
            hash: "hash".into(),
            source_app: None,
            source_app_icon: None,
            from_self: false,
            group_id: None,
            is_favorite: false,
            is_pinned: false,
            favorite_sort_index: None,
            created_at: 0,
            updated_at: 0,
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

    #[test]
    fn calculate_preview_placement_shrinks_wide_preview_to_side_space_without_overlapping_panel() {
        let panel = rect(600, 160, 420, 720);
        let placement = calculate_preview_placement(
            panel,
            PreviewWindowSize {
                width: 960,
                height: 520,
            },
            rect(0, 0, 1920, 1080),
            crate::clipboard::models::ClipboardPreviewPosition::Auto,
        );

        assert_eq!(placement.side, PreviewSide::Right);
        assert_eq!(placement.x, panel.right() + PREVIEW_GAP_PX);
        assert!(placement.x >= panel.right() + PREVIEW_GAP_PX);
        assert!(placement.x + placement.width as i32 <= 1920);
    }

    #[test]
    fn desired_image_preview_size_is_not_overly_wide_for_large_images() {
        let size = desired_image_preview_size(&image_item(2600, 1600));

        assert!(
            size.width <= 720,
            "expected image preview width to stay within a comfortable side-preview cap, got {}",
            size.width
        );
    }

    #[test]
    fn preview_request_token_rejects_show_after_cancel() {
        let _guard = preview_cache_test_lock()
            .lock()
            .expect("preview cache test lock poisoned");
        reset_preview_token_for_test();

        assert!(promote_preview_token(1));
        assert!(is_preview_token_current(1));
        assert!(cancel_preview_token(1));
        assert!(!is_preview_token_current(1));
        assert!(!promote_preview_token(1));
        assert!(promote_preview_token(2));
    }

    #[test]
    fn older_preview_cancel_does_not_hide_newer_request() {
        let _guard = preview_cache_test_lock()
            .lock()
            .expect("preview cache test lock poisoned");
        reset_preview_token_for_test();

        assert!(promote_preview_token(2));
        assert!(!cancel_preview_token(1));
        assert!(is_preview_token_current(2));
    }

    #[test]
    fn cached_preview_payloads_can_be_retrieved_after_the_latest_update() {
        let _guard = preview_cache_test_lock()
            .lock()
            .expect("preview cache test lock poisoned");
        clear_cached_preview_payloads();

        cache_image_preview_payload(ImagePreviewPayload {
            id: 11,
            image_path: "C:/preview.png".into(),
            zoom_step: 20,
            source_app: Some("Explorer".into()),
        });
        cache_text_preview_payload(TextPreviewPayload {
            id: 12,
            kind: "text",
            content: "hello".into(),
            source_app: Some("Notepad".into()),
        });

        assert_eq!(
            current_image_preview_payload(),
            Some(ImagePreviewPayload {
                id: 11,
                image_path: "C:/preview.png".into(),
                zoom_step: 20,
                source_app: Some("Explorer".into()),
            }),
        );
        assert_eq!(
            current_text_preview_payload(),
            Some(TextPreviewPayload {
                id: 12,
                kind: "text",
                content: "hello".into(),
                source_app: Some("Notepad".into()),
            }),
        );
    }

    #[test]
    fn clearing_cached_preview_payloads_drops_both_preview_kinds() {
        let _guard = preview_cache_test_lock()
            .lock()
            .expect("preview cache test lock poisoned");
        cache_image_preview_payload(ImagePreviewPayload {
            id: 21,
            image_path: "C:/preview.png".into(),
            zoom_step: 15,
            source_app: None,
        });
        cache_text_preview_payload(TextPreviewPayload {
            id: 22,
            kind: "html",
            content: "<b>hello</b>".into(),
            source_app: None,
        });

        clear_cached_preview_payloads();

        assert_eq!(current_image_preview_payload(), None);
        assert_eq!(current_text_preview_payload(), None);
    }
}
