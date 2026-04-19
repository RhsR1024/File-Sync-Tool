//! Tauri commands for clipboard manager (spec §5.3). Implemented incrementally across M2-M5.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, State};

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
