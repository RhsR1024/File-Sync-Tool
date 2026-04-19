//! Tauri commands for clipboard manager (spec §5.3). Implemented incrementally across M2-M5.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

use crate::AppState;
use crate::clipboard::db;
use crate::clipboard::models::{
    ClipboardItem, ClipboardListQuery, ClipboardListResult, ClipboardSettings, ClipboardStats,
};

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
    crate::clipboard::hotkey::change(app.clone(), &state.clipboard.hotkey_handle, &hotkey)?;
    state.clipboard.settings.write().hotkey = hotkey.clone();
    // Persist to config.json so the change survives restarts.
    {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("lock config: {e}"))?;
        cfg.clipboard.hotkey = hotkey;
        crate::config::save_config(&app, &cfg)?;
    }
    Ok(())
}

#[tauri::command]
pub fn cb_get_settings(state: State<'_, AppState>) -> ClipboardSettings {
    state.clipboard.settings.read().clone()
}

#[tauri::command]
pub fn cb_save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: ClipboardSettings,
) -> Result<ClipboardSettings, String> {
    let old = state.clipboard.settings.read().clone();

    // If hotkey changed, re-register the global shortcut first so we fail early on bad input.
    if settings.hotkey != old.hotkey {
        crate::clipboard::hotkey::change(
            app.clone(),
            &state.clipboard.hotkey_handle,
            &settings.hotkey,
        )?;
    }

    // If the enabled flag changed, start or stop the watcher.
    if settings.enabled && !old.enabled {
        state.clipboard.enable(app.clone());
    } else if !settings.enabled && old.enabled {
        state.clipboard.disable();
    }

    // Update in-memory clipboard settings.
    *state.clipboard.settings.write() = settings.clone();

    // Persist to config.json. Lock ordering: config lock is acquired last and released quickly.
    {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("lock config: {e}"))?;
        cfg.clipboard = settings.clone();
        crate::config::save_config(&app, &cfg)?;
    }

    Ok(settings)
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

#[tauri::command]
pub fn cb_reorder_favorites(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<(), String> {
    let mut conn = state.clipboard.db.lock();
    db::reorder_favorites(&mut conn, &ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_stats(state: State<'_, AppState>) -> Result<ClipboardStats, String> {
    let conn = state.clipboard.db.lock();
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM clipboard_items", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let image_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM clipboard_items WHERE kind = 'image'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    drop(conn);

    let db_bytes = std::fs::metadata(&state.clipboard.db_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);

    let images_bytes: i64 = std::fs::read_dir(&state.clipboard.image_dir)
        .map(|rd| {
            rd.filter_map(|r| r.ok())
                .filter_map(|d| d.metadata().ok().map(|m| m.len() as i64))
                .sum::<i64>()
        })
        .unwrap_or(0);

    Ok(ClipboardStats {
        total,
        db_bytes,
        image_count,
        images_bytes,
    })
}
