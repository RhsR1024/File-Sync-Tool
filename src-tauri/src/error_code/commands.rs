use std::path::PathBuf;

use tauri::{Manager, State};

use crate::error_code::{
    cache::{self, has_cache, read_meta},
    sync as sync_mod, MetaInfo, QueryRequest, QueryResult, SyncReport,
};
use crate::AppState;

const TOOL_NAME: &str = "错误码查询";

fn cache_root(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve_app_data_dir: {error}"))
}

fn emit_tool_log<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>, level: &str, message: &str) {
    crate::scanner::emit_tool_log(app_handle, TOOL_NAME, message, level);
}

#[tauri::command]
pub async fn error_code_sync(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<SyncReport, String> {
    let root = cache_root(&app_handle).inspect_err(|error| {
        emit_tool_log(
            &app_handle,
            "error",
            &format!("解析应用数据目录失败：{error}"),
        );
    })?;
    let log_app = app_handle.clone();
    let mut emit_sync_log = move |level: &str, message: String| {
        emit_tool_log(&log_app, level, &message);
    };

    sync_mod::run_sync(&root, &state.error_code, &mut emit_sync_log)
        .await
        .map_err(|error| {
            emit_tool_log(&app_handle, "error", &format!("错误码同步失败：{error}"));
            format!("{}|{}", error.toast_key(), error)
        })
}

// Async so ensure_loaded's cache read/parse happens on the tokio pool: as a sync
// command it would run on the main thread and freeze the UI and tray while loading.
#[tauri::command]
pub async fn error_code_query(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    request: QueryRequest,
) -> Result<QueryResult, String> {
    let root = cache_root(&app_handle)?;
    sync_mod::ensure_loaded(&root, &state.error_code)?;

    let store = state
        .error_code
        .lock()
        .map_err(|_| "store_poisoned".to_string())?;

    match request.mode.as_str() {
        "single" => {
            let code = request
                .value
                .trim()
                .parse::<u32>()
                .map_err(|_| "invalid_single".to_string())?;
            Ok(store.query_single(code, request.page))
        }
        "range" => {
            let raw = request.value.trim();
            let (start_s, end_s) = raw
                .split_once('-')
                .ok_or_else(|| "invalid_range_format".to_string())?;
            let start = start_s
                .trim()
                .parse::<u32>()
                .map_err(|_| "invalid_range_format".to_string())?;
            let end = end_s
                .trim()
                .parse::<u32>()
                .map_err(|_| "invalid_range_format".to_string())?;
            store
                .query_range(start, end, request.page)
                .map_err(str::to_string)
        }
        "keyword" => Ok(store.query_keyword(&request.value, request.page)),
        other => Err(format!("unknown_mode: {other}")),
    }
}

// Async for the same reason as error_code_query: keep cache loading off the main thread.
#[tauri::command]
pub async fn error_code_get_meta(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<MetaInfo, String> {
    let root = cache_root(&app_handle)?;
    sync_mod::ensure_loaded(&root, &state.error_code)?;

    let dir = cache::cache_dir(&root);
    let meta = read_meta(&dir);
    let cache_present = has_cache(&dir);
    let store = state
        .error_code
        .lock()
        .map_err(|_| "store_poisoned".to_string())?;

    Ok(MetaInfo {
        has_cache: cache_present,
        last_synced_at: store
            .last_synced_at
            .clone()
            .or_else(|| meta.as_ref().and_then(|value| value.last_synced_at.clone())),
        file_count: meta.as_ref().map(|value| value.file_count).unwrap_or(0),
        row_count: store.row_count(),
    })
}
