//! Tauri commands for clipboard manager (spec §5.3). Implemented incrementally across M2-M5.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::Ordering;

use rayon::prelude::*;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

use crate::clipboard::db;
use crate::clipboard::models::{
    ClipboardItem, ClipboardListQuery, ClipboardListResult, ClipboardSettings, ClipboardStats,
    FilePathStatus,
};
use crate::AppState;

fn cleanup_assets_after_mutation<T>(
    clipboard: &crate::clipboard::ClipboardState,
    result: Result<T, String>,
) -> Result<T, String> {
    let value = result?;
    clipboard.cleanup_orphan_assets();
    Ok(value)
}

fn collect_file_path_statuses(items: &[ClipboardItem]) -> Vec<FilePathStatus> {
    let paths: Vec<String> = items
        .iter()
        .filter_map(|item| item.file_paths.as_ref())
        .flat_map(|paths| paths.iter().cloned())
        .collect();

    paths.par_iter()
        .map(|path| match std::fs::metadata(path) {
            Ok(metadata) => FilePathStatus {
                path: path.clone(),
                exists: true,
                size: Some(metadata.len()),
            },
            Err(_) => FilePathStatus {
                path: path.clone(),
                exists: false,
                size: None,
            },
        })
        .collect()
}

fn collect_file_path_statuses_for_selection(
    items: &[ClipboardItem],
    requested_ids: usize,
) -> Result<Vec<FilePathStatus>, String> {
    if items.len() != requested_ids {
        return Err("one or more clipboard items no longer exist".to_string());
    }

    if items.iter().any(|item| item.kind != crate::clipboard::models::ContentKind::File) {
        return Err("all selected clipboard items must be file items".to_string());
    }

    if items
        .iter()
        .any(|item| item.file_paths.as_ref().map_or(true, |paths| paths.is_empty()))
    {
        return Err("selected file item is missing file paths".to_string());
    }

    Ok(collect_file_path_statuses(items))
}

fn load_items_by_ids(
    conn: &rusqlite::Connection,
    ids: &[i64],
) -> Result<Vec<ClipboardItem>, String> {
    ids.iter()
        .map(|id| match db::get_item(conn, *id) {
            Ok(item) => Ok(item),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err("one or more clipboard items no longer exist".to_string())
            }
            Err(error) => Err(error.to_string()),
        })
        .collect()
}

fn selection_target_for_explorer(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("path is empty".to_string());
    }

    let target = PathBuf::from(path);
    if !target.exists() {
        return Err(format!("path does not exist: {}", target.display()));
    }

    Ok(target)
}

fn open_and_select_in_explorer(path: &str) -> Result<(), String> {
    let target = selection_target_for_explorer(path)?;

    #[cfg(target_os = "windows")]
    {
        let target_display = target.to_string_lossy().into_owned();
        Command::new("explorer.exe")
            .args(["/select,", &target_display])
            .spawn()
            .map_err(|e| format!("open explorer: {e}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &target.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("open finder: {e}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = target
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| target.clone());

        if Command::new("xdg-open").arg(&parent).spawn().is_err() {
            Command::new("nautilus")
                .arg(&target)
                .spawn()
                .map_err(|e| format!("open file manager: {e}"))?;
        }
    }

    Ok(())
}

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
    let result = {
        let conn = state.clipboard.db.lock();
        db::delete_item(&conn, id).map_err(|e| e.to_string())
    };
    cleanup_assets_after_mutation(state.clipboard.as_ref(), result)
}

#[tauri::command]
pub fn cb_delete_batch(state: State<'_, AppState>, ids: Vec<i64>) -> Result<(), String> {
    let result = {
        let mut conn = state.clipboard.db.lock();
        db::delete_batch(&mut conn, &ids).map_err(|e| e.to_string())
    };
    cleanup_assets_after_mutation(state.clipboard.as_ref(), result)
}

#[tauri::command]
pub fn cb_clear(state: State<'_, AppState>, keep_favorites: bool) -> Result<u64, String> {
    let result = {
        let conn = state.clipboard.db.lock();
        db::clear_all(&conn, keep_favorites).map_err(|e| e.to_string())
    };
    cleanup_assets_after_mutation(state.clipboard.as_ref(), result)
}

#[tauri::command]
pub fn cb_toggle_favorite(state: State<'_, AppState>, id: i64) -> Result<ClipboardItem, String> {
    let conn = state.clipboard.db.lock();
    db::toggle_favorite(&conn, id).map_err(|e| e.to_string())
}

pub fn cb_toggle_panel_internal(app: AppHandle) -> Result<(), String> {
    let panel = app
        .get_webview_window("clipboard-panel")
        .ok_or_else(|| "clipboard-panel window not found".to_string())?;

    let visible = panel.is_visible().unwrap_or(false);
    if visible {
        crate::clipboard::preview::hide_preview_windows(&app);
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
pub fn cb_paste_plain(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::paste_item(&app, &item, true)
}

/// Copy an item to the system clipboard without simulating Ctrl+V. Used by the manager
/// page so clicking an entry places it on the clipboard for the user to paste manually.
#[tauri::command]
pub fn cb_copy(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::copy_item(&item)
}

#[tauri::command]
pub fn cb_paste_as_files(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::paste_file_item(&app, &item)
}

#[tauri::command]
pub fn cb_paste_as_path(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::paste_file_paths_as_text(&app, &item)
}

#[tauri::command]
pub fn cb_check_file_paths(
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<FilePathStatus>, String> {
    let requested_ids = ids.len();
    let items = {
        let conn = state.clipboard.db.lock();
        load_items_by_ids(&conn, &ids)?
    };
    collect_file_path_statuses_for_selection(&items, requested_ids)
}

#[tauri::command]
pub fn cb_save_image_as(
    state: State<'_, AppState>,
    id: i64,
    target_path: String,
) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    crate::clipboard::paste::save_image_item_to_path(&item, &target_path)
}

#[tauri::command]
pub fn cb_open_in_explorer(path: String) -> Result<(), String> {
    open_and_select_in_explorer(&path)
}

#[tauri::command]
pub fn cb_merge_paste(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
    separator: Option<String>,
) -> Result<(), String> {
    if ids.len() < 2 {
        return Err("at least two clipboard items must be selected".to_string());
    }

    let items = {
        let conn = state.clipboard.db.lock();
        load_items_by_ids(&conn, &ids)?
    };

    let merged = crate::clipboard::paste::merge_items_text(&items, separator.as_deref())?;
    crate::clipboard::paste::paste_text(&app, &merged)
}

#[tauri::command]
pub fn cb_reorder_favorites(state: State<'_, AppState>, ids: Vec<i64>) -> Result<(), String> {
    let mut conn = state.clipboard.db.lock();
    db::reorder_favorites(&mut conn, &ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cb_show_image_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.read_db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    let settings = state.clipboard.settings.read().clone();
    crate::clipboard::preview::show_image_preview(&app, &settings, &item)
}

#[tauri::command]
pub fn cb_show_text_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let item = {
        let conn = state.clipboard.read_db.lock();
        db::get_item(&conn, id).map_err(|e| e.to_string())?
    };
    let settings = state.clipboard.settings.read().clone();
    crate::clipboard::preview::show_text_preview(&app, &settings, &item)
}

#[tauri::command]
pub fn cb_hide_preview(app: AppHandle) {
    crate::clipboard::preview::hide_preview_windows(&app);
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

#[tauri::command]
pub fn cb_is_win_v_enabled() -> bool {
    crate::clipboard::win_v::is_win_v_replacement_enabled()
}

#[tauri::command]
pub async fn cb_enable_win_v(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    crate::clipboard::win_v::enable_win_v_replacement()?;
    // Re-register the clipboard panel hotkey as Super+V (the Win key).
    // Roll back the registry change if hotkey registration fails.
    if let Err(e) =
        crate::clipboard::hotkey::change(app.clone(), &state.clipboard.hotkey_handle, "Super+V")
    {
        let _ = crate::clipboard::win_v::disable_win_v_replacement();
        return Err(format!("register Super+V failed, rolled back: {e}"));
    }
    state.clipboard.settings.write().use_win_v_replacement = true;
    Ok(())
}

#[tauri::command]
pub async fn cb_disable_win_v(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    crate::clipboard::win_v::disable_win_v_replacement()?;
    // Restore the configured hotkey (default Alt+C).
    let hotkey = state.clipboard.settings.read().hotkey.clone();
    if let Err(e) = crate::clipboard::hotkey::change(app, &state.clipboard.hotkey_handle, &hotkey) {
        return Err(format!("restore hotkey failed: {e}"));
    }
    state.clipboard.settings.write().use_win_v_replacement = false;
    Ok(())
}

#[tauri::command]
pub fn cb_is_elevated() -> bool {
    crate::clipboard::admin::is_elevated()
}

#[tauri::command]
pub fn cb_is_run_as_admin_enabled() -> bool {
    crate::clipboard::admin::is_autostart_as_admin_enabled()
}

#[tauri::command]
pub fn cb_set_run_as_admin(state: State<'_, AppState>, enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe_path = exe.to_string_lossy().to_string();
    crate::clipboard::admin::set_autostart_as_admin(&exe_path, enable)?;
    state.clipboard.settings.write().run_as_admin = enable;
    Ok(())
}

#[tauri::command]
pub fn cb_set_panel_pinned(state: State<'_, AppState>, pinned: bool) {
    state
        .clipboard
        .panel_pinned
        .store(pinned, Ordering::Release);
}

#[tauri::command]
pub fn cb_is_panel_pinned(state: State<'_, AppState>) -> bool {
    state.clipboard.panel_pinned.load(Ordering::Acquire)
}

/// Hide the popup panel and bring the main window forward on the clipboard
/// settings route. Emits `clipboard-open-settings` for the frontend to drive
/// the navigation; if the main window is closed-to-tray it will reappear.
#[tauri::command]
pub fn cb_open_settings(app: AppHandle) -> Result<(), String> {
    crate::clipboard::preview::hide_preview_windows(&app);
    if let Some(panel) = app.get_webview_window("clipboard-panel") {
        let _ = panel.hide();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
        let _ = main.emit("clipboard-open-settings", ());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::db::{insert_item, NewItem};
    use crate::clipboard::models::{ClipboardItem, ClipboardSettings, ContentKind};
    use tempfile::TempDir;

    fn sample_asset_item(image_path: Option<String>, icon_path: Option<String>, hash: &str) -> NewItem {
        NewItem {
            kind: ContentKind::Text,
            content_preview: "asset".into(),
            content_full: Some("asset".into()),
            rtf_content: None,
            html: None,
            image_path,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: 5,
            hash: hash.into(),
            source_app: Some("Word".into()),
            source_app_icon: icon_path,
        }
    }

    fn sample_file_item(paths: Option<Vec<String>>) -> ClipboardItem {
        ClipboardItem {
            id: 9,
            kind: ContentKind::File,
            content_preview: "files".into(),
            content_full: None,
            rtf_content: None,
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: paths,
            byte_size: 0,
            char_count: 0,
            hash: "files-hash".into(),
            source_app: Some("Explorer".into()),
            source_app_icon: None,
            group_id: None,
            is_favorite: false,
            is_pinned: false,
            favorite_sort_index: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn sample_text_item() -> ClipboardItem {
        ClipboardItem {
            id: 10,
            kind: ContentKind::Text,
            content_preview: "text".into(),
            content_full: Some("text".into()),
            rtf_content: None,
            html: None,
            image_path: None,
            image_width: None,
            image_height: None,
            file_paths: None,
            byte_size: 4,
            char_count: 4,
            hash: "text-hash".into(),
            source_app: Some("Notepad".into()),
            source_app_icon: None,
            group_id: None,
            is_favorite: false,
            is_pinned: false,
            favorite_sort_index: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn cleanup_assets_after_mutation_removes_orphans_after_delete() {
        let temp_dir = TempDir::new().unwrap();
        let clipboard = crate::clipboard::ClipboardState::init(
            temp_dir.path(),
            ClipboardSettings::default(),
        )
        .unwrap();

        let image_path = clipboard.image_dir.join("delete.png");
        let icon_path = clipboard.icon_dir.join("delete-icon.png");
        std::fs::write(&image_path, b"png").unwrap();
        std::fs::write(&icon_path, b"png").unwrap();

        let id = {
            let conn = clipboard.db.lock();
            insert_item(
                &conn,
                &sample_asset_item(
                    Some(image_path.to_string_lossy().to_string()),
                    Some(icon_path.to_string_lossy().to_string()),
                    "delete-assets",
                ),
            )
            .unwrap()
        };

        let result = {
            let conn = clipboard.db.lock();
            db::delete_item(&conn, id).map_err(|e| e.to_string())
        };
        cleanup_assets_after_mutation(clipboard.as_ref(), result).unwrap();

        assert!(!image_path.exists());
        assert!(!icon_path.exists());
    }

    #[test]
    fn cleanup_assets_after_mutation_removes_orphans_after_clear() {
        let temp_dir = TempDir::new().unwrap();
        let clipboard = crate::clipboard::ClipboardState::init(
            temp_dir.path(),
            ClipboardSettings::default(),
        )
        .unwrap();

        let image_path = clipboard.image_dir.join("clear.png");
        let icon_path = clipboard.icon_dir.join("clear-icon.png");
        std::fs::write(&image_path, b"png").unwrap();
        std::fs::write(&icon_path, b"png").unwrap();

        {
            let conn = clipboard.db.lock();
            insert_item(
                &conn,
                &sample_asset_item(
                    Some(image_path.to_string_lossy().to_string()),
                    Some(icon_path.to_string_lossy().to_string()),
                    "clear-assets",
                ),
            )
            .unwrap();
        }

        let result = {
            let conn = clipboard.db.lock();
            db::clear_all(&conn, false).map_err(|e| e.to_string())
        };
        cleanup_assets_after_mutation(clipboard.as_ref(), result).unwrap();

        assert!(!image_path.exists());
        assert!(!icon_path.exists());
    }

    #[test]
    fn collect_file_path_statuses_reports_existing_and_missing_paths() {
        let temp_dir = TempDir::new().unwrap();
        let existing = temp_dir.path().join("existing.txt");
        std::fs::write(&existing, b"hello").unwrap();
        let missing = temp_dir.path().join("missing.txt");

        let statuses = collect_file_path_statuses(&[
            sample_file_item(Some(vec![
                existing.to_string_lossy().to_string(),
                missing.to_string_lossy().to_string(),
            ])),
            sample_file_item(None),
        ]);

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].path, existing.to_string_lossy());
        assert!(statuses[0].exists);
        assert_eq!(statuses[0].size, Some(5));
        assert_eq!(statuses[1].path, missing.to_string_lossy());
        assert!(!statuses[1].exists);
        assert_eq!(statuses[1].size, None);
    }

    #[test]
    fn collect_file_path_statuses_for_selection_rejects_mixed_or_stale_selection() {
        let err = collect_file_path_statuses_for_selection(&[sample_text_item()], 1).unwrap_err();
        assert!(err.contains("file items"));

        let err =
            collect_file_path_statuses_for_selection(&[sample_file_item(None)], 1).unwrap_err();
        assert!(err.contains("missing file paths"));

        let err =
            collect_file_path_statuses_for_selection(&[sample_file_item(Some(vec!["C:\\ok.txt".into()]))], 2)
                .unwrap_err();
        assert!(err.contains("no longer exist"));
    }

    #[test]
    fn selection_target_for_explorer_requires_an_existing_path() {
        let temp_dir = TempDir::new().unwrap();
        let existing = temp_dir.path().join("existing.txt");
        let missing = temp_dir.path().join("missing.txt");
        std::fs::write(&existing, b"hello").unwrap();

        assert_eq!(
            selection_target_for_explorer(existing.to_string_lossy().as_ref()).unwrap(),
            existing
        );

        let err = selection_target_for_explorer(missing.to_string_lossy().as_ref()).unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn load_items_by_ids_reports_stale_selection() {
        let temp_dir = TempDir::new().unwrap();
        let clipboard = crate::clipboard::ClipboardState::init(
            temp_dir.path(),
            ClipboardSettings::default(),
        )
        .unwrap();

        let existing_id = {
            let conn = clipboard.db.lock();
            insert_item(&conn, &sample_asset_item(None, None, "stale-selection")).unwrap()
        };

        let conn = clipboard.db.lock();
        let err = load_items_by_ids(&conn, &[existing_id, existing_id + 1]).unwrap_err();
        assert!(err.contains("no longer exist"));
    }
}
