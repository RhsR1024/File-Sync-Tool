use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, Form, Path as AxumPath, Query, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;

const ZIP_DOWNLOAD_MAX_BYTES: u64 = 500 * 1024 * 1024;
const ZIP_DOWNLOAD_MAX_FILES: usize = 20_000;
const ZIP_MAX_DEPTH: usize = 32;

// ─── Public Data Types ──────────────────────────────────────

const TOOL_NAME: &str = "文件共享";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedDir {
    pub alias: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileShareConfig {
    pub port: u16,
    pub shared_dirs: Vec<SharedDir>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileShareStatus {
    pub is_active: bool,
    pub connection_count: u32,
    pub uptime_secs: u64,
    pub server_url: String,
    pub all_urls: Vec<String>,
    pub shared_dirs: Vec<SharedDir>,
    pub connected_ips: Vec<String>,
}

// ─── Handle (stored in AppState) ────────────────────────────

pub struct FileShareHandle {
    active: Arc<AtomicBool>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    server_url: Mutex<String>,
    all_urls: Mutex<Vec<String>>,
    start_time: Mutex<Option<Instant>>,
    shared_dirs: Mutex<Vec<SharedDir>>,
    visitor_ips: Arc<Mutex<HashSet<String>>>,
}

impl FileShareHandle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            shutdown_tx: Mutex::new(None),
            server_url: Mutex::new(String::new()),
            all_urls: Mutex::new(Vec::new()),
            start_time: Mutex::new(None),
            shared_dirs: Mutex::new(Vec::new()),
            visitor_ips: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

// ─── Internal HTTP State ─────────────────────────────────────

struct HttpState {
    shared_dirs: Vec<SharedDir>,
    password_hash: Option<String>,
    visitor_ips: Arc<Mutex<HashSet<String>>>,
}

/// Deletes the temp file when dropped.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ZipSourceStats {
    file_count: usize,
    total_bytes: u64,
}

// ─── Tauri Commands ─────────────────────────────────────────

#[tauri::command]
pub async fn file_share_pick_directory() -> Result<Option<SharedDir>, String> {
    let picked = rfd::AsyncFileDialog::new()
        .set_title("选择共享目录 / Select Shared Directory")
        .pick_folder()
        .await;

    Ok(picked.map(|handle| {
        let path = handle.path().to_string_lossy().to_string();
        let alias = make_alias(&path);
        SharedDir { alias, path }
    }))
}

#[tauri::command]
pub async fn file_share_start(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
    config: FileShareConfig,
) -> Result<String, String> {
    let handle = &state.file_share;

    if handle.active.load(Ordering::SeqCst) {
        return Err("File share is already active".into());
    }
    if config.port < 1024 {
        return Err("Port must be >= 1024".into());
    }
    if config.shared_dirs.is_empty() {
        return Err("At least one shared directory is required".into());
    }
    for dir in &config.shared_dirs {
        if !Path::new(&dir.path).is_dir() {
            return Err(format!("Directory not found: {}", dir.path));
        }
    }

    let all_ips = get_lan_ips();
    let local_ip = all_ips
        .first()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let server_url = format!("http://{}:{}", local_ip, config.port);
    let all_urls: Vec<String> = all_ips
        .iter()
        .map(|ip| format!("http://{}:{}", ip, config.port))
        .collect();

    let password_hash = config.password.as_ref().map(|p| hash_password(p));
    let http_state = Arc::new(HttpState {
        shared_dirs: config.shared_dirs.clone(),
        password_hash,
        visitor_ips: handle.visitor_ips.clone(),
    });

    if let Ok(mut ips) = handle.visitor_ips.lock() {
        ips.clear();
    }
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    *handle.shutdown_tx.lock().unwrap() = Some(shutdown_tx);
    *handle.server_url.lock().unwrap() = server_url.clone();
    *handle.all_urls.lock().unwrap() = all_urls.clone();
    *handle.start_time.lock().unwrap() = Some(Instant::now());
    *handle.shared_dirs.lock().unwrap() = config.shared_dirs.clone();
    handle.active.store(true, Ordering::SeqCst);

    // Bind listener BEFORE spawning so that bind errors propagate to the caller
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        handle.active.store(false, Ordering::SeqCst);
        let msg = format!("Failed to bind port {}: {}", config.port, e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        msg
    })?;
    log::info!("File share HTTP server listening on {}", addr);

    // Clone state needed for cleanup when server stops unexpectedly
    let server_active = handle.active.clone();
    let server_app = app_handle.clone();
    let server_handle = tokio::spawn(async move {
        run_http_server(listener, http_state, shutdown_rx).await;
    });

    // Watcher: when the server task ends (normal or panic), clean up state
    tokio::spawn(async move {
        match server_handle.await {
            Ok(()) => {
                log::info!("File share HTTP server exited");
            }
            Err(e) => {
                let msg = format!("服务异常退出: {}", e);
                log::error!("File share server crashed: {}", e);
                crate::scanner::emit_tool_log(&server_app, TOOL_NAME, &msg, "error");
                let _ = server_app.emit(
                    "file-share-log",
                    serde_json::json!({ "level": "error", "message": msg }),
                );
            }
        }

        // Cleanup: mark inactive and notify frontend
        if server_active.swap(false, Ordering::SeqCst) {
            crate::scanner::emit_tool_log(&server_app, TOOL_NAME, "已停止", "info");
            let _ = server_app.emit(
                "file-share-log",
                serde_json::json!({ "level": "info", "message": "File share stopped" }),
            );
            let _ = server_app.emit(
                "file-share-status",
                FileShareStatus {
                    is_active: false,
                    connection_count: 0,
                    uptime_secs: 0,
                    server_url: String::new(),
                    all_urls: Vec::new(),
                    shared_dirs: Vec::new(),
                    connected_ips: Vec::new(),
                },
            );
        }
    });

    // Status reporter
    let reporter_app = app_handle.clone();
    let reporter_active = handle.active.clone();
    let reporter_url = server_url.clone();
    let reporter_all_urls = all_urls.clone();
    let reporter_dirs = config.shared_dirs.clone();
    let reporter_start = Instant::now();
    let reporter_ips = handle.visitor_ips.clone();
    tokio::spawn(async move {
        status_reporter(
            reporter_app,
            reporter_active,
            reporter_url,
            reporter_all_urls,
            reporter_dirs,
            reporter_start,
            reporter_ips,
        )
        .await;
    });

    let dir_names: Vec<&str> = config
        .shared_dirs
        .iter()
        .map(|d| d.alias.as_str())
        .collect();
    crate::scanner::emit_tool_log(
        &app_handle,
        TOOL_NAME,
        &format!(
            "已启动，共享 {} 个目录 [{}]，访问: {}",
            config.shared_dirs.len(),
            dir_names.join(", "),
            server_url
        ),
        "success",
    );

    let _ = app_handle.emit(
        "file-share-log",
        serde_json::json!({ "level": "info", "message": format!("File share started at {}", server_url) }),
    );

    Ok(server_url)
}

#[tauri::command]
pub async fn file_share_stop(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let handle = &state.file_share;
    if !handle.active.load(Ordering::SeqCst) {
        return Err("File share is not active".into());
    }

    handle.active.store(false, Ordering::SeqCst);

    if let Some(tx) = handle.shutdown_tx.lock().unwrap().take() {
        let _ = tx.send(());
    }

    if let Ok(mut ips) = handle.visitor_ips.lock() {
        ips.clear();
    }
    *handle.server_url.lock().unwrap() = String::new();
    *handle.all_urls.lock().unwrap() = Vec::new();
    *handle.start_time.lock().unwrap() = None;
    *handle.shared_dirs.lock().unwrap() = Vec::new();

    crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, "已停止", "info");

    let _ = app_handle.emit(
        "file-share-log",
        serde_json::json!({ "level": "info", "message": "File share stopped" }),
    );
    let _ = app_handle.emit(
        "file-share-status",
        FileShareStatus {
            is_active: false,
            connection_count: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
            shared_dirs: Vec::new(),
            connected_ips: Vec::new(),
        },
    );

    Ok(())
}

#[tauri::command]
pub fn file_share_get_status(state: State<'_, crate::AppState>) -> FileShareStatus {
    let handle = &state.file_share;
    if !handle.active.load(Ordering::Relaxed) {
        return FileShareStatus {
            is_active: false,
            connection_count: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
            shared_dirs: Vec::new(),
            connected_ips: Vec::new(),
        };
    }
    let uptime = handle
        .start_time
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let connected_ips = snapshot_connected_ips(&handle.visitor_ips);
    FileShareStatus {
        is_active: true,
        connection_count: connected_ips.len() as u32,
        uptime_secs: uptime,
        server_url: handle.server_url.lock().unwrap().clone(),
        all_urls: handle.all_urls.lock().unwrap().clone(),
        shared_dirs: handle.shared_dirs.lock().unwrap().clone(),
        connected_ips,
    }
}

// ─── HTTP Server ─────────────────────────────────────────────

async fn run_http_server(
    listener: tokio::net::TcpListener,
    state: Arc<HttpState>,
    shutdown_rx: oneshot::Receiver<()>,
) {
    let app = Router::new()
        .route("/", get(handler_root))
        .route("/browse/*path", get(handler_browse))
        .route("/file/*path", get(handler_file))
        .route("/zip/*path", get(handler_zip))
        .route("/login", get(handler_login_page))
        .route("/auth", post(handler_auth))
        .with_state(state);

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

// ─── HTTP Handlers ───────────────────────────────────────────

#[derive(Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

fn detect_lang(headers: &HeaderMap, query_lang: Option<&str>) -> &'static str {
    if let Some(l) = query_lang {
        if l.starts_with("zh") {
            return "zh";
        }
        return "en";
    }
    let accept = headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if accept.to_lowercase().contains("zh") {
        "zh"
    } else {
        "en"
    }
}

async fn handler_root(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return redirect_login();
        }
    }
    remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
    let lang = detect_lang(&headers, q.lang.as_deref());
    Html(root_html(&state.shared_dirs, lang)).into_response()
}

async fn handler_browse(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
    AxumPath(path): AxumPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return redirect_login();
        }
    }
    let lang = detect_lang(&headers, q.lang.as_deref());
    let (alias, rel) = split_alias_path(&path);

    let root = match find_root(&state.shared_dirs, alias) {
        Some(p) => p,
        None => return err_page(StatusCode::NOT_FOUND, lang),
    };
    match safe_join(&root, rel) {
        None => err_page(StatusCode::FORBIDDEN, lang),
        Some(p) if !p.is_dir() => err_page(StatusCode::NOT_FOUND, lang),
        Some(p) => {
            remember_connected_ip(&state.visitor_ips, addr.ip().to_string());
            let entries = tokio::task::spawn_blocking(move || list_dir(&p))
                .await
                .unwrap_or_default();
            Html(browse_html(alias, rel, &entries, lang)).into_response()
        }
    }
}

async fn handler_file(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    AxumPath(path): AxumPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return plain_response(StatusCode::UNAUTHORIZED, "Unauthorized");
        }
    }
    let (alias, rel) = split_alias_path(&path);
    let root = match find_root(&state.shared_dirs, alias) {
        Some(p) => p,
        None => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };
    let target = match safe_join(&root, rel) {
        Some(p) if p.is_file() => p,
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
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return plain_response(StatusCode::UNAUTHORIZED, "Unauthorized");
        }
    }
    let (alias, rel) = split_alias_path(&path);
    let root = match find_root(&state.shared_dirs, alias) {
        Some(p) => p,
        None => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };
    let target = match safe_join(&root, rel) {
        Some(p) if p.is_dir() => p,
        _ => return plain_response(StatusCode::NOT_FOUND, "Not Found"),
    };
    let limit_target = target.clone();
    match tokio::task::spawn_blocking(move || validate_zip_source(&limit_target)).await {
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
        zip_dir(&mut zip_w, &target, &target, options)?;
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

#[derive(Deserialize)]
struct LoginQuery {
    lang: Option<String>,
    error: Option<u8>,
}

async fn handler_login_page(headers: HeaderMap, Query(q): Query<LoginQuery>) -> Response {
    let lang = detect_lang(&headers, q.lang.as_deref());
    let has_error = q.error.unwrap_or(0) == 1;
    Html(login_html(has_error, lang)).into_response()
}

#[derive(Deserialize)]
struct AuthForm {
    password: String,
}

async fn handler_auth(
    AxumState(state): AxumState<Arc<HttpState>>,
    Form(form): Form<AuthForm>,
) -> Response {
    if let Some(expected) = &state.password_hash {
        if hash_password(&form.password) == *expected {
            return Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header("Location", "/")
                .header(
                    "Set-Cookie",
                    format!("fs_auth={}; HttpOnly; Path=/", expected),
                )
                .body(Body::empty())
                .unwrap();
        }
    }
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/login?error=1")
        .body(Body::empty())
        .unwrap()
}

// ─── Status Reporter ─────────────────────────────────────────

async fn status_reporter(
    app_handle: AppHandle,
    active: Arc<AtomicBool>,
    server_url: String,
    all_urls: Vec<String>,
    shared_dirs: Vec<SharedDir>,
    start_time: Instant,
    visitor_ips: Arc<Mutex<HashSet<String>>>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if !active.load(Ordering::Relaxed) {
            break;
        }
        let connected_ips = snapshot_connected_ips(&visitor_ips);
        let _ = app_handle.emit(
            "file-share-status",
            FileShareStatus {
                is_active: true,
                connection_count: connected_ips.len() as u32,
                uptime_secs: start_time.elapsed().as_secs(),
                server_url: server_url.clone(),
                all_urls: all_urls.clone(),
                shared_dirs: shared_dirs.clone(),
                connected_ips,
            },
        );
    }
}

// ─── Path Utilities ──────────────────────────────────────────

fn split_alias_path(path: &str) -> (&str, &str) {
    let s = path.trim_start_matches('/');
    match s.find('/') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    }
}

fn find_root(dirs: &[SharedDir], alias: &str) -> Option<PathBuf> {
    dirs.iter()
        .find(|d| d.alias == alias)
        .map(|d| PathBuf::from(&d.path))
}

fn remember_connected_ip(visitor_ips: &Arc<Mutex<HashSet<String>>>, ip: impl Into<String>) {
    if let Ok(mut ips) = visitor_ips.lock() {
        ips.insert(ip.into());
    }
}

fn snapshot_connected_ips(visitor_ips: &Arc<Mutex<HashSet<String>>>) -> Vec<String> {
    let mut ips: Vec<String> = visitor_ips
        .lock()
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default();
    ips.sort_unstable();
    ips
}

/// Returns the canonicalized target path, or None if it would escape the root (path traversal).
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim_matches('/');
    let target = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let canonical = target.canonicalize().ok()?;
    let root_canonical = root.canonicalize().ok()?;
    if canonical.starts_with(&root_canonical) {
        Some(canonical)
    } else {
        None
    }
}

struct DirEntry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: String,
}

fn list_dir(path: &Path) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    let Ok(rd) = std::fs::read_dir(path) else {
        return entries;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(meta) = e.metadata() else { continue };
        let is_dir = meta.is_dir();
        let size = if is_dir { 0 } else { meta.len() };
        let modified = meta
            .modified()
            .ok()
            .map(|t| {
                let dt: chrono::DateTime<chrono::Local> = t.into();
                dt.format("%Y-%m-%d %H:%M").to_string()
            })
            .unwrap_or_default();
        entries.push(DirEntry {
            name,
            is_dir,
            size,
            modified,
        });
    }
    // Directories first, then alphabetical
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

fn fmt_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{} B", b),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / 1024.0 / 1024.0),
        b => format!("{:.2} GB", b as f64 / 1024.0 / 1024.0 / 1024.0),
    }
}

pub fn make_alias(path: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("share");
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "share".to_string()
    } else {
        trimmed
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ─── Zip Utilities ───────────────────────────────────────────

fn validate_zip_source(path: &Path) -> Result<ZipSourceStats, String> {
    validate_zip_source_with_limits(path, ZIP_DOWNLOAD_MAX_BYTES, ZIP_DOWNLOAD_MAX_FILES)
}

fn validate_zip_source_with_limits(
    path: &Path,
    max_total_bytes: u64,
    max_files: usize,
) -> Result<ZipSourceStats, String> {
    let mut stats = ZipSourceStats {
        file_count: 0,
        total_bytes: 0,
    };
    collect_zip_source_stats(path, 0, &mut stats, max_total_bytes, max_files)?;
    Ok(stats)
}

fn collect_zip_source_stats(
    current: &Path,
    depth: usize,
    stats: &mut ZipSourceStats,
    max_total_bytes: u64,
    max_files: usize,
) -> Result<(), String> {
    if depth > ZIP_MAX_DEPTH {
        return Err(format!(
            "Directory nesting too deep (>{ZIP_MAX_DEPTH} levels)"
        ));
    }

    for entry in std::fs::read_dir(current)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let path = entry.path();
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            collect_zip_source_stats(&path, depth + 1, stats, max_total_bytes, max_files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        stats.file_count += 1;
        stats.total_bytes = stats.total_bytes.saturating_add(metadata.len());

        if stats.file_count > max_files {
            return Err(format!(
                "Directory contains too many files to download as ZIP (limit: {})",
                max_files
            ));
        }

        if stats.total_bytes > max_total_bytes {
            return Err(format!(
                "Directory is too large to download as ZIP (limit: {})",
                format_byte_limit(max_total_bytes)
            ));
        }
    }

    Ok(())
}

fn format_byte_limit(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{} GB", bytes / (1024 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{} B", bytes)
    }
}

fn zip_dir<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    current: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    zip_dir_inner(zip, base, current, options, 0)
}

fn zip_dir_inner<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    current: &Path,
    options: zip::write::SimpleFileOptions,
    depth: usize,
) -> Result<(), String> {
    if depth > ZIP_MAX_DEPTH {
        return Err(format!(
            "Directory nesting too deep (>{ZIP_MAX_DEPTH} levels)"
        ));
    }
    for e in std::fs::read_dir(current)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let path = e.path();
        let rel = path.strip_prefix(base).map_err(|e| e.to_string())?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            zip.add_directory(&rel_str, options)
                .map_err(|e| e.to_string())?;
            zip_dir_inner(zip, base, &path, options, depth + 1)?;
        } else {
            zip.start_file(&rel_str, options)
                .map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, zip).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ─── Auth Utilities ──────────────────────────────────────────

fn hash_password(pw: &str) -> String {
    let mut h = Sha256::new();
    h.update(pw.as_bytes());
    h.update(b"file_share_salt_fst_v1");
    format!("{:x}", h.finalize())
}

fn check_auth_cookie(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(|cookies| {
            cookies.split(';').any(|c| {
                c.trim()
                    .strip_prefix("fs_auth=")
                    .is_some_and(|v| v == expected)
            })
        })
        .unwrap_or(false)
}

fn redirect_login() -> Response {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/login")
        .body(Body::empty())
        .unwrap()
}

fn plain_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap()
}

fn err_page(status: StatusCode, lang: &str) -> Response {
    let msg = match (status.as_u16(), lang) {
        (403, "zh") => "403 禁止访问",
        (403, _) => "403 Forbidden",
        (404, "zh") => "404 找不到",
        _ => "404 Not Found",
    };
    Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .body(Body::from(format!(
            "<h2 style='font-family:sans-serif;color:#e2e8f0;background:#0f172a;padding:40px;margin:0'>{msg}</h2>"
        )))
        .unwrap()
}

fn get_lan_ips() -> Vec<String> {
    use std::net::{IpAddr, Ipv4Addr};
    let mut ips: Vec<Ipv4Addr> = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, ip)| match ip {
            IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() => Some(v4),
            _ => None,
        })
        .collect();

    // Sort: prefer real LAN IPs (192.168.x.x, 10.x.x.x, 172.16-31.x.x) over
    // VPN/TUN adapter IPs (198.18.x.x, 100.64-127.x.x, etc.)
    fn is_common_lan(ip: &Ipv4Addr) -> bool {
        let o = ip.octets();
        o[0] == 192 && o[1] == 168                          // 192.168.0.0/16
            || o[0] == 10                                    // 10.0.0.0/8
            || o[0] == 172 && (16..=31).contains(&o[1]) // 172.16.0.0/12
    }
    ips.sort_by_key(|ip| if is_common_lan(ip) { 0 } else { 1 });

    ips.into_iter().map(|ip| ip.to_string()).collect()
}

// ─── HTML Templates ──────────────────────────────────────────

fn common_css() -> &'static str {
    r#"*{margin:0;padding:0;box-sizing:border-box}
html,body{min-height:100%;background:#060911;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}
a{color:#38bdf8;text-decoration:none;transition:color .15s}a:hover{color:#7dd3fc}
.header{background:rgba(10,14,22,.95);border-bottom:1px solid rgba(255,255,255,.07);padding:0 20px;display:flex;align-items:center;gap:14px;height:56px;position:sticky;top:0;z-index:10;backdrop-filter:blur(10px)}
.header-icon{width:34px;height:34px;background:rgba(20,184,166,.1);border:1px solid rgba(20,184,166,.2);border-radius:9px;display:flex;align-items:center;justify-content:center;color:#2dd4bf;flex-shrink:0}
.header-info{flex:1;min-width:0}
.header-title{font-size:15px;font-weight:700;color:#f1f5f9;letter-spacing:-.01em}
.header-sub{font-size:11px;color:#334155;margin-top:1px}
.container{max-width:1040px;margin:0 auto;padding:28px 18px}
.section-label{font-size:10px;font-weight:700;letter-spacing:.12em;text-transform:uppercase;color:#334155;margin-bottom:14px}
.card-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:12px}
.dir-card{background:rgba(15,23,42,.7);border:1px solid rgba(255,255,255,.07);border-radius:16px;padding:22px 20px;transition:border-color .2s,box-shadow .2s,transform .15s;cursor:pointer;position:relative;overflow:hidden}
.dir-card::before{content:'';position:absolute;inset:0;background:radial-gradient(circle at top left,rgba(20,184,166,.05),transparent 60%);pointer-events:none}
.dir-card:hover{border-color:rgba(20,184,166,.3);box-shadow:0 0 0 1px rgba(20,184,166,.1),0 8px 24px rgba(0,0,0,.3);transform:translateY(-1px)}
.dir-card-icon{width:40px;height:40px;background:rgba(20,184,166,.08);border:1px solid rgba(20,184,166,.15);border-radius:10px;display:flex;align-items:center;justify-content:center;margin-bottom:14px;color:#2dd4bf}
.dir-alias{font-size:15px;font-weight:700;color:#f1f5f9;margin-bottom:5px;word-break:break-all;letter-spacing:-.01em}
.dir-path{font-size:11px;color:#334155;margin-bottom:18px;word-break:break-all;line-height:1.4;font-family:monospace}
.btn{display:inline-flex;align-items:center;gap:6px;padding:8px 18px;background:#0d9488;color:#fff;border:none;border-radius:9px;font-size:13px;font-weight:600;cursor:pointer;text-decoration:none;transition:background .15s,transform .1s}
.btn:hover{background:#0f766e;color:#fff;transform:translateY(-1px)}
.btn:active{transform:scale(.98)}
.btn-sm{display:inline-flex;align-items:center;gap:4px;padding:4px 12px;font-size:11px;font-weight:600;border-radius:7px;background:rgba(20,184,166,.1);color:#2dd4bf;border:1px solid rgba(20,184,166,.2);text-decoration:none;transition:all .15s;white-space:nowrap}
.btn-sm:hover{background:rgba(20,184,166,.18);color:#5eead4}
.breadcrumb{display:flex;flex-wrap:wrap;align-items:center;gap:4px;font-size:12px;color:#334155;margin-bottom:20px;padding:10px 14px;background:rgba(255,255,255,.03);border:1px solid rgba(255,255,255,.06);border-radius:10px}
.breadcrumb a{color:#475569;transition:color .15s}
.breadcrumb a:hover{color:#94a3b8}
.breadcrumb-sep{color:#1e293b;padding:0 2px}
.table-wrap{background:rgba(10,14,22,.7);border:1px solid rgba(255,255,255,.07);border-radius:14px;overflow:hidden}
table{width:100%;border-collapse:collapse;font-size:13px}
th{text-align:left;padding:10px 16px;color:#64748b;font-size:12px;font-weight:600;background:rgba(0,0,0,.3);border-bottom:1px solid rgba(255,255,255,.05)}
td{padding:11px 16px;border-top:1px solid rgba(255,255,255,.04);color:#94a3b8;vertical-align:middle}
tr:hover td{background:rgba(255,255,255,.025);color:#cbd5e1}
.name-cell{display:flex;align-items:center;gap:10px;min-width:0}
.name-cell a{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:#cbd5e1;font-weight:500;transition:color .15s;font-size:13px}
.name-cell a:hover{color:#f1f5f9}
.file-icon{width:28px;height:28px;border-radius:7px;display:flex;align-items:center;justify-content:center;flex-shrink:0;font-size:14px}
.file-icon-dir{background:rgba(20,184,166,.08);color:#2dd4bf}
.file-icon-file{background:rgba(148,163,184,.06);color:#64748b}
.col-size,.col-date{color:#475569;white-space:nowrap;width:120px;font-size:12px;font-family:monospace}
.col-actions{white-space:nowrap;text-align:right;width:130px}
.empty{text-align:center;padding:60px 0;color:#1e293b;font-size:14px}
.empty-icon{font-size:36px;display:block;margin-bottom:12px;opacity:.4}
@media(max-width:640px){
  .col-size,.col-date{display:none}
  .header-sub{display:none}
  .col-actions{width:44px}
  .btn-sm span{display:none}
  .btn-sm{padding:6px 8px;border-radius:8px;justify-content:center}
}"#
}

fn root_html(dirs: &[SharedDir], lang: &str) -> String {
    let title = if lang == "zh" {
        "局域网文件共享"
    } else {
        "LAN File Share"
    };
    let subtitle = if lang == "zh" {
        "选择一个目录开始浏览"
    } else {
        "Select a directory to start browsing"
    };
    let btn_browse = if lang == "zh" { "浏览" } else { "Browse" };
    let empty_msg = if lang == "zh" {
        "暂无共享目录"
    } else {
        "No shared directories"
    };

    let cards: String = dirs
        .iter()
        .map(|d| {
            let alias = html_escape(&d.alias);
            let path = html_escape(&d.path);
            let href = format!("/browse/{}/", url_encode(&d.alias));
            format!(
                r#"<div class="dir-card" onclick="location.href='{href}'">
  <div class="dir-card-icon">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
  </div>
  <div class="dir-alias">{alias}</div>
  <div class="dir-path">{path}</div>
  <a href="{href}" class="btn">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14"/><path d="m12 5 7 7-7 7"/></svg>
    {btn_browse}
  </a>
</div>"#
            )
        })
        .collect();

    let body = if dirs.is_empty() {
        format!(r#"<div class="empty"><span class="empty-icon">📂</span>{empty_msg}</div>"#)
    } else {
        format!(r#"<div class="section-label">{subtitle}</div><div class="card-grid">{cards}</div>"#)
    };

    let css = common_css();
    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>{css}</style>
</head>
<body>
<div class="header">
  <div class="header-icon">
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
  </div>
  <div class="header-info">
    <div class="header-title">{title}</div>
    <div class="header-sub">{subtitle}</div>
  </div>
</div>
<div class="container">{body}</div>
</body>
</html>"#
    )
}

fn browse_html(alias: &str, rel: &str, entries: &[DirEntry], lang: &str) -> String {
    let title = if lang == "zh" {
        "文件浏览"
    } else {
        "File Browser"
    };
    let col_name = if lang == "zh" { "名称" } else { "Name" };
    let col_size = if lang == "zh" { "大小" } else { "Size" };
    let col_date = if lang == "zh" {
        "修改时间"
    } else {
        "Modified"
    };
    let col_actions = if lang == "zh" { "操作" } else { "Actions" };
    let btn_zip = if lang == "zh" {
        "下载 ZIP"
    } else {
        "Download ZIP"
    };
    let home_lbl = if lang == "zh" { "首页" } else { "Home" };
    let parent_lbl = if lang == "zh" {
        "上级目录"
    } else {
        "Parent"
    };
    let empty_msg = if lang == "zh" {
        "此目录为空"
    } else {
        "This directory is empty"
    };

    // ── Breadcrumb ──
    let mut breadcrumb = format!(
        r#"<a href="/">{home_lbl}</a><span class="breadcrumb-sep">›</span><a href="/browse/{alias}/">{alias}</a>"#
    );
    if !rel.trim_matches('/').is_empty() {
        let mut acc = format!("/browse/{}", alias);
        for seg in rel.trim_matches('/').split('/').filter(|s| !s.is_empty()) {
            acc.push('/');
            acc.push_str(seg);
            let seg_safe = html_escape(seg);
            breadcrumb.push_str(&format!(
                r#"<span class="breadcrumb-sep">›</span><a href="{acc}/">{seg_safe}</a>"#
            ));
        }
    }

    // ── Parent dir row ──
    let parent_row = if !rel.trim_matches('/').is_empty() {
        let parent = {
            let r = rel.trim_matches('/');
            match r.rfind('/') {
                Some(i) => format!("/browse/{}/{}/", alias, &r[..i]),
                None => format!("/browse/{}/", alias),
            }
        };
        format!(
            r#"<tr>
  <td><div class="name-cell"><div class="file-icon file-icon-dir"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="15 18 9 12 15 6"/></svg></div><a href="{parent}">{parent_lbl}</a></div></td>
  <td class="col-size">—</td><td class="col-date">—</td><td class="col-actions"></td>
</tr>"#
        )
    } else {
        String::new()
    };

    // ── File/dir rows ──
    let rel_clean = rel.trim_matches('/');
    let rows: String = entries
        .iter()
        .map(|e| {
            let name_safe = html_escape(&e.name);
            let name_encoded = url_encode(&e.name);
            let (name_link, zip_cell, icon_class, icon_svg) = if e.is_dir {
                let href = if rel_clean.is_empty() {
                    format!("/browse/{}/{}/", alias, name_encoded)
                } else {
                    format!("/browse/{}/{}/{}/", alias, rel_clean, name_encoded)
                };
                let zip_href = if rel_clean.is_empty() {
                    format!("/zip/{}/{}", alias, name_encoded)
                } else {
                    format!("/zip/{}/{}/{}", alias, rel_clean, name_encoded)
                };
                (
                    format!(r#"<a href="{href}">{name_safe}</a>"#),
                    format!(r#"<a href="{zip_href}" class="btn btn-sm"><svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg><span>{btn_zip}</span></a>"#),
                    "file-icon-dir",
                    r#"<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>"#,
                )
            } else {
                let href = if rel_clean.is_empty() {
                    format!("/file/{}/{}", alias, name_encoded)
                } else {
                    format!("/file/{}/{}/{}", alias, rel_clean, name_encoded)
                };
                (
                    format!(r#"<a href="{href}">{name_safe}</a>"#),
                    String::new(),
                    "file-icon-file",
                    r#"<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>"#,
                )
            };
            let size_str = if e.is_dir {
                "—".to_string()
            } else {
                fmt_size(e.size)
            };
            let date = html_escape(&e.modified);
            format!(
                r#"<tr>
  <td><div class="name-cell"><div class="file-icon {icon_class}">{icon_svg}</div>{name_link}</div></td>
  <td class="col-size">{size_str}</td>
  <td class="col-date">{date}</td>
  <td class="col-actions">{zip_cell}</td>
</tr>"#
            )
        })
        .collect();

    let table_or_empty = if entries.is_empty() && rel.trim_matches('/').is_empty() {
        format!(r#"<div class="empty">{empty_msg}</div>"#)
    } else {
        format!(
            r#"<div class="table-wrap">
<table>
<thead><tr>
  <th>{col_name}</th>
  <th class="col-size">{col_size}</th>
  <th class="col-date">{col_date}</th>
  <th class="col-actions">{col_actions}</th>
</tr></thead>
<tbody>{parent_row}{rows}{empty_in_dir}</tbody>
</table>
</div>"#,
            empty_in_dir = if entries.is_empty() && !rel.trim_matches('/').is_empty() {
                format!(
                    r#"<tr><td colspan="4"><div class="empty" style="padding:32px 0">{empty_msg}</div></td></tr>"#
                )
            } else {
                String::new()
            }
        )
    };

    let css = common_css();
    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>{css}</style>
</head>
<body>
<div class="header">
  <div class="header-icon">
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
  </div>
  <div class="header-info">
    <div class="header-title">{title}</div>
  </div>
</div>
<div class="container">
  <nav class="breadcrumb">{breadcrumb}</nav>
  {table_or_empty}
</div>
</body>
</html>"#
    )
}

fn login_html(has_error: bool, lang: &str) -> String {
    let (title, heading, hint, placeholder, btn_label) = if lang == "zh" {
        ("访问验证", "受密码保护", "请输入访问密码", "密码", "进入")
    } else {
        (
            "Access Required",
            "Password Protected",
            "Enter the password to access this share",
            "Password",
            "Enter",
        )
    };
    let err_msg = if lang == "zh" {
        "密码错误，请重试"
    } else {
        "Incorrect password, please try again"
    };
    let err_block = if has_error {
        format!(r#"<div class="err"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>{err_msg}</div>"#)
    } else {
        String::new()
    };

    let css = r#"*{margin:0;padding:0;box-sizing:border-box}
html,body{height:100%;background:#060911;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}
.bg{height:100%;display:flex;align-items:center;justify-content:center;padding:20px;background:radial-gradient(ellipse at 50% 0%,rgba(20,184,166,.07) 0%,transparent 55%)}
.card{background:rgba(15,23,42,.9);border:1px solid rgba(255,255,255,.08);border-radius:20px;padding:36px 32px;width:100%;max-width:380px;box-shadow:0 25px 50px rgba(0,0,0,.5)}
.icon{width:44px;height:44px;background:rgba(20,184,166,.1);border:1px solid rgba(20,184,166,.18);border-radius:12px;display:flex;align-items:center;justify-content:center;margin-bottom:20px;color:#2dd4bf}
h1{font-size:20px;font-weight:700;color:#f1f5f9;margin-bottom:6px;letter-spacing:-.01em}
.desc{font-size:14px;color:#475569;margin-bottom:24px;line-height:1.5}
.field label{display:block;font-size:12px;font-weight:600;color:#64748b;margin-bottom:6px;text-transform:uppercase;letter-spacing:.05em}
input{width:100%;padding:10px 14px;background:rgba(0,0,0,.3);border:1px solid rgba(255,255,255,.09);border-radius:10px;color:#e2e8f0;font-size:14px;outline:none;transition:border-color .15s}
input:focus{border-color:rgba(20,184,166,.45);box-shadow:0 0 0 3px rgba(20,184,166,.08)}
input::placeholder{color:#334155}
button{width:100%;margin-top:18px;padding:11px;background:#0d9488;color:#fff;border:none;border-radius:10px;font-size:14px;font-weight:600;cursor:pointer;transition:background .15s;letter-spacing:.01em}
button:hover{background:#0f766e}
button:active{transform:scale(.99)}
.err{display:flex;align-items:center;gap:6px;color:#f87171;font-size:13px;margin-top:14px;background:rgba(239,68,68,.08);border:1px solid rgba(239,68,68,.15);border-radius:8px;padding:9px 12px}"#;

    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>{css}</style>
</head>
<body>
<div class="bg">
  <form class="card" method="POST" action="/auth">
    <div class="icon">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
    </div>
    <h1>{heading}</h1>
    <p class="desc">{hint}</p>
    <div class="field"><label>{placeholder}</label><input type="password" name="password" placeholder="••••••••" autofocus required></div>
    <button type="submit">{btn_label}</button>
    {err_block}
  </form>
</div>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::{
        remember_connected_ip, snapshot_connected_ips, validate_zip_source_with_limits,
        ZipSourceStats,
    };
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "fst-fileshare-test-{}-{}",
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

    #[test]
    fn validate_zip_source_counts_nested_files_and_bytes() {
        let dir = TestDir::new("stats");
        let nested = dir.path().join("nested");
        fs::create_dir_all(&nested).expect("nested dir should be created");
        fs::write(dir.path().join("a.txt"), b"abc").expect("root file should be created");
        fs::write(nested.join("b.txt"), b"12345").expect("nested file should be created");

        let stats = validate_zip_source_with_limits(dir.path(), 64, 10)
            .expect("directory should fit within limits");

        assert_eq!(
            stats,
            ZipSourceStats {
                file_count: 2,
                total_bytes: 8
            }
        );
    }

    #[test]
    fn validate_zip_source_rejects_oversized_directories() {
        let dir = TestDir::new("size-limit");
        fs::write(dir.path().join("big.bin"), vec![0u8; 11])
            .expect("oversized file should be created");

        let error = validate_zip_source_with_limits(dir.path(), 10, 10)
            .expect_err("directory should exceed byte limit");

        assert!(error.contains("too large"));
    }

    #[test]
    fn validate_zip_source_rejects_too_many_files() {
        let dir = TestDir::new("file-limit");
        fs::write(dir.path().join("one.txt"), b"1").expect("first file should be created");
        fs::write(dir.path().join("two.txt"), b"2").expect("second file should be created");

        let error = validate_zip_source_with_limits(dir.path(), 64, 1)
            .expect_err("directory should exceed file count limit");

        assert!(error.contains("too many files"));
    }

    #[test]
    fn remember_connected_ip_tracks_unique_addresses() {
        let visitor_ips = Arc::new(Mutex::new(HashSet::new()));

        remember_connected_ip(&visitor_ips, "192.168.0.8");
        remember_connected_ip(&visitor_ips, "192.168.0.8");
        remember_connected_ip(&visitor_ips, "192.168.0.12");

        assert_eq!(
            snapshot_connected_ips(&visitor_ips),
            vec!["192.168.0.12".to_string(), "192.168.0.8".to_string()]
        );
    }
}
