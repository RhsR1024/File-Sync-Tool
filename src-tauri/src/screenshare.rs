use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Query, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{body::Body, Form, Json, Router};
use bytes::{Bytes, BytesMut};
use scrap::{Capturer, Display};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{broadcast, oneshot};

// ─── Public Data Types ──────────────────────────────────────

const TOOL_NAME: &str = "屏幕共享";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareConfig {
    pub port: u16,
    pub password: Option<String>,
    pub monitor_index: usize,
    pub quality: u8,
    pub fps: u8,
    pub show_cursor: bool,
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
    pub fps_actual: f32,
    pub bitrate_kbps: u32,
    pub uptime_secs: u64,
    pub server_url: String,
    pub all_urls: Vec<String>,
}

// ─── Handle (stored in AppState) ────────────────────────────

pub struct ScreenShareHandle {
    active: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    viewer_count: Arc<AtomicU32>,
    fps_counter: Arc<AtomicU32>,
    bytes_sent: Arc<AtomicU64>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    server_url: Mutex<String>,
    all_urls: Mutex<Vec<String>>,
    start_time: Mutex<Option<Instant>>,
}

impl ScreenShareHandle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            viewer_count: Arc::new(AtomicU32::new(0)),
            fps_counter: Arc::new(AtomicU32::new(0)),
            bytes_sent: Arc::new(AtomicU64::new(0)),
            shutdown_tx: Mutex::new(None),
            server_url: Mutex::new(String::new()),
            all_urls: Mutex::new(Vec::new()),
            start_time: Mutex::new(None),
        }
    }
}

// ─── Internal: HTTP server state ────────────────────────────

struct HttpServerState {
    broadcast_tx: broadcast::Sender<Arc<Bytes>>,
    viewer_count: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    password_hash: Option<String>,
    bytes_sent: Arc<AtomicU64>,
}

/// RAII guard that decrements viewer count on drop.
struct ViewerGuard(Arc<AtomicU32>);

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        // Use a CAS loop to ensure we never underflow below 0.
        loop {
            let current = self.0.load(Ordering::Relaxed);
            if current == 0 {
                break;
            }
            if self
                .0
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
}

// ─── Tauri Commands ─────────────────────────────────────────

#[tauri::command]
pub fn screen_share_list_monitors() -> Result<Vec<MonitorInfo>, String> {
    let displays =
        Display::all().map_err(|e| format!("Failed to enumerate displays: {}", e))?;

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
pub async fn screen_share_start(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
    config: ScreenShareConfig,
) -> Result<String, String> {
    let handle = &state.screen_share;

    if handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is already active".into());
    }

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

    // Broadcast channel for JPEG frames
    let (broadcast_tx, _) = broadcast::channel::<Arc<Bytes>>(8);

    // Reset counters
    handle.cancel.store(false, Ordering::SeqCst);
    handle.viewer_count.store(0, Ordering::Relaxed);
    handle.fps_counter.store(0, Ordering::Relaxed);
    handle.bytes_sent.store(0, Ordering::Relaxed);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    *handle.shutdown_tx.lock().unwrap() = Some(shutdown_tx);
    *handle.server_url.lock().unwrap() = server_url.clone();
    *handle.all_urls.lock().unwrap() = all_urls.clone();
    *handle.start_time.lock().unwrap() = Some(Instant::now());

    // Bind listener BEFORE spawning so that bind errors propagate to the caller
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| {
            let msg = format!("Failed to bind port {}: {}", config.port, e);
            crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
            msg
        })?;
    log::info!("Screen share HTTP server listening on {}", addr);

    // Build axum shared state
    let password_hash = config.password.as_ref().map(|p| hash_password(p));
    let server_state = Arc::new(HttpServerState {
        broadcast_tx: broadcast_tx.clone(),
        viewer_count: handle.viewer_count.clone(),
        cancel: handle.cancel.clone(),
        password_hash,
        bytes_sent: handle.bytes_sent.clone(),
    });

    // Mark active BEFORE spawning tasks so that status reporter and other
    // components see the active flag immediately and don't exit early.
    handle.active.store(true, Ordering::SeqCst);

    // --- Spawn capture thread ---
    let capture_cancel = handle.cancel.clone();
    let capture_fps = handle.fps_counter.clone();
    let monitor_index = config.monitor_index;
    let quality = config.quality;
    let fps = config.fps;
    let capture_tx = broadcast_tx;
    let capture_app = app_handle.clone();

    if let Err(e) = std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            capture_loop(
                monitor_index,
                quality,
                fps,
                capture_tx,
                capture_cancel,
                capture_fps,
                capture_app,
            );
        })
    {
        // Roll back active state on failure
        handle.active.store(false, Ordering::SeqCst);
        let msg = format!("Failed to spawn capture thread: {}", e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        return Err(msg);
    }

    // --- Spawn HTTP server ---
    let ss_server_active = handle.active.clone();
    let ss_server_app = app_handle.clone();
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

        if ss_server_active.swap(false, Ordering::SeqCst) {
            crate::scanner::emit_tool_log(&ss_server_app, TOOL_NAME, "已停止", "info");
            let _ = ss_server_app.emit(
                "screen-share-log",
                serde_json::json!({ "level": "info", "message": "Screen share stopped" }),
            );
            let _ = ss_server_app.emit(
                "screen-share-status",
                ScreenShareStatus {
                    is_active: false,
                    viewer_count: 0,
                    fps_actual: 0.0,
                    bitrate_kbps: 0,
                    uptime_secs: 0,
                    server_url: String::new(),
                    all_urls: Vec::new(),
                },
            );
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
        )
        .await;
    });

    crate::scanner::emit_tool_log(
        &app_handle,
        TOOL_NAME,
        &format!("已启动，显示器 {} @ {}fps，访问: {}", config.monitor_index + 1, config.fps, server_url),
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

    if !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }

    // Signal stop
    handle.cancel.store(true, Ordering::SeqCst);
    handle.active.store(false, Ordering::SeqCst);

    // Shutdown HTTP server
    if let Some(tx) = handle.shutdown_tx.lock().unwrap().take() {
        let _ = tx.send(());
    }

    // Reset state
    handle.viewer_count.store(0, Ordering::Relaxed);
    *handle.server_url.lock().unwrap() = String::new();
    *handle.all_urls.lock().unwrap() = Vec::new();
    *handle.start_time.lock().unwrap() = None;

    crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, "已停止", "info");

    let _ = app_handle.emit(
        "screen-share-log",
        serde_json::json!({ "level": "info", "message": "Screen share stopped" }),
    );
    let _ = app_handle.emit(
        "screen-share-status",
        ScreenShareStatus {
            is_active: false,
            viewer_count: 0,
            fps_actual: 0.0,
            bitrate_kbps: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
        },
    );

    Ok(())
}

#[tauri::command]
pub fn screen_share_get_status(
    state: State<'_, crate::AppState>,
) -> ScreenShareStatus {
    let handle = &state.screen_share;
    let is_active = handle.active.load(Ordering::Relaxed);

    if !is_active {
        return ScreenShareStatus {
            is_active: false,
            viewer_count: 0,
            fps_actual: 0.0,
            bitrate_kbps: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
        };
    }

    let uptime = handle
        .start_time
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);

    ScreenShareStatus {
        is_active: true,
        viewer_count: handle.viewer_count.load(Ordering::Relaxed),
        fps_actual: handle.fps_counter.load(Ordering::Relaxed) as f32,
        bitrate_kbps: 0,
        uptime_secs: uptime,
        server_url: handle.server_url.lock().unwrap().clone(),
        all_urls: handle.all_urls.lock().unwrap().clone(),
    }
}

// ─── Screen Capture Loop ────────────────────────────────────

fn capture_loop(
    monitor_index: usize,
    quality: u8,
    fps: u8,
    tx: broadcast::Sender<Arc<Bytes>>,
    cancel: Arc<AtomicBool>,
    fps_counter: Arc<AtomicU32>,
    app_handle: AppHandle,
) {
    let mut capturer = match create_capturer(monitor_index) {
        Some(c) => c,
        None => {
            crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, "屏幕捕获初始化失败", "error");
            let _ = app_handle.emit(
                "screen-share-log",
                serde_json::json!({ "level": "error", "message": "Failed to initialize screen capturer" }),
            );
            return;
        }
    };

    let width = capturer.width();
    let height = capturer.height();
    let frame_interval = Duration::from_millis(1000 / fps.max(1) as u64);
    let mut first_real_frame = false;

    log::info!(
        "Capture loop started: {}x{} @ {} FPS, quality {}",
        width,
        height,
        fps,
        quality
    );

    // Send a placeholder frame so that viewers connecting immediately get something
    {
        let placeholder = make_placeholder_jpeg();
        let _ = tx.send(Arc::new(Bytes::from(placeholder)));
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let tick_start = Instant::now();

        match capturer.frame() {
            Ok(frame) => {
                let stride = frame.len() / height;
                let jpeg = encode_jpeg(&frame, width, height, stride, quality);
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
                    std::thread::sleep(Duration::from_millis(500));
                    let placeholder = make_placeholder_jpeg();
                    let _ = tx.send(Arc::new(Bytes::from(placeholder)));
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
                continue;
            }
            Err(e) => {
                log::warn!("Capture error: {}, retrying in 500ms", e);
                std::thread::sleep(Duration::from_millis(500));
                // Try to recreate capturer
                drop(capturer);
                match create_capturer(monitor_index) {
                    Some(c) => capturer = c,
                    None => {
                        log::error!("Failed to recreate capturer, stopping");
                        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, "屏幕捕获器重建失败，已停止", "error");
                        break;
                    }
                }
                continue;
            }
        }

        let elapsed = tick_start.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }

    log::info!("Capture loop ended");
}

fn create_capturer(monitor_index: usize) -> Option<Capturer> {
    let displays = Display::all().ok()?;
    let display = displays.into_iter().nth(monitor_index)?;
    Capturer::new(display)
        .map_err(|e| log::error!("Capturer::new failed: {}", e))
        .ok()
}

// ─── JPEG Encoding ──────────────────────────────────────────

fn encode_jpeg(bgra: &[u8], width: usize, height: usize, stride: usize, quality: u8) -> Vec<u8> {
    if width == 0 || height == 0 || stride < width * 4 || bgra.len() < height * stride {
        return Vec::new();
    }
    // Convert BGRA (with stride padding) to packed RGB
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        let row_start = y * stride;
        let row_end = row_start + width * 4;
        if row_end > bgra.len() {
            break;
        }
        let row = &bgra[row_start..row_end];
        for pixel in row.chunks_exact(4) {
            rgb.push(pixel[2]); // R
            rgb.push(pixel[1]); // G
            rgb.push(pixel[0]); // B
        }
    }

    let mut jpeg_buf = Vec::with_capacity(width * height / 4);
    {
        use image::ImageEncoder;
        let encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, quality);
        if let Err(e) = encoder.write_image(
            &rgb,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgb8,
        ) {
            log::warn!("JPEG encode failed: {}", e);
            return Vec::new();
        }
    }
    jpeg_buf
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

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(async { shutdown_rx.await.ok(); })
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
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            let has_error = q.error.unwrap_or(0) == 1;
            return Html(login_html(has_error)).into_response();
        }
    }
    Html(viewer_html()).into_response()
}

async fn handler_stream(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    headers: HeaderMap,
) -> Response {
    // Auth check
    if let Some(hash) = &state.password_hash {
        if !check_auth_cookie(&headers, hash) {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }

    state.viewer_count.fetch_add(1, Ordering::Relaxed);
    let viewer_count = state.viewer_count.clone();
    let bytes_sent = state.bytes_sent.clone();
    let cancel = state.cancel.clone();
    let mut rx = state.broadcast_tx.subscribe();

    let stream = async_stream::stream! {
        let _guard = ViewerGuard(viewer_count);

        loop {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            match rx.recv().await {
                Ok(frame) => {
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
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Slow viewer: skip to latest frame
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
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
    password: String,
}

async fn handler_auth(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Form(form): Form<AuthForm>,
) -> Response {
    if let Some(expected) = &state.password_hash {
        let submitted = hash_password(&form.password);
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
) -> impl IntoResponse {
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
) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut last_bytes: u64 = 0;

    loop {
        interval.tick().await;

        if !active.load(Ordering::Relaxed) {
            break;
        }

        let fps_count = fps_counter.swap(0, Ordering::Relaxed);
        let current_bytes = bytes_sent.load(Ordering::Relaxed);
        let bytes_delta = current_bytes.saturating_sub(last_bytes);
        last_bytes = current_bytes;

        let status = ScreenShareStatus {
            is_active: true,
            viewer_count: viewer_count.load(Ordering::Relaxed),
            fps_actual: fps_count as f32,
            bitrate_kbps: (bytes_delta * 8 / 1024) as u32,
            uptime_secs: start_time.elapsed().as_secs(),
            server_url: server_url.clone(),
            all_urls: all_urls.clone(),
        };

        let _ = app_handle.emit("screen-share-status", &status);
    }
}

// ─── Utility ────────────────────────────────────────────────

fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
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
        o[0] == 192 && o[1] == 168
            || o[0] == 10
            || o[0] == 172 && (16..=31).contains(&o[1])
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
<style>
*{margin:0;padding:0;box-sizing:border-box}
html,body{height:100%;background:#0f172a;color:#e2e8f0;font-family:system-ui,-apple-system,sans-serif;overflow:hidden}
.wrap{display:flex;flex-direction:column;height:100%}
.view{flex:1;display:flex;align-items:center;justify-content:center;overflow:hidden;padding:4px}
#screen{max-width:100%;max-height:100%;object-fit:contain;border-radius:4px;background:#1e293b}
.bar{height:44px;display:flex;align-items:center;gap:12px;padding:0 16px;background:#1e293b;border-top:1px solid #334155;flex-shrink:0}
.status{display:flex;align-items:center;gap:6px;font-size:13px}
.dot{width:8px;height:8px;border-radius:50%;flex-shrink:0}
.dot-on{background:#22c55e;box-shadow:0 0 6px #22c55e80;animation:pulse 2s infinite}
.dot-off{background:#ef4444}
@keyframes pulse{0%,100%{opacity:1}50%{opacity:.5}}
.spacer{flex:1}
.btn{background:#334155;border:1px solid #475569;color:#e2e8f0;padding:5px 14px;border-radius:6px;cursor:pointer;font-size:13px;transition:background .15s}
.btn:hover{background:#475569}
.viewers{font-size:12px;color:#94a3b8}
</style>
</head>
<body>
<div class="wrap">
  <div class="view">
    <img id="screen" src="/stream" alt="Screen Share">
  </div>
  <div class="bar">
    <div class="status">
      <div id="dot" class="dot dot-on"></div>
      <span id="status-text">Connected</span>
    </div>
    <span class="viewers" id="viewers"></span>
    <div class="spacer"></div>
    <button class="btn" onclick="toggleFs()">&#x26F6; Fullscreen</button>
  </div>
</div>
<script>
const img=document.getElementById('screen'),dot=document.getElementById('dot'),st=document.getElementById('status-text'),vw=document.getElementById('viewers');
let alive=true;
img.onerror=function(){
  alive=false;dot.className='dot dot-off';st.textContent='Disconnected';
  setTimeout(()=>{img.src='/stream?t='+Date.now()},3000);
};
img.onload=function(){
  clearTimeout(initialTimer);
  if(!alive){alive=true;dot.className='dot dot-on';st.textContent='Connected'}
};
// 5s timeout: if stream never delivers a frame, force reconnect
let initialTimer=setTimeout(()=>{
  if(img.naturalWidth===0){img.src='/stream?t='+Date.now()}
},5000);
setInterval(async()=>{
  try{const r=await fetch('/status');if(r.ok){const d=await r.json();vw.textContent=d.viewers>0?d.viewers+' viewer'+(d.viewers>1?'s':''):''}}catch{}
},3000);
function toggleFs(){
  if(!document.fullscreenElement)document.documentElement.requestFullscreen();
  else document.exitFullscreen();
}
</script>
</body>
</html>"#
        .to_string()
}

fn login_html(has_error: bool) -> String {
    let error_block = if has_error {
        r#"<div class="err">Incorrect password</div>"#
    } else {
        ""
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Screen Share</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
html,body{{height:100%;background:#0f172a;color:#e2e8f0;font-family:system-ui,-apple-system,sans-serif}}
.center{{height:100%;display:flex;align-items:center;justify-content:center}}
.card{{background:#1e293b;border-radius:16px;padding:32px;width:360px;border:1px solid #334155}}
h1{{font-size:20px;margin-bottom:6px}}
p{{font-size:14px;color:#94a3b8;margin-bottom:20px}}
input{{width:100%;padding:10px 14px;background:#0f172a;border:1px solid #334155;border-radius:8px;color:#e2e8f0;font-size:15px;outline:none}}
input:focus{{border-color:#3b82f6}}
button{{width:100%;margin-top:14px;padding:10px;background:#3b82f6;color:#fff;border:none;border-radius:8px;font-size:15px;font-weight:600;cursor:pointer}}
button:hover{{background:#2563eb}}
.err{{color:#f87171;font-size:13px;margin-top:10px}}
</style>
</head>
<body>
<div class="center">
  <form class="card" method="POST" action="/auth">
    <h1>Screen Share</h1>
    <p>Enter the access password to view</p>
    <input type="password" name="password" placeholder="Password" autofocus required>
    <button type="submit">Enter</button>
    {error_block}
  </form>
</div>
</body>
</html>"#
    )
}
