use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, GenericImageView, ImageFormat};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use uuid::Uuid;

const DATA_VERSION: u64 = 1;
const MAX_PAPERS: usize = 100;
const MAX_DATA_BYTES: usize = 32 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 4096;
const COMPRESS_IMAGE_DIMENSION: u32 = 2048;
const PAPER_WINDOW_PREFIX: &str = "paper-todo-";
const LAUNCHER_LABEL: &str = "paper-todo-launcher";
/// Collapsed width used until the webview reports the width its master capsule
/// actually needs. The capsule is sized to its label, so a fixed window width
/// would trail an empty strip behind `0 个` and clip `100 items`.
const LAUNCHER_COLLAPSED_WIDTH: u32 = 96;
const LAUNCHER_MIN_CAPSULE_WIDTH: u32 = 48;
const LAUNCHER_EXPANDED_WIDTH: u32 = 184;
const LAUNCHER_COLLAPSED_HEIGHT: u32 = 37;
const LAUNCHER_EXPANDED_HEIGHT: u32 = 360;
/// Logical width of the capsule's flat side parked past the display edge, so
/// its outline never draws a seam against the screen border.
const LAUNCHER_EDGE_OVERHANG: u32 = 8;
const LAUNCHER_CAPSULE_HEIGHT: u32 = 34;
/// Headroom added to the expanded window. Without it the reserved height is
/// exactly the sum of the rows, and the logical-to-physical rounding at
/// fractional DPI shaves the creation row's bottom border off.
const LAUNCHER_EXPANDED_SLACK: u32 = 4;
/// Physical pixels the cursor must travel before a press on the capsule counts
/// as a drag rather than an expand/collapse click.
const LAUNCHER_DRAG_THRESHOLD: i32 = 4;
const LAUNCHER_DRAG_TICK_MS: u64 = 8;
/// Ceiling on a single drag, so a missed button-up cannot pin a worker thread.
const LAUNCHER_DRAG_MAX_MS: u64 = 60_000;
const PAPER_MIN_WIDTH: f64 = 300.0;
const PAPER_MIN_HEIGHT: f64 = 220.0;
const PAPER_MAX_WIDTH: f64 = 900.0;
const PAPER_MAX_HEIGHT: f64 = 1000.0;
/// Foreground polling stays lazy: the fast cadence only runs while there is a
/// pinned paper that a fullscreen app could cover.
const FULLSCREEN_POLL_ACTIVE_MS: u64 = 750;
const FULLSCREEN_POLL_IDLE_MS: u64 = 3_000;
/// How often the edge launcher is pushed back into the topmost band. Another
/// app raising its own topmost window must not be able to hide it for longer
/// than this.
const LAUNCHER_TOPMOST_REFRESH_MS: u64 = 1_000;

#[derive(Default)]
pub struct PaperTodoRuntime {
    io_lock: Mutex<()>,
    hotkeys: Mutex<Vec<PaperHotkeyHandle>>,
    persistent_shell: Mutex<Option<Child>>,
    avoid_fullscreen: AtomicBool,
    fullscreen_active: AtomicBool,
    fullscreen_watch_started: AtomicBool,
    launcher_expanded: AtomicBool,
    launcher_item_count: AtomicUsize,
    /// Logical width the collapsed master capsule reported for its own label.
    /// Zero until the webview has measured itself.
    launcher_capsule_width: AtomicUsize,
}

impl Drop for PaperTodoRuntime {
    fn drop(&mut self) {
        if let Ok(shell) = self.persistent_shell.get_mut() {
            if let Some(child) = shell.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

struct PaperHotkeyHandle {
    app: AppHandle,
    shortcut: Shortcut,
    hotkey: String,
    action: &'static str,
}

impl PaperHotkeyHandle {
    fn unregister(self) {
        let _ = self.app.global_shortcut().unregister(self.shortcut);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperImageAsset {
    id: String,
    path: String,
    width: u32,
    height: u32,
    bytes: u64,
}

fn data_root(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| crate::config::get_data_dir(app))
        .join("paper_todo")
}

fn data_path(app: &AppHandle) -> PathBuf {
    data_root(app).join("data.json")
}

fn backup_path(app: &AppHandle) -> PathBuf {
    data_root(app).join("data.backup.json")
}

fn assets_path(app: &AppHandle) -> PathBuf {
    data_root(app).join("assets")
}

fn default_document() -> Value {
    let mut todo = create_paper_value("todo");
    let mut note = create_paper_value("note");
    // Default papers belong in the launcher but should not open two desktop
    // windows automatically on first run.
    todo["desktopOpen"] = json!(false);
    note["desktopOpen"] = json!(false);
    json!({
        "version": DATA_VERSION,
        "revision": 0,
        "papers": [todo, note],
        "settings": {
            "launcherEnabled": true,
            "launcherEdge": "right",
            "launcherOffset": 35,
            "hotkeys": {
                "showAll": "",
                "hideAll": "",
                "toggleAll": "Ctrl+Shift+Space",
                "newTodo": "Ctrl+Shift+T",
                "newNote": "Ctrl+Shift+N"
            }
        }
    })
}

fn is_valid_document(value: &Value) -> bool {
    value
        .get("papers")
        .and_then(Value::as_array)
        .map(|papers| papers.len() <= MAX_PAPERS)
        .unwrap_or(false)
        && value.get("settings").map(Value::is_object).unwrap_or(false)
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > MAX_DATA_BYTES {
        return None;
    }
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    is_valid_document(&value).then_some(value)
}

fn load_document_unlocked(app: &AppHandle) -> Value {
    if let Some(document) = read_json(&data_path(app)) {
        return document;
    }
    // The primary file is missing or corrupt. If the backup still holds a valid
    // document we recover from it, but first archive that backup under a
    // timestamped name so the next save cannot silently overwrite the only
    // surviving copy of the user's data.
    if let Some(document) = read_json(&backup_path(app)) {
        archive_recovery_source(app, &backup_path(app));
        return document;
    }
    default_document()
}

fn archive_recovery_source(app: &AppHandle, source: &Path) {
    if !source.exists() {
        return;
    }
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let archive = data_root(app).join(format!("data.recovery.{timestamp}.json"));
    if archive.exists() {
        return;
    }
    let _ = fs::copy(source, &archive);
}

fn write_document_unlocked(app: &AppHandle, document: &Value) -> Result<(), String> {
    if !is_valid_document(document) {
        return Err("便签数据格式无效".into());
    }
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_DATA_BYTES {
        return Err("便签数据超过 32 MB 限制，请拆分过长笔记或清理内容".into());
    }

    let path = data_path(app);
    let backup = backup_path(app);
    let temp = path.with_extension("tmp");
    fs::create_dir_all(data_root(app)).map_err(|error| error.to_string())?;

    // Only rotate the current primary file into the backup slot when it is a
    // valid document. Copying a corrupt primary over a good backup would
    // destroy the last recoverable snapshot.
    if path.exists() && read_json(&path).is_some() {
        fs::copy(&path, &backup).map_err(|error| format!("备份便签数据失败: {error}"))?;
    }

    let mut file = fs::File::create(&temp).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temp, &path).map_err(|error| error.to_string())?;
    Ok(())
}

fn next_revision(document: &mut Value) -> u64 {
    let revision = document
        .get("revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    document["revision"] = json!(revision);
    document["version"] = json!(DATA_VERSION);
    revision
}

fn persist_and_emit(
    app: &AppHandle,
    mut document: Value,
    paper_id: Option<&str>,
    source: Option<&str>,
) -> Result<u64, String> {
    let revision = next_revision(&mut document);
    write_document_unlocked(app, &document)?;
    // Carry the changed paper in the event so sibling windows can patch their
    // in-memory copy instead of each re-reading the whole document from disk on
    // every keystroke-triggered save. A missing snapshot for a known paperId
    // signals a removal; a null paperId means the listener should refresh.
    let paper_snapshot = paper_id.and_then(|id| {
        document
            .get("papers")
            .and_then(Value::as_array)
            .and_then(|papers| {
                papers
                    .iter()
                    .find(|paper| paper.get("id").and_then(Value::as_str) == Some(id))
                    .cloned()
            })
    });
    let _ = app.emit(
        "paper-todo-changed",
        json!({
            "revision": revision,
            "paperId": paper_id,
            "source": source,
            "paper": paper_snapshot,
        }),
    );
    Ok(revision)
}

#[tauri::command]
pub fn paper_todo_load(app: AppHandle, runtime: tauri::State<'_, PaperTodoRuntime>) -> Value {
    let _guard = runtime
        .io_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    load_document_unlocked(&app)
}

#[tauri::command]
pub fn paper_todo_save_paper(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
    paper: Value,
    source: String,
) -> Result<u64, String> {
    let id = paper
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "便签 ID 无效".to_string())?
        .to_string();
    if !paper.is_object() {
        return Err("便签数据无效".into());
    }

    let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
    let mut document = load_document_unlocked(&app);
    let papers = document["papers"]
        .as_array_mut()
        .ok_or_else(|| "便签列表无效".to_string())?;
    if let Some(index) = papers
        .iter()
        .position(|candidate| candidate.get("id").and_then(Value::as_str) == Some(id.as_str()))
    {
        papers[index] = paper;
    } else if papers.len() < MAX_PAPERS {
        papers.push(paper);
    } else {
        return Err(format!("便签数量不能超过 {MAX_PAPERS} 张"));
    }
    persist_and_emit(&app, document, Some(&id), Some(&source))
}

#[tauri::command]
pub fn paper_todo_delete_paper(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
    id: String,
    source: String,
) -> Result<u64, String> {
    let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
    let mut document = load_document_unlocked(&app);
    let papers = document["papers"]
        .as_array_mut()
        .ok_or_else(|| "便签列表无效".to_string())?;
    papers.retain(|paper| paper.get("id").and_then(Value::as_str) != Some(id.as_str()));
    persist_and_emit(&app, document, Some(&id), Some(&source))
}

#[tauri::command]
pub fn paper_todo_close_window(app: AppHandle, id: String) -> Result<(), String> {
    let label = format!("{PAPER_WINDOW_PREFIX}{}", safe_label(&id));
    if let Some(window) = app.get_webview_window(&label) {
        window.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn paper_todo_save_settings(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
    settings: Value,
    source: String,
) -> Result<u64, String> {
    if !settings.is_object() {
        return Err("便签设置无效".into());
    }
    let (revision, previous_settings) = {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        let mut document = load_document_unlocked(&app);
        let previous_settings = document["settings"].clone();
        document["settings"] = settings.clone();
        let revision = persist_and_emit(&app, document, None, Some(&source))?;
        (revision, previous_settings)
    };
    if let Err(error) = replace_hotkeys(&app, &runtime, &settings) {
        let _guard = runtime
            .io_lock
            .lock()
            .map_err(|lock_error| lock_error.to_string())?;
        let mut document = load_document_unlocked(&app);
        document["settings"] = previous_settings;
        let _ = persist_and_emit(&app, document, None, Some("shortcut-rollback"));
        return Err(error);
    }
    let avoid_fullscreen = settings
        .get("avoidFullscreen")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    runtime
        .avoid_fullscreen
        .store(avoid_fullscreen, Ordering::Relaxed);
    if !avoid_fullscreen && runtime.fullscreen_active.swap(false, Ordering::SeqCst) {
        apply_fullscreen_policy(&app, false);
    }
    ensure_launcher_window(&app, &settings)?;
    Ok(revision)
}

#[tauri::command]
pub fn paper_todo_save_order(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
    ids: Vec<String>,
    source: String,
) -> Result<u64, String> {
    let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
    let mut document = load_document_unlocked(&app);
    let current = document["papers"]
        .as_array_mut()
        .ok_or_else(|| "便签列表无效".to_string())?;
    let mut reordered = Vec::with_capacity(current.len());
    for id in ids {
        if let Some(index) = current
            .iter()
            .position(|paper| paper.get("id").and_then(Value::as_str) == Some(id.as_str()))
        {
            reordered.push(current.remove(index));
        }
    }
    reordered.append(current);
    document["papers"] = Value::Array(reordered);
    persist_and_emit(&app, document, None, Some(&source))
}

fn safe_label(id: &str) -> String {
    id.chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(64)
        .collect()
}

fn create_paper_value(kind: &str) -> Value {
    let now = chrono::Utc::now().timestamp_millis();
    json!({
        "id": Uuid::new_v4().to_string(),
        "kind": kind,
        "title": if kind == "note" { "笔记纸" } else { "待办纸" },
        "items": [],
        "content": "",
        "zoom": 100,
        "pinned": true,
        "hidden": false,
        "desktopOpen": true,
        "geometry": {
            "x": null,
            "y": null,
            "width": 380,
            "height": 520,
            "monitorName": null
        },
        "createdAt": now,
        "updatedAt": now
    })
}

fn create_and_open(app: &AppHandle, kind: &str) -> Result<(), String> {
    let runtime = app.state::<PaperTodoRuntime>();
    let (paper, settings) = {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        let mut document = load_document_unlocked(app);
        let paper = create_paper_value(kind);
        let papers = document["papers"]
            .as_array_mut()
            .ok_or_else(|| "便签列表无效".to_string())?;
        if papers.len() >= MAX_PAPERS {
            return Err(format!("便签数量不能超过 {MAX_PAPERS} 张"));
        }
        papers.push(paper.clone());
        let settings = document["settings"].clone();
        persist_and_emit(
            app,
            document,
            paper.get("id").and_then(Value::as_str),
            Some("native"),
        )?;
        (paper, settings)
    };
    open_window_internal(app, paper, settings)
}

pub fn dispatch_background(app: AppHandle, action: &'static str) {
    tauri::async_runtime::spawn_blocking(move || {
        let result = match action {
            "showAll" => set_all_windows_internal(&app, "show"),
            "hideAll" => set_all_windows_internal(&app, "hide"),
            "toggleAll" => set_all_windows_internal(&app, "toggle"),
            "newTodo" => create_and_open(&app, "todo"),
            "newNote" => create_and_open(&app, "note"),
            _ => Ok(()),
        };
        if let Err(error) = result {
            log::warn!("[paper-todo] background action {action} failed: {error}");
        }
    });
}

fn launcher_position(
    window: &tauri::WebviewWindow,
    settings: &Value,
    expanded: bool,
    logical_width: u32,
    logical_height: u32,
) -> Result<PhysicalPosition<i32>, String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let scale_factor = monitor.scale_factor();
    let window_width = (logical_width as f64 * scale_factor).round() as i32;
    let window_height = (logical_height as f64 * scale_factor).round() as i32;
    // Only the capsule's flat side is parked outside the display, so whatever
    // width the window carries stays visible apart from that overhang.
    let overhang = (LAUNCHER_EDGE_OVERHANG as f64 * scale_factor).round() as i32;
    let visible_width = (window_width - overhang).max(1);
    let edge = settings
        .get("launcherEdge")
        .and_then(Value::as_str)
        .unwrap_or("right");
    let offset = settings
        .get("launcherOffset")
        .and_then(Value::as_i64)
        .unwrap_or(35)
        .clamp(0, 100) as i32;
    // `launcherOffset` identifies the collapsed master capsule's top edge. Do
    // not reinterpret that percentage against the expanded window height: it
    // would move the capsule upward during every expansion, synthesize a
    // mouseleave under a stationary cursor, and immediately arm auto-collapse.
    let collapsed_height = (LAUNCHER_COLLAPSED_HEIGHT as f64 * scale_factor).round() as i32;
    let collapsed_available_height = monitor_size.height as i32 - collapsed_height;
    let anchored_y = monitor_position.y + collapsed_available_height.max(0) * offset / 100;
    let max_y = monitor_position.y + (monitor_size.height as i32 - window_height).max(0);
    let y = anchored_y.min(max_y);
    let x = if edge == "left" {
        if expanded {
            monitor_position.x
        } else {
            monitor_position.x - (window_width - visible_width)
        }
    } else if expanded {
        monitor_position.x + monitor_size.width as i32 - window_width
    } else {
        monitor_position.x + monitor_size.width as i32 - visible_width
    };
    Ok(PhysicalPosition::new(x, y))
}

/// Collapsed window width: the capsule's own measured width plus the overhang
/// that hides its flat side past the display edge.
fn collapsed_launcher_width(app: &AppHandle) -> u32 {
    let measured = app
        .state::<PaperTodoRuntime>()
        .launcher_capsule_width
        .load(Ordering::Relaxed) as u32;
    let capsule = if measured == 0 {
        LAUNCHER_COLLAPSED_WIDTH.saturating_sub(LAUNCHER_EDGE_OVERHANG)
    } else {
        measured
    };
    capsule.clamp(
        LAUNCHER_MIN_CAPSULE_WIDTH,
        LAUNCHER_EXPANDED_WIDTH - LAUNCHER_EDGE_OVERHANG,
    ) + LAUNCHER_EDGE_OVERHANG
}

fn sync_launcher_window(app: &AppHandle, settings: &Value) -> Result<(), String> {
    let enabled = settings
        .get("launcherEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let Some(window) = app.get_webview_window(LAUNCHER_LABEL) else {
        return Ok(());
    };
    if !enabled {
        window.hide().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let expanded = app
        .state::<PaperTodoRuntime>()
        .launcher_expanded
        .load(Ordering::Relaxed);
    let item_count = app
        .state::<PaperTodoRuntime>()
        .launcher_item_count
        .load(Ordering::Relaxed);
    let height = if expanded {
        LAUNCHER_COLLAPSED_HEIGHT
            .saturating_add((item_count as u32).saturating_mul(LAUNCHER_CAPSULE_HEIGHT))
            .saturating_add(LAUNCHER_EXPANDED_SLACK)
            .min(LAUNCHER_EXPANDED_HEIGHT)
    } else {
        LAUNCHER_COLLAPSED_HEIGHT
    };
    let width = if expanded {
        LAUNCHER_EXPANDED_WIDTH
    } else {
        collapsed_launcher_width(app)
    };
    window
        .set_size(tauri::LogicalSize::new(width as f64, height as f64))
        .map_err(|error| error.to_string())?;
    window
        .set_position(launcher_position(
            &window, settings, expanded, width, height,
        )?)
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    Ok(())
}

fn ensure_launcher_window(app: &AppHandle, settings: &Value) -> Result<(), String> {
    let created = app.get_webview_window(LAUNCHER_LABEL).is_none();
    if created {
        WebviewWindowBuilder::new(
            app,
            LAUNCHER_LABEL,
            WebviewUrl::App("index.html#/paper-todo/launcher".into()),
        )
        .title("PaperTodo 便签")
        .inner_size(
            LAUNCHER_COLLAPSED_WIDTH as f64,
            LAUNCHER_COLLAPSED_HEIGHT as f64,
        )
        .min_inner_size(
            (LAUNCHER_MIN_CAPSULE_WIDTH + LAUNCHER_EDGE_OVERHANG) as f64,
            LAUNCHER_COLLAPSED_HEIGHT as f64,
        )
        .max_inner_size(
            LAUNCHER_EXPANDED_WIDTH as f64,
            LAUNCHER_EXPANDED_HEIGHT as f64,
        )
        .decorations(false)
        .resizable(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    }
    sync_launcher_window(app, settings)?;
    if created {
        // Windows can apply its minimum tracking width after the builder's
        // first size request. Re-assert the collapsed geometry once the HWND
        // has settled so the launcher is correct before its first click.
        let app = app.clone();
        let settings = settings.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
            if let Err(error) = sync_launcher_window(&app, &settings) {
                log::warn!("[paper-todo] delayed launcher size sync failed: {error}");
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn paper_todo_create_paper(app: AppHandle, kind: String) -> Result<(), String> {
    if kind != "todo" && kind != "note" {
        return Err("未知纸片类型".into());
    }
    tauri::async_runtime::spawn_blocking(move || create_and_open(&app, &kind))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn paper_todo_set_launcher_expanded(
    app: AppHandle,
    expanded: bool,
    item_count: usize,
    capsule_width: Option<f64>,
) -> Result<(), String> {
    let runtime = app.state::<PaperTodoRuntime>();
    runtime.launcher_expanded.store(expanded, Ordering::Relaxed);
    runtime
        .launcher_item_count
        .store(item_count.min(MAX_PAPERS), Ordering::Relaxed);
    // Only a collapsed capsule can measure the label the collapsed window has
    // to fit; while expanded it reads `收起` instead, so the webview sends no
    // width and the last collapsed measurement stands.
    if let Some(width) = capsule_width.filter(|width| width.is_finite() && *width > 0.0) {
        runtime
            .launcher_capsule_width
            .store(width.ceil() as usize, Ordering::Relaxed);
    }
    let document = {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        load_document_unlocked(&app)
    };
    sync_launcher_window(&app, &document["settings"])
}

/// Drag the edge launcher along the display edge it is parked on.
///
/// The capsule is the drag handle now that it has no grip icon, and the whole
/// travel has to stay on the primary monitor's side. `startDragging` cannot do
/// that: it hands the window to the OS drag loop, which moves it freely in two
/// axes and only lets us snap it back after the button is released. Reading the
/// cursor directly keeps `x` pinned and `y` clamped for every frame of the
/// drag, and it survives the pointer leaving the 60 px wide capsule — which
/// webview mouse events would not.
///
/// Returns whether the press ever became a drag, so a press that never moved
/// can be treated as the expand/collapse click.
#[cfg(target_os = "windows")]
fn run_launcher_drag(app: &AppHandle) -> Result<bool, String> {
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let window = app
        .get_webview_window(LAUNCHER_LABEL)
        .ok_or_else(|| "边缘入口窗口不存在".to_string())?;
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let origin = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let min_y = monitor.position().y;
    let max_y = min_y + (monitor.size().height as i32 - size.height as i32).max(0);

    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.map_err(|error| error.to_string())?;
    let start_cursor_y = cursor.y;
    let deadline = Instant::now() + Duration::from_millis(LAUNCHER_DRAG_MAX_MS);
    let mut moved = false;
    let mut last_y = origin.y;
    while Instant::now() < deadline {
        let pressed = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000;
        if pressed == 0 {
            break;
        }
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            break;
        }
        let travel = cursor.y - start_cursor_y;
        if moved || travel.abs() >= LAUNCHER_DRAG_THRESHOLD {
            moved = true;
            let target = (origin.y + travel).clamp(min_y, max_y);
            if target != last_y {
                last_y = target;
                let _ = window.set_position(PhysicalPosition::new(origin.x, target));
            }
        }
        std::thread::sleep(Duration::from_millis(LAUNCHER_DRAG_TICK_MS));
    }
    Ok(moved)
}

#[cfg(not(target_os = "windows"))]
fn run_launcher_drag(_app: &AppHandle) -> Result<bool, String> {
    Ok(false)
}

#[tauri::command]
pub async fn paper_todo_drag_launcher(app: AppHandle) -> Result<bool, String> {
    // The loop polls the cursor and the persistence that follows it touches the
    // data file; neither belongs on the WebView callback thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<bool, String> {
        let moved = run_launcher_drag(&app)?;
        if moved {
            save_launcher_offset(&app)?;
        }
        Ok(moved)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Persist where the drag left the launcher, as a percentage of the travel its
/// edge allows, and re-snap the window onto that anchor.
fn save_launcher_offset(app: &AppHandle) -> Result<i64, String> {
    let runtime = app.state::<PaperTodoRuntime>();
    let window = app
        .get_webview_window(LAUNCHER_LABEL)
        .ok_or_else(|| "边缘入口窗口不存在".to_string())?;
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let window_size = window.outer_size().map_err(|error| error.to_string())?;
    let available_height = monitor.size().height.saturating_sub(window_size.height) as i64;
    let relative_y = i64::from(position.y - monitor.position().y).clamp(0, available_height);
    let offset = if available_height == 0 {
        0
    } else {
        ((relative_y * 100 + available_height / 2) / available_height).clamp(0, 100)
    };

    let settings = {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        let mut document = load_document_unlocked(app);
        document["settings"]["launcherOffset"] = json!(offset);
        let settings = document["settings"].clone();
        persist_and_emit(app, document, None, Some("launcher-drag"))?;
        settings
    };
    sync_launcher_window(app, &settings)?;
    Ok(offset)
}

#[tauri::command]
pub fn paper_todo_open_settings(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().map_err(|error| error.to_string())?;
    }
    window
        .eval("window.location.hash = '#/tools/paper-todo'")
        .map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn register_hotkey(
    app: &AppHandle,
    hotkey: &str,
    action: &'static str,
) -> Result<PaperHotkeyHandle, String> {
    let shortcut =
        Shortcut::from_str(hotkey).map_err(|error| format!("快捷键 {hotkey} 格式无效: {error}"))?;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            dispatch_background(app.clone(), action);
        })
        .map_err(|error| format!("注册快捷键 {hotkey} 失败: {error}"))?;
    Ok(PaperHotkeyHandle {
        app: app.clone(),
        shortcut,
        hotkey: hotkey.to_string(),
        action,
    })
}

fn replace_hotkeys(
    app: &AppHandle,
    runtime: &PaperTodoRuntime,
    settings: &Value,
) -> Result<(), String> {
    let hotkeys = settings.get("hotkeys").and_then(Value::as_object);
    let mut desired: Vec<(String, &'static str)> = Vec::new();
    if let Some(hotkeys) = hotkeys {
        for action in ["showAll", "hideAll", "toggleAll", "newTodo", "newNote"] {
            let hotkey = hotkeys
                .get(action)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if hotkey.is_empty() {
                continue;
            }
            let _ = Shortcut::from_str(hotkey)
                .map_err(|error| format!("快捷键 {hotkey} 格式无效: {error}"))?;
            desired.push((hotkey.to_string(), action));
        }
    }

    let mut current = runtime.hotkeys.lock().map_err(|error| error.to_string())?;
    let previous: Vec<(String, &'static str)> = current
        .iter()
        .map(|handle| (handle.hotkey.clone(), handle.action))
        .collect();
    if previous == desired {
        return Ok(());
    }
    for handle in current.drain(..) {
        handle.unregister();
    }

    let mut next = Vec::new();
    for (hotkey, action) in &desired {
        match register_hotkey(app, hotkey, action) {
            Ok(handle) => next.push(handle),
            Err(error) => {
                for handle in next {
                    handle.unregister();
                }
                *current = previous
                    .iter()
                    .filter_map(|(old_hotkey, old_action)| {
                        register_hotkey(app, old_hotkey, old_action).ok()
                    })
                    .collect();
                return Err(error);
            }
        }
    }
    *current = next;
    Ok(())
}

#[cfg(target_os = "windows")]
fn foreground_is_fullscreen() -> bool {
    use std::mem::size_of;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
        GetWindowThreadProcessId,
    };

    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return false;
        }
        // The desktop, shell, and taskbar windows all span a full monitor but
        // are not real fullscreen apps; treating them as fullscreen would drop
        // pinned papers whenever the user clicks the desktop. Exclude them the
        // same way PaperTodo's FullscreenForegroundWindowDetector does.
        if window == GetShellWindow() || window == GetDesktopWindow() {
            return false;
        }
        let mut class_buffer = [0u16; 256];
        let class_length = GetClassNameW(window, &mut class_buffer);
        if class_length > 0 {
            let class_name = String::from_utf16_lossy(&class_buffer[..class_length as usize]);
            if matches!(
                class_name.as_str(),
                "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
            ) {
                return false;
            }
        }
        // Our own paper windows can be maximized/borderless; never let them
        // trigger the fullscreen avoidance policy against themselves.
        let mut process_id = 0u32;
        GetWindowThreadProcessId(window, Some(&mut process_id));
        if process_id == GetCurrentProcessId() {
            return false;
        }
        let mut rect = RECT::default();
        if GetWindowRect(window, &mut rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let target = info.rcMonitor;
        let tolerance = 2;
        (rect.left - target.left).abs() <= tolerance
            && (rect.top - target.top).abs() <= tolerance
            && (rect.right - target.right).abs() <= tolerance
            && (rect.bottom - target.bottom).abs() <= tolerance
    }
}

#[cfg(not(target_os = "windows"))]
fn foreground_is_fullscreen() -> bool {
    false
}

/// Push a window back into the topmost band without touching focus or z-order
/// among other topmost windows' owners.
///
/// `always_on_top` only sets `HWND_TOPMOST` once. Windows orders topmost
/// windows among themselves by activation, so any other app that raises its own
/// topmost window — installers, media players, IME candidate lists, several
/// conferencing tools — ends up drawn above the edge launcher and hides it for
/// the rest of the session. Re-asserting it periodically is what keeps the
/// capsule reachable.
#[cfg(target_os = "windows")]
fn reassert_topmost(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    };

    let Ok(handle) = window.hwnd() else {
        return;
    };
    unsafe {
        let _ = SetWindowPos(
            HWND(handle.0 as *mut _),
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn reassert_topmost(_window: &tauri::WebviewWindow) {}

/// Keep the edge launcher, and every pinned paper, in front of whatever was
/// raised since the last tick. Skipped while a fullscreen app owns the
/// foreground so the existing avoid-fullscreen policy still wins.
///
/// Returns whether the launcher is on screen, so the caller can hold a cadence
/// that keeps it reachable.
fn reassert_paper_topmost(app: &AppHandle, fullscreen: bool) -> bool {
    if fullscreen {
        return false;
    }
    let mut launcher_visible = false;
    if let Some(window) = app.get_webview_window(LAUNCHER_LABEL) {
        if window.is_visible().unwrap_or(false) {
            launcher_visible = true;
            reassert_topmost(&window);
        }
    }
    for (label, window) in app.webview_windows() {
        if !label.starts_with(PAPER_WINDOW_PREFIX)
            || label == LAUNCHER_LABEL
            || !window.is_always_on_top().unwrap_or(false)
        {
            continue;
        }
        reassert_topmost(&window);
    }
    launcher_visible
}

fn has_paper_windows(app: &AppHandle) -> bool {
    app.webview_windows()
        .keys()
        .any(|label| label.starts_with(PAPER_WINDOW_PREFIX) && label.as_str() != LAUNCHER_LABEL)
}

fn apply_fullscreen_policy(app: &AppHandle, fullscreen: bool) {
    let runtime = app.state::<PaperTodoRuntime>();
    let document = {
        let _guard = match runtime.io_lock.lock() {
            Ok(guard) => guard,
            Err(error) => error.into_inner(),
        };
        load_document_unlocked(app)
    };
    for paper in document["papers"].as_array().into_iter().flatten() {
        let Some(id) = paper.get("id").and_then(Value::as_str) else {
            continue;
        };
        let pinned = paper.get("pinned").and_then(Value::as_bool).unwrap_or(true);
        if let Some(window) =
            app.get_webview_window(&format!("{PAPER_WINDOW_PREFIX}{}", safe_label(id)))
        {
            let _ = window.set_always_on_top(pinned && !fullscreen);
        }
    }
}

fn start_fullscreen_watch(app: &AppHandle) {
    let runtime = app.state::<PaperTodoRuntime>();
    if runtime
        .fullscreen_watch_started
        .swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = FULLSCREEN_POLL_IDLE_MS;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
            let runtime = app.state::<PaperTodoRuntime>();
            // Skip the per-tick foreground probe entirely when the policy is
            // disabled or there is no paper window that could need protection,
            // and back the cadence off so an unused feature costs nearly
            // nothing over a long-running session.
            if !runtime.avoid_fullscreen.load(Ordering::Relaxed) || !has_paper_windows(&app) {
                interval = FULLSCREEN_POLL_IDLE_MS;
                if runtime.fullscreen_active.swap(false, Ordering::SeqCst) {
                    apply_fullscreen_policy(&app, false);
                }
                // The launcher outlives every paper, so its topmost band still
                // needs refreshing on the idle path, and at a cadence the user
                // does not perceive as the capsule going missing.
                if reassert_paper_topmost(&app, false) {
                    interval = LAUNCHER_TOPMOST_REFRESH_MS;
                }
                continue;
            }
            interval = FULLSCREEN_POLL_ACTIVE_MS;
            let fullscreen = foreground_is_fullscreen();
            let previous = runtime.fullscreen_active.swap(fullscreen, Ordering::SeqCst);
            if previous != fullscreen {
                apply_fullscreen_policy(&app, fullscreen);
            }
            let _ = reassert_paper_topmost(&app, fullscreen);
        }
    });
}

pub fn initialize(app: &AppHandle) {
    let runtime = app.state::<PaperTodoRuntime>();
    let document = {
        let _guard = match runtime.io_lock.lock() {
            Ok(guard) => guard,
            Err(error) => error.into_inner(),
        };
        load_document_unlocked(app)
    };
    runtime.avoid_fullscreen.store(
        document["settings"]
            .get("avoidFullscreen")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        Ordering::Relaxed,
    );
    start_fullscreen_watch(app);
    if let Err(error) = replace_hotkeys(app, &runtime, &document["settings"]) {
        log::warn!("[paper-todo] failed to restore global shortcuts: {error}");
    }
    let settings = document["settings"].clone();
    if let Err(error) = ensure_launcher_window(app, &settings) {
        log::warn!("[paper-todo] failed to create edge launcher: {error}");
    }
    if let Some(papers) = document["papers"].as_array() {
        for paper in papers {
            let should_restore = paper
                .get("desktopOpen")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && !paper
                    .get("hidden")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            if should_restore {
                if let Err(error) = open_window_internal(app, paper.clone(), settings.clone()) {
                    log::warn!("[paper-todo] failed to restore paper window: {error}");
                }
            }
        }
    }
}

pub fn handle_startup_args(app: &AppHandle, args: &[String]) -> bool {
    let mut handled = false;
    for argument in args {
        let command = argument.trim_start_matches('-').to_ascii_lowercase();
        let action = match command.as_str() {
            "show" | "open" => Some("showAll"),
            "hide" => Some("hideAll"),
            "toggle" => Some("toggleAll"),
            "new-todo" | "todo" => Some("newTodo"),
            "new-note" | "note" => Some("newNote"),
            "exit" | "quit" => {
                app.exit(0);
                handled = true;
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            handled = true;
            dispatch_background(app.clone(), action);
        }
    }
    handled
}

#[tauri::command]
pub async fn paper_todo_open_window(
    app: AppHandle,
    paper: Value,
    settings: Value,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_window_internal(&app, paper, settings))
        .await
        .map_err(|error| error.to_string())?
}

fn open_window_internal(app: &AppHandle, paper: Value, settings: Value) -> Result<(), String> {
    let id = paper
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "便签 ID 无效".to_string())?;
    let safe_id = safe_label(id);
    if safe_id.is_empty() {
        return Err("便签 ID 无效".into());
    }
    let label = format!("{PAPER_WINDOW_PREFIX}{safe_id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let pinned = paper.get("pinned").and_then(Value::as_bool).unwrap_or(true);
    let geometry = paper.get("geometry").and_then(Value::as_object);
    let width = geometry
        .and_then(|value| value.get("width"))
        .and_then(Value::as_f64)
        .unwrap_or(380.0)
        .clamp(PAPER_MIN_WIDTH, PAPER_MAX_WIDTH);
    let height = geometry
        .and_then(|value| value.get("height"))
        .and_then(Value::as_f64)
        .unwrap_or(520.0)
        .clamp(PAPER_MIN_HEIGHT, PAPER_MAX_HEIGHT);
    let skip_taskbar = settings
        .get("hideFromTaskbar")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let route = format!("index.html#/paper-todo/window/{safe_id}");
    let paper_window_index = app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with(PAPER_WINDOW_PREFIX) && label.as_str() != LAUNCHER_LABEL)
        .count();
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(route.into()))
        .title(
            paper
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("PaperTodo 便签"),
        )
        .inner_size(width, height)
        .min_inner_size(PAPER_MIN_WIDTH, PAPER_MIN_HEIGHT)
        .decorations(false)
        .resizable(true)
        .transparent(true)
        // The paper surface draws its own rounded silhouette. Native shadows
        // on a transparent borderless HWND show up as dark seams on three
        // sides, especially at fractional display scaling.
        .shadow(false)
        .skip_taskbar(skip_taskbar)
        .always_on_top(
            pinned
                && !app
                    .state::<PaperTodoRuntime>()
                    .fullscreen_active
                    .load(Ordering::Relaxed),
        )
        // Keep the borderless transparent window hidden until its Vue route has
        // loaded the paper document. Showing it immediately exposes WebView's
        // default white canvas when initialization is still in flight.
        .visible(false);

    let saved_position = (
        geometry
            .and_then(|value| value.get("x"))
            .and_then(Value::as_f64),
        geometry
            .and_then(|value| value.get("y"))
            .and_then(Value::as_f64),
    );
    if let (Some(x), Some(y)) = saved_position {
        builder = builder.position(x, y);
    }
    let window = builder.build().map_err(|error| error.to_string())?;
    // Reposition when there is no saved spot, and also when the saved spot no
    // longer lands on a connected monitor. A paper last placed on a secondary
    // display that has since been unplugged would otherwise be restored to
    // coordinates no screen covers: the window is genuinely "shown" and still
    // counted by the launcher badge, but the user can never see it.
    let needs_placement =
        saved_position.0.is_none() || saved_position.1.is_none() || !window_is_on_screen(&window);
    if needs_placement {
        // Best-effort: the window already exists and its Vue route will reveal
        // it regardless. Failing here would report the paper as unopened while
        // leaving a live window behind, so fall back to the OS default spot.
        let _ = position_new_paper_on_primary(&window, width, height, paper_window_index);
    }
    Ok(())
}

/// Whether a meaningful part of the window rect overlaps a connected monitor.
/// Requires more than a hairline so a window parked one pixel inside the screen
/// edge still counts as lost.
fn window_is_on_screen(window: &tauri::WebviewWindow) -> bool {
    const MIN_VISIBLE: i32 = 48;
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        // Unable to verify: assume the saved spot is fine rather than yanking
        // a correctly placed paper back to the primary monitor.
        return true;
    };
    let Ok(monitors) = window.available_monitors() else {
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    let screens: Vec<Rect> = monitors
        .iter()
        .map(|monitor| {
            let origin = monitor.position();
            let extent = monitor.size();
            Rect::new(
                origin.x,
                origin.y,
                extent.width as i32,
                extent.height as i32,
            )
        })
        .collect();
    let rect = Rect::new(
        position.x,
        position.y,
        size.width as i32,
        size.height as i32,
    );
    rect_is_on_any_screen(rect, &screens, MIN_VISIBLE)
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Rect {
    fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Whether `rect` overlaps any screen by at least `min_visible` on both axes.
/// Windows smaller than that threshold only need to be fully covered.
fn rect_is_on_any_screen(rect: Rect, screens: &[Rect], min_visible: i32) -> bool {
    let need_x = min_visible.min(rect.width);
    let need_y = min_visible.min(rect.height);
    screens.iter().any(|screen| {
        let overlap_x = (rect.x + rect.width).min(screen.x + screen.width) - rect.x.max(screen.x);
        let overlap_y = (rect.y + rect.height).min(screen.y + screen.height) - rect.y.max(screen.y);
        overlap_x >= need_x && overlap_y >= need_y
    })
}

fn position_new_paper_on_primary(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
    cascade_index: usize,
) -> Result<(), String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let scale = monitor.scale_factor();
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let window_width = (width * scale).round() as i32;
    let window_height = (height * scale).round() as i32;
    let inset = (24.0 * scale).round() as i32;
    let cascade = ((cascade_index % 8) as f64 * 28.0 * scale).round() as i32;
    let max_x = monitor_position.x + monitor_size.width as i32 - window_width - inset;
    let max_y = monitor_position.y + monitor_size.height as i32 - window_height - inset;
    let x = (monitor_position.x + inset + cascade)
        .clamp(monitor_position.x, max_x.max(monitor_position.x));
    let y = (monitor_position.y + inset + cascade)
        .clamp(monitor_position.y, max_y.max(monitor_position.y));
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn paper_todo_set_window_pinned(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
    id: String,
    pinned: bool,
) -> Result<(), String> {
    let label = format!("{PAPER_WINDOW_PREFIX}{}", safe_label(&id));
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "便签窗口不存在".to_string())?;
    window
        .set_always_on_top(pinned && !runtime.fullscreen_active.load(Ordering::Relaxed))
        .map_err(|error| error.to_string())
}

/// Move a window back inside its current monitor without changing its size.
/// This also rescues papers restored from a display that was disconnected.
fn pull_window_into_monitor(window: &tauri::WebviewWindow) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let origin = monitor.position();
    let extent = monitor.size();
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let current = window.outer_position().map_err(|error| error.to_string())?;
    let max_x = origin.x + extent.width as i32 - size.width as i32;
    let max_y = origin.y + extent.height as i32 - size.height as i32;
    let x = current.x.clamp(origin.x, max_x.max(origin.x));
    let y = current.y.clamp(origin.y, max_y.max(origin.y));
    if x == current.x && y == current.y {
        return Ok(());
    }
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn paper_todo_set_all_windows(app: AppHandle, action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || set_all_windows_internal(&app, &action))
        .await
        .map_err(|error| error.to_string())?
}

fn set_all_windows_internal(app: &AppHandle, action: &str) -> Result<(), String> {
    let windows: Vec<_> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| {
            label.starts_with(PAPER_WINDOW_PREFIX) && label.as_str() != LAUNCHER_LABEL
        })
        .map(|(_, window)| window)
        .collect();
    let should_show = match action {
        "show" => true,
        "hide" => false,
        // Match PaperTodo's show/hide contract: these commands change native
        // visibility without changing whether a paper is part of the desktop
        // set or destroying its editor state.
        "toggle" => !windows
            .iter()
            .any(|window| window.is_visible().unwrap_or(false)),
        _ => return Err("未知窗口操作".into()),
    };
    if should_show {
        let runtime = app.state::<PaperTodoRuntime>();
        let (papers, settings) = {
            let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
            let mut document = load_document_unlocked(app);
            let (papers, changed) = prepare_papers_for_show_all(&mut document)?;
            let settings = document["settings"].clone();
            if changed {
                persist_and_emit(app, document, None, Some("show-all"))?;
            }
            (papers, settings)
        };
        // "Show all" follows PaperTodo's user-facing meaning: every saved
        // paper is restored, including papers whose individual window was
        // closed earlier. Existing windows are shown immediately; newly built
        // windows reveal themselves after their Vue route has loaded.
        //
        // One unopenable paper must not strand the rest: a single malformed id
        // or a transient window-build failure previously aborted the whole
        // command, so "show all" appeared to do nothing even though the badge
        // still counted every saved paper. Keep going and only report failure
        // when nothing could be restored at all.
        let total = papers.len();
        let mut failures: Vec<String> = Vec::new();
        for paper in papers {
            if let Err(error) = open_window_internal(app, paper, settings.clone()) {
                failures.push(error);
            }
        }
        if total > 0 && failures.len() == total {
            return Err(format!("无法显示任何便签: {}", failures.join("; ")));
        }
    }
    for window in windows {
        if should_show {
            let _ = window.show();
            // "Show all" has to mean visible even after a display was removed
            // or its work area changed while the paper was closed.
            let _ = pull_window_into_monitor(&window);
        } else {
            let _ = window.hide();
        }
    }
    Ok(())
}

fn prepare_papers_for_show_all(document: &mut Value) -> Result<(Vec<Value>, bool), String> {
    let papers = document["papers"]
        .as_array_mut()
        .ok_or_else(|| "便签列表无效".to_string())?;
    let mut changed = false;
    for paper in papers.iter_mut() {
        if !paper
            .get("desktopOpen")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            paper["desktopOpen"] = json!(true);
            changed = true;
        }
        if paper
            .get("hidden")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            paper["hidden"] = json!(false);
            changed = true;
        }
    }
    Ok((papers.clone(), changed))
}

fn persist_image_bytes(
    app: &AppHandle,
    bytes: &[u8],
    extension: &str,
    width: u32,
    height: u32,
) -> Result<PaperImageAsset, String> {
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("图片压缩后仍超过 8 MB".into());
    }
    let id = Uuid::new_v4().simple().to_string();
    let target = assets_path(app).join(format!("{id}.{extension}"));
    fs::create_dir_all(assets_path(app)).map_err(|error| error.to_string())?;
    fs::write(&target, bytes).map_err(|error| format!("保存图片失败: {error}"))?;
    Ok(PaperImageAsset {
        id,
        path: target.to_string_lossy().to_string(),
        width,
        height,
        bytes: bytes.len() as u64,
    })
}

fn encode_dynamic_image(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut buffer, format)
        .map_err(|error| format!("编码图片失败: {error}"))?;
    Ok(buffer.into_inner())
}

fn store_import_bytes(
    app: &AppHandle,
    original: Vec<u8>,
    original_format: Option<ImageFormat>,
    auto_compress: bool,
) -> Result<PaperImageAsset, String> {
    let image =
        image::load_from_memory(&original).map_err(|error| format!("读取图片失败: {error}"))?;
    let (width, height) = image.dimensions();
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(format!(
            "图片尺寸不能超过 {MAX_IMAGE_DIMENSION} x {MAX_IMAGE_DIMENSION}"
        ));
    }
    let needs_downscale =
        auto_compress && (width > COMPRESS_IMAGE_DIMENSION || height > COMPRESS_IMAGE_DIMENSION);
    // When the source is already a compressed web format and does not need
    // resizing, store the original bytes verbatim. Re-encoding a JPEG photo as
    // PNG previously multiplied its size several times over.
    if !needs_downscale && matches!(original_format, Some(ImageFormat::Jpeg | ImageFormat::Png)) {
        let extension = if original_format == Some(ImageFormat::Jpeg) {
            "jpg"
        } else {
            "png"
        };
        return persist_image_bytes(app, &original, extension, width, height);
    }
    // Re-encode: preserve JPEG for photographic sources, otherwise use PNG so
    // transparency and clipboard captures stay lossless.
    let target = if needs_downscale {
        image.thumbnail(COMPRESS_IMAGE_DIMENSION, COMPRESS_IMAGE_DIMENSION)
    } else {
        image
    };
    let (out_width, out_height) = target.dimensions();
    let (format, extension) = if original_format == Some(ImageFormat::Jpeg) {
        (ImageFormat::Jpeg, "jpg")
    } else {
        (ImageFormat::Png, "png")
    };
    let encoded = encode_dynamic_image(&target, format)?;
    persist_image_bytes(app, &encoded, extension, out_width, out_height)
}

#[tauri::command]
pub async fn paper_todo_import_image(
    app: AppHandle,
    source: String,
    auto_compress: bool,
) -> Result<Option<PaperImageAsset>, String> {
    tauri::async_runtime::spawn_blocking(move || match source.as_str() {
        "file" => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg"])
                .pick_file()
            else {
                return Ok(None);
            };
            let original_bytes = fs::metadata(&path)
                .map_err(|error| error.to_string())?
                .len();
            if !auto_compress && original_bytes > MAX_IMAGE_BYTES {
                return Err("图片超过 8 MB，请开启自动压缩".into());
            }
            let bytes = fs::read(&path).map_err(|error| format!("读取图片失败: {error}"))?;
            let format = image::guess_format(&bytes).ok();
            store_import_bytes(&app, bytes, format, auto_compress).map(Some)
        }
        "clipboard" => {
            let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
            let data = clipboard
                .get_image()
                .map_err(|_| "剪贴板中没有可用图片".to_string())?;
            let pixels = data.bytes.into_owned();
            let rgba = image::RgbaImage::from_raw(data.width as u32, data.height as u32, pixels)
                .ok_or_else(|| "剪贴板图片格式无效".to_string())?;
            let mut image = DynamicImage::ImageRgba8(rgba);
            let (width, height) = image.dimensions();
            if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
                return Err(format!(
                    "图片尺寸不能超过 {MAX_IMAGE_DIMENSION} x {MAX_IMAGE_DIMENSION}"
                ));
            }
            if auto_compress
                && (width > COMPRESS_IMAGE_DIMENSION || height > COMPRESS_IMAGE_DIMENSION)
            {
                image = image.thumbnail(COMPRESS_IMAGE_DIMENSION, COMPRESS_IMAGE_DIMENSION);
            }
            let (out_width, out_height) = image.dimensions();
            let encoded = encode_dynamic_image(&image, ImageFormat::Png)?;
            persist_image_bytes(&app, &encoded, "png", out_width, out_height).map(Some)
        }
        _ => Err("未知图片来源".into()),
    })
    .await
    .map_err(|error| error.to_string())?
}

fn find_asset_path(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let safe_id = safe_label(id);
    if safe_id.is_empty() {
        return None;
    }
    // Assets keep their original extension (png/jpg), so resolve by id stem
    // instead of assuming a single container format.
    let entries = fs::read_dir(assets_path(app)).ok()?;
    entries.filter_map(Result::ok).find_map(|entry| {
        let path = entry.path();
        (path.file_stem().and_then(|stem| stem.to_str()) == Some(safe_id.as_str())).then_some(path)
    })
}

#[tauri::command]
pub fn paper_todo_resolve_assets(app: AppHandle, ids: Vec<String>) -> HashMap<String, String> {
    ids.into_iter()
        .filter_map(|id| {
            let path = find_asset_path(&app, &id)?;
            Some((id, path.to_string_lossy().to_string()))
        })
        .collect()
}

fn safe_extension(extension: &str) -> String {
    let extension = extension.trim().trim_start_matches('.');
    let safe: String = extension
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(10)
        .collect();
    if safe.is_empty() {
        "md".into()
    } else {
        safe
    }
}

fn image_ref_regex() -> &'static regex::Regex {
    static IMAGE_REF: OnceLock<regex::Regex> = OnceLock::new();
    IMAGE_REF.get_or_init(|| regex::Regex::new(r"i:([a-fA-F0-9]{16,64})").unwrap())
}

fn script_marker_regex() -> &'static regex::Regex {
    static SCRIPT_MARKER: OnceLock<regex::Regex> = OnceLock::new();
    SCRIPT_MARKER.get_or_init(|| {
        regex::Regex::new(r"(?is)^\s*!(?:p|power|pf|powerf)\s*(?:\r?\n|$)").unwrap()
    })
}

fn persistent_marker_regex() -> &'static regex::Regex {
    static PERSISTENT_MARKER: OnceLock<regex::Regex> = OnceLock::new();
    PERSISTENT_MARKER
        .get_or_init(|| regex::Regex::new(r"(?is)^\s*!(?:pf|powerf)\s*(?:\r?\n|$)").unwrap())
}

fn external_note_content(app: &AppHandle, content: &str) -> String {
    image_ref_regex()
        .replace_all(content, |captures: &regex::Captures<'_>| {
            let id = captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            if let Some(path) = find_asset_path(app, id) {
                format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
            } else {
                captures.get(0).unwrap().as_str().to_string()
            }
        })
        .into_owned()
}

#[tauri::command]
pub async fn paper_todo_open_external(
    app: AppHandle,
    paper: Value,
    extension: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let id = paper.get("id").and_then(Value::as_str).unwrap_or("note");
        let content = paper
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let temp_dir = data_root(&app).join("external");
        fs::create_dir_all(&temp_dir).map_err(|error| error.to_string())?;
        let path = temp_dir.join(format!("{}.{}", safe_label(id), safe_extension(&extension)));
        fs::write(&path, external_note_content(&app, content))
            .map_err(|error| error.to_string())?;

        #[cfg(target_os = "windows")]
        Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map_err(|error| format!("打开外部编辑器失败: {error}"))?;
        #[cfg(not(target_os = "windows"))]
        Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|error| format!("打开外部编辑器失败: {error}"))?;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn paper_todo_run_script(
    app: AppHandle,
    paper_id: String,
    content: String,
    prefer_power_shell7: bool,
    hidden: bool,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let marker = script_marker_regex();
        let persistent_marker = persistent_marker_regex();
        if !marker.is_match(&content) {
            return Err("只有以 !p、!power、!pf 或 !powerf 开头的笔记可以运行".into());
        }
        let script = marker.replace(&content, "").into_owned();
        let candidates: &[&str] = if prefer_power_shell7 {
            &["pwsh.exe", "powershell.exe"]
        } else {
            &["powershell.exe", "pwsh.exe"]
        };

        if persistent_marker.is_match(&content) {
            let runtime = app.state::<PaperTodoRuntime>();
            let mut shell = runtime
                .persistent_shell
                .lock()
                .map_err(|error| error.to_string())?;
            if shell
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten())
                .is_some()
            {
                *shell = None;
            }
            if shell.is_none() {
                let mut last_error = None;
                for executable in candidates {
                    let mut command = Command::new(executable);
                    command
                        .args([
                            "-NoLogo",
                            "-NoProfile",
                            "-ExecutionPolicy",
                            "Bypass",
                            "-NoExit",
                            "-Command",
                            "-",
                        ])
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    #[cfg(target_os = "windows")]
                    if hidden {
                        use std::os::windows::process::CommandExt;
                        command.creation_flags(0x08000000);
                    }
                    match command.spawn() {
                        Ok(child) => {
                            *shell = Some(child);
                            break;
                        }
                        Err(error) => last_error = Some(error),
                    }
                }
                if shell.is_none() {
                    return Err(format!(
                        "无法启动常驻 PowerShell: {}",
                        last_error
                            .map(|error| error.to_string())
                            .unwrap_or_default()
                    ));
                }
            }
            let stdin = shell
                .as_mut()
                .and_then(|child| child.stdin.as_mut())
                .ok_or_else(|| "常驻 PowerShell 输入通道不可用".to_string())?;
            stdin
                .write_all(format!("{script}\r\n").as_bytes())
                .map_err(|error| format!("投递脚本失败: {error}"))?;
            stdin
                .flush()
                .map_err(|error| format!("投递脚本失败: {error}"))?;
            return Ok(());
        }

        let script_dir = data_root(&app).join("scripts");
        fs::create_dir_all(&script_dir).map_err(|error| error.to_string())?;
        let script_path = script_dir.join(format!("{}.ps1", safe_label(&paper_id)));
        fs::write(&script_path, script).map_err(|error| error.to_string())?;

        let mut last_error = None;
        for executable in candidates {
            let mut command = Command::new(executable);
            command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            command.arg(&script_path);
            #[cfg(target_os = "windows")]
            if hidden {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            match command.spawn() {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(error),
            }
        }
        Err(format!(
            "无法启动 PowerShell: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_default()
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn paper_todo_export(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
) -> Result<Option<String>, String> {
    let document = {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        load_document_unlocked(&app)
    };
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("paper-todo-backup.json")
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return Ok(None);
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
        fs::write(&path, bytes).map_err(|error| error.to_string())?;
        Ok(Some(path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn paper_todo_import(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
) -> Result<Option<Value>, String> {
    let imported = tauri::async_runtime::spawn_blocking(|| -> Result<Option<Value>, String> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        else {
            return Ok(None);
        };
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_DATA_BYTES {
            return Err("导入文件超过 32 MB".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if !is_valid_document(&value) {
            return Err("导入文件不是有效的 PaperTodo 便签数据".into());
        }
        Ok(Some(value))
    })
    .await
    .map_err(|error| error.to_string())??;
    let Some(mut document) = imported else {
        return Ok(None);
    };
    {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        next_revision(&mut document);
        write_document_unlocked(&app, &document)?;
    }
    replace_hotkeys(&app, &runtime, &document["settings"])?;
    runtime.avoid_fullscreen.store(
        document["settings"]
            .get("avoidFullscreen")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        Ordering::Relaxed,
    );
    ensure_launcher_window(&app, &document["settings"])?;
    let _ = app.emit(
        "paper-todo-changed",
        json!({ "revision": document["revision"], "paperId": null }),
    );
    Ok(Some(document))
}

#[tauri::command]
pub fn paper_todo_clean_assets(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
) -> Result<usize, String> {
    let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
    let serialized =
        serde_json::to_string(&load_document_unlocked(&app)).map_err(|error| error.to_string())?;
    let assets = assets_path(&app);
    if !assets.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(assets).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if !serialized.contains(&format!("i:{id}")) && fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::{
        default_document, is_valid_document, prepare_papers_for_show_all, rect_is_on_any_screen,
        safe_extension, safe_label, Rect,
    };
    use serde_json::json;

    const PRIMARY: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn default_document_is_valid() {
        let document = default_document();
        assert!(is_valid_document(&document));
        let papers = document["papers"].as_array().expect("default papers");
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0]["kind"], "todo");
        assert_eq!(papers[1]["kind"], "note");
        assert!(papers.iter().all(|paper| paper["desktopOpen"] == false));
    }

    #[test]
    fn labels_and_extensions_drop_shell_characters() {
        assert_eq!(safe_label("abc-123_../"), "abc-123");
        assert_eq!(safe_extension(".md & calc"), "mdcalc");
    }

    #[test]
    fn show_all_reopens_every_saved_paper() {
        let mut document = json!({
            "papers": [
                { "id": "closed", "desktopOpen": false, "hidden": false },
                { "id": "legacy-hidden", "desktopOpen": false, "hidden": true },
                { "id": "open", "desktopOpen": true, "hidden": false }
            ],
            "settings": {}
        });

        let (papers, changed) = prepare_papers_for_show_all(&mut document).unwrap();

        assert!(changed);
        assert_eq!(papers.len(), 3);
        assert!(papers.iter().all(|paper| paper["desktopOpen"] == true));
        assert!(papers.iter().all(|paper| paper["hidden"] == false));
    }

    #[test]
    fn papers_saved_on_a_detached_monitor_count_as_off_screen() {
        // Geometry left over from a secondary display to the left of the
        // primary one. With that monitor unplugged the paper would be restored
        // where no screen can show it.
        let orphan = Rect::new(-1600, 120, 380, 520);
        assert!(!rect_is_on_any_screen(orphan, &[PRIMARY], 48));

        // The same geometry is honoured while the second monitor is attached.
        let secondary = Rect::new(-1920, 0, 1920, 1080);
        assert!(rect_is_on_any_screen(orphan, &[PRIMARY, secondary], 48));
    }

    #[test]
    fn papers_overlapping_a_screen_edge_stay_where_they_are() {
        // Mostly on screen, hanging off the right edge: still reachable.
        assert!(rect_is_on_any_screen(
            Rect::new(1700, 400, 380, 520),
            &[PRIMARY],
            48,
        ));
        // Only a sliver remains on screen, so treat it as lost.
        assert!(!rect_is_on_any_screen(
            Rect::new(1900, 400, 380, 520),
            &[PRIMARY],
            48,
        ));
        // Flush against the top-left corner is fully visible.
        assert!(rect_is_on_any_screen(
            Rect::new(0, 0, 380, 520),
            &[PRIMARY],
            48,
        ));
    }

    #[test]
    fn native_chinese_paper_todo_branding_is_consistent() {
        let main_source = include_str!("main.rs");
        assert!(main_source.contains(".text(TRAY_PAPER_TODO_ID, \"PaperTodo 便签\")"));
        assert!(main_source.contains("show_main_window(app, \"托盘菜单「PaperTodo 便签」\")"));

        let paper_todo_source = include_str!("paper_todo.rs");
        assert!(paper_todo_source.contains(".title(\"PaperTodo 便签\")"));
        assert!(paper_todo_source.contains(".unwrap_or(\"PaperTodo 便签\")"));
        assert!(paper_todo_source
            .contains("return Err(\"导入文件不是有效的 PaperTodo 便签数据\".into());"));
    }
}
