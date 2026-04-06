use super::*;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{ConnectInfo, Multipart, Path as AxumPath, Query, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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
        .route("/api/tree", get(handler_tree))
        .route("/api/tree/search", get(handler_tree_search))
        .route("/api/nodes/directory", post(handler_node_create_directory))
        .route("/api/nodes/text", post(handler_node_create_text))
        .route("/api/nodes/rename", patch(handler_node_rename))
        .route("/api/nodes", delete(handler_node_delete))
        .route("/api/upload/files", post(handler_upload_files))
        .route("/api/upload/directory", post(handler_upload_directory))
        .route("/api/download/file", get(handler_node_file))
        .route("/api/download/archive", get(handler_node_archive))
        .route("/api/preview", get(handler_preview))
        .with_state(state)
}

// ─── HTTP Handlers ───────────────────────────────────────────

#[derive(Deserialize)]
struct ApiTreeQuery {
    node_id: Option<String>,
}

#[derive(Deserialize)]
struct ApiTreeSearchQuery {
    keyword: String,
    node_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ApiTreeCurrentKind {
    Home,
    ShareRoot,
    Directory,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ApiTreeNodeKind {
    ShareRoot,
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize)]
struct ApiTreeCurrent {
    node_id: Option<String>,
    name: String,
    kind: ApiTreeCurrentKind,
}

#[derive(Debug, Clone, Serialize)]
struct ApiTreeBreadcrumb {
    node_id: Option<String>,
    label: String,
}

#[derive(Debug, Clone, Serialize)]
struct ApiTreeNode {
    node_id: String,
    parent_id: Option<String>,
    kind: ApiTreeNodeKind,
    name: String,
    root_id: String,
    root_alias: String,
    relative_path: String,
    display_path: String,
    is_dir: bool,
    size: Option<u64>,
    modified: Option<String>,
    permissions: model::FileSharePermissionSet,
}

#[derive(Debug, Clone, Serialize)]
struct ApiTreeResponse {
    current: ApiTreeCurrent,
    breadcrumbs: Vec<ApiTreeBreadcrumb>,
    children: Vec<ApiTreeNode>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiTreeSearchResponse {
    scope: String,
    results: Vec<ApiTreeNode>,
}

#[derive(Debug, Clone)]
enum NodeLocator {
    ShareRoot { root_id: String },
    Directory { root_id: String, relative_path: String },
    File { root_id: String, relative_path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedNodeKind {
    ShareRoot,
    Directory,
    File,
}

#[derive(Debug, Clone)]
struct ResolvedNode {
    kind: ResolvedNodeKind,
    root: ops::ResolvedRoot,
    relative_path: String,
    path: PathBuf,
}

#[derive(Deserialize)]
struct ApiPreviewQuery {
    node_id: String,
}

#[derive(Deserialize)]
struct ApiNodeCreateDirectoryRequest {
    parent_node_id: String,
    name: String,
}

#[derive(Deserialize)]
struct ApiNodeCreateTextRequest {
    parent_node_id: String,
    name: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiNodeRenameRequest {
    node_id: String,
    to_name: String,
}

#[derive(Deserialize)]
struct ApiNodeDeleteRequest {
    node_id: String,
}

#[derive(Deserialize)]
struct ApiNodeQuery {
    node_id: String,
}

#[derive(Deserialize)]
struct ApiLoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ApiSessionFeatures {
    pub(super) image_preview_enabled: bool,
    pub(super) thumbnail_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ApiSessionResponse {
    pub(super) username: String,
    pub(super) is_guest: bool,
    pub(super) permissions: model::FileSharePermissionSet,
    pub(super) features: ApiSessionFeatures,
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
        match authenticate_account(&state, request.username.trim(), &request.password, addr.ip())
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

async fn handler_tree(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<ApiTreeQuery>,
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

    let tree = match q.node_id.as_deref() {
        None => {
            let runtime = state.request_runtime();
            build_home_tree_response(&runtime.roots, &principal.permissions)
        }
        Some(node_id) => match load_tree_node_response(&state, node_id, &principal.permissions).await {
            Ok(response) => response,
            Err(response) => return response,
        },
    };

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    Json(tree).into_response()
}

async fn handler_tree_search(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<ApiTreeSearchQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    let required_permission = if q.node_id.is_some() {
        model::FileSharePermission::SearchCurrent
    } else {
        model::FileSharePermission::SearchGlobal
    };
    let principal = match require_request_permission(
        &state,
        &headers,
        addr.ip(),
        required_permission,
        false,
    ) {
        Ok(principal) => principal,
        Err(response) => return response,
    };

    let response = match q.node_id.as_deref() {
        None => {
            let roots = state.request_runtime().roots;
            let keyword = q.keyword.clone();
            let permissions = principal.permissions.clone();
            let results = match tokio::task::spawn_blocking(move || {
                search::search_tree_globally(&roots, &keyword)
            })
            .await
            {
                Ok(Ok(results)) => results,
                Ok(Err(_)) | Err(_) => {
                    return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Search Failed");
                }
            };
            ApiTreeSearchResponse {
                scope: "global".to_string(),
                results: results
                    .into_iter()
                    .map(|result| search_match_to_tree_node(result, &permissions))
                    .collect(),
            }
        }
        Some(node_id) => {
            let locator = match decode_node_id(node_id) {
                Ok(locator) => locator,
                Err(_) => return plain_response(StatusCode::BAD_REQUEST, "Invalid Node Id"),
            };
            let (root, relative_path) = match locate_search_scope(&state, &locator) {
                Ok(value) => value,
                Err(response) => return response,
            };
            let keyword = q.keyword.clone();
            let permissions = principal.permissions.clone();
            let results = match tokio::task::spawn_blocking(move || {
                search::search_tree_subtree(&root, relative_path.as_deref(), &keyword, search::GLOBAL_SEARCH_MAX_RESULTS)
            })
            .await
            {
                Ok(Ok(results)) => results,
                Ok(Err(_)) | Err(_) => {
                    return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Search Failed");
                }
            };
            ApiTreeSearchResponse {
                scope: "subtree".to_string(),
                results: results
                    .into_iter()
                    .map(|result| search_match_to_tree_node(result, &permissions))
                    .collect(),
            }
        }
    };

    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    Json(response).into_response()
}

async fn handler_node_create_directory(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiNodeCreateDirectoryRequest>,
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

    let (root, parent) = match resolve_parent_directory_node(&state, &request.parent_node_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
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

async fn handler_node_create_text(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiNodeCreateTextRequest>,
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

    let (root, parent) = match resolve_parent_directory_node(&state, &request.parent_node_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
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

async fn handler_node_rename(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiNodeRenameRequest>,
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

    let node = match resolve_node(&state, &request.node_id) {
        Ok(node) => node,
        Err(response) => return response,
    };

    let result = match node.kind {
        ResolvedNodeKind::ShareRoot => {
            let Some(config_path) = state.saved_config_path.clone() else {
                return plain_response(StatusCode::BAD_REQUEST, "Root Rename Requires Saved Settings");
            };
            let root_id = node.root.id.clone();
            let to_name = request.to_name.clone();
            tokio::task::spawn_blocking(move || rename_saved_share_root(&config_path, &root_id, &to_name)).await
        }
        ResolvedNodeKind::Directory | ResolvedNodeKind::File => {
            let root = node.root.clone();
            let relative_path = node.relative_path.clone();
            let to_name = request.to_name.clone();
            tokio::task::spawn_blocking(move || ops::rename_entry_in_place(&root, &relative_path, &to_name)).await
        }
    };

    match result {
        Ok(Ok(())) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            plain_response(StatusCode::OK, "Renamed")
        }
        _ => plain_response(StatusCode::BAD_REQUEST, "Rename Failed"),
    }
}

async fn handler_node_delete(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<ApiNodeDeleteRequest>,
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

    let node = match resolve_node(&state, &request.node_id) {
        Ok(node) => node,
        Err(response) => return response,
    };
    let delete_mode = state.request_runtime().config.delete_mode.clone();

    let result = match node.kind {
        ResolvedNodeKind::ShareRoot => {
            let Some(config_path) = state.saved_config_path.clone() else {
                return plain_response(StatusCode::BAD_REQUEST, "Root Delete Requires Saved Settings");
            };
            let root_id = node.root.id.clone();
            tokio::task::spawn_blocking(move || {
                delete_saved_share_root(&config_path, &root_id, delete_mode)
            })
            .await
        }
        ResolvedNodeKind::Directory | ResolvedNodeKind::File => {
            let root = node.root.clone();
            let relative_path = node.relative_path.clone();
            tokio::task::spawn_blocking(move || ops::delete_entry(&root, &relative_path, delete_mode)).await
        }
    };

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

    let request = match read_upload_request(multipart, state.upload_body_limit_bytes).await {
        Ok(request) => request,
        Err(message) => return upload_read_error_response(&message),
    };
    let (root, parent) = match resolve_parent_directory_node(&state, &request.parent_node_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let files = request.files;

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

    let request = match read_upload_request(multipart, state.upload_body_limit_bytes).await {
        Ok(request) => request,
        Err(message) => return upload_read_error_response(&message),
    };
    let (root, parent) = match resolve_parent_directory_node(&state, &request.parent_node_id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let files = request.files;

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

async fn handler_preview(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(query): Query<ApiPreviewQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if !state.request_runtime().config.image_preview_enabled {
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

    let node = match resolve_node(&state, &query.node_id) {
        Ok(node) if node.kind == ResolvedNodeKind::File => node,
        Ok(_) => return plain_response(StatusCode::BAD_REQUEST, "Preview Requires File Node"),
        Err(response) => return response,
    };
    let preview = match tokio::task::spawn_blocking(move || ops::stream_preview(&node.root, &node.relative_path)).await {
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

async fn handler_node_file(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(query): Query<ApiNodeQuery>,
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

    let node = match resolve_node(&state, &query.node_id) {
        Ok(node) if node.kind == ResolvedNodeKind::File => node,
        Ok(_) => return plain_response(StatusCode::BAD_REQUEST, "File Download Requires File Node"),
        Err(response) => return response,
    };

    let file = match tokio::fs::File::open(&node.path).await {
        Ok(file) => file,
        Err(_) => return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "IO Error"),
    };

    let filename = node
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let content_type = mime_guess::from_path(&node.path)
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

async fn handler_node_archive(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(query): Query<ApiNodeQuery>,
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

    let node = match resolve_node(&state, &query.node_id) {
        Ok(node) if node.kind == ResolvedNodeKind::ShareRoot || node.kind == ResolvedNodeKind::Directory => node,
        Ok(_) => {
            return plain_response(
                StatusCode::BAD_REQUEST,
                "Archive Download Requires Directory Node",
            )
        }
        Err(response) => return response,
    };

    let limit_target = node.path.clone();
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
        node.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive")
    );
    let disposition = format!("attachment; filename*=UTF-8''{}", url_encode(&zip_name));

    let target = node.path.clone();
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
        let _t = tmp;
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

    let asset_path = format!("assets/{}", path.trim_start_matches('/'));
    web_assets::serve_asset(&asset_path)
        .unwrap_or_else(|| plain_response(StatusCode::NOT_FOUND, "Not Found"))
}

fn build_home_tree_response(
    roots: &[ops::ResolvedRoot],
    permissions: &model::FileSharePermissionSet,
) -> ApiTreeResponse {
    ApiTreeResponse {
        current: ApiTreeCurrent {
            node_id: None,
            name: "首页".to_string(),
            kind: ApiTreeCurrentKind::Home,
        },
        breadcrumbs: vec![ApiTreeBreadcrumb {
            node_id: None,
            label: "首页".to_string(),
        }],
        children: roots
            .iter()
            .map(|root| share_root_to_tree_node(root, permissions))
            .collect(),
    }
}

async fn load_tree_node_response(
    state: &Arc<HttpState>,
    node_id: &str,
    permissions: &model::FileSharePermissionSet,
) -> Result<ApiTreeResponse, Response> {
    let locator = decode_node_id(node_id)
        .map_err(|_| plain_response(StatusCode::BAD_REQUEST, "Invalid Node Id"))?;
    match locator {
        NodeLocator::ShareRoot { root_id } => {
            let root = find_root(state, &root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            load_directory_tree_response(root, String::new(), ApiTreeCurrentKind::ShareRoot, permissions).await
        }
        NodeLocator::Directory {
            root_id,
            relative_path,
        } => {
            let root = find_root(state, &root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            load_directory_tree_response(root, relative_path, ApiTreeCurrentKind::Directory, permissions).await
        }
        NodeLocator::File { .. } => Err(plain_response(
            StatusCode::BAD_REQUEST,
            "File Node Cannot Be Browsed",
        )),
    }
}

async fn load_directory_tree_response(
    root: ops::ResolvedRoot,
    current_relative_path: String,
    current_kind: ApiTreeCurrentKind,
    permissions: &model::FileSharePermissionSet,
) -> Result<ApiTreeResponse, Response> {
    let target = match ops::resolve_relative_path(&root, &current_relative_path) {
        Ok(path) if path.is_dir() => path,
        _ => return Err(plain_response(StatusCode::NOT_FOUND, "Directory Not Found")),
    };
    let entries = match tokio::task::spawn_blocking(move || ops::list_directory(&target)).await {
        Ok(Ok(entries)) => entries,
        Ok(Err(_)) | Err(_) => {
            return Err(plain_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "List Failed",
            ));
        }
    };

    let children = entries
        .into_iter()
        .map(|mut entry| {
            entry.relative_path = ops::join_relative_path(&current_relative_path, &entry.relative_path);
            dir_entry_to_tree_node(&root, entry, permissions)
        })
        .collect::<Vec<_>>();

    let current_node_id = match current_kind {
        ApiTreeCurrentKind::ShareRoot => Some(encode_node_id(&NodeLocator::ShareRoot {
            root_id: root.id.clone(),
        })),
        ApiTreeCurrentKind::Directory => Some(encode_node_id(&NodeLocator::Directory {
            root_id: root.id.clone(),
            relative_path: current_relative_path.clone(),
        })),
        ApiTreeCurrentKind::Home => None,
    };
    let current_name = match current_kind {
        ApiTreeCurrentKind::ShareRoot => root.alias.clone(),
        ApiTreeCurrentKind::Directory => last_path_segment(&current_relative_path),
        ApiTreeCurrentKind::Home => "首页".to_string(),
    };

    Ok(ApiTreeResponse {
        current: ApiTreeCurrent {
            node_id: current_node_id,
            name: current_name,
            kind: current_kind,
        },
        breadcrumbs: build_breadcrumbs(&root, Some(&current_relative_path)),
        children,
    })
}

fn locate_search_scope(
    state: &HttpState,
    locator: &NodeLocator,
) -> Result<(ops::ResolvedRoot, Option<String>), Response> {
    match locator {
        NodeLocator::ShareRoot { root_id } => {
            let root = find_root(state, root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            Ok((root, Some(String::new())))
        }
        NodeLocator::Directory {
            root_id,
            relative_path,
        } => {
            let root = find_root(state, root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            let target = ops::resolve_relative_path(&root, relative_path)
                .map_err(|_| plain_response(StatusCode::NOT_FOUND, "Directory Not Found"))?;
            if !target.is_dir() {
                return Err(plain_response(StatusCode::NOT_FOUND, "Directory Not Found"));
            }
            Ok((root, Some(relative_path.clone())))
        }
        NodeLocator::File { .. } => Err(plain_response(
            StatusCode::BAD_REQUEST,
            "File Node Cannot Be Searched",
        )),
    }
}

fn share_root_to_tree_node(
    root: &ops::ResolvedRoot,
    permissions: &model::FileSharePermissionSet,
) -> ApiTreeNode {
    ApiTreeNode {
        node_id: encode_node_id(&NodeLocator::ShareRoot {
            root_id: root.id.clone(),
        }),
        parent_id: None,
        kind: ApiTreeNodeKind::ShareRoot,
        name: root.alias.clone(),
        root_id: root.id.clone(),
        root_alias: root.alias.clone(),
        relative_path: String::new(),
        display_path: root.alias.clone(),
        is_dir: true,
        size: None,
        modified: None,
        permissions: permissions.clone(),
    }
}

fn dir_entry_to_tree_node(
    root: &ops::ResolvedRoot,
    entry: ops::DirEntry,
    permissions: &model::FileSharePermissionSet,
) -> ApiTreeNode {
    let kind = if entry.is_dir {
        ApiTreeNodeKind::Directory
    } else {
        ApiTreeNodeKind::File
    };
    let node_id = encode_node_id(&if entry.is_dir {
        NodeLocator::Directory {
            root_id: root.id.clone(),
            relative_path: entry.relative_path.clone(),
        }
    } else {
        NodeLocator::File {
            root_id: root.id.clone(),
            relative_path: entry.relative_path.clone(),
        }
    });

    ApiTreeNode {
        node_id,
        parent_id: parent_node_id(&root.id, &entry.relative_path),
        kind,
        name: entry.name,
        root_id: root.id.clone(),
        root_alias: root.alias.clone(),
        relative_path: entry.relative_path.clone(),
        display_path: display_path(&root.alias, &entry.relative_path),
        is_dir: entry.is_dir,
        size: if entry.is_dir { None } else { Some(entry.size) },
        modified: if entry.modified.is_empty() {
            None
        } else {
            Some(entry.modified)
        },
        permissions: permissions.clone(),
    }
}

fn search_match_to_tree_node(
    result: search::SearchNodeMatch,
    permissions: &model::FileSharePermissionSet,
) -> ApiTreeNode {
    let kind = match result.kind {
        search::SearchNodeKind::ShareRoot => ApiTreeNodeKind::ShareRoot,
        search::SearchNodeKind::Directory => ApiTreeNodeKind::Directory,
        search::SearchNodeKind::File => ApiTreeNodeKind::File,
    };
    let node_id = match kind {
        ApiTreeNodeKind::ShareRoot => encode_node_id(&NodeLocator::ShareRoot {
            root_id: result.root_id.clone(),
        }),
        ApiTreeNodeKind::Directory => encode_node_id(&NodeLocator::Directory {
            root_id: result.root_id.clone(),
            relative_path: result.relative_path.clone(),
        }),
        ApiTreeNodeKind::File => encode_node_id(&NodeLocator::File {
            root_id: result.root_id.clone(),
            relative_path: result.relative_path.clone(),
        }),
    };

    ApiTreeNode {
        node_id,
        parent_id: match kind {
            ApiTreeNodeKind::ShareRoot => None,
            ApiTreeNodeKind::Directory | ApiTreeNodeKind::File => {
                parent_node_id(&result.root_id, &result.relative_path)
            }
        },
        kind,
        name: result.name,
        root_id: result.root_id,
        root_alias: result.root_alias,
        relative_path: result.relative_path,
        display_path: result.display_path,
        is_dir: !matches!(kind, ApiTreeNodeKind::File),
        size: result.size,
        modified: result.modified,
        permissions: permissions.clone(),
    }
}

fn build_breadcrumbs(
    root: &ops::ResolvedRoot,
    current_relative_path: Option<&str>,
) -> Vec<ApiTreeBreadcrumb> {
    let mut breadcrumbs = vec![ApiTreeBreadcrumb {
        node_id: None,
        label: "首页".to_string(),
    }];

    let root_node_id = encode_node_id(&NodeLocator::ShareRoot {
        root_id: root.id.clone(),
    });
    breadcrumbs.push(ApiTreeBreadcrumb {
        node_id: Some(root_node_id),
        label: root.alias.clone(),
    });

    let normalized = current_relative_path
        .unwrap_or_default()
        .trim()
        .trim_matches('/')
        .to_string();
    if normalized.is_empty() {
        return breadcrumbs;
    }

    let mut current = String::new();
    for segment in normalized.split('/').filter(|segment| !segment.is_empty()) {
        current = ops::join_relative_path(&current, segment);
        breadcrumbs.push(ApiTreeBreadcrumb {
            node_id: Some(encode_node_id(&NodeLocator::Directory {
                root_id: root.id.clone(),
                relative_path: current.clone(),
            })),
            label: segment.to_string(),
        });
    }

    breadcrumbs
}

fn parent_node_id(root_id: &str, relative_path: &str) -> Option<String> {
    let normalized = relative_path.trim().trim_matches('/');
    if normalized.is_empty() {
        return None;
    }

    let parent = parent_relative_path(normalized);
    Some(if parent.is_empty() {
        encode_node_id(&NodeLocator::ShareRoot {
            root_id: root_id.to_string(),
        })
    } else {
        encode_node_id(&NodeLocator::Directory {
            root_id: root_id.to_string(),
            relative_path: parent,
        })
    })
}

fn display_path(root_alias: &str, relative_path: &str) -> String {
    if relative_path.trim().is_empty() {
        root_alias.to_string()
    } else {
        format!("{root_alias}/{}", relative_path.trim_matches('/'))
    }
}

fn parent_relative_path(relative_path: &str) -> String {
    let normalized = relative_path.trim().trim_matches('/');
    if normalized.is_empty() {
        return String::new();
    }

    let mut segments = normalized.split('/').collect::<Vec<_>>();
    segments.pop();
    segments.join("/")
}

fn last_path_segment(relative_path: &str) -> String {
    relative_path
        .trim()
        .trim_matches('/')
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("首页")
        .to_string()
}

fn encode_node_id(locator: &NodeLocator) -> String {
    match locator {
        NodeLocator::ShareRoot { root_id } => {
            format!("root.{}", encode_node_id_part(root_id))
        }
        NodeLocator::Directory {
            root_id,
            relative_path,
        } => format!(
            "dir.{}.{}",
            encode_node_id_part(root_id),
            encode_node_id_part(relative_path)
        ),
        NodeLocator::File {
            root_id,
            relative_path,
        } => format!(
            "file.{}.{}",
            encode_node_id_part(root_id),
            encode_node_id_part(relative_path)
        ),
    }
}

fn decode_node_id(node_id: &str) -> Result<NodeLocator, String> {
    let parts = node_id.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        ["root", root_id] => Ok(NodeLocator::ShareRoot {
            root_id: decode_node_id_part(root_id)?,
        }),
        ["dir", root_id, relative_path] => {
            let relative_path = decode_node_id_part(relative_path)?;
            if relative_path.trim().is_empty() {
                return Err("Directory node id is missing a relative path".to_string());
            }
            Ok(NodeLocator::Directory {
                root_id: decode_node_id_part(root_id)?,
                relative_path,
            })
        }
        ["file", root_id, relative_path] => {
            let relative_path = decode_node_id_part(relative_path)?;
            if relative_path.trim().is_empty() {
                return Err("File node id is missing a relative path".to_string());
            }
            Ok(NodeLocator::File {
                root_id: decode_node_id_part(root_id)?,
                relative_path,
            })
        }
        _ => Err("Invalid node id".to_string()),
    }
}

fn encode_node_id_part(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value.as_bytes())
}

fn decode_node_id_part(value: &str) -> Result<String, String> {
    let decoded = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "Invalid node id".to_string())?;
    String::from_utf8(decoded).map_err(|_| "Invalid node id".to_string())
}

fn resolve_parent_directory_node(
    state: &HttpState,
    node_id: &str,
) -> Result<(ops::ResolvedRoot, String), Response> {
    let node = resolve_node(state, node_id)?;
    match node.kind {
        ResolvedNodeKind::ShareRoot | ResolvedNodeKind::Directory => {
            Ok((node.root, node.relative_path))
        }
        ResolvedNodeKind::File => Err(plain_response(
            StatusCode::BAD_REQUEST,
            "Parent Node Must Be A Directory",
        )),
    }
}

fn resolve_node(state: &HttpState, node_id: &str) -> Result<ResolvedNode, Response> {
    let locator = decode_node_id(node_id)
        .map_err(|_| plain_response(StatusCode::BAD_REQUEST, "Invalid Node Id"))?;
    match locator {
        NodeLocator::ShareRoot { root_id } => {
            let root = find_root(state, &root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            let path = ops::resolve_relative_path(&root, "")
                .map_err(|_| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            Ok(ResolvedNode {
                kind: ResolvedNodeKind::ShareRoot,
                root,
                relative_path: String::new(),
                path,
            })
        }
        NodeLocator::Directory {
            root_id,
            relative_path,
        } => {
            let root = find_root(state, &root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            let path = ops::resolve_relative_path(&root, &relative_path)
                .map_err(|_| plain_response(StatusCode::NOT_FOUND, "Directory Not Found"))?;
            if !path.is_dir() {
                return Err(plain_response(StatusCode::NOT_FOUND, "Directory Not Found"));
            }
            Ok(ResolvedNode {
                kind: ResolvedNodeKind::Directory,
                root,
                relative_path,
                path,
            })
        }
        NodeLocator::File {
            root_id,
            relative_path,
        } => {
            let root = find_root(state, &root_id)
                .ok_or_else(|| plain_response(StatusCode::NOT_FOUND, "Root Not Found"))?;
            let path = ops::resolve_relative_path(&root, &relative_path)
                .map_err(|_| plain_response(StatusCode::NOT_FOUND, "File Not Found"))?;
            if !path.is_file() {
                return Err(plain_response(StatusCode::NOT_FOUND, "File Not Found"));
            }
            Ok(ResolvedNode {
                kind: ResolvedNodeKind::File,
                root,
                relative_path,
                path,
            })
        }
    }
}

fn rename_saved_share_root(config_path: &PathBuf, root_id: &str, to_name: &str) -> Result<(), String> {
    let mut saved = persist::load_persisted_file_share_config_from_path(config_path)?;
    let Some(root) = saved.roots.iter_mut().find(|root| root.id == root_id) else {
        return Err(format!("Root not found: {root_id}"));
    };

    let resolved_root = ops::ResolvedRoot {
        id: root.id.clone(),
        alias: root.alias.clone(),
        path: PathBuf::from(&root.path),
    };
    let previous_path = PathBuf::from(&root.path);
    let previous_alias = root.alias.clone();
    let renamed_path = ops::rename_share_root(&resolved_root, to_name)?;

    root.path = user_visible_path_string(&renamed_path);
    root.alias = to_name.trim().to_string();

    if let Err(err) = persist::save_persisted_file_share_config_to_path(config_path, &saved) {
        let _ = std::fs::rename(&renamed_path, &previous_path);
        return Err(format!(
            "Failed to save renamed shared root {}: {}",
            previous_alias, err
        ));
    }

    Ok(())
}

fn delete_saved_share_root(
    config_path: &PathBuf,
    root_id: &str,
    delete_mode: model::DeleteMode,
) -> Result<(), String> {
    let mut saved = persist::load_persisted_file_share_config_from_path(config_path)?;
    let Some(index) = saved.roots.iter().position(|root| root.id == root_id) else {
        return Err(format!("Root not found: {root_id}"));
    };
    let root = saved.roots[index].clone();
    let resolved_root = ops::ResolvedRoot {
        id: root.id,
        alias: root.alias,
        path: PathBuf::from(root.path),
    };

    ops::delete_share_root(&resolved_root, delete_mode)?;
    saved.roots.remove(index);
    persist::save_persisted_file_share_config_to_path(config_path, &saved)
}

fn user_visible_path_string(path: &PathBuf) -> String {
    let display = path.to_string_lossy().to_string();
    if cfg!(windows) {
        display.strip_prefix(r"\\?\").unwrap_or(&display).to_string()
    } else {
        display
    }
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

        fn config_path(&self) -> PathBuf {
            self.0.join("file_share_http.json")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_state_with_roots(
        roots: &[(&str, &Path)],
        upload_body_limit_bytes: usize,
    ) -> Arc<HttpState> {
        let config_roots = roots
            .iter()
            .enumerate()
            .map(|(index, (alias, path))| model::FileShareRoot {
                id: format!("root-{}", index + 1),
                alias: (*alias).to_string(),
                path: path.to_string_lossy().to_string(),
                enabled: true,
            })
            .collect::<Vec<_>>();
        let resolved_roots = roots
            .iter()
            .enumerate()
            .map(|(index, (alias, path))| ops::ResolvedRoot {
                id: format!("root-{}", index + 1),
                alias: (*alias).to_string(),
                path: (*path).to_path_buf(),
            })
            .collect::<Vec<_>>();

        Arc::new(HttpState {
            saved_config_path: None,
            config: RuntimeFileShareConfig {
                port: 8080,
                roots: config_roots,
                guest_access_enabled: true,
                guest_account: model::PersistedFileShareUser {
                    username: model::DEFAULT_GUEST_USERNAME.to_string(),
                    enabled: true,
                    preset: model::PermissionPreset::ReadWrite,
                    permissions: model::FileSharePermissionSet::read_write(),
                    password_hash: None,
                },
                accounts: Vec::new(),
                session_ttl_minutes: 30,
                ip_filter_mode: model::IpFilterMode::Off,
                ip_rules: Vec::new(),
                image_preview_enabled: true,
                thumbnail_enabled: false,
                delete_mode: model::DeleteMode::RecycleBin,
            },
            roots: resolved_roots,
            sessions: Mutex::new(auth::SessionStore::default()),
            ip_rules: Vec::new(),
            upload_body_limit_bytes,
            visitor_ips: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    fn test_state(root_path: &Path, upload_body_limit_bytes: usize) -> Arc<HttpState> {
        test_state_with_roots(&[("root", root_path)], upload_body_limit_bytes)
    }

    fn test_state_with_named_roots(
        roots: &[(&str, &Path)],
        upload_body_limit_bytes: usize,
    ) -> Arc<HttpState> {
        test_state_with_roots(roots, upload_body_limit_bytes)
    }

    fn write_saved_config(
        roots: &[(&str, &Path)],
        permissions: model::FileSharePermissionSet,
        delete_mode: model::DeleteMode,
        config_path: &Path,
    ) {
        let saved = model::PersistedFileShareConfig {
            version: model::FILE_SHARE_CONFIG_VERSION,
            port: 8080,
            roots: roots
                .iter()
                .enumerate()
                .map(|(index, (alias, path))| model::FileShareRoot {
                    id: format!("root-{}", index + 1),
                    alias: (*alias).to_string(),
                    path: path.to_string_lossy().to_string(),
                    enabled: true,
                })
                .collect(),
            guest_access_enabled: true,
            guest_account: model::PersistedFileShareUser {
                username: model::DEFAULT_GUEST_USERNAME.to_string(),
                enabled: true,
                preset: model::PermissionPreset::Custom,
                permissions,
                password_hash: None,
            },
            accounts: Vec::new(),
            session_ttl_minutes: 30,
            ip_filter_mode: model::IpFilterMode::Off,
            ip_rules: Vec::new(),
            image_preview_enabled: true,
            thumbnail_enabled: false,
            delete_mode,
            remember_settings: true,
            auto_start_on_page_open: false,
            auto_start_with_windows: false,
        };
        persist::save_persisted_file_share_config_to_path(config_path, &saved)
            .expect("saved config should be written");
    }

    fn test_state_with_saved_config(
        roots: &[(&str, &Path)],
        upload_body_limit_bytes: usize,
        permissions: model::FileSharePermissionSet,
        delete_mode: model::DeleteMode,
        config_path: &Path,
    ) -> Arc<HttpState> {
        write_saved_config(roots, permissions, delete_mode, config_path);
        let saved = persist::load_persisted_file_share_config_from_path(config_path)
            .expect("saved config should load");
        let runtime_config = runtime_config_from_saved(saved).expect("runtime config should build");

        Arc::new(HttpState {
            saved_config_path: Some(config_path.to_path_buf()),
            config: runtime_config.clone(),
            roots: runtime_roots(&runtime_config),
            sessions: Mutex::new(auth::SessionStore::default()),
            ip_rules: parse_runtime_ip_rules(&runtime_config).expect("ip rules should parse"),
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

    #[tokio::test]
    async fn login_accepts_username_payload_and_returns_username_session() {
        let dir = TestDir::new("login-username");
        let app = build_router(test_state(dir.path(), 1024));

        let response = app
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json"),
                Body::from(r#"{"username":"guest","password":"ignored"}"#),
            ))
            .await
            .expect("login request should complete");

        let status = response.status();
        let set_cookie = response.headers().get(header::SET_COOKIE).cloned();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("login body should be readable");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("login response should be json");

        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected login body: {}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(payload["username"], "guest");
        assert!(payload.get("account_id").is_none());
        assert!(payload.get("account_name").is_none());
        assert!(set_cookie.is_some());
    }

    #[tokio::test]
    async fn session_response_uses_username_contract() {
        let dir = TestDir::new("session-username");
        let app = build_router(test_state(dir.path(), 1024));

        let response = app
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/session"),
                Body::empty(),
            ))
            .await
            .expect("session request should complete");

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("session body should be readable");
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("session response should be json");

        assert_eq!(payload["username"], "guest");
        assert!(payload.get("account_id").is_none());
        assert!(payload.get("account_name").is_none());
        assert_eq!(payload["is_guest"], true);
    }

    #[tokio::test]
    async fn web_asset_route_serves_embedded_assets() {
        let dir = TestDir::new("web-assets");
        let app = build_router(test_state(dir.path(), 1024));
        let asset_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist/file-share-web/assets");
        let asset_path = fs::read_dir(&asset_dir)
            .expect("built asset directory should exist")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .find(|name| name.starts_with("index-") && name.ends_with(".js"))
            .expect("built js asset should be embedded");
        let request_path = format!("/assets/{asset_path}");

        let response = app
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri(&request_path),
                Body::empty(),
            ))
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK, "{request_path} should be served");
    }

    #[tokio::test]
    async fn tree_route_returns_home_and_nested_directory_nodes() {
        let soft = TestDir::new("tree-soft");
        let soft_nested = soft
            .path()
            .join("实用工具")
            .join("流程图绘制工具Drawio Desktop v13.9.9");
        fs::create_dir_all(&soft_nested).expect("soft nested directory should exist");
        fs::write(soft.path().join("readme.txt"), b"ok").expect("soft file should exist");

        let soft2 = TestDir::new("tree-soft2");
        fs::create_dir_all(soft2.path().join("资料")).expect("soft2 directory should exist");

        let app = build_router(test_state_with_named_roots(
            &[("soft", soft.path()), ("soft2", soft2.path())],
            1024,
        ));

        let home_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/tree"),
                Body::empty(),
            ))
            .await
            .expect("home tree request should complete");

        let home_status = home_response.status();
        let home_body = to_bytes(home_response.into_body(), usize::MAX)
            .await
            .expect("home tree response body should be readable");
        let home_payload: serde_json::Value =
            serde_json::from_slice(&home_body).expect("home tree response should be valid json");

        assert_eq!(home_status, StatusCode::OK);
        assert_eq!(home_payload["current"]["kind"].as_str(), Some("home"));
        assert_eq!(home_payload["breadcrumbs"][0]["label"].as_str(), Some("首页"));

        let soft_node_id = home_payload["children"]
            .as_array()
            .and_then(|children| {
                children
                    .iter()
                    .find(|child| child["name"] == "soft")
                    .and_then(|child| child["node_id"].as_str())
            })
            .expect("home tree should include the soft share root")
            .to_string();

        let root_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("GET")
                    .uri(&format!("/api/tree?node_id={soft_node_id}")),
                Body::empty(),
            ))
            .await
            .expect("share root tree request should complete");

        let root_status = root_response.status();
        let root_body = to_bytes(root_response.into_body(), usize::MAX)
            .await
            .expect("share root tree response body should be readable");
        let root_payload: serde_json::Value =
            serde_json::from_slice(&root_body).expect("share root tree response should be valid json");

        assert_eq!(root_status, StatusCode::OK);
        assert_eq!(root_payload["current"]["kind"].as_str(), Some("share_root"));
        assert_eq!(root_payload["breadcrumbs"][1]["label"].as_str(), Some("soft"));

        let tools_node_id = root_payload["children"]
            .as_array()
            .and_then(|children| {
                children
                    .iter()
                    .find(|child| child["name"] == "实用工具")
                    .and_then(|child| child["node_id"].as_str())
            })
            .expect("share root tree should include the nested tools directory")
            .to_string();

        let nested_response = app
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("GET")
                    .uri(&format!("/api/tree?node_id={tools_node_id}")),
                Body::empty(),
            ))
            .await
            .expect("nested directory tree request should complete");

        let nested_status = nested_response.status();
        let nested_body = to_bytes(nested_response.into_body(), usize::MAX)
            .await
            .expect("nested directory tree response body should be readable");
        let nested_payload: serde_json::Value =
            serde_json::from_slice(&nested_body).expect("nested directory tree response should be valid json");

        assert_eq!(nested_status, StatusCode::OK);
        assert_eq!(nested_payload["current"]["kind"].as_str(), Some("directory"));
        assert_eq!(
            nested_payload["children"][0]["name"].as_str(),
            Some("流程图绘制工具Drawio Desktop v13.9.9")
        );
        assert_eq!(
            nested_payload["children"][0]["relative_path"].as_str(),
            Some("实用工具/流程图绘制工具Drawio Desktop v13.9.9")
        );
    }

    #[tokio::test]
    async fn tree_search_returns_share_root_hits_and_scoped_results() {
        let soft = TestDir::new("tree-search-soft");
        fs::create_dir_all(soft.path().join("实用工具")).expect("soft tools directory should exist");
        fs::write(
            soft.path().join("实用工具").join("drawio-notes.txt"),
            b"ok",
        )
        .expect("soft drawio file should exist");

        let docs = TestDir::new("tree-search-docs");
        fs::write(docs.path().join("drawio-manual.txt"), b"ok")
            .expect("docs drawio file should exist");

        let app = build_router(test_state_with_named_roots(
            &[("soft", soft.path()), ("docs", docs.path())],
            1024,
        ));

        let global_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("GET")
                    .uri("/api/tree/search?keyword=soft"),
                Body::empty(),
            ))
            .await
            .expect("global tree search should complete");

        let global_status = global_response.status();
        let global_body = to_bytes(global_response.into_body(), usize::MAX)
            .await
            .expect("global tree search body should be readable");
        let global_payload: serde_json::Value =
            serde_json::from_slice(&global_body).expect("global search response should be valid json");

        assert_eq!(global_status, StatusCode::OK);
        assert_eq!(global_payload["scope"].as_str(), Some("global"));
        assert!(
            global_payload["results"]
                .as_array()
                .expect("global results should be an array")
                .iter()
                .any(|result| result["kind"] == "share_root" && result["name"] == "soft"),
            "global search should include matching share roots themselves"
        );

        let home_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/tree"),
                Body::empty(),
            ))
            .await
            .expect("home tree request should complete");
        let home_body = to_bytes(home_response.into_body(), usize::MAX)
            .await
            .expect("home body should be readable");
        let home_payload: serde_json::Value =
            serde_json::from_slice(&home_body).expect("home payload should be valid json");
        let soft_node_id = home_payload["children"]
            .as_array()
            .and_then(|children| {
                children
                    .iter()
                    .find(|child| child["name"] == "soft")
                    .and_then(|child| child["node_id"].as_str())
            })
            .expect("soft root should exist in home tree");

        let scoped_response = app
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("GET")
                    .uri(&format!("/api/tree/search?keyword=drawio&node_id={soft_node_id}")),
                Body::empty(),
            ))
            .await
            .expect("scoped tree search should complete");

        let scoped_status = scoped_response.status();
        let scoped_body = to_bytes(scoped_response.into_body(), usize::MAX)
            .await
            .expect("scoped tree search body should be readable");
        let scoped_payload: serde_json::Value =
            serde_json::from_slice(&scoped_body).expect("scoped search response should be valid json");

        assert_eq!(scoped_status, StatusCode::OK);
        assert_eq!(scoped_payload["scope"].as_str(), Some("subtree"));
        assert!(
            scoped_payload["results"]
                .as_array()
                .expect("scoped results should be an array")
                .iter()
                .any(|result| result["display_path"] == "soft/实用工具/drawio-notes.txt"),
            "subtree search should include matches inside the selected share root"
        );
        assert!(
            scoped_payload["results"]
                .as_array()
                .expect("scoped results should be an array")
                .iter()
                .all(|result| result["root_alias"] == "soft"),
            "subtree search should stay inside the selected share root"
        );
    }

    #[tokio::test]
    async fn node_archive_download_supports_share_roots() {
        let soft = TestDir::new("node-download-soft");
        fs::write(soft.path().join("readme.txt"), b"ok").expect("root file should exist");
        let app = build_router(test_state_with_named_roots(&[("soft", soft.path())], 1024));

        let home_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/tree"),
                Body::empty(),
            ))
            .await
            .expect("home tree should load");
        let home_body = to_bytes(home_response.into_body(), usize::MAX)
            .await
            .expect("home tree body should be readable");
        let home_payload: serde_json::Value =
            serde_json::from_slice(&home_body).expect("home tree should be json");
        let soft_node_id = home_payload["children"][0]["node_id"]
            .as_str()
            .expect("share root should expose a node id");

        let response = app
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("GET")
                    .uri(&format!("/api/download/archive?node_id={soft_node_id}")),
                Body::empty(),
            ))
            .await
            .expect("archive download request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/zip")
        );
    }

    #[tokio::test]
    async fn node_rename_updates_saved_root_path_and_alias() {
        let roots_dir = TestDir::new("node-rename-root");
        let soft_path = roots_dir.path().join("soft");
        fs::create_dir_all(&soft_path).expect("soft root should exist");
        fs::write(soft_path.join("keep.txt"), b"ok").expect("soft file should exist");

        let config_dir = TestDir::new("node-rename-config");
        let config_path = config_dir.config_path();
        let app = build_router(test_state_with_saved_config(
            &[("soft", &soft_path)],
            1024,
            model::FileSharePermissionSet::read_write(),
            model::DeleteMode::Permanent,
            &config_path,
        ));

        let home_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/tree"),
                Body::empty(),
            ))
            .await
            .expect("home tree should load");
        let home_body = to_bytes(home_response.into_body(), usize::MAX)
            .await
            .expect("home tree body should be readable");
        let home_payload: serde_json::Value =
            serde_json::from_slice(&home_body).expect("home tree should be json");
        let soft_node_id = home_payload["children"][0]["node_id"]
            .as_str()
            .expect("share root should expose a node id");

        let response = app
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/nodes/rename")
                    .header(header::CONTENT_TYPE, "application/json"),
                Body::from(format!(
                    "{{\"node_id\":\"{soft_node_id}\",\"to_name\":\"soft-renamed\"}}"
                )),
            ))
            .await
            .expect("rename request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let renamed_path = roots_dir.path().join("soft-renamed");
        assert!(!soft_path.exists(), "old root path should be renamed");
        assert!(renamed_path.exists(), "new root path should exist");

        let saved = persist::load_persisted_file_share_config_from_path(&config_path)
            .expect("saved config should reload");
        assert_eq!(saved.roots[0].alias, "soft-renamed");
        assert_eq!(
            PathBuf::from(&saved.roots[0].path),
            renamed_path,
            "saved root path should be updated"
        );
    }

    #[tokio::test]
    async fn node_delete_removes_saved_root_configuration() {
        let roots_dir = TestDir::new("node-delete-root");
        let soft_path = roots_dir.path().join("soft");
        fs::create_dir_all(&soft_path).expect("soft root should exist");
        fs::write(soft_path.join("delete-me.txt"), b"ok").expect("soft file should exist");

        let config_dir = TestDir::new("node-delete-config");
        let config_path = config_dir.config_path();
        let app = build_router(test_state_with_saved_config(
            &[("soft", &soft_path)],
            1024,
            model::FileSharePermissionSet::read_write(),
            model::DeleteMode::Permanent,
            &config_path,
        ));

        let home_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/tree"),
                Body::empty(),
            ))
            .await
            .expect("home tree should load");
        let home_body = to_bytes(home_response.into_body(), usize::MAX)
            .await
            .expect("home tree body should be readable");
        let home_payload: serde_json::Value =
            serde_json::from_slice(&home_body).expect("home tree should be json");
        let soft_node_id = home_payload["children"][0]["node_id"]
            .as_str()
            .expect("share root should expose a node id");

        let response = app
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/nodes")
                    .header(header::CONTENT_TYPE, "application/json"),
                Body::from(format!("{{\"node_id\":\"{soft_node_id}\"}}")),
            ))
            .await
            .expect("delete request should complete");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(!soft_path.exists(), "share root directory should be deleted");

        let saved = persist::load_persisted_file_share_config_from_path(&config_path)
            .expect("saved config should reload");
        assert!(saved.roots.is_empty(), "deleted share root should be removed from config");
    }

    #[tokio::test]
    async fn node_write_requests_return_forbidden_after_permission_revocation() {
        let soft = TestDir::new("node-permission-root");
        fs::create_dir_all(soft.path()).expect("soft root should exist");

        let config_dir = TestDir::new("node-permission-config");
        let config_path = config_dir.config_path();
        let app = build_router(test_state_with_saved_config(
            &[("soft", soft.path())],
            1024,
            model::FileSharePermissionSet::read_write(),
            model::DeleteMode::Permanent,
            &config_path,
        ));

        let home_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/tree"),
                Body::empty(),
            ))
            .await
            .expect("home tree should load");
        let home_body = to_bytes(home_response.into_body(), usize::MAX)
            .await
            .expect("home tree body should be readable");
        let home_payload: serde_json::Value =
            serde_json::from_slice(&home_body).expect("home tree should be json");
        let soft_node_id = home_payload["children"][0]["node_id"]
            .as_str()
            .expect("share root should expose a node id");

        write_saved_config(
            &[("soft", soft.path())],
            model::FileSharePermissionSet::read_only(),
            model::DeleteMode::Permanent,
            &config_path,
        );

        let create_response = app
            .clone()
            .oneshot(request_with_connect_info(
                Request::builder()
                    .method("POST")
                    .uri("/api/nodes/directory")
                    .header(header::CONTENT_TYPE, "application/json"),
                Body::from(format!(
                    "{{\"parent_node_id\":\"{soft_node_id}\",\"name\":\"new-folder\"}}"
                )),
            ))
            .await
            .expect("create request should complete");

        assert_eq!(create_response.status(), StatusCode::FORBIDDEN);

        let session_response = app
            .oneshot(request_with_connect_info(
                Request::builder().method("GET").uri("/api/session"),
                Body::empty(),
            ))
            .await
            .expect("session request should complete");
        let session_body = to_bytes(session_response.into_body(), usize::MAX)
            .await
            .expect("session body should be readable");
        let session_payload: serde_json::Value =
            serde_json::from_slice(&session_body).expect("session response should be json");

        assert_eq!(session_payload["permissions"]["create_directory"], false);
        assert_eq!(session_payload["permissions"]["rename"], false);
        assert_eq!(session_payload["permissions"]["delete"], false);
        assert_eq!(session_payload["features"]["image_preview_enabled"], true);
        assert_eq!(session_payload["features"]["thumbnail_enabled"], false);
    }
}

