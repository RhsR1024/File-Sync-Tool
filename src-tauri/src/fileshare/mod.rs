use std::collections::HashSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::body::Body;
use axum::extract::Multipart;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::oneshot;

pub mod model;
pub mod auth;
pub mod persist;
pub mod ops;
pub mod search;
pub mod web_assets;
pub mod http;


// ─── Public Data Types ──────────────────────────────────────

const TOOL_NAME: &str = "文件共享";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
struct RuntimeFileShareConfig {
    port: u16,
    roots: Vec<model::FileShareRoot>,
    guest_access_enabled: bool,
    accounts: Vec<model::PersistedFileShareAccount>,
    session_ttl_minutes: u32,
    ip_filter_mode: model::IpFilterMode,
    ip_rules: Vec<String>,
    image_preview_enabled: bool,
    delete_mode: model::DeleteMode,
}

// ─── Handle (stored in AppState) ────────────────────────────

#[derive(Debug, Clone)]
struct FileShareRuntimeSnapshot {
    server_url: String,
    all_urls: Vec<String>,
    shared_dirs: Vec<SharedDir>,
    start_time: Instant,
}

struct FileShareRuntime {
    shutdown_tx: oneshot::Sender<()>,
    snapshot: FileShareRuntimeSnapshot,
}

pub struct FileShareHandle {
    active: Arc<AtomicBool>,
    runtime: Mutex<Option<FileShareRuntime>>,
    visitor_ips: Arc<Mutex<HashSet<String>>>,
}

impl FileShareHandle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            runtime: Mutex::new(None),
            visitor_ips: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn set_runtime(&self, runtime: FileShareRuntime) {
        *self.runtime.lock().unwrap() = Some(runtime);
    }

    fn take_runtime(&self) -> Option<FileShareRuntime> {
        self.runtime.lock().unwrap().take()
    }

    fn runtime_snapshot(&self) -> Option<FileShareRuntimeSnapshot> {
        self.runtime
            .lock()
            .unwrap()
            .as_ref()
            .map(|runtime| runtime.snapshot.clone())
    }
}

// ─── Internal HTTP State ─────────────────────────────────────

struct HttpState {
    config: RuntimeFileShareConfig,
    roots: Vec<ops::ResolvedRoot>,
    sessions: Mutex<auth::SessionStore>,
    ip_rules: Vec<auth::IpRule>,
    upload_body_limit_bytes: usize,
    visitor_ips: Arc<Mutex<HashSet<String>>>,
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
    let runtime_config = runtime_config_from_legacy(config.clone())?;

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

    let http_state = Arc::new(HttpState {
        roots: runtime_roots(&runtime_config),
        sessions: Mutex::new(auth::SessionStore::default()),
        ip_rules: parse_runtime_ip_rules(&runtime_config)?,
        upload_body_limit_bytes: http::UPLOAD_BODY_LIMIT_BYTES,
        config: runtime_config,
        visitor_ips: handle.visitor_ips.clone(),
    });

    if let Ok(mut ips) = handle.visitor_ips.lock() {
        ips.clear();
    }
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let start_time = Instant::now();

    // Bind listener BEFORE spawning so that bind errors propagate to the caller
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        let msg = format!("Failed to bind port {}: {}", config.port, e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        msg
    })?;
    log::info!("File share HTTP server listening on {}", addr);

    handle.set_runtime(FileShareRuntime {
        shutdown_tx,
        snapshot: FileShareRuntimeSnapshot {
            server_url: server_url.clone(),
            all_urls: all_urls.clone(),
            shared_dirs: config.shared_dirs.clone(),
            start_time,
        },
    });
    handle.active.store(true, Ordering::SeqCst);

    // Clone state needed for cleanup when server stops unexpectedly
    let server_active = handle.active.clone();
    let server_app = app_handle.clone();
    let server_handle_state = state.file_share.clone();
    let server_handle = tokio::spawn(async move {
        http::run_http_server(listener, http_state, shutdown_rx).await;
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
            let _ = server_handle_state.take_runtime();
            if let Ok(mut ips) = server_handle_state.visitor_ips.lock() {
                ips.clear();
            }
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
    let reporter_start = start_time;
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
pub async fn file_share_start_saved(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<String, String> {
    let handle = &state.file_share;

    if handle.active.load(Ordering::SeqCst) {
        return Err("File share is already active".into());
    }

    let saved = persist::load_persisted_file_share_config(&app_handle)?;
    let runtime_config = runtime_config_from_saved(saved)?;
    let shared_dirs = runtime_shared_dirs(&runtime_config);

    let all_ips = get_lan_ips();
    let local_ip = all_ips
        .first()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let server_url = format!("http://{}:{}", local_ip, runtime_config.port);
    let all_urls: Vec<String> = all_ips
        .iter()
        .map(|ip| format!("http://{}:{}", ip, runtime_config.port))
        .collect();
    let http_state = Arc::new(HttpState {
        roots: runtime_roots(&runtime_config),
        sessions: Mutex::new(auth::SessionStore::default()),
        ip_rules: parse_runtime_ip_rules(&runtime_config)?,
        upload_body_limit_bytes: http::UPLOAD_BODY_LIMIT_BYTES,
        config: runtime_config.clone(),
        visitor_ips: handle.visitor_ips.clone(),
    });

    if let Ok(mut ips) = handle.visitor_ips.lock() {
        ips.clear();
    }
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let start_time = Instant::now();

    let addr = format!("0.0.0.0:{}", runtime_config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        let msg = format!("Failed to bind port {}: {}", runtime_config.port, e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        msg
    })?;
    log::info!("File share HTTP server listening on {}", addr);

    handle.set_runtime(FileShareRuntime {
        shutdown_tx,
        snapshot: FileShareRuntimeSnapshot {
            server_url: server_url.clone(),
            all_urls: all_urls.clone(),
            shared_dirs: shared_dirs.clone(),
            start_time,
        },
    });
    handle.active.store(true, Ordering::SeqCst);

    let server_active = handle.active.clone();
    let server_app = app_handle.clone();
    let server_handle_state = state.file_share.clone();
    let server_handle = tokio::spawn(async move {
        http::run_http_server(listener, http_state, shutdown_rx).await;
    });

    tokio::spawn(async move {
        match server_handle.await {
            Ok(()) => {
                log::info!("File share HTTP server exited");
            }
            Err(e) => {
                let msg = format!("File share server exited unexpectedly: {}", e);
                log::error!("File share server crashed: {}", e);
                crate::scanner::emit_tool_log(&server_app, TOOL_NAME, &msg, "error");
                let _ = server_app.emit(
                    "file-share-log",
                    serde_json::json!({ "level": "error", "message": msg }),
                );
            }
        }

        if server_active.swap(false, Ordering::SeqCst) {
            let _ = server_handle_state.take_runtime();
            if let Ok(mut ips) = server_handle_state.visitor_ips.lock() {
                ips.clear();
            }
            crate::scanner::emit_tool_log(&server_app, TOOL_NAME, "File share stopped", "info");
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

    let reporter_app = app_handle.clone();
    let reporter_active = handle.active.clone();
    let reporter_url = server_url.clone();
    let reporter_all_urls = all_urls.clone();
    let reporter_dirs = shared_dirs.clone();
    let reporter_start = start_time;
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

    let dir_names: Vec<&str> = shared_dirs.iter().map(|d| d.alias.as_str()).collect();
    crate::scanner::emit_tool_log(
        &app_handle,
        TOOL_NAME,
        &format!(
            "Started file share with {} roots [{}] at {}",
            shared_dirs.len(),
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

    if let Some(runtime) = handle.take_runtime() {
        let _ = runtime.shutdown_tx.send(());
    }

    if let Ok(mut ips) = handle.visitor_ips.lock() {
        ips.clear();
    }

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
    let Some(runtime) = handle.runtime_snapshot() else {
        return FileShareStatus {
            is_active: false,
            connection_count: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
            shared_dirs: Vec::new(),
            connected_ips: Vec::new(),
        };
    };
    let connected_ips = snapshot_connected_ips(&handle.visitor_ips);
    FileShareStatus {
        is_active: true,
        connection_count: connected_ips.len() as u32,
        uptime_secs: runtime.start_time.elapsed().as_secs(),
        server_url: runtime.server_url,
        all_urls: runtime.all_urls,
        shared_dirs: runtime.shared_dirs,
        connected_ips,
    }
}

// ─── HTTP Server ─────────────────────────────────────────────

async fn status_reporter(
    app_handle: AppHandle,
    active: Arc<AtomicBool>,
    server_url: String,
    all_urls: Vec<String>,
    shared_dirs: Vec<SharedDir>,
    start_time: Instant,
    visitor_ips: Arc<Mutex<HashSet<String>>>,
) {
    while active.load(Ordering::Relaxed) {
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

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn split_alias_path(path: &str) -> (&str, &str) {
    let trimmed = path.trim_matches('/');
    match trimmed.split_once('/') {
        Some((alias, rel)) => (alias, rel),
        None => (trimmed, ""),
    }
}


const SESSION_COOKIE_NAME: &str = "fs_session";

struct UploadedFilePayload {
    relative_path: String,
    contents: Vec<u8>,
}

fn runtime_config_from_legacy(config: FileShareConfig) -> Result<RuntimeFileShareConfig, String> {
    if config.port < 1024 {
        return Err("Port must be >= 1024".to_string());
    }
    if config.shared_dirs.is_empty() {
        return Err("At least one shared directory is required".to_string());
    }

    let roots = config
        .shared_dirs
        .iter()
        .map(|dir| model::FileShareRoot {
            id: dir.alias.clone(),
            alias: dir.alias.clone(),
            path: dir.path.clone(),
            enabled: true,
        })
        .collect::<Vec<_>>();

    Ok(RuntimeFileShareConfig {
        port: config.port,
        roots,
        guest_access_enabled: true,
        accounts: vec![model::PersistedFileShareAccount {
            id: model::GUEST_ACCOUNT_ID.to_string(),
            name: model::GUEST_ACCOUNT_NAME.to_string(),
            enabled: true,
            preset: model::PermissionPreset::ReadOnly,
            permissions: model::FileSharePermissionSet::read_only(),
            password_hash: config.password.map(|password| hash_password(&password)),
        }],
        session_ttl_minutes: 30,
        ip_filter_mode: model::IpFilterMode::Off,
        ip_rules: Vec::new(),
        image_preview_enabled: true,
        delete_mode: model::DeleteMode::RecycleBin,
    })
}

fn runtime_config_from_saved(
    config: model::PersistedFileShareConfig,
) -> Result<RuntimeFileShareConfig, String> {
    if config.port < 1024 {
        return Err("Port must be >= 1024".to_string());
    }

    let roots = config
        .roots
        .into_iter()
        .filter(|root| root.enabled)
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Err("At least one enabled shared directory is required".to_string());
    }

    for root in &roots {
        if !Path::new(&root.path).is_dir() {
            return Err(format!("Directory not found: {}", root.path));
        }
    }

    Ok(RuntimeFileShareConfig {
        port: config.port,
        roots,
        guest_access_enabled: config.guest_access_enabled,
        accounts: config.accounts,
        session_ttl_minutes: config
            .session_ttl_minutes
            .clamp(1, model::MAX_SESSION_TTL_MINUTES),
        ip_filter_mode: config.ip_filter_mode,
        ip_rules: config.ip_rules,
        image_preview_enabled: config.image_preview_enabled,
        delete_mode: config.delete_mode,
    })
}

fn runtime_shared_dirs(config: &RuntimeFileShareConfig) -> Vec<SharedDir> {
    config
        .roots
        .iter()
        .map(|root| SharedDir {
            alias: root.alias.clone(),
            path: root.path.clone(),
        })
        .collect()
}

fn runtime_roots(config: &RuntimeFileShareConfig) -> Vec<ops::ResolvedRoot> {
    config
        .roots
        .iter()
        .map(|root| ops::ResolvedRoot {
            id: root.id.clone(),
            alias: root.alias.clone(),
            path: PathBuf::from(&root.path),
        })
        .collect()
}

fn parse_runtime_ip_rules(config: &RuntimeFileShareConfig) -> Result<Vec<auth::IpRule>, String> {
    auth::parse_ip_rules(&config.ip_rules)
}

fn find_root(state: &HttpState, key: &str) -> Option<ops::ResolvedRoot> {
    state
        .roots
        .iter()
        .find(|root| root.id == key || root.alias == key)
        .cloned()
}

fn find_enabled_account<'a>(
    config: &'a RuntimeFileShareConfig,
    account_id: &str,
) -> Option<&'a model::PersistedFileShareAccount> {
    config.accounts.iter().find(|account| {
        account.enabled
            && account.id == account_id
            && (account.id != model::GUEST_ACCOUNT_ID || config.guest_access_enabled)
    })
}

fn principal_for_account(
    account: &model::PersistedFileShareAccount,
) -> auth::ResolvedPrincipal {
    auth::ResolvedPrincipal {
        account_id: account.id.clone(),
        permissions: account.permissions.clone(),
    }
}

fn build_session_response(
    state: &HttpState,
    principal: &auth::ResolvedPrincipal,
) -> http::ApiSessionResponse {
    let account = state
        .config
        .accounts
        .iter()
        .find(|account| account.id == principal.account_id);
    http::ApiSessionResponse {
        account_id: principal.account_id.clone(),
        account_name: account
            .map(|account| account.name.clone())
            .unwrap_or_else(|| principal.account_id.clone()),
        is_guest: principal.account_id == model::GUEST_ACCOUNT_ID,
        permissions: principal.permissions.clone(),
    }
}

fn session_ttl(config: &RuntimeFileShareConfig) -> Duration {
    Duration::from_secs(u64::from(config.session_ttl_minutes.max(1)) * 60)
}

fn authenticate_account(
    state: &HttpState,
    account_id: &str,
    password: &str,
    client_ip: IpAddr,
) -> Result<(auth::ResolvedPrincipal, String), String> {
    let account = find_enabled_account(&state.config, account_id)
        .ok_or_else(|| "Account not found".to_string())?;
    if let Some(expected_hash) = &account.password_hash {
        if !verify_password_hash(expected_hash, password) {
            return Err("Invalid credentials".to_string());
        }
    }

    let principal = principal_for_account(account);
    let token = state
        .sessions
        .lock()
        .map_err(|_| "Session store unavailable".to_string())?
        .create(
            account.id.clone(),
            session_ttl(&state.config),
            client_ip.to_string(),
        );

    Ok((principal, token))
}

fn resolve_request_principal(
    state: &HttpState,
    headers: &HeaderMap,
    client_ip: IpAddr,
) -> Result<auth::ResolvedPrincipal, StatusCode> {
    if !auth::is_ip_allowed(state.config.ip_filter_mode.clone(), &state.ip_rules, client_ip) {
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(token) = find_cookie(headers, SESSION_COOKIE_NAME) {
        if let Ok(mut sessions) = state.sessions.lock() {
            if let Some(record) = sessions.validate(token, &client_ip.to_string()) {
                if let Some(account) = find_enabled_account(&state.config, &record.account_id) {
                    return Ok(principal_for_account(account));
                }
            }
        }
    }

    if let Some(guest) = find_enabled_account(&state.config, model::GUEST_ACCOUNT_ID) {
        if guest.password_hash.is_none() {
            return Ok(principal_for_account(guest));
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

fn reject_blocked_ip_response(
    state: &HttpState,
    client_ip: IpAddr,
    _login_redirect: bool,
) -> Option<Response> {
    if auth::is_ip_allowed(state.config.ip_filter_mode.clone(), &state.ip_rules, client_ip) {
        None
    } else {
        Some(plain_response(StatusCode::FORBIDDEN, "Forbidden"))
    }
}

#[allow(clippy::result_large_err)]
fn require_request_permission(
    state: &HttpState,
    headers: &HeaderMap,
    client_ip: IpAddr,
    permission: model::FileSharePermission,
    _login_redirect: bool,
) -> Result<auth::ResolvedPrincipal, Response> {
    let principal = resolve_request_principal(state, headers, client_ip).map_err(|status| {
        plain_response(
            status,
            if status == StatusCode::FORBIDDEN {
                "Forbidden"
            } else {
                "Unauthorized"
            },
        )
    })?;

    auth::require_permission(&principal, permission)
        .map_err(|_| plain_response(StatusCode::FORBIDDEN, "Forbidden"))?;
    Ok(principal)
}

fn find_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                cookie
                    .trim()
                    .strip_prefix(&format!("{name}="))
                    .map(str::trim)
            })
        })
}

fn session_cookie_header(token: &str) -> String {
    format!("{SESSION_COOKIE_NAME}={token}; HttpOnly; Path=/; SameSite=Lax")
}

fn clear_cookie_header(name: &str) -> String {
    format!("{name}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax")
}

async fn read_upload_request(
    mut multipart: Multipart,
    max_total_bytes: usize,
) -> Result<(String, String, Vec<UploadedFilePayload>), String> {
    let mut root = None;
    let mut parent = String::new();
    let mut files = Vec::new();
    let mut total_bytes = 0usize;

    while let Some(mut field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let field_name = field.name().unwrap_or_default().to_string();
        if let Some(file_name) = field.file_name().map(|value| value.to_string()) {
            let mut contents = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(|e| e.to_string())? {
                total_bytes = total_bytes
                    .checked_add(chunk.len())
                    .ok_or_else(|| "Upload Too Large".to_string())?;
                if total_bytes > max_total_bytes {
                    return Err("Upload Too Large".to_string());
                }
                contents.extend_from_slice(&chunk);
            }
            files.push(UploadedFilePayload {
                relative_path: file_name.replace('\\', "/"),
                contents,
            });
            continue;
        }

        let text = field.text().await.map_err(|e| e.to_string())?;
        match field_name.as_str() {
            "root" => root = Some(text),
            "parent" => parent = text,
            _ => {}
        }
    }

    let root = root.ok_or_else(|| "Root is required".to_string())?;
    if files.is_empty() {
        return Err("At least one file is required".to_string());
    }

    Ok((root, parent, files))
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

fn legacy_hash_password(pw: &str) -> String {
    let mut h = Sha256::new();
    h.update(pw.as_bytes());
    h.update(b"file_share_salt_fst_v1");
    format!("{:x}", h.finalize())
}

fn hash_password(pw: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .expect("argon2 password hashing should succeed")
        .to_string()
}

fn verify_password_hash(expected_hash: &str, candidate: &str) -> bool {
    if expected_hash.starts_with("$argon2") {
        PasswordHash::new(expected_hash)
            .ok()
            .and_then(|parsed| {
                Argon2::default()
                    .verify_password(candidate.as_bytes(), &parsed)
                    .ok()
            })
            .is_some()
    } else {
        legacy_hash_password(candidate)
            .as_bytes()
            .ct_eq(expected_hash.as_bytes())
            .into()
    }
}

fn plain_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .body(Body::from(body))
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

    fn is_common_lan(ip: &Ipv4Addr) -> bool {
        let o = ip.octets();
        o[0] == 192 && o[1] == 168
            || o[0] == 10
            || o[0] == 172 && (16..=31).contains(&o[1])
    }

    ips.sort_by_key(|ip| if is_common_lan(ip) { 0 } else { 1 });
    ips.into_iter().map(|ip| ip.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        hash_password, legacy_hash_password, remember_connected_ip, snapshot_connected_ips,
        verify_password_hash, FileShareHandle, FileShareRuntime, FileShareRuntimeSnapshot,
        SharedDir,
    };
    use crate::fileshare::ops::{validate_zip_source_with_limits, ZipSourceStats};
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tokio::sync::oneshot;

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

    #[test]
    fn password_verifier_accepts_argon2_hashes() {
        let hash = hash_password("secret-123");

        assert!(hash.starts_with("$argon2"));
        assert!(verify_password_hash(&hash, "secret-123"));
        assert!(!verify_password_hash(&hash, "wrong-password"));
    }

    #[test]
    fn password_verifier_keeps_legacy_sha256_hashes_compatible() {
        let legacy_hash = legacy_hash_password("secret-123");

        assert!(verify_password_hash(&legacy_hash, "secret-123"));
        assert!(!verify_password_hash(&legacy_hash, "wrong-password"));
    }

    #[test]
    fn file_share_handle_runtime_snapshot_round_trips() {
        let handle = FileShareHandle::new();
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        let shared_dirs = vec![SharedDir {
            alias: "soft".to_string(),
            path: "D:/soft".to_string(),
        }];

        handle.set_runtime(FileShareRuntime {
            shutdown_tx,
            snapshot: FileShareRuntimeSnapshot {
                server_url: "http://127.0.0.1:8080".to_string(),
                all_urls: vec!["http://127.0.0.1:8080".to_string()],
                shared_dirs: shared_dirs.clone(),
                start_time: Instant::now(),
            },
        });

        let snapshot = handle
            .runtime_snapshot()
            .expect("runtime snapshot should be available");
        assert_eq!(snapshot.server_url, "http://127.0.0.1:8080");
        assert_eq!(snapshot.all_urls, vec!["http://127.0.0.1:8080".to_string()]);
        assert_eq!(snapshot.shared_dirs, shared_dirs);
        assert!(handle.take_runtime().is_some());
        assert!(handle.runtime_snapshot().is_none());
    }
}
