//! Tauri commands for clipboard manager (spec §5.3). Implemented incrementally across M2-M5.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

use crate::AppState;
use crate::clipboard::db;
use crate::clipboard::models::{ClipboardItem, ClipboardListQuery, ClipboardListResult};

#[tauri::command]
pub fn cb_is_enabled(state: State<'_, AppState>) -> bool {
    state.clipboard.is_enabled.load(Ordering::Acquire)
}

#[tauri::command]
pub fn cb_enable(app: AppHandle, state: State<'_, AppState>) {
    state.clipboard.enable(app);
}

#[tauri::command]
pub fn cb_disable(state: State<'_, AppState>) {
    state.clipboard.disable();
}

#[tauri::command]
pub fn cb_list(
    state: State<'_, AppState>,
    query: ClipboardListQuery,
) -> Result<ClipboardListResult, String> {
    let conn = state.clipboard.db.lock();
    db::list_items(&conn, &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_get(state: State<'_, AppState>, id: i64) -> Result<ClipboardItem, String> {
    let conn = state.clipboard.db.lock();
    db::get_item(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.clipboard.db.lock();
    db::delete_item(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_delete_batch(state: State<'_, AppState>, ids: Vec<i64>) -> Result<(), String> {
    let mut conn = state.clipboard.db.lock();
    db::delete_batch(&mut conn, &ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_clear(state: State<'_, AppState>, keep_favorites: bool) -> Result<u64, String> {
    let conn = state.clipboard.db.lock();
    db::clear_all(&conn, keep_favorites).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_toggle_favorite(
    state: State<'_, AppState>,
    id: i64,
) -> Result<ClipboardItem, String> {
    let conn = state.clipboard.db.lock();
    db::toggle_favorite(&conn, id).map_err(|e| e.to_string())
}

pub fn cb_toggle_panel_internal(app: AppHandle) -> Result<(), String> {
    let panel = app
        .get_webview_window("clipboard-panel")
        .ok_or_else(|| "clipboard-panel window not found".to_string())?;

    let visible = panel.is_visible().unwrap_or(false);
    if visible {
        let _ = panel.hide();
        return Ok(());
    }

    // Position near cursor, clamped to current monitor bounds
    if let Ok(pos) = app.cursor_position() {
        let monitor = panel.current_monitor().ok().flatten();
        let (sw, sh) = monitor
            .map(|m| {
                let sz = m.size();
                (sz.width as i32, sz.height as i32)
            })
            .unwrap_or((1920, 1080));
        let w = 420i32;
        let h = 720i32;
        let cx = (pos.x as i32).min(sw - w - 10).max(10);
        let cy = (pos.y as i32).min(sh - h - 10).max(10);
        let _ = panel.set_position(PhysicalPosition::new(cx, cy));
    }

    let _ = panel.show();
    let _ = panel.set_focus();
    let _ = app.emit("clipboard-panel-shown", ());
    Ok(())
}

#[tauri::command]
pub fn cb_toggle_panel(app: AppHandle) -> Result<(), String> {
    cb_toggle_panel_internal(app)
}

#[tauri::command]
pub fn cb_set_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkey: String,
) -> Result<(), String> {
    crate::clipboard::hotkey::change(app, &state.clipboard.hotkey_handle, &hotkey)?;
    state.clipboard.settings.write().hotkey = hotkey;
    // Persisting to config.json is wired up in Task 4.6 (settings panel). For now, the in-memory
    // setting is authoritative for the lifetime of the process.
    Ok(())
}

#[tauri::command]
pub fn cb_paste(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::paste_item(&app, &item, false)
}

#[tauri::command]
pub fn cb_paste_plain(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::paste_item(&app, &item, true)
}
