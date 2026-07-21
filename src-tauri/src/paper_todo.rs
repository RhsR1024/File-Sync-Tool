use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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
const LAUNCHER_WIDTH: u32 = 236;
const LAUNCHER_HEIGHT: u32 = 64;
const LAUNCHER_VISIBLE_WIDTH: i32 = 42;

#[derive(Default)]
pub struct PaperTodoRuntime {
    io_lock: Mutex<()>,
    hotkeys: Mutex<Vec<PaperHotkeyHandle>>,
    persistent_shell: Mutex<Option<Child>>,
    avoid_fullscreen: AtomicBool,
    fullscreen_active: AtomicBool,
    fullscreen_watch_started: AtomicBool,
    launcher_expanded: AtomicBool,
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
    json!({
        "version": DATA_VERSION,
        "revision": 0,
        "papers": [],
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
    read_json(&data_path(app))
        .or_else(|| read_json(&backup_path(app)))
        .unwrap_or_else(default_document)
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

    if path.exists() {
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
    let _ = app.emit(
        "paper-todo-changed",
        json!({ "revision": revision, "paperId": paper_id, "source": source }),
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
        "collapsed": false,
        "hidden": false,
        "desktopOpen": true,
        "geometry": {
            "x": null,
            "y": null,
            "width": 380,
            "height": 520,
            "monitorName": null,
            "dockEdge": null
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
) -> Result<PhysicalPosition<i32>, String> {
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let window_size = window.outer_size().map_err(|error| error.to_string())?;
    let scale_factor = window.scale_factor().map_err(|error| error.to_string())?;
    let visible_width = (LAUNCHER_VISIBLE_WIDTH as f64 * scale_factor).round() as i32;
    let edge = settings
        .get("launcherEdge")
        .and_then(Value::as_str)
        .unwrap_or("right");
    let offset = settings
        .get("launcherOffset")
        .and_then(Value::as_i64)
        .unwrap_or(35)
        .clamp(0, 100) as i32;
    let available_height = monitor_size.height as i32 - window_size.height as i32;
    let y = monitor_position.y + available_height.max(0) * offset / 100;
    let x = if edge == "left" {
        if expanded {
            monitor_position.x
        } else {
            monitor_position.x - (window_size.width as i32 - visible_width)
        }
    } else if expanded {
        monitor_position.x + monitor_size.width as i32 - window_size.width as i32
    } else {
        monitor_position.x + monitor_size.width as i32 - visible_width
    };
    Ok(PhysicalPosition::new(x, y))
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
    window
        .set_position(launcher_position(&window, settings, expanded)?)
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    Ok(())
}

fn ensure_launcher_window(app: &AppHandle, settings: &Value) -> Result<(), String> {
    if app.get_webview_window(LAUNCHER_LABEL).is_none() {
        WebviewWindowBuilder::new(
            app,
            LAUNCHER_LABEL,
            WebviewUrl::App("index.html#/paper-todo/launcher".into()),
        )
        .title("Paper Todo")
        .inner_size(LAUNCHER_WIDTH as f64, LAUNCHER_HEIGHT as f64)
        .min_inner_size(LAUNCHER_WIDTH as f64, LAUNCHER_HEIGHT as f64)
        .max_inner_size(LAUNCHER_WIDTH as f64, LAUNCHER_HEIGHT as f64)
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
    sync_launcher_window(app, settings)
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
pub fn paper_todo_set_launcher_expanded(app: AppHandle, expanded: bool) -> Result<(), String> {
    let runtime = app.state::<PaperTodoRuntime>();
    runtime.launcher_expanded.store(expanded, Ordering::Relaxed);
    let document = {
        let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
        load_document_unlocked(&app)
    };
    sync_launcher_window(&app, &document["settings"])
}

#[tauri::command]
pub fn paper_todo_save_launcher_position(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
) -> Result<i64, String> {
    let window = app
        .get_webview_window(LAUNCHER_LABEL)
        .ok_or_else(|| "边缘入口窗口不存在".to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
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
        let mut document = load_document_unlocked(&app);
        document["settings"]["launcherOffset"] = json!(offset);
        let settings = document["settings"].clone();
        persist_and_emit(&app, document, None, Some("launcher-drag"))?;
        settings
    };
    sync_launcher_window(&app, &settings)?;
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
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
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
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
            let runtime = app.state::<PaperTodoRuntime>();
            let fullscreen =
                runtime.avoid_fullscreen.load(Ordering::Relaxed) && foreground_is_fullscreen();
            let previous = runtime.fullscreen_active.swap(fullscreen, Ordering::SeqCst);
            if previous != fullscreen {
                apply_fullscreen_policy(&app, fullscreen);
            }
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
                if let Err(error) =
                    open_window_internal(app, paper.clone(), settings.clone())
                {
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

    let collapsed = paper
        .get("collapsed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pinned = paper.get("pinned").and_then(Value::as_bool).unwrap_or(true);
    let geometry = paper.get("geometry").and_then(Value::as_object);
    let width = if collapsed {
        280.0
    } else {
        geometry
            .and_then(|value| value.get("width"))
            .and_then(Value::as_f64)
            .unwrap_or(380.0)
            .clamp(300.0, 900.0)
    };
    let height = if collapsed {
        58.0
    } else {
        geometry
            .and_then(|value| value.get("height"))
            .and_then(Value::as_f64)
            .unwrap_or(520.0)
            .clamp(220.0, 1000.0)
    };
    let skip_taskbar = settings
        .get("hideFromTaskbar")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let route = format!("index.html#/paper-todo/window/{safe_id}");
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(route.into()))
        .title(
            paper
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Paper Todo"),
        )
        .inner_size(width, height)
        .min_inner_size(280.0, 58.0)
        .decorations(false)
        .resizable(!collapsed)
        .transparent(true)
        .shadow(!collapsed)
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

    if let (Some(x), Some(y)) = (
        geometry
            .and_then(|value| value.get("x"))
            .and_then(Value::as_f64),
        geometry
            .and_then(|value| value.get("y"))
            .and_then(Value::as_f64),
    ) {
        builder = builder.position(x, y);
    }
    builder.build().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn paper_todo_set_window_mode(
    app: AppHandle,
    runtime: tauri::State<'_, PaperTodoRuntime>,
    id: String,
    collapsed: bool,
    pinned: bool,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let label = format!("{PAPER_WINDOW_PREFIX}{}", safe_label(&id));
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "便签窗口不存在".to_string())?;
    window
        .set_always_on_top(pinned && !runtime.fullscreen_active.load(Ordering::Relaxed))
        .map_err(|error| error.to_string())?;
    window
        .set_resizable(!collapsed)
        .map_err(|error| error.to_string())?;
    window
        .set_shadow(!collapsed)
        .map_err(|error| error.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(
            if collapsed {
                280.0
            } else {
                width.clamp(300.0, 900.0)
            },
            if collapsed {
                58.0
            } else {
                height.clamp(220.0, 1000.0)
            },
        ))
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn paper_todo_dock_window(app: AppHandle, id: String, edge: String) -> Result<(), String> {
    let label = format!("{PAPER_WINDOW_PREFIX}{}", safe_label(&id));
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "便签窗口不存在".to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or_else(|| "未找到显示器".to_string())?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let window_size = window.outer_size().map_err(|error| error.to_string())?;
    let current = window.outer_position().unwrap_or(*monitor_position);
    let resolved_edge = if edge == "nearest" {
        let window_center = current.x + window_size.width as i32 / 2;
        let monitor_center = monitor_position.x + monitor_size.width as i32 / 2;
        if window_center < monitor_center {
            "left"
        } else {
            "right"
        }
    } else {
        edge.as_str()
    };
    let x = if resolved_edge == "left" {
        monitor_position.x
    } else {
        monitor_position.x + monitor_size.width as i32 - window_size.width as i32
    };
    let max_y = monitor_position.y + monitor_size.height as i32 - window_size.height as i32;
    let y = current
        .y
        .clamp(monitor_position.y, max_y.max(monitor_position.y));
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn paper_todo_set_all_windows(app: AppHandle, action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || set_all_windows_internal(&app, &action))
        .await
        .map_err(|error| error.to_string())?
}

fn set_all_windows_internal(app: &AppHandle, action: &str) -> Result<(), String> {
    if action == "show" {
        let runtime = app.state::<PaperTodoRuntime>();
        let document = {
            let _guard = runtime.io_lock.lock().map_err(|error| error.to_string())?;
            load_document_unlocked(app)
        };
        let settings = document["settings"].clone();
        if let Some(papers) = document["papers"].as_array() {
            for paper in papers {
                if paper
                    .get("desktopOpen")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let _ = open_window_internal(app, paper.clone(), settings.clone());
                }
            }
        }
    }
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
        "toggle" => !windows
            .iter()
            .any(|window| window.is_visible().unwrap_or(false)),
        _ => return Err("未知窗口操作".into()),
    };
    for window in windows {
        if should_show {
            let _ = window.show();
        } else {
            let _ = window.hide();
        }
    }
    Ok(())
}

fn store_dynamic_image(
    app: &AppHandle,
    mut image: DynamicImage,
    auto_compress: bool,
) -> Result<PaperImageAsset, String> {
    let (width, height) = image.dimensions();
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(format!(
            "图片尺寸不能超过 {MAX_IMAGE_DIMENSION} x {MAX_IMAGE_DIMENSION}"
        ));
    }
    if auto_compress && (width > COMPRESS_IMAGE_DIMENSION || height > COMPRESS_IMAGE_DIMENSION) {
        image = image.thumbnail(COMPRESS_IMAGE_DIMENSION, COMPRESS_IMAGE_DIMENSION);
    }

    let id = Uuid::new_v4().simple().to_string();
    let target = assets_path(app).join(format!("{id}.png"));
    fs::create_dir_all(assets_path(app)).map_err(|error| error.to_string())?;
    image
        .save_with_format(&target, ImageFormat::Png)
        .map_err(|error| format!("保存图片失败: {error}"))?;
    let bytes = fs::metadata(&target)
        .map_err(|error| error.to_string())?
        .len();
    if bytes > MAX_IMAGE_BYTES {
        let _ = fs::remove_file(&target);
        return Err("图片压缩后仍超过 8 MB".into());
    }
    let (width, height) = image.dimensions();
    Ok(PaperImageAsset {
        id,
        path: target.to_string_lossy().to_string(),
        width,
        height,
        bytes,
    })
}

#[tauri::command]
pub async fn paper_todo_import_image(
    app: AppHandle,
    source: String,
    auto_compress: bool,
) -> Result<Option<PaperImageAsset>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let image = match source.as_str() {
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
                image::open(path).map_err(|error| format!("读取图片失败: {error}"))?
            }
            "clipboard" => {
                let mut clipboard = arboard::Clipboard::new().map_err(|error| error.to_string())?;
                let data = clipboard
                    .get_image()
                    .map_err(|_| "剪贴板中没有可用图片".to_string())?;
                let pixels = data.bytes.into_owned();
                let rgba =
                    image::RgbaImage::from_raw(data.width as u32, data.height as u32, pixels)
                        .ok_or_else(|| "剪贴板图片格式无效".to_string())?;
                DynamicImage::ImageRgba8(rgba)
            }
            _ => return Err("未知图片来源".into()),
        };
        store_dynamic_image(&app, image, auto_compress).map(Some)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn paper_todo_resolve_assets(app: AppHandle, ids: Vec<String>) -> HashMap<String, String> {
    ids.into_iter()
        .filter_map(|id| {
            let safe_id = safe_label(&id);
            let path = assets_path(&app).join(format!("{safe_id}.png"));
            path.exists()
                .then(|| (id, path.to_string_lossy().to_string()))
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

fn external_note_content(app: &AppHandle, content: &str) -> String {
    let image_ref = regex::Regex::new(r"i:([a-fA-F0-9]{16,64})").unwrap();
    image_ref
        .replace_all(content, |captures: &regex::Captures<'_>| {
            let id = captures
                .get(1)
                .map(|value| value.as_str())
                .unwrap_or_default();
            let path = assets_path(app).join(format!("{id}.png"));
            if path.exists() {
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
        let marker = regex::Regex::new(r"(?is)^\s*!(?:p|power|pf|powerf)\s*(?:\r?\n|$)").unwrap();
        let persistent_marker =
            regex::Regex::new(r"(?is)^\s*!(?:pf|powerf)\s*(?:\r?\n|$)").unwrap();
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
            return Err("导入文件不是有效的 Paper Todo 数据".into());
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
    use super::{default_document, is_valid_document, safe_extension, safe_label};

    #[test]
    fn default_document_is_valid() {
        assert!(is_valid_document(&default_document()));
    }

    #[test]
    fn labels_and_extensions_drop_shell_characters() {
        assert_eq!(safe_label("abc-123_../"), "abc-123");
        assert_eq!(safe_extension(".md & calc"), "mdcalc");
    }
}
