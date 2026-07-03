#![allow(clippy::too_many_arguments)]

use std::collections::HashMap;
use std::convert::Infallible;
use std::io;
#[cfg(target_os = "windows")]
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Query, State as AxumState};
use axum::http::{header::USER_AGENT, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{body::Body, Form, Json, Router};
use bytes::{Bytes, BytesMut};
use scrap::{Capturer, Display, Frame};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{broadcast, oneshot};
#[cfg(target_os = "windows")]
use windows::core::{factory, Error as WindowsError, IInspectable, Interface, HRESULT};
#[cfg(target_os = "windows")]
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
#[cfg(target_os = "windows")]
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
#[cfg(target_os = "windows")]
use windows::Graphics::DirectX::Direct3D11::{IDirect3DDevice, IDirect3DSurface};
#[cfg(target_os = "windows")]
use windows::Graphics::DirectX::DirectXPixelFormat;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HMODULE, LPARAM, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
#[cfg(target_os = "windows")]
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

// ─── Public Data Types ──────────────────────────────────────

const TOOL_NAME: &str = "屏幕共享";

const VIEWER_IP_TTL: Duration = Duration::from_secs(12);
/// DXGI DuplicateOutput 偶发瞬时失败，创建时做 3 次短重试；
/// 长退避由捕获循环的暂停-重试机制负责，此处不需要更长的重试梯子。
const DXGI_CREATE_RETRY_DELAYS_MS: [u64; 3] = [0, 200, 400];
const CAPTURE_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const CAPTURE_RETRY_CANCEL_POLL_MS: u64 = 100;
/// 采集器重建的无限重试退避表；到顶后维持 30s 间隔直到会话被取消。
/// 锁屏可能持续数小时——共享必须活着等到解锁自动恢复。
const CAPTURE_RECREATE_BACKOFF_MS: [u64; 6] = [1000, 2000, 4000, 8000, 15000, 30000];

fn capture_recreate_backoff(attempt: u32) -> Duration {
    let index = (attempt as usize).min(CAPTURE_RECREATE_BACKOFF_MS.len() - 1);
    Duration::from_millis(CAPTURE_RECREATE_BACKOFF_MS[index])
}
type ViewerIpMap = HashMap<String, Instant>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureBackendKind {
    Dxgi,
    Wgc,
}

impl CaptureBackendKind {
    fn label(self) -> &'static str {
        match self {
            Self::Dxgi => "DXGI",
            Self::Wgc => "WGC",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareBackendMode {
    #[default]
    Auto,
    Wgc,
    Dxgi,
}

impl ScreenShareBackendMode {
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Wgc => "wgc",
            Self::Dxgi => "dxgi",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureStartKind {
    InitialStart,
    RuntimeRecreate,
}

/// 决定采集后端尝试顺序。
/// - 初始启动：严格尊重用户选择（显式 DXGI 表示"要无边框"，失败就报错，不悄悄换成有黄框的 WGC）。
///   Auto 模式 WGC 优先——它无独占语义、不被锁屏杀死，是稳定性默认。
/// - 运行期重建：保命优先，先试刚才还活着的后端，另一个作为降级，绝不因单后端失败而停止共享。
fn capture_backend_order(
    mode: ScreenShareBackendMode,
    kind: CaptureStartKind,
    current: Option<CaptureBackendKind>,
) -> Vec<CaptureBackendKind> {
    match kind {
        CaptureStartKind::InitialStart => match mode {
            ScreenShareBackendMode::Auto => vec![CaptureBackendKind::Wgc, CaptureBackendKind::Dxgi],
            ScreenShareBackendMode::Wgc => vec![CaptureBackendKind::Wgc],
            ScreenShareBackendMode::Dxgi => vec![CaptureBackendKind::Dxgi],
        },
        CaptureStartKind::RuntimeRecreate => {
            let first = current.unwrap_or(match mode {
                ScreenShareBackendMode::Dxgi => CaptureBackendKind::Dxgi,
                _ => CaptureBackendKind::Wgc,
            });
            let second = match first {
                CaptureBackendKind::Wgc => CaptureBackendKind::Dxgi,
                CaptureBackendKind::Dxgi => CaptureBackendKind::Wgc,
            };
            vec![first, second]
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareConfig {
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub monitor_index: usize,
    pub quality: u8,
    pub fps: u8,
    pub show_cursor: bool,
    #[serde(default)]
    pub capture_backend_mode: ScreenShareBackendMode,
    /// Bind address: "0.0.0.0" for all interfaces, or a specific IP like "192.168.1.100".
    /// When None, defaults to "0.0.0.0".
    #[serde(default)]
    pub bind_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub ip: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenShareStatus {
    pub is_active: bool,
    pub viewer_count: u32,
    pub connection_count: u32,
    pub fps_actual: f32,
    pub bitrate_kbps: u32,
    pub uptime_secs: u64,
    pub server_url: String,
    pub all_urls: Vec<String>,
    pub connected_ips: Vec<String>,
    /// True while the capture source is down and being rebuilt (e.g. lock
    /// screen); the HTTP server and viewer connections stay alive throughout.
    pub capture_paused: bool,
}

// ─── Handle (stored in AppState) ────────────────────────────

pub struct ScreenShareHandle {
    active: Arc<AtomicBool>,
    starting: AtomicBool,
    /// Current session's cancel token. Each session gets a FRESH Arc so a new
    /// start can never un-cancel streams/threads left over from a previous
    /// session (the old token stays cancelled forever).
    cancel: Mutex<Arc<AtomicBool>>,
    session_id: AtomicU64,
    viewer_count: Arc<AtomicU32>,
    fps_counter: Arc<AtomicU32>,
    bytes_sent: Arc<AtomicU64>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    server_url: Mutex<String>,
    all_urls: Mutex<Vec<String>>,
    start_time: Mutex<Option<Instant>>,
    viewer_ips: Arc<Mutex<ViewerIpMap>>,
    capture_paused: Arc<AtomicBool>,
}

impl ScreenShareHandle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            starting: AtomicBool::new(false),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            session_id: AtomicU64::new(0),
            viewer_count: Arc::new(AtomicU32::new(0)),
            fps_counter: Arc::new(AtomicU32::new(0)),
            bytes_sent: Arc::new(AtomicU64::new(0)),
            shutdown_tx: Mutex::new(None),
            server_url: Mutex::new(String::new()),
            all_urls: Mutex::new(Vec::new()),
            start_time: Mutex::new(None),
            viewer_ips: Arc::new(Mutex::new(HashMap::new())),
            capture_paused: Arc::new(AtomicBool::new(false)),
        }
    }
}

struct ScreenShareStartGuard {
    handle: Arc<ScreenShareHandle>,
    session_id: u64,
    completed: bool,
}

impl ScreenShareStartGuard {
    fn session_id(&self) -> u64 {
        self.session_id
    }

    fn mark_active(mut self) {
        if is_current_session(&self.handle, self.session_id) {
            self.handle.active.store(true, Ordering::SeqCst);
            self.handle.starting.store(false, Ordering::SeqCst);
        }
        self.completed = true;
    }
}

impl Drop for ScreenShareStartGuard {
    fn drop(&mut self) {
        if !self.completed && is_current_session(&self.handle, self.session_id) {
            reset_runtime_state(&self.handle);
        }
    }
}

fn inactive_status() -> ScreenShareStatus {
    ScreenShareStatus {
        is_active: false,
        viewer_count: 0,
        connection_count: 0,
        fps_actual: 0.0,
        bitrate_kbps: 0,
        uptime_secs: 0,
        server_url: String::new(),
        all_urls: Vec::new(),
        connected_ips: Vec::new(),
        capture_paused: false,
    }
}

fn clear_runtime_state(handle: &ScreenShareHandle, cancel: bool) {
    handle.active.store(false, Ordering::SeqCst);
    {
        let mut token = handle.cancel.lock().unwrap();
        // Cancel whatever session owned this token so its capture thread and
        // MJPEG streams always exit, even if a new session starts right after.
        token.store(true, Ordering::SeqCst);
        if !cancel {
            // Fresh start: install a brand-new, un-cancelled token.
            *token = Arc::new(AtomicBool::new(false));
        }
    }
    handle.viewer_count.store(0, Ordering::Relaxed);
    handle.fps_counter.store(0, Ordering::Relaxed);
    handle.bytes_sent.store(0, Ordering::Relaxed);
    *handle.shutdown_tx.lock().unwrap() = None;
    *handle.server_url.lock().unwrap() = String::new();
    *handle.all_urls.lock().unwrap() = Vec::new();
    *handle.start_time.lock().unwrap() = None;
    handle.capture_paused.store(false, Ordering::SeqCst);
    if let Ok(mut ips) = handle.viewer_ips.lock() {
        ips.clear();
    }
}

fn prepare_runtime_state_for_start(handle: &ScreenShareHandle) -> u64 {
    clear_runtime_state(handle, false);
    handle.session_id.fetch_add(1, Ordering::SeqCst) + 1
}

fn reset_runtime_state(handle: &ScreenShareHandle) {
    clear_runtime_state(handle, true);
    handle.starting.store(false, Ordering::SeqCst);
    handle.session_id.fetch_add(1, Ordering::SeqCst);
}

fn begin_screen_share_start(
    handle: &Arc<ScreenShareHandle>,
) -> Result<ScreenShareStartGuard, String> {
    if handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is already active".into());
    }

    if handle
        .starting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Screen share is already starting".into());
    }

    if handle.active.load(Ordering::SeqCst) {
        handle.starting.store(false, Ordering::SeqCst);
        return Err("Screen share is already active".into());
    }

    let session_id = prepare_runtime_state_for_start(handle);
    Ok(ScreenShareStartGuard {
        handle: handle.clone(),
        session_id,
        completed: false,
    })
}

fn is_current_session(handle: &ScreenShareHandle, session_id: u64) -> bool {
    handle.session_id.load(Ordering::SeqCst) == session_id
}

fn current_cancel_token(handle: &ScreenShareHandle) -> Arc<AtomicBool> {
    handle.cancel.lock().unwrap().clone()
}

fn record_viewer_ip(viewer_ips: &Arc<Mutex<ViewerIpMap>>, ip: impl Into<String>) {
    record_viewer_ip_at(viewer_ips, ip, Instant::now());
}

fn record_viewer_ip_at(
    viewer_ips: &Arc<Mutex<ViewerIpMap>>,
    ip: impl Into<String>,
    seen_at: Instant,
) {
    if let Ok(mut ips) = viewer_ips.lock() {
        ips.insert(ip.into(), seen_at);
    }
}

fn snapshot_viewer_ips(viewer_ips: &Arc<Mutex<ViewerIpMap>>) -> Vec<String> {
    snapshot_viewer_ips_at(viewer_ips, Instant::now())
}

fn snapshot_viewer_ips_at(viewer_ips: &Arc<Mutex<ViewerIpMap>>, now: Instant) -> Vec<String> {
    let mut ips: Vec<String> = viewer_ips
        .lock()
        .map(|mut map| {
            map.retain(|_, seen_at| {
                now.checked_duration_since(*seen_at).unwrap_or_default() <= VIEWER_IP_TTL
            });
            map.keys().cloned().collect()
        })
        .unwrap_or_default();
    ips.sort_unstable();
    ips
}

fn emit_inactive_status(app_handle: &AppHandle) {
    let _ = app_handle.emit("screen-share-status", inactive_status());
}

// ─── Internal: HTTP server state ────────────────────────────

struct HttpServerState {
    app_handle: AppHandle,
    broadcast_tx: broadcast::Sender<Arc<Bytes>>,
    viewer_count: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    auth_hash: Option<String>,
    auth_username: Option<String>,
    bytes_sent: Arc<AtomicU64>,
    viewer_ips: Arc<Mutex<ViewerIpMap>>,
}

/// RAII guard that decrements viewer count and removes IP on drop.
struct ViewerGuard {
    app_handle: AppHandle,
    count: Arc<AtomicU32>,
    ips: Arc<Mutex<ViewerIpMap>>,
    ip: String,
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        let updated_count = loop {
            let current = self.count.load(Ordering::Relaxed);
            if current == 0 {
                break 0;
            }
            if self
                .count
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break current - 1;
            }
        };
        if let Ok(mut set) = self.ips.lock() {
            set.remove(&self.ip);
        }
        crate::scanner::emit_tool_log(
            &self.app_handle,
            TOOL_NAME,
            &format!(
                "Viewer disconnected: ip={}, remaining_viewers={}",
                self.ip, updated_count
            ),
            "info",
        );
    }
}

// ─── Tauri Commands ─────────────────────────────────────────

#[tauri::command]
pub fn screen_share_list_monitors() -> Result<Vec<MonitorInfo>, String> {
    let displays = Display::all().map_err(|e| format!("Failed to enumerate displays: {}", e))?;

    Ok(displays
        .iter()
        .enumerate()
        .map(|(i, d)| MonitorInfo {
            index: i,
            name: format!("Display {}", i + 1),
            width: d.width() as u32,
            height: d.height() as u32,
            is_primary: i == 0,
        })
        .collect())
}

#[tauri::command]
pub fn screen_share_list_interfaces() -> Vec<NetworkInterfaceInfo> {
    use std::net::IpAddr;
    let mut interfaces: Vec<NetworkInterfaceInfo> = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(name, ip)| match ip {
            IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() => {
                Some(NetworkInterfaceInfo {
                    name,
                    ip: v4.to_string(),
                })
            }
            _ => None,
        })
        .collect();

    // Sort: common LAN IPs first (192.168.x.x, 10.x.x.x, 172.16-31.x.x)
    interfaces.sort_by_key(|iface| {
        let parts: Vec<u8> = iface.ip.split('.').filter_map(|s| s.parse().ok()).collect();
        if parts.len() == 4 {
            let is_lan = parts[0] == 192 && parts[1] == 168
                || parts[0] == 10
                || parts[0] == 172 && (16..=31).contains(&parts[1]);
            if is_lan {
                0
            } else {
                1
            }
        } else {
            2
        }
    });

    interfaces
}

#[tauri::command]
pub async fn screen_share_start(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
    config: ScreenShareConfig,
) -> Result<String, String> {
    let handle = &state.screen_share;

    // Validate
    if config.port < 1024 {
        return Err("Port must be >= 1024".into());
    }
    if config.quality < 10 || config.quality > 100 {
        return Err("Quality must be 10-100".into());
    }
    if config.fps < 1 || config.fps > 30 {
        return Err("FPS must be 1-30".into());
    }

    let start_guard = begin_screen_share_start(handle)?;
    let session_id = start_guard.session_id();
    let session_cancel = current_cancel_token(handle);

    // Verify monitor exists (in a block so displays is dropped before any .await)
    {
        let displays =
            Display::all().map_err(|e| format!("Failed to enumerate displays: {}", e))?;
        if config.monitor_index >= displays.len() {
            return Err(format!(
                "Monitor index {} out of range ({})",
                config.monitor_index,
                displays.len()
            ));
        }
        crate::scanner::emit_tool_log(
            &app_handle,
            TOOL_NAME,
            &format!(
                "启动前显示器清单: {}",
                describe_display_inventory(&displays)
            ),
            "info",
        );
    }

    // Get local IPs (all non-loopback IPv4)
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

    // Bind listener BEFORE spawning so that bind errors propagate to the caller
    let bind_ip = config.bind_address.as_deref().unwrap_or("0.0.0.0");
    let addr = format!("{}:{}", bind_ip, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        let msg = format!("Failed to bind port {}: {}", config.port, e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        msg
    })?;
    log::info!("Screen share HTTP server listening on {}", addr);

    // Broadcast channel for JPEG frames
    let (broadcast_tx, _) = broadcast::channel::<Arc<Bytes>>(8);

    let auth_hash = config
        .password
        .as_ref()
        .map(|p| hash_credential(config.username.as_deref(), p));
    let auth_username = config.username.clone();

    // --- Spawn capture thread ---
    let capture_cancel = session_cancel.clone();
    let capture_fps = handle.fps_counter.clone();
    let capture_viewers = handle.viewer_count.clone();
    let capture_handle = handle.clone();
    let monitor_index = config.monitor_index;
    let quality = config.quality;
    let fps = config.fps;
    let show_cursor = config.show_cursor;
    let backend_mode = config.capture_backend_mode;
    let capture_tx = broadcast_tx.clone();
    let capture_app = app_handle.clone();
    let (startup_tx, startup_rx) = oneshot::channel::<Result<(), String>>();

    if let Err(e) = std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            capture_loop(
                monitor_index,
                quality,
                fps,
                show_cursor,
                backend_mode,
                capture_tx,
                capture_cancel,
                capture_fps,
                capture_viewers,
                capture_handle,
                session_id,
                Some(startup_tx),
                capture_app,
            );
        })
    {
        let msg = format!("Failed to spawn capture thread: {}", e);
        reset_runtime_state(handle);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        let _ = app_handle.emit(
            "screen-share-log",
            serde_json::json!({ "level": "error", "message": msg }),
        );
        emit_inactive_status(&app_handle);
        return Err(msg);
    }

    let startup_detail = match tokio::time::timeout(CAPTURE_STARTUP_TIMEOUT, startup_rx).await {
        Ok(Ok(Ok(()))) => None,
        Ok(Ok(Err(detail))) => Some(detail),
        Ok(Err(_)) => Some("Screen capture thread exited before reporting startup status".into()),
        Err(_) => Some("Screen capture startup timed out".into()),
    };

    if let Some(detail) = startup_detail {
        reset_runtime_state(handle);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &detail, "error");
        let _ = app_handle.emit(
            "screen-share-log",
            serde_json::json!({ "level": "error", "message": detail }),
        );
        emit_inactive_status(&app_handle);
        return Err(detail);
    }

    if session_cancel.load(Ordering::SeqCst) || !is_current_session(handle, session_id) {
        return Err("Screen share startup was cancelled".into());
    }

    *handle.server_url.lock().unwrap() = server_url.clone();
    *handle.all_urls.lock().unwrap() = all_urls.clone();
    *handle.start_time.lock().unwrap() = Some(Instant::now());
    start_guard.mark_active();

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    *handle.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

    let server_state = Arc::new(HttpServerState {
        app_handle: app_handle.clone(),
        broadcast_tx: broadcast_tx.clone(),
        viewer_count: handle.viewer_count.clone(),
        cancel: session_cancel.clone(),
        auth_hash,
        auth_username,
        bytes_sent: handle.bytes_sent.clone(),
        viewer_ips: handle.viewer_ips.clone(),
    });

    // --- Spawn HTTP server ---
    let ss_server_active = handle.active.clone();
    let ss_server_app = app_handle.clone();
    let ss_runtime_handle = handle.clone();
    let ss_session_id = session_id;
    let server_join = tokio::spawn(async move {
        run_http_server(listener, server_state, shutdown_rx).await;
    });

    // Watcher: when the server task ends (normal or panic), clean up state
    tokio::spawn(async move {
        match server_join.await {
            Ok(()) => {
                log::info!("Screen share HTTP server exited");
            }
            Err(e) => {
                let msg = format!("服务异常退出: {}", e);
                log::error!("Screen share server crashed: {}", e);
                crate::scanner::emit_tool_log(&ss_server_app, TOOL_NAME, &msg, "error");
                let _ = ss_server_app.emit(
                    "screen-share-log",
                    serde_json::json!({ "level": "error", "message": msg }),
                );
            }
        }

        if is_current_session(&ss_runtime_handle, ss_session_id)
            && ss_server_active.swap(false, Ordering::SeqCst)
        {
            reset_runtime_state(&ss_runtime_handle);
            crate::scanner::emit_tool_log(&ss_server_app, TOOL_NAME, "已停止", "info");
            let _ = ss_server_app.emit(
                "screen-share-log",
                serde_json::json!({ "level": "info", "message": "Screen share stopped" }),
            );
            emit_inactive_status(&ss_server_app);
        }
    });

    // --- Spawn status reporter ---
    let reporter_app = app_handle.clone();
    let reporter_active = handle.active.clone();
    let reporter_viewers = handle.viewer_count.clone();
    let reporter_fps = handle.fps_counter.clone();
    let reporter_bytes = handle.bytes_sent.clone();
    let reporter_url = server_url.clone();
    let reporter_all_urls = all_urls.clone();
    let reporter_start = Instant::now();
    let reporter_ips = handle.viewer_ips.clone();
    let reporter_capture_paused = handle.capture_paused.clone();
    let reporter_runtime_handle = handle.clone();
    let reporter_session_id = session_id;

    tokio::spawn(async move {
        status_reporter(
            reporter_app,
            reporter_active,
            reporter_viewers,
            reporter_fps,
            reporter_bytes,
            reporter_url,
            reporter_all_urls,
            reporter_start,
            reporter_ips,
            reporter_capture_paused,
            reporter_runtime_handle,
            reporter_session_id,
        )
        .await;
    });

    crate::scanner::emit_tool_log(
        &app_handle,
        TOOL_NAME,
        &format!(
            "已启动，显示器 {} @ {}fps，访问: {}",
            config.monitor_index + 1,
            config.fps,
            server_url
        ),
        "success",
    );

    let _ = app_handle.emit(
        "screen-share-log",
        serde_json::json!({ "level": "info", "message": format!("Screen share started at {}", server_url) }),
    );

    Ok(server_url)
}

#[tauri::command]
pub async fn screen_share_stop(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let handle = &state.screen_share;

    if !handle.active.load(Ordering::SeqCst) && !handle.starting.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }

    // Signal stop
    current_cancel_token(handle).store(true, Ordering::SeqCst);

    // Shutdown HTTP server
    if let Some(tx) = handle.shutdown_tx.lock().unwrap().take() {
        let _ = tx.send(());
    }

    reset_runtime_state(handle);

    crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, "已停止", "info");

    let _ = app_handle.emit(
        "screen-share-log",
        serde_json::json!({ "level": "info", "message": "Screen share stopped" }),
    );
    emit_inactive_status(&app_handle);

    tokio::time::sleep(Duration::from_millis(1200)).await;

    Ok(())
}

#[tauri::command]
pub fn screen_share_get_status(state: State<'_, crate::AppState>) -> ScreenShareStatus {
    let handle = &state.screen_share;
    let is_active = handle.active.load(Ordering::Relaxed);

    if !is_active {
        return inactive_status();
    }

    let uptime = handle
        .start_time
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);

    let connected_ips = snapshot_viewer_ips(&handle.viewer_ips);

    ScreenShareStatus {
        is_active: true,
        viewer_count: handle.viewer_count.load(Ordering::Relaxed),
        connection_count: connected_ips.len() as u32,
        fps_actual: handle.fps_counter.load(Ordering::Relaxed) as f32,
        bitrate_kbps: 0,
        uptime_secs: uptime,
        server_url: handle.server_url.lock().unwrap().clone(),
        all_urls: handle.all_urls.lock().unwrap().clone(),
        connected_ips,
        capture_paused: handle.capture_paused.load(Ordering::Relaxed),
    }
}

fn sanitize_log_field(value: &str) -> String {
    let sanitized = value
        .trim()
        .replace([';', '\r', '\n', '\t'], " ")
        .replace("  ", " ");
    if sanitized.is_empty() {
        "-".to_string()
    } else {
        sanitized
    }
}

// ─── Cursor Overlay (Windows) ──────────────────────────────
#[cfg(target_os = "windows")]
mod cursor_overlay {
    use std::mem;
    use std::ptr;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Cached cursor rendering to avoid re-rendering when cursor icon unchanged.
    pub struct CursorCache {
        last_cursor_handle: isize,
        /// Pre-multiplied BGRA pixels (rendered on black background).
        on_black: Vec<u8>,
        /// Per-pixel alpha derived from dual-render.
        alpha: Vec<u8>,
        width: i32,
        height: i32,
        hotspot_x: i32,
        hotspot_y: i32,
    }

    impl CursorCache {
        pub fn new() -> Self {
            Self {
                last_cursor_handle: 0,
                on_black: Vec::new(),
                alpha: Vec::new(),
                width: 0,
                height: 0,
                hotspot_x: 0,
                hotspot_y: 0,
            }
        }
    }

    /// Monitor rectangle in virtual-screen coordinates.
    #[derive(Clone)]
    pub struct MonitorRect {
        pub left: i32,
        pub top: i32,
    }

    /// Get the virtual-screen origin for a monitor by index.
    /// Enumerates monitors with primary first (matching scrap's ordering).
    pub fn get_monitor_rect(monitor_index: usize) -> Option<MonitorRect> {
        struct Entry {
            left: i32,
            top: i32,
            is_primary: bool,
        }

        let mut entries: Vec<Entry> = Vec::new();

        unsafe {
            unsafe extern "system" fn callback(
                hmon: HMONITOR,
                _hdc: HDC,
                _rect: *mut RECT,
                data: LPARAM,
            ) -> BOOL {
                let entries = &mut *(data.0 as *mut Vec<Entry>);
                let mut info: MONITORINFO = mem::zeroed();
                info.cbSize = mem::size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(hmon, &mut info).as_bool() {
                    entries.push(Entry {
                        left: info.rcMonitor.left,
                        top: info.rcMonitor.top,
                        is_primary: (info.dwFlags & MONITORINFOF_PRIMARY) != 0,
                    });
                }
                BOOL(1)
            }

            let _ = EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(callback),
                LPARAM(&mut entries as *mut Vec<Entry> as isize),
            );
        }

        // Sort: primary first, then by position (left, top)
        entries.sort_by(|a, b| {
            b.is_primary
                .cmp(&a.is_primary)
                .then(a.left.cmp(&b.left))
                .then(a.top.cmp(&b.top))
        });

        entries.get(monitor_index).map(|e| MonitorRect {
            left: e.left,
            top: e.top,
        })
    }

    /// Draw the mouse cursor onto the BGRA frame buffer.
    pub fn draw_cursor(
        frame: &mut [u8],
        frame_width: usize,
        frame_height: usize,
        stride: usize,
        monitor_rect: &MonitorRect,
        cache: &mut CursorCache,
    ) {
        unsafe {
            // Get current cursor info
            let mut ci: CURSORINFO = mem::zeroed();
            ci.cbSize = mem::size_of::<CURSORINFO>() as u32;
            if GetCursorInfo(&mut ci).is_err() {
                return;
            }
            if ci.flags != CURSOR_SHOWING {
                return;
            }

            let cursor_handle = ci.hCursor.0 as isize;

            // Re-render cursor image only if the icon handle changed
            if cursor_handle != cache.last_cursor_handle {
                if !render_cursor_image(ci.hCursor, cache) {
                    return;
                }
                cache.last_cursor_handle = cursor_handle;
            }

            // Convert screen coords to frame-relative coords
            let draw_x = ci.ptScreenPos.x - monitor_rect.left - cache.hotspot_x;
            let draw_y = ci.ptScreenPos.y - monitor_rect.top - cache.hotspot_y;

            // Composite cursor onto frame
            composite(
                frame,
                frame_width,
                frame_height,
                stride,
                &cache.on_black,
                &cache.alpha,
                cache.width,
                cache.height,
                draw_x,
                draw_y,
            );
        }
    }

    /// Render cursor icon to BGRA using dual-render technique (black & white backgrounds)
    /// to derive correct alpha channel.
    unsafe fn render_cursor_image(hcursor: HCURSOR, cache: &mut CursorCache) -> bool {
        // Get cursor hotspot
        let mut ii: ICONINFO = mem::zeroed();
        if GetIconInfo(hcursor, &mut ii).is_err() {
            return false;
        }
        cache.hotspot_x = ii.xHotspot as i32;
        cache.hotspot_y = ii.yHotspot as i32;

        // Determine cursor size from mask bitmap
        let mut bm: BITMAP = mem::zeroed();
        let bm_size = mem::size_of::<BITMAP>() as i32;
        if ii.hbmMask.is_invalid()
            || GetObjectW(ii.hbmMask, bm_size, Some(ptr::addr_of_mut!(bm) as *mut _)) == 0
        {
            if !ii.hbmMask.is_invalid() {
                let _ = DeleteObject(ii.hbmMask);
            }
            if !ii.hbmColor.is_invalid() {
                let _ = DeleteObject(ii.hbmColor);
            }
            return false;
        }

        let cw = bm.bmWidth;
        // Monochrome cursors have mask height = 2 * actual height
        let ch = if ii.hbmColor.is_invalid() {
            bm.bmHeight / 2
        } else {
            bm.bmHeight
        };

        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask);
        }
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor);
        }

        if cw <= 0 || ch <= 0 || cw > 256 || ch > 256 {
            return false;
        }

        cache.width = cw;
        cache.height = ch;

        let pixel_count = (cw * ch) as usize;
        let byte_count = pixel_count * 4;

        // Create memory DC + DIB section
        let hdc_screen = GetDC(HWND::default());
        let hdc_mem = CreateCompatibleDC(hdc_screen);

        let mut bmi: BITMAPINFO = mem::zeroed();
        bmi.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = cw;
        bmi.bmiHeader.biHeight = -ch; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut std::ffi::c_void = ptr::null_mut();
        let hbm = match CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bm) => bm,
            Err(_) => {
                let _ = DeleteDC(hdc_mem);
                ReleaseDC(HWND::default(), hdc_screen);
                return false;
            }
        };
        let old_bm = SelectObject(hdc_mem, hbm);
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u8, byte_count);

        // ---- Render on BLACK background ----
        pixels.iter_mut().for_each(|b| *b = 0);
        let _ = DrawIconEx(hdc_mem, 0, 0, hcursor, cw, ch, 0, None, DI_NORMAL);
        cache.on_black = pixels.to_vec();

        // ---- Render on WHITE background ----
        pixels.iter_mut().for_each(|b| *b = 255);
        let _ = DrawIconEx(hdc_mem, 0, 0, hcursor, cw, ch, 0, None, DI_NORMAL);
        let on_white = pixels.to_vec();

        // Derive alpha: alpha = 255 - max(white_c - black_c) across RGB channels
        cache.alpha.resize(pixel_count, 0);
        for i in 0..pixel_count {
            let off = i * 4;
            let db = on_white[off].saturating_sub(cache.on_black[off]);
            let dg = on_white[off + 1].saturating_sub(cache.on_black[off + 1]);
            let dr = on_white[off + 2].saturating_sub(cache.on_black[off + 2]);
            cache.alpha[i] = 255 - db.max(dg).max(dr);
        }

        // Cleanup
        SelectObject(hdc_mem, old_bm);
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(HWND::default(), hdc_screen);

        true
    }

    /// Alpha-composite cursor onto frame using pre-multiplied blending.
    fn composite(
        frame: &mut [u8],
        fw: usize,
        fh: usize,
        stride: usize,
        on_black: &[u8],
        alpha: &[u8],
        cw: i32,
        ch: i32,
        draw_x: i32,
        draw_y: i32,
    ) {
        for cy in 0..ch {
            let fy = draw_y + cy;
            if fy < 0 || fy >= fh as i32 {
                continue;
            }
            for cx in 0..cw {
                let fx = draw_x + cx;
                if fx < 0 || fx >= fw as i32 {
                    continue;
                }

                let ci = (cy * cw + cx) as usize;
                let a = alpha[ci] as u32;
                if a == 0 {
                    continue;
                }

                let src_off = ci * 4;
                let dst_off = fy as usize * stride + fx as usize * 4;
                if dst_off + 3 >= frame.len() {
                    continue;
                }

                // Pre-multiplied alpha blend: dst = src_premul + dst * (1 - alpha)
                let inv_a = 255 - a;
                frame[dst_off] =
                    (on_black[src_off] as u32 + frame[dst_off] as u32 * inv_a / 255) as u8;
                frame[dst_off + 1] =
                    (on_black[src_off + 1] as u32 + frame[dst_off + 1] as u32 * inv_a / 255) as u8;
                frame[dst_off + 2] =
                    (on_black[src_off + 2] as u32 + frame[dst_off + 2] as u32 * inv_a / 255) as u8;
            }
        }
    }
}

// ─── Screen Capture Loop ────────────────────────────────────

enum CapturedFrame<'a> {
    Dxgi(Frame<'a>, usize),
    Borrowed { pixels: &'a [u8], stride: usize },
}

impl CapturedFrame<'_> {
    fn pixels(&self) -> &[u8] {
        match self {
            Self::Dxgi(frame, _) => frame,
            Self::Borrowed { pixels, .. } => pixels,
        }
    }

    fn stride(&self) -> usize {
        match self {
            Self::Dxgi(_, stride) => *stride,
            Self::Borrowed { stride, .. } => *stride,
        }
    }
}

enum CaptureSource {
    Dxgi(Capturer),
    #[cfg(target_os = "windows")]
    Wgc(WgcCapturer),
}

impl CaptureSource {
    fn backend_kind(&self) -> CaptureBackendKind {
        match self {
            Self::Dxgi(_) => CaptureBackendKind::Dxgi,
            #[cfg(target_os = "windows")]
            Self::Wgc(_) => CaptureBackendKind::Wgc,
        }
    }

    fn width(&self) -> usize {
        match self {
            Self::Dxgi(capturer) => capturer.width(),
            #[cfg(target_os = "windows")]
            Self::Wgc(capturer) => capturer.width(),
        }
    }

    fn height(&self) -> usize {
        match self {
            Self::Dxgi(capturer) => capturer.height(),
            #[cfg(target_os = "windows")]
            Self::Wgc(capturer) => capturer.height(),
        }
    }

    fn frame(&mut self) -> io::Result<CapturedFrame<'_>> {
        match self {
            Self::Dxgi(capturer) => {
                let height = capturer.height();
                let frame: Frame<'_> = capturer.frame()?;
                let stride = frame.len() / height;
                Ok(CapturedFrame::Dxgi(frame, stride))
            }
            #[cfg(target_os = "windows")]
            Self::Wgc(capturer) => capturer.frame(),
        }
    }
}

fn capture_loop(
    monitor_index: usize,
    quality: u8,
    fps: u8,
    show_cursor: bool,
    backend_mode: ScreenShareBackendMode,
    tx: broadcast::Sender<Arc<Bytes>>,
    cancel: Arc<AtomicBool>,
    fps_counter: Arc<AtomicU32>,
    viewer_count: Arc<AtomicU32>,
    runtime_handle: Arc<ScreenShareHandle>,
    session_id: u64,
    startup_tx: Option<oneshot::Sender<Result<(), String>>>,
    app_handle: AppHandle,
) {
    let mut startup_tx = startup_tx;
    let mut source = match create_capture_source(
        monitor_index,
        show_cursor,
        backend_mode,
        CaptureStartKind::InitialStart,
        None,
        &cancel,
        &runtime_handle,
        session_id,
        &app_handle,
    ) {
        Ok(c) => c,
        Err(err) => {
            let detail = format!(
                "屏幕捕获初始化失败: monitor_index={}, viewers={}, cause={}",
                monitor_index,
                viewer_count.load(Ordering::Relaxed),
                err
            );
            if let Some(tx) = startup_tx.take() {
                let _ = tx.send(Err(detail));
            } else {
                log::error!("{}", detail);
            }
            return;
        }
    };

    if let Some(tx) = startup_tx.take() {
        let _ = tx.send(Ok(()));
    }

    let width = source.width();
    let height = source.height();
    let frame_interval = Duration::from_millis(1000 / fps.max(1) as u64);
    let mut first_real_frame = false;

    // Cursor overlay setup
    #[cfg(target_os = "windows")]
    let monitor_rect = if show_cursor {
        cursor_overlay::get_monitor_rect(monitor_index)
    } else {
        None
    };
    #[cfg(target_os = "windows")]
    let mut cursor_cache = cursor_overlay::CursorCache::new();

    log::info!(
        "Capture loop started: {}x{} @ {} FPS, quality {}, cursor {}",
        width,
        height,
        fps,
        quality,
        if show_cursor { "on" } else { "off" },
    );

    // Send a placeholder frame so that viewers connecting immediately get something
    {
        let placeholder = make_placeholder_jpeg();
        let _ = tx.send(Arc::new(Bytes::from(placeholder)));
    }

    // Pre-allocate reusable buffers to avoid per-frame heap allocations.
    // For cursor overlay: a scratch buffer to hold the frame pixels.
    #[cfg(target_os = "windows")]
    let mut frame_scratch: Vec<u8> = Vec::new();

    // For JPEG encoding: persistent RGB buffer + output buffer
    let mut rgb_buf: Vec<u8> = Vec::with_capacity(width * height * 3);
    let mut jpeg_buf: Vec<u8> = Vec::with_capacity(width * height / 4);

    loop {
        if cancel.load(Ordering::Relaxed) || !is_current_session(&runtime_handle, session_id) {
            break;
        }

        let tick_start = Instant::now();

        let current_backend = source.backend_kind();
        match source.frame() {
            Ok(frame) => {
                let stride = frame.stride();
                let frame_pixels = frame.pixels();

                #[cfg(target_os = "windows")]
                let source_pixels: &[u8] =
                    if show_cursor && current_backend == CaptureBackendKind::Dxgi {
                        if let Some(ref mon_rect) = monitor_rect {
                            // Copy frame into persistent scratch buffer (avoids per-frame reallocation)
                            if frame_scratch.len() != frame_pixels.len() {
                                frame_scratch.resize(frame_pixels.len(), 0);
                            }
                            frame_scratch.copy_from_slice(frame_pixels);
                            cursor_overlay::draw_cursor(
                                &mut frame_scratch,
                                width,
                                height,
                                stride,
                                mon_rect,
                                &mut cursor_cache,
                            );
                            &frame_scratch
                        } else {
                            frame_pixels
                        }
                    } else {
                        frame_pixels
                    };

                #[cfg(not(target_os = "windows"))]
                let source_pixels: &[u8] = frame_pixels;

                let jpeg = encode_jpeg_reuse(
                    source_pixels,
                    width,
                    height,
                    stride,
                    quality,
                    &mut rgb_buf,
                    &mut jpeg_buf,
                );

                if !jpeg.is_empty() {
                    let data = Arc::new(Bytes::from(jpeg));
                    let _ = tx.send(data);
                    fps_counter.fetch_add(1, Ordering::Relaxed);
                    first_real_frame = true;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !first_real_frame {
                    // Still warming up DXGI — send placeholder every 500ms to keep stream alive
                    if !wait_for_capture_retry_delay(
                        Duration::from_millis(500),
                        &cancel,
                        &runtime_handle,
                        session_id,
                    ) {
                        break;
                    }
                    let placeholder = make_placeholder_jpeg();
                    let _ = tx.send(Arc::new(Bytes::from(placeholder)));
                } else {
                    // Screen unchanged; sleep briefly (not busy-wait)
                    std::thread::sleep(Duration::from_millis(5));
                }
                continue;
            }
            Err(e) => {
                let capture_error_detail = format!(
                    "捕获循环异常，进入暂停重试: monitor_index={}, viewers={}, first_real_frame={}, error_kind={:?}, error={}",
                    monitor_index,
                    viewer_count.load(Ordering::Relaxed),
                    first_real_frame,
                    e.kind(),
                    e
                );
                log::warn!("{}", capture_error_detail);
                crate::scanner::emit_tool_log(
                    &app_handle,
                    TOOL_NAME,
                    &capture_error_detail,
                    "warn",
                );
                let _ = app_handle.emit(
                    "screen-share-log",
                    serde_json::json!({ "level": "warn", "message": capture_error_detail }),
                );

                if is_current_session(&runtime_handle, session_id) {
                    runtime_handle.capture_paused.store(true, Ordering::SeqCst);
                }
                // The HTTP server and viewer connections stay alive during the pause;
                // viewers keep the last frame and see a "retrying" hint via /status.
                drop(source);

                let mut retry_attempt = 0u32;
                let recovered = loop {
                    if !wait_for_capture_retry_delay(
                        capture_recreate_backoff(retry_attempt),
                        &cancel,
                        &runtime_handle,
                        session_id,
                    ) {
                        break None;
                    }
                    match create_capture_source(
                        monitor_index,
                        show_cursor,
                        backend_mode,
                        CaptureStartKind::RuntimeRecreate,
                        Some(current_backend),
                        &cancel,
                        &runtime_handle,
                        session_id,
                        &app_handle,
                    ) {
                        Ok(new_source) => break Some(new_source),
                        Err(err) => {
                            retry_attempt = retry_attempt.saturating_add(1);
                            let retry_msg = format!(
                                "屏幕捕获器重建失败，{}s 后继续重试: attempt={}, monitor_index={}, viewers={}, cause={}",
                                capture_recreate_backoff(retry_attempt).as_secs(),
                                retry_attempt,
                                monitor_index,
                                viewer_count.load(Ordering::Relaxed),
                                err
                            );
                            log::warn!("{}", retry_msg);
                            crate::scanner::emit_tool_log(
                                &app_handle,
                                TOOL_NAME,
                                &retry_msg,
                                "warn",
                            );
                            let _ = app_handle.emit(
                                "screen-share-log",
                                serde_json::json!({ "level": "warn", "message": retry_msg }),
                            );
                        }
                    }
                };

                match recovered {
                    Some(new_source) => {
                        source = new_source;
                        if is_current_session(&runtime_handle, session_id) {
                            runtime_handle.capture_paused.store(false, Ordering::SeqCst);
                        }
                        let resumed_msg = format!(
                            "屏幕捕获已恢复: retries={}, monitor_index={}, backend={}",
                            retry_attempt,
                            monitor_index,
                            source.backend_kind().label()
                        );
                        log::info!("{}", resumed_msg);
                        crate::scanner::emit_tool_log(
                            &app_handle,
                            TOOL_NAME,
                            &resumed_msg,
                            "success",
                        );
                        let _ = app_handle.emit(
                            "screen-share-log",
                            serde_json::json!({ "level": "info", "message": resumed_msg }),
                        );
                        first_real_frame = false;
                        continue;
                    }
                    None => break, // 会话被取消，正常退出
                }
            }
        }

        let elapsed = tick_start.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }

    log::info!("Capture loop ended");
}

#[derive(Debug)]
struct CaptureCreateError {
    detail: String,
    kind: Option<std::io::ErrorKind>,
}

impl CaptureCreateError {
    fn retryable(&self) -> bool {
        self.kind
            .map(should_retry_capture_creation)
            .unwrap_or(false)
    }
}

fn should_retry_capture_creation(error_kind: std::io::ErrorKind) -> bool {
    matches!(
        error_kind,
        std::io::ErrorKind::Other
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
    )
}

fn capture_creation_hint(error_kind: std::io::ErrorKind) -> Option<&'static str> {
    if should_retry_capture_creation(error_kind) {
        Some(
            "possible cause: the desktop is on the lock/UAC secure desktop, or the session is disconnected",
        )
    } else {
        None
    }
}

fn wait_for_capture_retry_delay(
    delay: Duration,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
) -> bool {
    if cancel.load(Ordering::Relaxed) || !is_current_session(runtime_handle, session_id) {
        return false;
    }

    let started_at = Instant::now();
    while started_at.elapsed() < delay {
        if cancel.load(Ordering::Relaxed) || !is_current_session(runtime_handle, session_id) {
            return false;
        }

        let remaining = delay.saturating_sub(started_at.elapsed());
        let sleep_for = remaining.min(Duration::from_millis(CAPTURE_RETRY_CANCEL_POLL_MS));
        if sleep_for.is_zero() {
            break;
        }
        std::thread::sleep(sleep_for);
    }

    !(cancel.load(Ordering::Relaxed) || !is_current_session(runtime_handle, session_id))
}

fn capture_runtime_state_summary(runtime_handle: &ScreenShareHandle, session_id: u64) -> String {
    format!(
        "state={{active={},starting={},cancel={},session_id={},current_session_id={},is_current={}}}",
        runtime_handle.active.load(Ordering::SeqCst),
        runtime_handle.starting.load(Ordering::SeqCst),
        current_cancel_token(runtime_handle).load(Ordering::SeqCst),
        session_id,
        runtime_handle.session_id.load(Ordering::SeqCst),
        is_current_session(runtime_handle, session_id)
    )
}

fn emit_capture_create_diagnostic(app_handle: &AppHandle, level: &str, message: String) {
    match level {
        "error" => log::error!("{}", message),
        "warn" => log::warn!("{}", message),
        "success" => log::info!("{}", message),
        _ => log::info!("{}", message),
    }
    crate::scanner::emit_tool_log(app_handle, TOOL_NAME, &message, level);
    let _ = app_handle.emit(
        "screen-share-log",
        serde_json::json!({ "level": level, "message": message }),
    );
}

fn format_capture_backend_fallback_message(
    from: CaptureBackendKind,
    to: CaptureBackendKind,
    session_id: u64,
    monitor_index: usize,
    cause: &str,
) -> String {
    format!(
        "{} capture backend failed, switching to {}: session_id={}, monitor_index={}, cause={}",
        from.label(),
        to.label(),
        session_id,
        monitor_index,
        sanitize_log_field(cause)
    )
}

fn format_capture_backend_failure_message(
    backend: CaptureBackendKind,
    session_id: u64,
    monitor_index: usize,
    cause: &str,
) -> String {
    format!(
        "{} capture backend failed: session_id={}, monitor_index={}, cause={}",
        backend.label(),
        session_id,
        monitor_index,
        sanitize_log_field(cause)
    )
}

fn format_capture_backend_selected_message(
    backend: CaptureBackendKind,
    session_id: u64,
    monitor_index: usize,
    width: usize,
    height: usize,
) -> String {
    format!(
        "using screen capture backend: backend={}, session_id={}, monitor_index={}, size={}x{}",
        backend.label(),
        session_id,
        monitor_index,
        width,
        height
    )
}

fn create_capture_source(
    monitor_index: usize,
    show_cursor: bool,
    backend_mode: ScreenShareBackendMode,
    start_kind: CaptureStartKind,
    current_backend: Option<CaptureBackendKind>,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
    app_handle: &AppHandle,
) -> Result<CaptureSource, String> {
    #[cfg(not(target_os = "windows"))]
    let _ = show_cursor;

    let order = capture_backend_order(backend_mode, start_kind, current_backend);
    let mut failures: Vec<String> = Vec::new();

    for (index, backend) in order.iter().enumerate() {
        let result: Result<CaptureSource, String> = match backend {
            CaptureBackendKind::Dxgi => create_capturer(
                monitor_index,
                cancel,
                runtime_handle,
                session_id,
                app_handle,
            )
            .map(CaptureSource::Dxgi),
            #[cfg(target_os = "windows")]
            CaptureBackendKind::Wgc => {
                create_wgc_capturer(monitor_index, show_cursor, session_id, app_handle)
                    .map(CaptureSource::Wgc)
            }
            #[cfg(not(target_os = "windows"))]
            CaptureBackendKind::Wgc => {
                Err("WGC capture backend is only available on Windows".to_string())
            }
        };

        match result {
            Ok(source) => {
                emit_capture_create_diagnostic(
                    app_handle,
                    "success",
                    format_capture_backend_selected_message(
                        source.backend_kind(),
                        session_id,
                        monitor_index,
                        source.width(),
                        source.height(),
                    ),
                );
                return Ok(source);
            }
            Err(error) => {
                let has_next = index + 1 < order.len();
                if has_next {
                    emit_capture_create_diagnostic(
                        app_handle,
                        "warn",
                        format_capture_backend_fallback_message(
                            *backend,
                            order[index + 1],
                            session_id,
                            monitor_index,
                            &error,
                        ),
                    );
                } else {
                    emit_capture_create_diagnostic(
                        app_handle,
                        "error",
                        format_capture_backend_failure_message(
                            *backend,
                            session_id,
                            monitor_index,
                            &error,
                        ),
                    );
                }
                failures.push(format!("{}: {}", backend.label(), error));
            }
        }

        if cancel.load(Ordering::Relaxed) || !is_current_session(runtime_handle, session_id) {
            return Err("screen capture init cancelled".to_string());
        }
    }

    Err(failures.join("; "))
}

#[cfg(target_os = "windows")]
struct WgcCapturer {
    _item: GraphicsCaptureItem,
    _d3d_device: ID3D11Device,
    _winrt_device: IDirect3DDevice,
    context: ID3D11DeviceContext,
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    _frame_arrived_handler: TypedEventHandler<Direct3D11CaptureFramePool, IInspectable>,
    frame_arrived_token: EventRegistrationToken,
    frame_rx: mpsc::Receiver<()>,
    staging: Option<ID3D11Texture2D>,
    frame_buf: Vec<u8>,
    stride: usize,
    width: usize,
    height: usize,
}

#[cfg(target_os = "windows")]
impl WgcCapturer {
    fn new(monitor_index: usize, show_cursor: bool) -> Result<Self, String> {
        initialize_winrt_for_wgc()?;
        if !GraphicsCaptureSession::IsSupported()
            .map_err(|error| format_windows_error("GraphicsCaptureSession::IsSupported", &error))?
        {
            return Err("Windows Graphics Capture is not supported on this device".to_string());
        }

        let hmonitor = wgc_monitor_for_index(monitor_index)?;
        let item = create_graphics_capture_item_for_monitor(hmonitor)?;
        let size = item
            .Size()
            .map_err(|error| format_windows_error("GraphicsCaptureItem::Size", &error))?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err(format!(
                "GraphicsCaptureItem returned invalid size {}x{}",
                size.Width, size.Height
            ));
        }

        let (d3d_device, context, winrt_device) = create_wgc_d3d_device()?;
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|error| {
            format_windows_error("Direct3D11CaptureFramePool::CreateFreeThreaded", &error)
        })?;
        let session = frame_pool.CreateCaptureSession(&item).map_err(|error| {
            format_windows_error("Direct3D11CaptureFramePool::CreateCaptureSession", &error)
        })?;
        let _ = session.SetIsCursorCaptureEnabled(show_cursor);

        let (frame_tx, frame_rx) = mpsc::sync_channel::<()>(2);
        let frame_arrived_handler =
            TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |_, _| {
                let _ = frame_tx.try_send(());
                Ok(())
            });
        let frame_arrived_token =
            frame_pool
                .FrameArrived(&frame_arrived_handler)
                .map_err(|error| {
                    format_windows_error("Direct3D11CaptureFramePool::FrameArrived", &error)
                })?;

        session.StartCapture().map_err(|error| {
            format_windows_error("GraphicsCaptureSession::StartCapture", &error)
        })?;

        let width = size.Width as usize;
        let height = size.Height as usize;
        let mut capturer = Self {
            _item: item,
            _d3d_device: d3d_device,
            _winrt_device: winrt_device,
            context,
            frame_pool,
            session,
            _frame_arrived_handler: frame_arrived_handler,
            frame_arrived_token,
            frame_rx,
            staging: None,
            frame_buf: Vec::with_capacity(width * height * 4),
            stride: width * 4,
            width,
            height,
        };
        capturer.ensure_staging(width as u32, height as u32, DXGI_FORMAT_B8G8R8A8_UNORM)?;
        Ok(capturer)
    }

    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn frame(&mut self) -> io::Result<CapturedFrame<'_>> {
        match self.frame_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "WGC frame not ready",
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "WGC frame signal disconnected",
                ));
            }
        }
        while self.frame_rx.try_recv().is_ok() {}

        let frame = self.frame_pool.TryGetNextFrame().map_err(|error| {
            windows_error_to_io("Direct3D11CaptureFramePool::TryGetNextFrame", error)
        })?;
        let content_size = frame
            .ContentSize()
            .map_err(|error| windows_error_to_io("Direct3D11CaptureFrame::ContentSize", error))?;
        if content_size.Width <= 0 || content_size.Height <= 0 {
            let _ = frame.Close();
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "WGC frame returned invalid content size {}x{}",
                    content_size.Width, content_size.Height
                ),
            ));
        }

        let surface = frame
            .Surface()
            .map_err(|error| windows_error_to_io("Direct3D11CaptureFrame::Surface", error))?;
        self.copy_surface_to_frame_buffer(&surface)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        let _ = frame.Close();

        Ok(CapturedFrame::Borrowed {
            pixels: &self.frame_buf,
            stride: self.stride,
        })
    }

    fn ensure_staging(
        &mut self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Result<(), String> {
        let needs_recreate = self.staging.is_none()
            || self.width != width as usize
            || self.height != height as usize
            || self.stride != width as usize * 4;
        if !needs_recreate {
            return Ok(());
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        unsafe {
            self._d3d_device
                .CreateTexture2D(&desc, None, Some(&mut staging))
                .map_err(|error| format_windows_error("ID3D11Device::CreateTexture2D", &error))?;
        }

        self.width = width as usize;
        self.height = height as usize;
        self.stride = self.width * 4;
        self.frame_buf.resize(self.stride * self.height, 0);
        self.staging = Some(staging.ok_or_else(|| {
            "ID3D11Device::CreateTexture2D returned no staging texture".to_string()
        })?);
        Ok(())
    }

    fn copy_surface_to_frame_buffer(&mut self, surface: &IDirect3DSurface) -> Result<(), String> {
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|error| format_windows_error("IDirect3DSurface::cast", &error))?;
        let texture: ID3D11Texture2D = unsafe {
            access.GetInterface().map_err(|error| {
                format_windows_error("IDirect3DDxgiInterfaceAccess::GetInterface", &error)
            })?
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            texture.GetDesc(&mut desc);
        }

        self.ensure_staging(desc.Width, desc.Height, desc.Format)?;
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| "WGC staging texture missing after creation".to_string())?;
        let src_resource: ID3D11Resource = texture
            .cast()
            .map_err(|error| format_windows_error("ID3D11Texture2D::cast source", &error))?;
        let dst_resource: ID3D11Resource = staging
            .cast()
            .map_err(|error| format_windows_error("ID3D11Texture2D::cast staging", &error))?;

        unsafe {
            self.context.CopyResource(&dst_resource, &src_resource);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&dst_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| format_windows_error("ID3D11DeviceContext::Map", &error))?;

            let mapped_result = copy_mapped_bgra_to_buffer(
                &mapped,
                self.width,
                self.height,
                self.stride,
                &mut self.frame_buf,
            );
            self.context.Unmap(&dst_resource, 0);
            mapped_result
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for WgcCapturer {
    fn drop(&mut self) {
        let _ = self.frame_pool.RemoveFrameArrived(self.frame_arrived_token);
        let _ = self.session.Close();
        let _ = self.frame_pool.Close();
        let _ = self._winrt_device.Close();
    }
}

#[cfg(target_os = "windows")]
const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
struct WgcMonitorHandle {
    handle: HMONITOR,
    is_primary: bool,
}

#[cfg(target_os = "windows")]
fn initialize_winrt_for_wgc() -> Result<(), String> {
    match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == RPC_E_CHANGED_MODE => Ok(()),
        Err(error) => Err(format_windows_error("RoInitialize", &error)),
    }
}

#[cfg(target_os = "windows")]
fn format_windows_error(stage: &str, error: &WindowsError) -> String {
    format!(
        "{} failed: hresult=0x{:08X}, error={}",
        stage,
        error.code().0 as u32,
        sanitize_log_field(&error.message())
    )
}

#[cfg(target_os = "windows")]
fn windows_error_to_io(stage: &str, error: WindowsError) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format_windows_error(stage, &error))
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_wgc_monitor_proc(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    const MONITORINFOF_PRIMARY: u32 = 1;
    let monitors = &mut *(data.0 as *mut Vec<WgcMonitorHandle>);
    let mut info = MONITORINFO {
        cbSize: mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let is_primary = if GetMonitorInfoW(monitor, &mut info).0 != 0 {
        info.dwFlags & MONITORINFOF_PRIMARY != 0
    } else {
        false
    };
    monitors.push(WgcMonitorHandle {
        handle: monitor,
        is_primary,
    });
    BOOL(1)
}

#[cfg(target_os = "windows")]
fn enumerate_wgc_monitors_primary_first() -> Result<Vec<WgcMonitorHandle>, String> {
    let mut monitors = Vec::<WgcMonitorHandle>::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_wgc_monitor_proc),
            LPARAM(&mut monitors as *mut _ as isize),
        )
    };
    if ok.0 == 0 {
        return Err(format_windows_error(
            "EnumDisplayMonitors",
            &WindowsError::from_win32(),
        ));
    }

    monitors.sort_by_key(|monitor| if monitor.is_primary { 0 } else { 1 });
    Ok(monitors)
}

#[cfg(target_os = "windows")]
fn wgc_monitor_for_index(monitor_index: usize) -> Result<HMONITOR, String> {
    let monitors = enumerate_wgc_monitors_primary_first()?;
    monitors
        .get(monitor_index)
        .map(|monitor| monitor.handle)
        .ok_or_else(|| {
            format!(
                "WGC monitor index {} is unavailable; detected {} monitor(s)",
                monitor_index,
                monitors.len()
            )
        })
}

#[cfg(target_os = "windows")]
fn create_graphics_capture_item_for_monitor(
    monitor: HMONITOR,
) -> Result<GraphicsCaptureItem, String> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|error| format_windows_error("GraphicsCaptureItem activation factory", &error))?;
    unsafe {
        interop.CreateForMonitor(monitor).map_err(|error| {
            format_windows_error("IGraphicsCaptureItemInterop::CreateForMonitor", &error)
        })
    }
}

#[cfg(target_os = "windows")]
fn create_wgc_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice), String> {
    let mut d3d_device = None;
    let mut d3d_context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            Some(&mut d3d_context),
        )
        .map_err(|error| format_windows_error("D3D11CreateDevice", &error))?;
    }

    let d3d_device =
        d3d_device.ok_or_else(|| "D3D11CreateDevice returned no device".to_string())?;
    let d3d_context =
        d3d_context.ok_or_else(|| "D3D11CreateDevice returned no immediate context".to_string())?;
    let dxgi_device: IDXGIDevice = d3d_device
        .cast()
        .map_err(|error| format_windows_error("ID3D11Device::cast IDXGIDevice", &error))?;
    let inspectable = unsafe {
        CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)
            .map_err(|error| format_windows_error("CreateDirect3D11DeviceFromDXGIDevice", &error))?
    };
    let winrt_device: IDirect3DDevice = inspectable
        .cast()
        .map_err(|error| format_windows_error("IInspectable::cast IDirect3DDevice", &error))?;
    Ok((d3d_device, d3d_context, winrt_device))
}

#[cfg(target_os = "windows")]
unsafe fn copy_mapped_bgra_to_buffer(
    mapped: &D3D11_MAPPED_SUBRESOURCE,
    width: usize,
    height: usize,
    dst_stride: usize,
    dst: &mut Vec<u8>,
) -> Result<(), String> {
    if mapped.pData.is_null() {
        return Err("ID3D11DeviceContext::Map returned null data pointer".to_string());
    }
    let src_stride = mapped.RowPitch as usize;
    if src_stride < dst_stride {
        return Err(format!(
            "mapped row pitch {} is smaller than expected stride {}",
            src_stride, dst_stride
        ));
    }

    let required_len = dst_stride
        .checked_mul(height)
        .ok_or_else(|| "WGC frame buffer size overflow".to_string())?;
    if dst.len() != required_len {
        dst.resize(required_len, 0);
    }

    let src_len = src_stride
        .checked_mul(height)
        .ok_or_else(|| "WGC mapped source size overflow".to_string())?;
    let src = std::slice::from_raw_parts(mapped.pData as *const u8, src_len);
    let row_bytes = width
        .checked_mul(4)
        .ok_or_else(|| "WGC row byte count overflow".to_string())?;
    for y in 0..height {
        let src_offset = y * src_stride;
        let dst_offset = y * dst_stride;
        dst[dst_offset..dst_offset + row_bytes]
            .copy_from_slice(&src[src_offset..src_offset + row_bytes]);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn create_wgc_capturer(
    monitor_index: usize,
    show_cursor: bool,
    session_id: u64,
    app_handle: &AppHandle,
) -> Result<WgcCapturer, String> {
    emit_capture_create_diagnostic(
        app_handle,
        "info",
        format!(
            "WGC capture backend create start: session_id={}, monitor_index={}, cursor={}",
            session_id, monitor_index, show_cursor
        ),
    );
    WgcCapturer::new(monitor_index, show_cursor)
}

fn create_capturer(
    monitor_index: usize,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
    app_handle: &AppHandle,
) -> Result<Capturer, String> {
    let mut last_error = None;
    let started_at = Instant::now();
    let retry_delays: &[u64] = &DXGI_CREATE_RETRY_DELAYS_MS;
    let total_attempts = retry_delays.len();

    emit_capture_create_diagnostic(
        app_handle,
        "info",
        format!(
            "屏幕捕获器创建开始: session_id={}, monitor_index={}, attempts={}, retry_delays_ms={:?}, startup_timeout_ms={}, {}",
            session_id,
            monitor_index,
            total_attempts,
            retry_delays,
            CAPTURE_STARTUP_TIMEOUT.as_millis(),
            capture_runtime_state_summary(runtime_handle, session_id)
        ),
    );

    for (attempt_index, delay_ms) in retry_delays.iter().enumerate() {
        if !wait_for_capture_retry_delay(
            Duration::from_millis(*delay_ms),
            cancel,
            runtime_handle,
            session_id,
        ) {
            emit_capture_create_diagnostic(
                app_handle,
                "info",
                format!(
                    "屏幕捕获器创建已取消: session_id={}, attempt={}/{}, delay_ms={}, elapsed_ms={}, monitor_index={}, {}",
                    session_id,
                    attempt_index + 1,
                    total_attempts,
                    delay_ms,
                    started_at.elapsed().as_millis(),
                    monitor_index,
                    capture_runtime_state_summary(runtime_handle, session_id)
                ),
            );
            return Err("screen capture init cancelled".to_string());
        }

        match create_capturer_once(monitor_index) {
            Ok(capturer) => {
                if cancel.load(Ordering::Relaxed) || !is_current_session(runtime_handle, session_id)
                {
                    emit_capture_create_diagnostic(
                        app_handle,
                        "info",
                        format!(
                            "屏幕捕获器创建成功后已过期: session_id={}, attempt={}/{}, elapsed_ms={}, monitor_index={}, {}",
                            session_id,
                            attempt_index + 1,
                            total_attempts,
                            started_at.elapsed().as_millis(),
                            monitor_index,
                            capture_runtime_state_summary(runtime_handle, session_id)
                        ),
                    );
                    return Err("screen capture init cancelled".to_string());
                }
                emit_capture_create_diagnostic(
                    app_handle,
                    "success",
                    format!(
                        "屏幕捕获器创建成功: session_id={}, attempt={}/{}, elapsed_ms={}, monitor_index={}, {}",
                        session_id,
                        attempt_index + 1,
                        total_attempts,
                        started_at.elapsed().as_millis(),
                        monitor_index,
                        capture_runtime_state_summary(runtime_handle, session_id)
                    ),
                );
                return Ok(capturer);
            }
            Err(error) => {
                let is_last_attempt = attempt_index + 1 == retry_delays.len();
                let retryable = error.retryable();
                emit_capture_create_diagnostic(
                    app_handle,
                    if is_last_attempt || !retryable {
                        "error"
                    } else {
                        "warn"
                    },
                    format!(
                        "屏幕捕获器创建失败: session_id={}, attempt={}/{}, next_delay_ms={}, elapsed_ms={}, monitor_index={}, retryable={}, error_kind={:?}, cause={}, {}",
                        session_id,
                        attempt_index + 1,
                        total_attempts,
                        retry_delays
                            .get(attempt_index + 1)
                            .copied()
                            .unwrap_or(0),
                        started_at.elapsed().as_millis(),
                        monitor_index,
                        retryable,
                        error.kind,
                        error.detail,
                        capture_runtime_state_summary(runtime_handle, session_id)
                    ),
                );
                if !error.retryable() || is_last_attempt {
                    let attempts = attempt_index + 1;
                    return Err(match last_error.take() {
                        Some(previous) if attempts > 1 => format!(
                            "{}; last_retry_failure={}; attempts={}",
                            previous, error.detail, attempts
                        ),
                        _ if attempts > 1 => format!("{}; attempts={}", error.detail, attempts),
                        _ => error.detail,
                    });
                }
                last_error = Some(error.detail);
            }
        }
    }

    Err("screen capture init exhausted retries".to_string())
}

fn create_capturer_once(monitor_index: usize) -> Result<Capturer, CaptureCreateError> {
    let displays = Display::all().map_err(|error| CaptureCreateError {
        detail: format!(
            "Display::all failed: kind={:?}, error={}",
            error.kind(),
            error
        ),
        kind: Some(error.kind()),
    })?;
    let display_count = displays.len();
    let inventory = describe_display_inventory(&displays);
    let display = displays
        .into_iter()
        .nth(monitor_index)
        .ok_or_else(|| CaptureCreateError {
            detail: format!(
                "monitor index {} is unavailable; detected {} display(s): {}",
                monitor_index, display_count, inventory
            ),
            kind: None,
        })?;
    Capturer::new(display).map_err(|error| {
        let hint = capture_creation_hint(error.kind())
            .map(|message| format!(", hint={message}"))
            .unwrap_or_default();
        CaptureCreateError {
            detail: format!(
                "Capturer::new failed for monitor index {} with {} display(s) [{}]: kind={:?}, error={}{}",
                monitor_index,
                display_count,
                inventory,
                error.kind(),
                error,
                hint
            ),
            kind: Some(error.kind()),
        }
    })
}

fn describe_display_inventory(displays: &[Display]) -> String {
    if displays.is_empty() {
        return "none".to_string();
    }

    let mut parts = Vec::with_capacity(displays.len());
    for (index, display) in displays.iter().enumerate() {
        let role = if index == 0 { ", primary" } else { "" };
        parts.push(format!(
            "#{} {}x{}{}",
            index,
            display.width(),
            display.height(),
            role
        ));
    }

    parts.join("; ")
}

fn summarize_user_agent(headers: &HeaderMap) -> String {
    let raw = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("-");

    let mut summary: String = raw.chars().take(120).collect();
    if raw.chars().count() > 120 {
        summary.push_str("...");
    }
    summary
}

// ─── JPEG Encoding ──────────────────────────────────────────

#[allow(dead_code)]
fn encode_jpeg(bgra: &[u8], width: usize, height: usize, stride: usize, quality: u8) -> Vec<u8> {
    let mut rgb_buf = Vec::with_capacity(width * height * 3);
    let mut jpeg_buf = Vec::with_capacity(width * height / 4);
    encode_jpeg_reuse(
        bgra,
        width,
        height,
        stride,
        quality,
        &mut rgb_buf,
        &mut jpeg_buf,
    )
}

/// Same as encode_jpeg but reuses caller-provided buffers to avoid allocations.
fn encode_jpeg_reuse(
    bgra: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    quality: u8,
    rgb_buf: &mut Vec<u8>,
    jpeg_buf: &mut Vec<u8>,
) -> Vec<u8> {
    if width == 0 || height == 0 || stride < width * 4 || bgra.len() < height * stride {
        return Vec::new();
    }

    // Convert BGRA (with stride padding) to packed RGB — reusing buffer
    rgb_buf.clear();
    let needed = width * height * 3;
    if rgb_buf.capacity() < needed {
        rgb_buf.reserve(needed - rgb_buf.capacity());
    }
    for y in 0..height {
        let row_start = y * stride;
        let row_end = row_start + width * 4;
        if row_end > bgra.len() {
            break;
        }
        let row = &bgra[row_start..row_end];
        for pixel in row.chunks_exact(4) {
            rgb_buf.push(pixel[2]); // R
            rgb_buf.push(pixel[1]); // G
            rgb_buf.push(pixel[0]); // B
        }
    }

    jpeg_buf.clear();
    {
        use image::ImageEncoder;
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut *jpeg_buf, quality);
        if let Err(e) = encoder.write_image(
            rgb_buf,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgb8,
        ) {
            log::warn!("JPEG encode failed: {}", e);
            return Vec::new();
        }
    }
    jpeg_buf.clone()
}

// ─── HTTP Server ────────────────────────────────────────────

async fn run_http_server(
    listener: tokio::net::TcpListener,
    state: Arc<HttpServerState>,
    shutdown_rx: oneshot::Receiver<()>,
) {
    let app = Router::new()
        .route("/", get(handler_index))
        .route("/stream", get(handler_stream))
        .route("/auth", post(handler_auth))
        .route("/status", get(handler_status))
        .with_state(state);

    if let Err(e) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        shutdown_rx.await.ok();
    })
    .await
    {
        log::error!("Screen share HTTP server error: {}", e);
    }

    log::info!("Screen share HTTP server stopped");
}

// ─── HTTP Handlers ──────────────────────────────────────────

#[derive(Deserialize)]
struct IndexQuery {
    error: Option<u8>,
}

async fn handler_index(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(q): Query<IndexQuery>,
    headers: HeaderMap,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
            let has_error = q.error.unwrap_or(0) == 1;
            let need_username = state.auth_username.is_some();
            return Html(login_html(has_error, need_username)).into_response();
        }
    }
    Html(viewer_html()).into_response()
}

async fn handler_stream(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    // Auth check
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }

    let client_ip = addr.ip().to_string();
    let viewer_total = state.viewer_count.fetch_add(1, Ordering::Relaxed) + 1;
    record_viewer_ip(&state.viewer_ips, client_ip.clone());
    crate::scanner::emit_tool_log(
        &state.app_handle,
        TOOL_NAME,
        &format!(
            "Viewer connected: ip={}, viewers={}, user_agent={}",
            client_ip,
            viewer_total,
            summarize_user_agent(&headers)
        ),
        "info",
    );
    let viewer_guard = ViewerGuard {
        app_handle: state.app_handle.clone(),
        count: state.viewer_count.clone(),
        ips: state.viewer_ips.clone(),
        ip: client_ip,
    };
    let bytes_sent = state.bytes_sent.clone();
    let cancel = state.cancel.clone();
    let mut rx = state.broadcast_tx.subscribe();

    let stream = async_stream::stream! {
        let _guard = viewer_guard;

        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            // Poll with a timeout instead of an unbounded `recv().await` so that a stop
            // (which sets `cancel` but produces no further frames) wakes this stream within
            // ~250ms. Otherwise the long-lived MJPEG response would block axum's graceful
            // shutdown indefinitely, keeping the listener — and the bound port — alive and
            // causing the next start to fail with "address already in use" (os error 10048).
            match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
                Ok(Ok(frame)) => {
                    let frame_len = frame.len();
                    let mut buf = BytesMut::with_capacity(frame_len + 128);
                    buf.extend_from_slice(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: ");
                    buf.extend_from_slice(frame_len.to_string().as_bytes());
                    buf.extend_from_slice(b"\r\n\r\n");
                    buf.extend_from_slice(&frame);
                    buf.extend_from_slice(b"\r\n");
                    bytes_sent.fetch_add(buf.len() as u64, Ordering::Relaxed);
                    yield Ok::<_, Infallible>(buf.freeze());
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    // Slow viewer: skip to latest frame
                    continue;
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    break;
                }
                Err(_) => {
                    // No frame within the poll window; loop back to re-check `cancel` so
                    // graceful shutdown can complete promptly after a stop.
                    continue;
                }
            }
        }
    };

    Response::builder()
        .header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
        .header("Cache-Control", "no-cache, no-store")
        .header("Connection", "keep-alive")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from_stream(stream))
        .unwrap()
}

#[derive(Deserialize)]
struct AuthForm {
    username: Option<String>,
    password: String,
}

async fn handler_auth(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Form(form): Form<AuthForm>,
) -> Response {
    if let Some(expected) = &state.auth_hash {
        let submitted = hash_credential(form.username.as_deref(), &form.password);
        if submitted == *expected {
            return Response::builder()
                .status(StatusCode::SEE_OTHER)
                .header("Location", "/")
                .header(
                    "Set-Cookie",
                    format!("ss_auth={}; HttpOnly; Path=/", expected),
                )
                .body(Body::empty())
                .unwrap();
        }
    }
    // Wrong password → redirect with error flag
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/?error=1")
        .body(Body::empty())
        .unwrap()
}

async fn handler_status(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    record_viewer_ip(&state.viewer_ips, addr.ip().to_string());
    Json(serde_json::json!({
        "active": !state.cancel.load(Ordering::Relaxed),
        "viewers": state.viewer_count.load(Ordering::Relaxed),
    }))
}

// ─── Status Reporter ────────────────────────────────────────

async fn status_reporter(
    app_handle: AppHandle,
    active: Arc<AtomicBool>,
    viewer_count: Arc<AtomicU32>,
    fps_counter: Arc<AtomicU32>,
    bytes_sent: Arc<AtomicU64>,
    server_url: String,
    all_urls: Vec<String>,
    start_time: Instant,
    viewer_ips: Arc<Mutex<ViewerIpMap>>,
    capture_paused: Arc<AtomicBool>,
    runtime_handle: Arc<ScreenShareHandle>,
    session_id: u64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut last_bytes: u64 = 0;

    loop {
        interval.tick().await;

        if !active.load(Ordering::Relaxed) || !is_current_session(&runtime_handle, session_id) {
            break;
        }

        let fps_count = fps_counter.swap(0, Ordering::Relaxed);
        let current_bytes = bytes_sent.load(Ordering::Relaxed);
        let bytes_delta = current_bytes.saturating_sub(last_bytes);
        last_bytes = current_bytes;

        let connected_ips = snapshot_viewer_ips(&viewer_ips);

        let status = ScreenShareStatus {
            is_active: true,
            viewer_count: viewer_count.load(Ordering::Relaxed),
            connection_count: connected_ips.len() as u32,
            fps_actual: fps_count as f32,
            bitrate_kbps: (bytes_delta * 8 / 1024) as u32,
            uptime_secs: start_time.elapsed().as_secs(),
            server_url: server_url.clone(),
            all_urls: all_urls.clone(),
            connected_ips,
            capture_paused: capture_paused.load(Ordering::Relaxed),
        };

        let _ = app_handle.emit("screen-share-status", &status);
    }
}

// ─── Utility ────────────────────────────────────────────────

/// Hash credential: if username is provided, hash "username:password", else just password.
fn hash_credential(username: Option<&str>, password: &str) -> String {
    let mut hasher = Sha256::new();
    if let Some(user) = username {
        hasher.update(user.as_bytes());
        hasher.update(b":");
    }
    hasher.update(password.as_bytes());
    hasher.update(b"screen_share_salt_fst");
    format!("{:x}", hasher.finalize())
}

fn check_auth_cookie(headers: &HeaderMap, expected_hash: &str) -> bool {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(|cookies| {
            cookies.split(';').any(|c| {
                let c = c.trim();
                c.strip_prefix("ss_auth=")
                    .is_some_and(|value| value == expected_hash)
            })
        })
        .unwrap_or(false)
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
        o[0] == 192 && o[1] == 168 || o[0] == 10 || o[0] == 172 && (16..=31).contains(&o[1])
    }
    ips.sort_by_key(|ip| if is_common_lan(ip) { 0 } else { 1 });

    ips.into_iter().map(|ip| ip.to_string()).collect()
}

/// Generate a tiny 1×1 dark-blue JPEG placeholder so viewers see something immediately.
fn make_placeholder_jpeg() -> Vec<u8> {
    let rgb = [15u8, 23, 42]; // #0f172a – matches viewer background
    let mut buf = Vec::with_capacity(256);
    {
        use image::ImageEncoder;
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 30);
        let _ = encoder.write_image(&rgb, 1, 1, image::ExtendedColorType::Rgb8);
    }
    buf
}

// ─── Embedded HTML ──────────────────────────────────────────

fn viewer_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Screen Share</title>
<link rel="icon" type="image/svg+xml" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%236366f1' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Crect x='2' y='4' width='20' height='14' rx='2'/%3E%3Cpath d='M12 18v4'/%3E%3Cpath d='M8 22h8'/%3E%3Cpath d='M12 14V8'/%3E%3Cpath d='m8 12 4-4 4 4'/%3E%3C/svg%3E">
<style>
*{margin:0;padding:0;box-sizing:border-box}
html,body{height:100%;background:#060911;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;overflow:hidden}
.wrap{display:flex;flex-direction:column;height:100%;position:relative}
.view{flex:1;display:flex;align-items:center;justify-content:center;overflow:hidden;background:#060911;position:relative}
#screen{max-width:100%;max-height:100%;object-fit:contain;display:block;transform:translateZ(0);will-change:transform;backface-visibility:hidden}
.paused-overlay{display:none;position:absolute;inset:0;background:rgba(6,9,17,.75);backdrop-filter:blur(4px);align-items:center;justify-content:center;z-index:5}
.paused-overlay.show{display:flex}
.paused-badge{display:flex;align-items:center;gap:10px;background:rgba(15,23,42,.9);border:1px solid rgba(255,255,255,.08);padding:14px 28px;border-radius:14px;font-size:16px;font-weight:600;color:#94a3b8;letter-spacing:.02em}
.bar{position:relative;flex-shrink:0;display:flex;align-items:center;gap:8px;padding:10px 14px;padding-bottom:max(10px,env(safe-area-inset-bottom));min-height:52px;background:rgba(10,14,22,.95);border-top:1px solid rgba(255,255,255,.06);backdrop-filter:blur(12px);flex-wrap:wrap;row-gap:8px}
.status-pill{display:flex;align-items:center;gap:7px;background:rgba(255,255,255,.05);border:1px solid rgba(255,255,255,.07);border-radius:20px;padding:5px 12px;font-size:12px;font-weight:500;color:#94a3b8;letter-spacing:.01em;white-space:nowrap}
.dot{width:7px;height:7px;border-radius:50%;flex-shrink:0}
.dot-on{background:#22c55e;box-shadow:0 0 8px #22c55e90;animation:pulse 2s infinite}
.dot-off{background:#ef4444;box-shadow:0 0 6px #ef444460}
.dot-pause{background:#f59e0b;box-shadow:0 0 6px #f59e0b60}
.dot-retry{background:#f97316;animation:blink .7s infinite}
@keyframes pulse{0%,100%{opacity:1;transform:scale(1)}50%{opacity:.6;transform:scale(.85)}}
@keyframes blink{0%,100%{opacity:1}50%{opacity:.25}}
.viewers-badge{font-size:11px;color:#475569;background:rgba(255,255,255,.04);border:1px solid rgba(255,255,255,.06);border-radius:20px;padding:4px 10px;white-space:nowrap}
.spacer{flex:1;min-width:8px}
.ctrl{display:flex;align-items:center;gap:5px;font-size:12px;color:#475569}
.ctrl label{color:#4b5563;white-space:nowrap}
.ctrl select{background:rgba(255,255,255,.06);border:1px solid rgba(255,255,255,.09);color:#94a3b8;padding:4px 24px 4px 8px;border-radius:7px;font-size:11px;cursor:pointer;outline:none;appearance:none;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10' viewBox='0 0 24 24' fill='none' stroke='%2364748b' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 6px center}
.ctrl select:hover{border-color:rgba(255,255,255,.15)}
.btn{background:rgba(255,255,255,.07);border:1px solid rgba(255,255,255,.1);color:#cbd5e1;padding:6px 14px;border-radius:8px;cursor:pointer;font-size:12px;font-weight:500;transition:all .15s;white-space:nowrap;letter-spacing:.01em;display:inline-flex;align-items:center;gap:5px}
.btn:hover{background:rgba(255,255,255,.12);border-color:rgba(255,255,255,.18)}
.btn-play{background:rgba(34,197,94,.12);border-color:rgba(34,197,94,.25);color:#4ade80}
.btn-play:hover{background:rgba(34,197,94,.2)}
.btn-pause{background:rgba(245,158,11,.1);border-color:rgba(245,158,11,.2);color:#fbbf24}
.btn-pause:hover{background:rgba(245,158,11,.18)}
.btn-fs{background:rgba(99,102,241,.1);border-color:rgba(99,102,241,.2);color:#a5b4fc}
.btn-fs:hover{background:rgba(99,102,241,.18)}
</style>
</head>
<body>
<div class="wrap">
  <div class="view">
    <img id="screen" src="/stream" alt="">
    <div id="paused-overlay" class="paused-overlay">
      <div class="paused-badge">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
        <span id="pausedText"></span>
      </div>
    </div>
  </div>
  <div class="bar">
    <div class="status-pill">
      <div id="dot" class="dot dot-on"></div>
      <span id="status-text"></span>
    </div>
    <span class="viewers-badge" id="viewers" style="display:none"></span>
    <div class="spacer"></div>
    <div class="ctrl">
      <label for="fpsLimit" id="refreshLabel"></label>
      <select id="fpsLimit">
        <option value="0" id="optOriginal"></option>
        <option value="500">~2 FPS</option>
        <option value="1000">~1 FPS</option>
        <option value="2000">0.5 FPS</option>
        <option value="5000">5 s</option>
      </select>
    </div>
    <button id="btnPause" class="btn btn-pause" onclick="togglePause()">
      <svg id="iconPause" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
      <svg id="iconPlay" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" style="display:none"><polygon points="5 3 19 12 5 21 5 3"/></svg>
      <span id="btnPauseText"></span>
    </button>
    <button class="btn btn-fs" onclick="toggleFs()">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/></svg>
      <span id="fsText"></span>
    </button>
  </div>
</div>
<script>
// ── i18n ──
const isZh=navigator.language&&navigator.language.toLowerCase().startsWith('zh');
const T={
  connected:isZh?'已连接':'Connected',
  disconnected:isZh?'已断开':'Disconnected',
  serverStopped:isZh?'服务已停止':'Server stopped',
  reconnecting:isZh?'重新连接中':'Reconnecting',
  paused:isZh?'已暂停':'Paused',
  pause:isZh?'暂停':'Pause',
  resume:isZh?'继续':'Resume',
  refresh:isZh?'刷新率':'Refresh',
  original:isZh?'原始':'Original',
  fullscreen:isZh?'全屏':'Fullscreen',
  viewer:isZh?'位观看者':'viewer',viewers:isZh?'位观看者':'viewers',
};
// Apply i18n to static elements
document.getElementById('pausedText').textContent=T.paused;
document.getElementById('btnPauseText').textContent=T.pause;
document.getElementById('refreshLabel').textContent=T.refresh;
document.getElementById('optOriginal').textContent=T.original;
document.getElementById('fsText').textContent=T.fullscreen;

const img=document.getElementById('screen'),dot=document.getElementById('dot'),st=document.getElementById('status-text'),vw=document.getElementById('viewers');
const btnPause=document.getElementById('btnPause'),overlay=document.getElementById('paused-overlay'),fpsSelect=document.getElementById('fpsLimit');
let alive=true,paused=false,fpsLimitMs=0,refreshTimer=null;
let lastFrameTime=Date.now(),reconnectAttempts=0;
let reconnectPending=false,lastReconnectAt=0;
const MIN_HEARTBEAT_RECONNECT_MS=60000;
st.textContent=T.connected;

// Ask the OS/browser to keep this tab treated as active — defeats Edge's
// Efficiency Mode deprioritization that otherwise stalls fetch('/status').
if('wakeLock' in navigator){navigator.wakeLock.request('screen').catch(()=>{});}

// Hold the last rendered frame during a reconnect so the viewer never sees
// a black frame while the new MJPEG stream is opening its first chunk.
let heldFrame=null;
function holdCurrentFrame(){
  if(!img.naturalWidth)return;
  releaseHeldFrame();
  try{
    const c=document.createElement('canvas');
    c.width=img.naturalWidth;c.height=img.naturalHeight;
    c.getContext('2d').drawImage(img,0,0);
    const h=document.createElement('div');
    h.style.cssText='position:absolute;inset:0;background-image:url('+c.toDataURL('image/jpeg',0.6)+');background-size:contain;background-position:center;background-repeat:no-repeat;pointer-events:none;z-index:2';
    img.parentElement.appendChild(h);
    heldFrame=h;
  }catch(e){/* tainted canvas or decode error — fall back to black */}
}
function releaseHeldFrame(){if(heldFrame){heldFrame.remove();heldFrame=null;}}

// ── Stream connection ──
function connectStream(){
  // Freeze last frame as a placeholder so reassigning img.src does not flash black.
  if(img.naturalWidth>0)holdCurrentFrame();
  img.src='/stream?t='+Date.now();
  lastFrameTime=Date.now();
}
function disconnectStream(){
  img.src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7';
}
function setConnected(){
  if(!alive){alive=true;reconnectAttempts=0}
  dot.className='dot dot-on';st.textContent=T.connected;
}
function setDisconnected(msg){
  alive=false;
  dot.className='dot dot-off';st.textContent=msg||T.disconnected;
}
function setReconnecting(){
  dot.className='dot dot-retry';
  st.textContent=T.reconnecting+(reconnectAttempts>1?' ('+reconnectAttempts+')':'')+'...';
}
function tryReconnect(){
  if(paused)return;
  if(reconnectPending)return;  // debounce — avoid stacking concurrent reconnects
  reconnectPending=true;
  reconnectAttempts++;
  setReconnecting();
  // Exponential backoff: 2s, 3s, 4s, 5s max
  let delay=Math.min(2000+reconnectAttempts*500,5000);
  setTimeout(()=>{
    reconnectPending=false;
    if(paused)return;
    if(fpsLimitMs>0){startPolling()}else{connectStream()}
  },delay);
}

// Track when the MJPEG stream delivers a new frame
// For MJPEG, onload fires on the first frame only in most browsers.
// We use a combination of onerror + heartbeat to detect stream loss.
img.onerror=function(){
  if(paused)return;
  holdCurrentFrame();
  setDisconnected();
  tryReconnect();
};
img.onload=function(){
  clearTimeout(initialTimer);
  lastFrameTime=Date.now();
  setConnected();
  releaseHeldFrame();
  heartbeatFails=0;
};

// 5s timeout: if stream never delivers a frame, force reconnect
let initialTimer=setTimeout(()=>{
  if(img.naturalWidth===0){connectStream()}
},5000);

// ── Heartbeat: detect stream loss via /status polling ──
// MJPEG streams don't fire onerror when the TCP connection drops mid-stream.
// This heartbeat detects that and triggers reconnection.
// Tolerances are tuned to survive Edge's Efficiency Mode / SDSM, which can
// delay fetch() long enough to trip short timeouts even when the stream is fine.
let heartbeatFails=0;
document.addEventListener('visibilitychange',()=>{if(!document.hidden)heartbeatFails=0;});
setInterval(async()=>{
  if(paused||document.hidden)return;
  try{
    const r=await fetch('/status',{signal:AbortSignal.timeout(6000),cache:'no-store'});
    if(r.ok){
      const d=await r.json();
      heartbeatFails=0;
      if(d.viewers>0){vw.textContent=d.viewers+' '+(d.viewers>1?T.viewers:T.viewer);vw.style.display=''}else{vw.style.display='none'}
      if(!d.active&&alive){
        // Server stopped sharing
        setDisconnected(T.serverStopped);
        disconnectStream();
        tryReconnect();
      }
    } else {
      heartbeatFails++;
    }
  }catch{
    heartbeatFails++;
  }
  // If heartbeat fails 10+ times in a row AND it's been >=60s since the last
  // heartbeat-triggered reconnect, only then reconnect. This double gate stops
  // the "flicker loop" we saw on Edge where fetch('/status') keeps timing out
  // under Efficiency Mode even though the MJPEG stream is still delivering.
  // Real TCP drops are caught by img.onerror and go through a separate path.
  if(heartbeatFails>=10&&alive){
    const now=Date.now();
    if(now-lastReconnectAt<MIN_HEARTBEAT_RECONNECT_MS)return;
    lastReconnectAt=now;
    heartbeatFails=0;           // reset so the next fail does not instantly retrigger
    holdCurrentFrame();         // freeze last frame as placeholder
    setDisconnected();
    // Skip disconnectStream() here — connectStream() will reassign img.src,
    // which itself aborts the old MJPEG connection. Doing it twice caused the flash.
    tryReconnect();
  }
},3000);

// ── Stale frame detection ──
// If we haven't received a new frame in a while via MJPEG stream,
// proactively reconnect. For MJPEG, we can only detect the initial load,
// so we use a periodic check against the heartbeat instead.
// The above heartbeat covers this case.

// ── Pause / Resume ──
const iconPause=document.getElementById('iconPause'),iconPlay=document.getElementById('iconPlay'),btnPauseText=document.getElementById('btnPauseText');
function togglePause(){
  paused=!paused;
  if(paused){
    disconnectStream();
    dot.className='dot dot-pause';st.textContent=T.paused;
    iconPause.style.display='none';iconPlay.style.display='';btnPauseText.textContent=T.resume;
    btnPause.className='btn btn-play';
    overlay.classList.add('show');
    if(refreshTimer){clearInterval(refreshTimer);refreshTimer=null}
  } else {
    overlay.classList.remove('show');
    reconnectAttempts=0;
    setConnected();
    iconPause.style.display='';iconPlay.style.display='none';btnPauseText.textContent=T.pause;
    btnPause.className='btn btn-pause';
    if(fpsLimitMs>0){startPolling()}else{connectStream()}
  }
}

// ── Client-side FPS limit (polling mode) ──
function startPolling(){
  disconnectStream();
  if(refreshTimer)clearInterval(refreshTimer);
  refreshTimer=setInterval(()=>{
    if(!paused){
      const pollImg=new Image();
      pollImg.onload=function(){lastFrameTime=Date.now();setConnected();img.src=pollImg.src};
      pollImg.onerror=function(){if(!paused){setDisconnected();tryReconnect()}};
      pollImg.src='/stream?single=1&t='+Date.now();
    }
  },fpsLimitMs);
  // Fetch first frame immediately
  img.src='/stream?single=1&t='+Date.now();
  lastFrameTime=Date.now();
}
fpsSelect.onchange=function(){
  fpsLimitMs=parseInt(this.value)||0;
  if(paused)return;
  if(refreshTimer){clearInterval(refreshTimer);refreshTimer=null}
  reconnectAttempts=0;
  if(fpsLimitMs>0){startPolling()}else{connectStream()}
};

function toggleFs(){
  if(!document.fullscreenElement)document.documentElement.requestFullscreen();
  else document.exitFullscreen();
}
</script>
</body>
</html>"#
        .to_string()
}

fn login_html(has_error: bool, need_username: bool) -> String {
    let error_block = if has_error {
        r#"<div class="err"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="display:inline;vertical-align:middle;margin-right:5px"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>Incorrect credentials</div>"#
    } else {
        ""
    };
    let username_field = if need_username {
        r#"<div class="field"><label>Username</label><input type="text" name="username" placeholder="Enter username" autofocus required></div>"#
    } else {
        ""
    };
    let password_autofocus = if need_username { "" } else { " autofocus" };
    let description = if need_username {
        "Enter your credentials to view the screen share"
    } else {
        "Enter the access password to view the screen share"
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Screen Share</title>
<link rel="icon" type="image/svg+xml" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='%236366f1' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Crect x='2' y='4' width='20' height='14' rx='2'/%3E%3Cpath d='M12 18v4'/%3E%3Cpath d='M8 22h8'/%3E%3Cpath d='M12 14V8'/%3E%3Cpath d='m8 12 4-4 4 4'/%3E%3C/svg%3E">
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
html,body{{height:100%;background:#060911;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}}
.bg{{height:100%;display:flex;align-items:center;justify-content:center;padding:20px;background:radial-gradient(ellipse at 50% 0%,rgba(99,102,241,.08) 0%,transparent 60%)}}
.card{{background:rgba(15,23,42,.9);border:1px solid rgba(255,255,255,.08);border-radius:20px;padding:36px 32px;width:100%;max-width:380px;box-shadow:0 25px 50px rgba(0,0,0,.5)}}
.icon{{width:44px;height:44px;background:rgba(99,102,241,.12);border:1px solid rgba(99,102,241,.2);border-radius:12px;display:flex;align-items:center;justify-content:center;margin-bottom:20px;color:#818cf8}}
h1{{font-size:20px;font-weight:700;color:#f1f5f9;margin-bottom:6px;letter-spacing:-.01em}}
.desc{{font-size:14px;color:#475569;margin-bottom:24px;line-height:1.5}}
.field{{margin-bottom:14px}}
.field label{{display:block;font-size:12px;font-weight:600;color:#64748b;margin-bottom:6px;text-transform:uppercase;letter-spacing:.05em}}
input{{width:100%;padding:10px 14px;background:rgba(0,0,0,.3);border:1px solid rgba(255,255,255,.09);border-radius:10px;color:#e2e8f0;font-size:14px;outline:none;transition:border-color .15s}}
input:focus{{border-color:rgba(99,102,241,.5);box-shadow:0 0 0 3px rgba(99,102,241,.1)}}
input::placeholder{{color:#334155}}
button{{width:100%;margin-top:6px;padding:11px;background:#4f46e5;color:#fff;border:none;border-radius:10px;font-size:14px;font-weight:600;cursor:pointer;transition:background .15s;letter-spacing:.01em}}
button:hover{{background:#4338ca}}
button:active{{transform:scale(.99)}}
.err{{display:flex;align-items:center;color:#f87171;font-size:13px;margin-top:14px;background:rgba(239,68,68,.08);border:1px solid rgba(239,68,68,.15);border-radius:8px;padding:9px 12px}}
</style>
</head>
<body>
<div class="bg">
  <form class="card" method="POST" action="/auth">
    <div class="icon">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="2" y="3" width="20" height="14" rx="2" ry="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>
    </div>
    <h1>Screen Share</h1>
    <p class="desc">{description}</p>
    {username_field}
    <div class="field"><label>Password</label><input type="password" name="password" placeholder="Enter password"{password_autofocus} required></div>
    <button type="submit">Enter</button>
    {error_block}
  </form>
</div>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_share_embedded_pages_include_svg_favicon() {
        let viewer = viewer_html();
        let login = login_html(false, false);

        for html in [viewer, login] {
            assert!(html.contains(r#"<link rel="icon" type="image/svg+xml""#));
            assert!(html.contains("data:image/svg+xml"));
        }
    }

    #[test]
    fn prepare_runtime_state_for_start_clears_stale_runtime_state() {
        let handle = ScreenShareHandle::new();
        handle.active.store(true, Ordering::SeqCst);
        current_cancel_token(&handle).store(true, Ordering::SeqCst);
        handle.viewer_count.store(3, Ordering::Relaxed);
        handle.fps_counter.store(7, Ordering::Relaxed);
        handle.bytes_sent.store(4096, Ordering::Relaxed);
        *handle.shutdown_tx.lock().unwrap() = Some(oneshot::channel::<()>().0);
        *handle.server_url.lock().unwrap() = "http://stale".into();
        *handle.all_urls.lock().unwrap() = vec!["http://stale".into()];
        *handle.start_time.lock().unwrap() = Some(Instant::now());
        record_viewer_ip(&handle.viewer_ips, "10.0.0.1");

        prepare_runtime_state_for_start(&handle);

        assert!(!handle.active.load(Ordering::SeqCst));
        assert!(!current_cancel_token(&handle).load(Ordering::SeqCst));
        assert_eq!(handle.viewer_count.load(Ordering::Relaxed), 0);
        assert_eq!(handle.fps_counter.load(Ordering::Relaxed), 0);
        assert_eq!(handle.bytes_sent.load(Ordering::Relaxed), 0);
        assert!(handle.shutdown_tx.lock().unwrap().is_none());
        assert!(handle.server_url.lock().unwrap().is_empty());
        assert!(handle.all_urls.lock().unwrap().is_empty());
        assert!(handle.start_time.lock().unwrap().is_none());
        assert!(handle.viewer_ips.lock().unwrap().is_empty());
    }

    #[test]
    fn reset_runtime_state_marks_handle_inactive_and_clears_runtime_fields() {
        let handle = ScreenShareHandle::new();
        handle.active.store(true, Ordering::SeqCst);
        handle.viewer_count.store(2, Ordering::Relaxed);
        handle.fps_counter.store(5, Ordering::Relaxed);
        handle.bytes_sent.store(2048, Ordering::Relaxed);
        *handle.shutdown_tx.lock().unwrap() = Some(oneshot::channel::<()>().0);
        *handle.server_url.lock().unwrap() = "http://active".into();
        *handle.all_urls.lock().unwrap() = vec!["http://active".into()];
        *handle.start_time.lock().unwrap() = Some(Instant::now());
        record_viewer_ip(&handle.viewer_ips, "10.0.0.2");

        reset_runtime_state(&handle);

        assert!(!handle.active.load(Ordering::SeqCst));
        assert!(current_cancel_token(&handle).load(Ordering::SeqCst));
        assert_eq!(handle.viewer_count.load(Ordering::Relaxed), 0);
        assert_eq!(handle.fps_counter.load(Ordering::Relaxed), 0);
        assert_eq!(handle.bytes_sent.load(Ordering::Relaxed), 0);
        assert!(handle.shutdown_tx.lock().unwrap().is_none());
        assert!(handle.server_url.lock().unwrap().is_empty());
        assert!(handle.all_urls.lock().unwrap().is_empty());
        assert!(handle.start_time.lock().unwrap().is_none());
        assert!(handle.viewer_ips.lock().unwrap().is_empty());
    }

    #[test]
    fn new_session_gets_fresh_cancel_token_and_old_token_stays_cancelled() {
        let handle = ScreenShareHandle::new();
        let old_token = current_cancel_token(&handle);

        // 停止/失败路径：当前 token 被取消
        reset_runtime_state(&handle);
        assert!(old_token.load(Ordering::SeqCst));

        // 新会话启动：拿到全新的未取消 token，旧 token 永久保持取消
        prepare_runtime_state_for_start(&handle);
        let new_token = current_cancel_token(&handle);
        assert!(!new_token.load(Ordering::SeqCst));
        assert!(old_token.load(Ordering::SeqCst));
        assert!(!Arc::ptr_eq(&old_token, &new_token));
    }

    #[test]
    fn auto_mode_tries_wgc_before_dxgi_on_initial_start() {
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Auto,
                CaptureStartKind::InitialStart,
                None
            ),
            vec![CaptureBackendKind::Wgc, CaptureBackendKind::Dxgi]
        );
    }

    #[test]
    fn explicit_modes_are_strict_on_initial_start() {
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Dxgi,
                CaptureStartKind::InitialStart,
                None
            ),
            vec![CaptureBackendKind::Dxgi]
        );
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Wgc,
                CaptureStartKind::InitialStart,
                None
            ),
            vec![CaptureBackendKind::Wgc]
        );
    }

    #[test]
    fn runtime_recreate_prefers_current_backend_then_survival_fallback() {
        // 运行中重建：先试当前存活过的后端，另一个作为保命降级——即使用户显式选了 DXGI
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Dxgi,
                CaptureStartKind::RuntimeRecreate,
                Some(CaptureBackendKind::Dxgi)
            ),
            vec![CaptureBackendKind::Dxgi, CaptureBackendKind::Wgc]
        );
        assert_eq!(
            capture_backend_order(
                ScreenShareBackendMode::Auto,
                CaptureStartKind::RuntimeRecreate,
                Some(CaptureBackendKind::Wgc)
            ),
            vec![CaptureBackendKind::Wgc, CaptureBackendKind::Dxgi]
        );
    }

    #[test]
    fn capture_recreate_backoff_grows_and_caps_at_30s() {
        assert_eq!(capture_recreate_backoff(0), Duration::from_millis(1000));
        assert_eq!(capture_recreate_backoff(1), Duration::from_millis(2000));
        assert_eq!(capture_recreate_backoff(2), Duration::from_millis(4000));
        assert_eq!(capture_recreate_backoff(5), Duration::from_millis(30000));
        assert_eq!(capture_recreate_backoff(100), Duration::from_millis(30000));
    }

    #[test]
    fn begin_start_rejects_second_start_until_guard_is_released() {
        let handle = Arc::new(ScreenShareHandle::new());

        let first = begin_screen_share_start(&handle).expect("first start should reserve runtime");
        let second = begin_screen_share_start(&handle);
        let second_error = match second {
            Ok(_) => panic!("second start should be rejected while startup is reserved"),
            Err(error) => error,
        };

        assert!(second_error.contains("starting"));
        assert!(handle.starting.load(Ordering::SeqCst));
        assert!(!handle.active.load(Ordering::SeqCst));

        drop(first);

        assert!(!handle.starting.load(Ordering::SeqCst));
        assert!(current_cancel_token(&handle).load(Ordering::SeqCst));
    }

    #[test]
    fn reset_runtime_state_invalidates_reserved_capture_session() {
        let handle = Arc::new(ScreenShareHandle::new());
        let start = begin_screen_share_start(&handle).expect("start should reserve a session");
        let session_id = start.session_id();

        assert!(is_current_session(&handle, session_id));

        reset_runtime_state(&handle);

        assert!(!is_current_session(&handle, session_id));
        assert!(!handle.starting.load(Ordering::SeqCst));
        assert!(current_cancel_token(&handle).load(Ordering::SeqCst));
    }

    #[test]
    fn capture_retry_delay_exits_immediately_when_cancelled_or_stale() {
        let handle = Arc::new(ScreenShareHandle::new());
        let start = begin_screen_share_start(&handle).expect("start should reserve a session");
        let session_id = start.session_id();

        let session_token = current_cancel_token(&handle);
        session_token.store(true, Ordering::SeqCst);
        let cancelled_at = Instant::now();
        assert!(!wait_for_capture_retry_delay(
            Duration::from_secs(5),
            &session_token,
            &handle,
            session_id,
        ));
        assert!(cancelled_at.elapsed() < Duration::from_millis(250));

        session_token.store(false, Ordering::SeqCst);
        reset_runtime_state(&handle);
        let stale_at = Instant::now();
        assert!(!wait_for_capture_retry_delay(
            Duration::from_secs(5),
            &session_token,
            &handle,
            session_id,
        ));
        assert!(stale_at.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn dxgi_failure_fallback_message_names_wgc_target_backend() {
        let message = format_capture_backend_fallback_message(
            CaptureBackendKind::Dxgi,
            CaptureBackendKind::Wgc,
            7,
            0,
            "Capturer::new failed",
        );

        assert!(message.contains("DXGI"));
        assert!(message.contains("WGC"));
        assert!(message.contains("session_id=7"));
        assert!(message.contains("monitor_index=0"));
        assert!(message.contains("Capturer::new failed"));
    }

    #[test]
    fn capture_backend_failure_message_includes_backend_and_cause() {
        let message = format_capture_backend_failure_message(
            CaptureBackendKind::Wgc,
            2,
            1,
            "CreateForMonitor failed",
        );

        assert!(message.contains("WGC"));
        assert!(message.contains("session_id=2"));
        assert!(message.contains("monitor_index=1"));
        assert!(message.contains("CreateForMonitor failed"));
    }

    #[test]
    fn screen_share_backend_modes_serialize_as_expected_labels() {
        assert_eq!(ScreenShareBackendMode::Auto.label(), "auto");
        assert_eq!(ScreenShareBackendMode::Wgc.label(), "wgc");
        assert_eq!(ScreenShareBackendMode::Dxgi.label(), "dxgi");
        assert_eq!(
            ScreenShareBackendMode::default(),
            ScreenShareBackendMode::Auto
        );
    }

    #[test]
    fn snapshot_viewer_ips_prunes_stale_heartbeats() {
        let now = Instant::now();
        let ips = Arc::new(Mutex::new(std::collections::HashMap::new()));

        record_viewer_ip_at(&ips, "10.0.0.1", now - VIEWER_IP_TTL / 2);
        record_viewer_ip_at(&ips, "10.0.0.2", now - VIEWER_IP_TTL * 2);

        assert_eq!(
            snapshot_viewer_ips_at(&ips, now),
            vec!["10.0.0.1".to_string()]
        );
        assert_eq!(
            snapshot_viewer_ips_at(&ips, now + VIEWER_IP_TTL * 2),
            Vec::<String>::new()
        );
    }

    #[test]
    fn should_retry_capture_creation_retries_transient_desktop_duplication_errors() {
        assert!(should_retry_capture_creation(std::io::ErrorKind::Other));
        assert!(should_retry_capture_creation(
            std::io::ErrorKind::Interrupted
        ));
        assert!(should_retry_capture_creation(
            std::io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn should_retry_capture_creation_skips_non_transient_input_errors() {
        assert!(!should_retry_capture_creation(
            std::io::ErrorKind::InvalidInput
        ));
        assert!(!should_retry_capture_creation(
            std::io::ErrorKind::InvalidData
        ));
    }
}
