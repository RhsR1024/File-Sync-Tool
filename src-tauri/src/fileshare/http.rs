use super::*;
use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{ConnectInfo, Multipart, Path as AxumPath, Query, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;

pub(super) const UPLOAD_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

pub(super) async fn run_http_server(
    listener: tokio::net::TcpListener,
    state: Arc<HttpState>,
    shutdown_rx: oneshot::Receiver<()>,
) {
    let app = build_router(state);

    if let Err(e) = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        })
        .await
    {
        log::error!("File share HTTP server error: {}", e);
    }

    log::info!("File share HTTP server stopped");
}

fn build_router(state: Arc<HttpState>) -> Router {
    Router::new()
        .route("/", get(handler_web_root))
        .route("/assets/*path", get(handler_web_asset))
        .route("/api/session", get(handler_session))
        .route("/api/auth/login", post(handler_login))
        .route("/api/auth/logout", post(handler_logout))
        .route("/api/roots", get(handler_roots))
        .route("/api/list", get(handler_list))
        .route("/api/search", get(handler_search))
        .route("/api/upload/files", post(handler_upload_files))
        .route("/api/upload/directory", post(handler_upload_directory))
        .route("/api/entries/directory", post(handler_create_directory))
        .route("/api/entries/text", post(handler_create_text))
        .route("/api/entries/rename", patch(handler_rename))
        .route("/api/entries", delete(handler_delete))
        .route("/api/preview", get(handler_preview))
        .route("/download/file/*path", get(handler_file))
        .route("/download/zip/*path", get(handler_zip))
        .with_state(state)
}

// ─── HTTP Handlers ───────────────────────────────────────────

#[derive(Deserialize)]
struct ApiListQuery {
    root: String,
    path: Option<String>,
}

#[derive(Deserialize)]
struct ApiSearchQuery {
    keyword: String,
    scope: Option<String>,
    root: Option<String>,
    path: Option<String>,
}

#[derive(Serialize)]
struct ApiListResponse {
    root_id: String,
    root_alias: String,
    path: String,
    entries: Vec<ops::DirEntry>,
}

#[derive(Deserialize)]
struct ApiPreviewQuery {
    root: String,
    path: String,
}

#[derive(Deserialize)]
struct ApiCreateDirectoryRequest {
    root: String,
    parent: Option<String>,
    name: String,
}

#[derive(Deserialize)]
struct ApiCreateTextRequest {
    root: String,
    parent: Option<String>,
    name: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiRenameRequest {
    root: String,
    path: String,
    to_name: String,
}

#[derive(Deserialize)]
struct ApiDeleteRequest {
    root: String,
    path: String,
}

#[derive(Deserialize)]
struct ApiLoginRequest {
    account_id: String,
    password: String,
}

#[derive(Serialize)]
pub(super) struct ApiSessionResponse {
    pub(super) account_id: String,
    pub(super) account_name: String,
    pub(super) is_guest: bool,
    pub(super) permissions: model::FileSharePermissionSet,
}

async fn handler_session(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let principal = match require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::Browse,
        false,
    ) {
        Ok(principal) => principal,
        Err(response) => return response,
    };

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    Json(build_session_response(&state, &principal)).into_response()
}

async fn handler_login(
    AxumState(state): AxumState<Arc<HttpState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiLoginRequest>,
) -> Response {
    if let Some(response) = reject_blocked_ip_response(&state, addr.ip(), false) {
        return response;
    }

    let (principal, token) =
        match authenticate_account(&state, request.account_id.trim(), &request.password, addr.ip())
        {
            Ok(result) => result,
            Err(_) => return plain_response(StatusCode::UNAUTHORIZED, "Unauthorized"),
        };

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    let mut response = Json(build_session_response(&state, &principal)).into_response();
    response
        .headers_mut()
        .insert("Set-Cookie", session_cookie_header(&token).parse().unwrap());
    response
}

async fn handler_logout(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(response) = reject_blocked_ip_response(&state, addr.ip(), false) {
        return response;
    }

    if let Some(token) = find_cookie(&headers, SESSION_COOKIE_NAME) {
        if let Ok(mut sessions) = state.sessions.lock() {
            sessions.logout(token);
        }
    }

    let mut response = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap();
    response.headers_mut().insert(
        "Set-Cookie",
        clear_cookie_header(SESSION_COOKIE_NAME).parse().unwrap(),
    );
    response
}

async fn handler_roots(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let principal = match require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::Browse,
        false,
    ) {
        Ok(principal) => principal,
        Err(response) => return response,
    };

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    let _ = principal;
    Json(runtime_shared_dirs(&state.config)).into_response()
}

async fn handler_list(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<ApiListQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::Browse,
        false,
    ) {
        return response;
    }

    let root = match find_root(&state, &q.root) {
        Some(dir) => dir,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };
    let requested_path = q.path.unwrap_or_default();
    let target = match ops::resolve_relative_path(&root, &requested_path) {
        Ok(path) if path.is_dir() => path,
        _ => return plain_response(StatusCode::NOT_FOUND, "Directory Not Found"),
    };
    let entries = match tokio::task::spawn_blocking(move || ops::list_directory(&target)).await {
        Ok(Ok(entries)) => entries,
        Ok(Err(_)) | Err(_) => {
            return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "List Failed");
        }
    };
    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());

    Json(ApiListResponse {
        root_id: root.id,
        root_alias: root.alias,
        path: requested_path.trim_matches('/').to_string(),
        entries,
    })
    .into_response()
}

async fn handler_search(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<ApiSearchQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let scope = q.scope.as_deref().unwrap_or("global");
    if scope.eq_ignore_ascii_case("current") {
        if let Err(response) = require_request_permission(
            &state,
            &headers,
            addr.ip(),
            model::FileSharePermission::SearchCurrent,
            false,
        ) {
            return response;
        }
        let root_alias = match q.root.as_deref() {
            Some(value) if !value.trim().is_empty() => value,
            _ => return plain_response(StatusCode::BAD_REQUEST, "Root is required"),
        };
        let root = match find_root(&state, root_alias) {
            Some(dir) => dir,
            None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
        };
        let current_path = q.path.unwrap_or_default();
        let target = match ops::resolve_relative_path(&root, &current_path) {
            Ok(path) if path.is_dir() => path,
            _ => return plain_response(StatusCode::NOT_FOUND, "Directory Not Found"),
        };
        let mut results =
            match tokio::task::spawn_blocking(move || search::search_current_directory(&target, &q.keyword)).await
            {
                Ok(Ok(results)) => results,
                Ok(Err(_)) | Err(_) => {
                    return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Search Failed");
                }
            };
        for result in &mut results {
            result.root_id = root.id.clone();
            result.root_alias = root.alias.clone();
        }
        remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
        return Json(results).into_response();
    }

    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::SearchGlobal,
        false,
    ) {
        return response;
    }

    let roots = state.roots.clone();
    let results = match tokio::task::spawn_blocking(move || search::search_all_roots(&roots, &q.keyword)).await {
        Ok(Ok(results)) => results,
        Ok(Err(_)) | Err(_) => {
            return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Search Failed");
        }
    };

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    Json(results).into_response()
}

async fn handler_upload_files(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::UploadFile,
        false,
    ) {
        return response;
    }

    let multipart = match multipart {
        Ok(multipart) => multipart,
        Err(err) => return multipart_rejection_response(err),
    };

    let (root_id, parent, files) = match read_upload_request(multipart, state.upload_body_limit_bytes).await {
        Ok(request) => request,
        Err(message) => return upload_read_error_response(&message),
    };
    let root = match find_root(&state, &root_id) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };

    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        for file in files {
            ops::write_uploaded_file(&root, &parent, &file.relative_path, &file.contents, false)?;
        }
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            plain_response(StatusCode::CREATED, "Created")
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Upload Failed"),
    }
}

async fn handler_upload_directory(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::UploadDirectory,
        false,
    ) {
        return response;
    }

    let multipart = match multipart {
        Ok(multipart) => multipart,
        Err(err) => return multipart_rejection_response(err),
    };

    let (root_id, parent, files) = match read_upload_request(multipart, state.upload_body_limit_bytes).await {
        Ok(request) => request,
        Err(message) => return upload_read_error_response(&message),
    };
    let root = match find_root(&state, &root_id) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };

    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        for file in files {
            ops::write_uploaded_file(&root, &parent, &file.relative_path, &file.contents, true)?;
        }
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            plain_response(StatusCode::CREATED, "Created")
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Upload Failed"),
    }
}

async fn handler_create_directory(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiCreateDirectoryRequest>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::CreateDirectory,
        false,
    ) {
        return response;
    }

    let root = match find_root(&state, &request.root) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };
    let parent = request.parent.unwrap_or_default();
    let result =
        tokio::task::spawn_blocking(move || ops::create_directory(&root, &parent, &request.name))
            .await;

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            plain_response(StatusCode::CREATED, "Created")
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Create Failed"),
    }
}

async fn handler_create_text(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiCreateTextRequest>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::CreateText,
        false,
    ) {
        return response;
    }

    let root = match find_root(&state, &request.root) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };
    let parent = request.parent.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || {
        ops::create_text_file(&root, &parent, &request.name, &request.content)
    })
    .await;

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            plain_response(StatusCode::CREATED, "Created")
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Create Failed"),
    }
}

async fn handler_rename(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiRenameRequest>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::Rename,
        false,
    ) {
        return response;
    }

    let root = match find_root(&state, &request.root) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };
    let result = tokio::task::spawn_blocking(move || {
        ops::rename_entry_in_place(&root, &request.path, &request.to_name)
    })
    .await;

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            plain_response(StatusCode::OK, "Renamed")
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Rename Failed"),
    }
}

async fn handler_delete(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiDeleteRequest>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::Delete,
        false,
    ) {
        return response;
    }

    let root = match find_root(&state, &request.root) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };
    let delete_mode = state.config.delete_mode.clone();
    let result =
        tokio::task::spawn_blocking(move || ops::delete_entry(&root, &request.path, delete_mode))
            .await;

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Body::empty())
                .unwrap()
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Delete Failed"),
    }
}

async fn handler_preview(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(query): Query<ApiPreviewQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if !state.config.image_preview_enabled {
        return plain_response(StatusCode::FORBIDDEN, "Preview Disabled");
    }
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::PreviewImage,
        false,
    ) {
        return response;
    }

    let root = match find_root(&state, &query.root) {
        Some(root) => root,
        None => return plain_response(StatusCode::NOT_FOUND, "Root Not Found"),
    };
    let preview = match tokio::task::spawn_blocking(move || ops::stream_preview(&root, &query.path)).await {
        Ok(Ok(preview)) => preview,
        _ => return plain_response(StatusCode::NOT_FOUND, "Preview Not Found"),
    };
    let file = match tokio::fs::File::open(&preview.path).await {
        Ok(file) => file,
        Err(_) => return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Preview Failed"),
    };
    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());

    let file_name = preview.file_name.clone();
    let stream = async_stream::stream! {
        let mut f = file;
        let mut buf = vec![0u8; 65536];
        loop {
            match f.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buf[..n])),
                Err(e) => { yield Err(e); break; }
            }
        }
    };

    Response::builder()
        .header("Content-Type", preview.content_type)
        .header(
            "Content-Disposition",
            format!("inline; filename*=UTF-8''{}", url_encode(&file_name)),
        )
        .header("Cache-Control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn handler_web_root(
    AxumState(state): AxumState<Arc<HttpState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(response) = reject_blocked_ip_response(&state, addr.ip(), false) {
        return response;
    }

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    web_assets::serve_index()
}

async fn handler_web_asset(
    AxumState(state): AxumState<Arc<HttpState>>,
    AxumPath(path): AxumPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(response) = reject_blocked_ip_response(&state, addr.ip(), false) {
        return response;
    }

    web_assets::serve_asset(&path)
        .unwrap_or_else(|| plain_response(StatusCode::NOT_FOUND, "Not Found"))
}

#[allow(dead_code)]
async fn handler_file(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::DownloadFile,
        false,
    ) {
        return response;
    }
    let (alias, rel) = split_alias_path(&path);
    let root = match find_root(&state, alias) {
        Some(dir) => dir,
        None => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };
    let target = match ops::resolve_relative_path(&root, rel) {
        Ok(p) if p.is_file() => p,
        _ => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };

    let file = match tokio::fs::File::open(&target).await {
        Ok(f) => f,
        Err(_) => return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "IO Error"),
    };

    let filename = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let content_type = mime_guess::from_path(&target)
        .first_or_octet_stream()
        .to_string();
    let disposition = format!("attachment; filename*=UTF-8''{}", url_encode(filename));
    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());

    let stream = async_stream::stream! {
        let mut f = file;
        let mut buf = vec![0u8; 65536];
        loop {
            match f.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buf[..n])),
                Err(e) => { yield Err(e); break; }
            }
        }
    };

    Response::builder()
        .header("Content-Type", content_type)
        .header("Content-Disposition", disposition)
        .header("Cache-Control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn handler_zip(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Err(response) = require_request_permission(
        &state,
        &headers,
        addr.ip(),
        model::FileSharePermission::DownloadArchive,
        false,
    ) {
        return response;
    }
    let (alias, rel) = split_alias_path(&path);
    let root = match find_root(&state, alias) {
        Some(dir) => dir,
        None => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };
    let target = match ops::resolve_relative_path(&root, rel) {
        Ok(p) if p.is_dir() => p,
        _ => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };
    let limit_target = target.clone();
    match tokio::task::spawn_blocking(move || ops::validate_zip_source(&limit_target)).await {
        Ok(Ok(_)) => {}
        Ok(Err(message)) => {
            return Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .body(Body::from(message))
                .unwrap();
        }
        Err(_) => {
            return plain_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to inspect directory",
            )
        }
    }

    let zip_name = format!(
        "{}.zip",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive")
    );
    let disposition = format!("attachment; filename*=UTF-8''{}", url_encode(&zip_name));

    let tmp_path = std::env::temp_dir().join(format!("fst-zip-{}.zip", uuid::Uuid::new_v4()));
    let tmp_clone = tmp_path.clone();
    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());

    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let file = std::fs::File::create(&tmp_clone).map_err(|e| e.to_string())?;
        let mut zip_w = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        ops::zip_dir(&mut zip_w, &target, &target, options)?;
        zip_w.finish().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await;

    let ok = matches!(result, Ok(Ok(())));
    if !ok {
        let _ = std::fs::remove_file(&tmp_path);
        return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create zip");
    }

    let tmp = TempFile(tmp_path.clone());
    let file = match tokio::fs::File::open(&tmp_path).await {
        Ok(f) => f,
        Err(_) => {
            return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to open zip");
        }
    };

    let stream = async_stream::stream! {
        let _t = tmp; // temp file deleted when stream ends
        let mut f = file;
        let mut buf = vec![0u8; 65536];
        loop {
            match f.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buf[..n])),
                Err(e) => { yield Err(e); break; }
            }
        }
    };

    Response::builder()
        .header("Content-Type", "application/zip")
        .header("Content-Disposition", disposition)
        .header("Cache-Control", "no-cache")
        .body(Body::from_stream(stream))
        .unwrap()
}

fn multipart_rejection_response(err: MultipartRejection) -> Response {
    let body = err.body_text();
    let body_lower = body.to_ascii_lowercase();
    let status = if body_lower.contains("too large") || body_lower.contains("length limit") {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        err.status()
    };

    match status {
        StatusCode::PAYLOAD_TOO_LARGE => plain_response(StatusCode::PAYLOAD_TOO_LARGE, "Upload Too Large"),
        StatusCode::BAD_REQUEST => plain_response(StatusCode::BAD_REQUEST, "Invalid Upload"),
        _ => plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Invalid Upload"),
    }
}

fn upload_read_error_response(message: &str) -> Response {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("too large") || normalized.contains("length limit") {
        plain_response(StatusCode::PAYLOAD_TOO_LARGE, "Upload Too Large")
    } else {
        plain_response(StatusCode::BAD_REQUEST, "Invalid Upload")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::connect_info::ConnectInfo;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use tower::util::ServiceExt;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fst-file-share-http-{}-{}",
                label,
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).expect("test temp dir should be created");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_state(root_path: &Path, upload_body_limit_bytes: usize) -> Arc<HttpState> {
        Arc::new(HttpState {
            config: RuntimeFileShareConfig {
                port: 8080,
                roots: vec![model::FileShareRoot {
                    id: "root-1".to_string(),
                    alias: "root".to_string(),
                    path: root_path.to_string_lossy().to_string(),
                    enabled: true,
                }],
                guest_access_enabled: true,
                accounts: vec![model::PersistedFileShareAccount {
                    id: model::GUEST_ACCOUNT_ID.to_string(),
                    name: model::GUEST_ACCOUNT_NAME.to_string(),
                    enabled: true,
                    preset: model::PermissionPreset::ReadWrite,
                    permissions: model::FileSharePermissionSet::read_write(),
                    password_hash: None,
                }],
                session_ttl_minutes: 30,
                ip_filter_mode: model::IpFilterMode::Off,
                ip_rules: Vec::new(),
                image_preview_enabled: true,
                delete_mode: model::DeleteMode::RecycleBin,
            },
            roots: vec![ops::ResolvedRoot {
                id: "root-1".to_string(),
                alias: "root".to_string(),
                path: root_path.to_path_buf(),
            }],
            sessions: Mutex::new(auth::SessionStore::default()),
            ip_rules: Vec::new(),
            upload_body_limit_bytes,
            visitor_ips: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    fn multipart_body(file_size: usize) -> (String, Vec<u8>) {
        let boundary = "fst-boundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"root\"\r\n\r\nroot\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"parent\"\r\n\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"big.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend(std::iter::repeat_n(b'x', file_size));
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    fn request_with_connect_info(builder: axum::http::request::Builder, body: Body) -> Request<Body> {
        builder
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 34567))))
            .body(body)
            .unwrap()
    }

    #[tokio::test]
    async fn legacy_html_routes_are_not_registered() {
        let dir = TestDir::new("routes");
        let app = build_router(test_state(dir.path(), 1024));

        for (method, path) in [
            ("GET", "/login"),
            ("POST", "/auth"),
            ("GET", "/browse/root/"),
            ("GET", "/file/root/demo.txt"),
            ("GET", "/zip/root/"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    request_with_connect_info(
                        Request::builder().method(method).uri(path),
                        Body::empty(),
                    ),
                )
                .await
                .expect("request should complete");

            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path} should be removed");
        }
    }

    #[tokio::test]
    async fn upload_routes_reject_payloads_over_limit() {
        let dir = TestDir::new("upload-limit");
        let app = build_router(test_state(dir.path(), 256));
        let (content_type, body) = multipart_body(1024);

        let response = app
            .oneshot(
                request_with_connect_info(
                    Request::builder()
                        .method("POST")
                        .uri("/api/upload/files")
                        .header(header::CONTENT_TYPE, content_type),
                    Body::from(body),
                ),
            )
            .await
            .expect("request should complete");

        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");

        assert_eq!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "unexpected upload response body: {}",
            String::from_utf8_lossy(&body)
        );
    }
}

