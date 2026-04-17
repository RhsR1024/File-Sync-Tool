#![allow(clippy::too_many_arguments)]

use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Query, State as AxumState};
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
    pub username: Option<String>,
    pub password: Option<String>,
    pub monitor_index: usize,
    pub quality: u8,
    pub fps: u8,
    pub show_cursor: bool,
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
    viewer_ips: Arc<Mutex<HashSet<String>>>,
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
            viewer_ips: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

// ─── Internal: HTTP server state ────────────────────────────

struct HttpServerState {
    broadcast_tx: broadcast::Sender<Arc<Bytes>>,
    viewer_count: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    auth_hash: Option<String>,
    auth_username: Option<String>,
    bytes_sent: Arc<AtomicU64>,
    viewer_ips: Arc<Mutex<HashSet<String>>>,
}

/// RAII guard that decrements viewer count and removes IP on drop.
struct ViewerGuard {
    count: Arc<AtomicU32>,
    ips: Arc<Mutex<HashSet<String>>>,
    ip: String,
}

impl Drop for ViewerGuard {
    fn drop(&mut self) {
        loop {
            let current = self.count.load(Ordering::Relaxed);
            if current == 0 {
                break;
            }
            if self
                .count
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
        if let Ok(mut set) = self.ips.lock() {
            set.remove(&self.ip);
        }
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
    let bind_ip = config.bind_address.as_deref().unwrap_or("0.0.0.0");
    let addr = format!("{}:{}", bind_ip, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        let msg = format!("Failed to bind port {}: {}", config.port, e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        msg
    })?;
    log::info!("Screen share HTTP server listening on {}", addr);

    // Build axum shared state
    let auth_hash = config
        .password
        .as_ref()
        .map(|p| hash_credential(config.username.as_deref(), p));
    let auth_username = config.username.clone();
    if let Ok(mut ips) = handle.viewer_ips.lock() {
        ips.clear();
    }
    let server_state = Arc::new(HttpServerState {
        broadcast_tx: broadcast_tx.clone(),
        viewer_count: handle.viewer_count.clone(),
        cancel: handle.cancel.clone(),
        auth_hash,
        auth_username,
        bytes_sent: handle.bytes_sent.clone(),
        viewer_ips: handle.viewer_ips.clone(),
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
    let show_cursor = config.show_cursor;
    let capture_tx = broadcast_tx;
    let capture_app = app_handle.clone();

    if let Err(e) = std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            capture_loop(
                monitor_index,
                quality,
                fps,
                show_cursor,
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
                    connection_count: 0,
                    fps_actual: 0.0,
                    bitrate_kbps: 0,
                    uptime_secs: 0,
                    server_url: String::new(),
                    all_urls: Vec::new(),
                    connected_ips: Vec::new(),
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
    let reporter_ips = handle.viewer_ips.clone();

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
    if let Ok(mut ips) = handle.viewer_ips.lock() {
        ips.clear();
    }

    let _ = app_handle.emit(
        "screen-share-status",
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
        },
    );

    tokio::time::sleep(Duration::from_millis(1200)).await;

    Ok(())
}

#[tauri::command]
pub fn screen_share_get_status(state: State<'_, crate::AppState>) -> ScreenShareStatus {
    let handle = &state.screen_share;
    let is_active = handle.active.load(Ordering::Relaxed);

    if !is_active {
        return ScreenShareStatus {
            is_active: false,
            viewer_count: 0,
            connection_count: 0,
            fps_actual: 0.0,
            bitrate_kbps: 0,
            uptime_secs: 0,
            server_url: String::new(),
            all_urls: Vec::new(),
            connected_ips: Vec::new(),
        };
    }

    let uptime = handle
        .start_time
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);

    let connected_ips = handle
        .viewer_ips
        .lock()
        .map(|s| s.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();

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

fn capture_loop(
    monitor_index: usize,
    quality: u8,
    fps: u8,
    show_cursor: bool,
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
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let tick_start = Instant::now();

        match capturer.frame() {
            Ok(frame) => {
                let stride = frame.len() / height;

                #[cfg(target_os = "windows")]
                let source_pixels: &[u8] = if show_cursor {
                    if let Some(ref mon_rect) = monitor_rect {
                        // Copy frame into persistent scratch buffer (avoids per-frame reallocation)
                        if frame_scratch.len() != frame.len() {
                            frame_scratch.resize(frame.len(), 0);
                        }
                        frame_scratch.copy_from_slice(&frame);
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
                        &frame
                    }
                } else {
                    &frame
                };

                #[cfg(not(target_os = "windows"))]
                let source_pixels: &[u8] = &frame;

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
                    std::thread::sleep(Duration::from_millis(500));
                    let placeholder = make_placeholder_jpeg();
                    let _ = tx.send(Arc::new(Bytes::from(placeholder)));
                } else {
                    // Screen unchanged; sleep briefly (not busy-wait)
                    std::thread::sleep(Duration::from_millis(5));
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
                        crate::scanner::emit_tool_log(
                            &app_handle,
                            TOOL_NAME,
                            "屏幕捕获器重建失败，已停止",
                            "error",
                        );
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
    state.viewer_count.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut ips) = state.viewer_ips.lock() {
        ips.insert(client_ip.clone());
    }
    let viewer_guard = ViewerGuard {
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

async fn handler_status(AxumState(state): AxumState<Arc<HttpServerState>>) -> impl IntoResponse {
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
    viewer_ips: Arc<Mutex<HashSet<String>>>,
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

        let connected_ips = viewer_ips
            .lock()
            .map(|s| s.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();

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
<style>
*{margin:0;padding:0;box-sizing:border-box}
html,body{height:100%;background:#060911;color:#e2e8f0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;overflow:hidden}
.wrap{display:flex;flex-direction:column;height:100%;position:relative}
.view{flex:1;display:flex;align-items:center;justify-content:center;overflow:hidden;background:#060911;position:relative}
#screen{max-width:100%;max-height:100%;object-fit:contain;display:block}
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
st.textContent=T.connected;

// ── Stream connection ──
function connectStream(){
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
  reconnectAttempts++;
  setReconnecting();
  // Exponential backoff: 2s, 3s, 4s, 5s max
  let delay=Math.min(2000+reconnectAttempts*500,5000);
  setTimeout(()=>{
    if(paused)return;
    if(fpsLimitMs>0){startPolling()}else{connectStream()}
  },delay);
}

// Track when the MJPEG stream delivers a new frame
// For MJPEG, onload fires on the first frame only in most browsers.
// We use a combination of onerror + heartbeat to detect stream loss.
img.onerror=function(){
  if(paused)return;
  setDisconnected();
  tryReconnect();
};
img.onload=function(){
  clearTimeout(initialTimer);
  lastFrameTime=Date.now();
  setConnected();
};

// 5s timeout: if stream never delivers a frame, force reconnect
let initialTimer=setTimeout(()=>{
  if(img.naturalWidth===0){connectStream()}
},5000);

// ── Heartbeat: detect stream loss via /status polling ──
// MJPEG streams don't fire onerror when the TCP connection drops mid-stream.
// This heartbeat detects that and triggers reconnection.
let heartbeatFails=0;
setInterval(async()=>{
  if(paused)return;
  try{
    const r=await fetch('/status',{signal:AbortSignal.timeout(3000)});
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
  // If heartbeat fails 2+ times in a row and we think we're alive, reconnect
  if(heartbeatFails>=2&&alive){
    setDisconnected();
    disconnectStream();
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
