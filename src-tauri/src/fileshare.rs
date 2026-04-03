use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::Body;
use axum::extract::{Form, Path as AxumPath, Query, State as AxumState};
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
    pub download_count: u32,
    pub uptime_secs: u64,
    pub server_url: String,
    pub all_urls: Vec<String>,
    pub shared_dirs: Vec<SharedDir>,
}

// ─── Handle (stored in AppState) ────────────────────────────

pub struct FileShareHandle {
    active: Arc<AtomicBool>,
    connection_count: Arc<AtomicU32>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    server_url: Mutex<String>,
    all_urls: Mutex<Vec<String>>,
    start_time: Mutex<Option<Instant>>,
    shared_dirs: Mutex<Vec<SharedDir>>,
}

impl FileShareHandle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            connection_count: Arc::new(AtomicU32::new(0)),
            shutdown_tx: Mutex::new(None),
            server_url: Mutex::new(String::new()),
            all_urls: Mutex::new(Vec::new()),
            start_time: Mutex::new(None),
            shared_dirs: Mutex::new(Vec::new()),
        }
    }
}

// ─── Internal HTTP State ─────────────────────────────────────

struct HttpState {
    shared_dirs: Vec<SharedDir>,
    password_hash: Option<String>,
    connection_count: Arc<AtomicU32>,
}

/// Decrements connection_count when dropped.
struct ConnectionGuard(Arc<AtomicU32>);

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        loop {
            let cur = self.0.load(Ordering::Relaxed);
            if cur == 0 {
                break;
            }
            if self
                .0
                .compare_exchange_weak(cur, cur - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
}

/// Deletes the temp file when dropped.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
        connection_count: handle.connection_count.clone(),
    });

    handle.connection_count.store(0, Ordering::Relaxed);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    *handle.shutdown_tx.lock().unwrap() = Some(shutdown_tx);
    *handle.server_url.lock().unwrap() = server_url.clone();
    *handle.all_urls.lock().unwrap() = all_urls.clone();
    *handle.start_time.lock().unwrap() = Some(Instant::now());
    *handle.shared_dirs.lock().unwrap() = config.shared_dirs.clone();
    handle.active.store(true, Ordering::SeqCst);

    // Bind listener BEFORE spawning so that bind errors propagate to the caller
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| {
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
                    download_count: 0,
                    uptime_secs: 0,
                    server_url: String::new(),
                    all_urls: Vec::new(),
                    shared_dirs: Vec::new(),
                },
            );
        }
    });

    // Status reporter
    let reporter_app = app_handle.clone();
    let reporter_active = handle.active.clone();
    let reporter_count = handle.connection_count.clone();
    let reporter_url = server_url.clone();
    let reporter_all_urls = all_urls.clone();
    let reporter_dirs = config.shared_dirs.clone();
    let reporter_start = Instant::now();
    tokio::spawn(async move {
        status_reporter(
            reporter_app,
            reporter_active,
            reporter_count,
            reporter_url,
            reporter_all_urls,
            reporter_dirs,
            reporter_start,
        )
        .await;
    });

    let dir_names: Vec<&str> = config.shared_dirs.iter().map(|d| d.alias.as_str()).collect();
    crate::scanner::emit_tool_log(
        &app_handle,
        TOOL_NAME,
        &format!("已启动，共享 {} 个目录 [{}]，访问: {}", config.shared_dirs.len(), dir_names.join(", "), server_url),
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

    handle.connection_count.store(0, Ordering::Relaxed);
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
            download_count: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
            shared_dirs: Vec::new(),
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
            download_count: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
            shared_dirs: Vec::new(),
        };
    }
    let uptime = handle
        .start_time
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    FileShareStatus {
        is_active: true,
        download_count: handle.connection_count.load(Ordering::Relaxed),
        uptime_secs: uptime,
        server_url: handle.server_url.lock().unwrap().clone(),
        all_urls: handle.all_urls.lock().unwrap().clone(),
        shared_dirs: handle.shared_dirs.lock().unwrap().clone(),
    }
}

// ─── HTTP Server ─────────────────────────────────────────────

async fn run_http_server(listener: tokio::net::TcpListener, state: Arc<HttpState>, shutdown_rx: oneshot::Receiver<()>) {
    let app = Router::new()
        .route("/", get(handler_root))
        .route("/browse/*path", get(handler_browse))
        .route("/file/*path", get(handler_file))
        .route("/zip/*path", get(handler_zip))
        .route("/login", get(handler_login_page))
        .route("/auth", post(handler_auth))
        .with_state(state);

    if let Err(e) = axum::serve(listener, app)
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
) -> Response {
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return redirect_login();
        }
    }
    let lang = detect_lang(&headers, q.lang.as_deref());
    Html(root_html(&state.shared_dirs, lang)).into_response()
}

async fn handler_browse(
    AxumState(state): AxumState<Arc<HttpState>>,
    headers: HeaderMap,
    Query(q): Query<LangQuery>,
    AxumPath(path): AxumPath<String>,
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

    state.connection_count.fetch_add(1, Ordering::Relaxed);
    let guard = ConnectionGuard(state.connection_count.clone());

    let stream = async_stream::stream! {
        let _g = guard;
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

    state.connection_count.fetch_add(1, Ordering::Relaxed);
    let conn_count = state.connection_count.clone();

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
        // Decrement immediately on failure
        drop(ConnectionGuard(conn_count));
        let _ = std::fs::remove_file(&tmp_path);
        return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to create zip");
    }

    let tmp = TempFile(tmp_path.clone());
    let file = match tokio::fs::File::open(&tmp_path).await {
        Ok(f) => f,
        Err(_) => {
            drop(ConnectionGuard(conn_count));
            return plain_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to open zip");
        }
    };

    let stream = async_stream::stream! {
        let _g = ConnectionGuard(conn_count);
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

async fn handler_login_page(
    headers: HeaderMap,
    Query(q): Query<LoginQuery>,
) -> Response {
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
    connection_count: Arc<AtomicU32>,
    server_url: String,
    all_urls: Vec<String>,
    shared_dirs: Vec<SharedDir>,
    start_time: Instant,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        interval.tick().await;
        if !active.load(Ordering::Relaxed) {
            break;
        }
        let _ = app_handle.emit(
            "file-share-status",
            FileShareStatus {
                is_active: true,
                download_count: connection_count.load(Ordering::Relaxed),
                uptime_secs: start_time.elapsed().as_secs(),
                server_url: server_url.clone(),
                all_urls: all_urls.clone(),
                shared_dirs: shared_dirs.clone(),
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
    const MAX_DEPTH: usize = 32;
    if depth > MAX_DEPTH {
        return Err(format!("Directory nesting too deep (>{MAX_DEPTH} levels)"));
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
            cookies
                .split(';')
                .any(|c| c.trim().strip_prefix("fs_auth=").is_some_and(|v| v == expected))
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
            || o[0] == 172 && (16..=31).contains(&o[1])      // 172.16.0.0/12
    }
    ips.sort_by_key(|ip| if is_common_lan(ip) { 0 } else { 1 });

    ips.into_iter().map(|ip| ip.to_string()).collect()
}

// ─── HTML Templates ──────────────────────────────────────────

fn common_css() -> &'static str {
    r#"*{margin:0;padding:0;box-sizing:border-box}
html,body{min-height:100%;background:#0f172a;color:#e2e8f0;font-family:system-ui,-apple-system,sans-serif}
a{color:#60a5fa;text-decoration:none}a:hover{color:#93c5fd}
.header{background:#1e293b;border-bottom:1px solid #334155;padding:14px 20px;display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.header-title{font-size:17px;font-weight:700;color:#f1f5f9}
.header-sub{font-size:12px;color:#64748b;margin-top:2px}
.container{max-width:1000px;margin:0 auto;padding:20px 16px}
.card-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:14px}
.dir-card{background:#1e293b;border:1px solid #334155;border-radius:14px;padding:22px;transition:border-color .15s,box-shadow .15s}
.dir-card:hover{border-color:#3b82f6;box-shadow:0 0 0 1px #3b82f640}
.dir-alias{font-size:16px;font-weight:700;color:#f1f5f9;margin-bottom:6px;word-break:break-all}
.dir-path{font-size:12px;color:#64748b;margin-bottom:18px;word-break:break-all}
.btn{display:inline-block;padding:8px 20px;background:#3b82f6;color:#fff;border:none;border-radius:8px;font-size:14px;font-weight:600;cursor:pointer;text-decoration:none;transition:background .15s}
.btn:hover{background:#2563eb;color:#fff}
.btn-sm{padding:4px 12px;font-size:12px;border-radius:6px;background:#334155;color:#e2e8f0}
.btn-sm:hover{background:#475569;color:#fff}
.breadcrumb{display:flex;flex-wrap:wrap;align-items:center;gap:6px;font-size:13px;color:#94a3b8;margin-bottom:18px}
.breadcrumb-sep{color:#475569;padding:0 2px}
.table-wrap{background:#1e293b;border:1px solid #334155;border-radius:12px;overflow:hidden}
table{width:100%;border-collapse:collapse;font-size:14px}
th{text-align:left;padding:10px 14px;color:#64748b;font-size:11px;text-transform:uppercase;letter-spacing:.06em;background:#0f172a;font-weight:600}
td{padding:10px 14px;border-top:1px solid #1e293b;color:#cbd5e1;vertical-align:middle}
tr:hover td{background:rgba(255,255,255,.025)}
.name-cell{display:flex;align-items:center;gap:8px;min-width:0}
.name-cell a{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.icon{font-size:15px;flex-shrink:0}
.col-size,.col-date{color:#64748b;white-space:nowrap;width:120px}
.col-actions{white-space:nowrap;text-align:right;width:140px}
.empty{text-align:center;padding:56px 0;color:#475569;font-size:14px}
@media(max-width:640px){.col-size,.col-date{display:none}.header-sub{display:none}}"#
}

fn root_html(dirs: &[SharedDir], lang: &str) -> String {
    let title = if lang == "zh" { "局域网文件共享" } else { "LAN File Share" };
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
                r#"<div class="dir-card">
  <div class="dir-alias">📁 {alias}</div>
  <div class="dir-path">{path}</div>
  <a href="{href}" class="btn">{btn_browse}</a>
</div>"#
            )
        })
        .collect();

    let body = if dirs.is_empty() {
        format!(r#"<div class="empty">{empty_msg}</div>"#)
    } else {
        format!(r#"<div class="card-grid">{cards}</div>"#)
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
  <div>
    <div class="header-title">📂 {title}</div>
    <div class="header-sub">{subtitle}</div>
  </div>
</div>
<div class="container" style="margin-top:20px">{body}</div>
</body>
</html>"#
    )
}

fn browse_html(alias: &str, rel: &str, entries: &[DirEntry], lang: &str) -> String {
    let title = if lang == "zh" { "文件浏览" } else { "File Browser" };
    let col_name = if lang == "zh" { "名称" } else { "Name" };
    let col_size = if lang == "zh" { "大小" } else { "Size" };
    let col_date = if lang == "zh" { "修改时间" } else { "Modified" };
    let col_actions = if lang == "zh" { "操作" } else { "Actions" };
    let btn_zip = if lang == "zh" { "下载 ZIP" } else { "Download ZIP" };
    let home_lbl = if lang == "zh" { "首页" } else { "Home" };
    let parent_lbl = if lang == "zh" { "上级目录" } else { "Parent" };
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
        format!(r#"<tr>
  <td><div class="name-cell"><span class="icon">⬆</span><a href="{parent}">{parent_lbl}</a></div></td>
  <td class="col-size">—</td><td class="col-date">—</td><td class="col-actions"></td>
</tr>"#)
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
            let (name_link, zip_cell) = if e.is_dir {
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
                    format!(r#"<a href="{zip_href}" class="btn btn-sm">{btn_zip}</a>"#),
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
                )
            };
            let icon = if e.is_dir { "📁" } else { "📄" };
            let size_str = if e.is_dir {
                "—".to_string()
            } else {
                fmt_size(e.size)
            };
            let date = html_escape(&e.modified);
            format!(
                r#"<tr>
  <td><div class="name-cell"><span class="icon">{icon}</span>{name_link}</div></td>
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
  <div class="header-title">📂 {title}</div>
</div>
<div class="container">
  <nav class="breadcrumb" style="margin-top:16px">{breadcrumb}</nav>
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
        format!(r#"<div class="err">{err_msg}</div>"#)
    } else {
        String::new()
    };

    // Inline CSS (no common_css to avoid extra CSS blocks)
    let css = r#"*{margin:0;padding:0;box-sizing:border-box}
html,body{height:100%;background:#0f172a;color:#e2e8f0;font-family:system-ui,-apple-system,sans-serif}
.center{height:100%;display:flex;align-items:center;justify-content:center;padding:16px}
.card{background:#1e293b;border-radius:16px;padding:32px;width:360px;border:1px solid #334155}
h1{font-size:20px;margin-bottom:6px;color:#f1f5f9}
p{font-size:14px;color:#94a3b8;margin-bottom:20px}
input{width:100%;padding:10px 14px;background:#0f172a;border:1px solid #334155;border-radius:8px;color:#e2e8f0;font-size:15px;outline:none}
input:focus{border-color:#3b82f6}
button{width:100%;margin-top:14px;padding:10px;background:#3b82f6;color:#fff;border:none;border-radius:8px;font-size:15px;font-weight:600;cursor:pointer}
button:hover{background:#2563eb}
.err{color:#f87171;font-size:13px;margin-top:12px;text-align:center}"#;

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
<div class="center">
  <form class="card" method="POST" action="/auth">
    <h1>🔒 {heading}</h1>
    <p>{hint}</p>
    <input type="password" name="password" placeholder="{placeholder}" autofocus required>
    <button type="submit">{btn_label}</button>
    {err_block}
  </form>
</div>
</body>
</html>"#
    )
}
