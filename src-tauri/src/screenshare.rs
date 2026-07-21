#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::io;
#[cfg(target_os = "windows")]
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::screenshare_input::{
    parse_input_event, source_rect_for_monitor, InputContext, InputEvent, InputWorkerHandle,
    QueuedInput, ScreenRect,
};
use crate::screenshare_media::{
    H264EncoderWorker, H264MediaEvent, H264MediaMetricsSnapshot, H264MediaState,
    H264StreamDescriptor, H264StreamSnapshot,
};
use crate::screenshare_web_assets;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State as AxumState};
use axum::http::{header::USER_AGENT, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{body::Body, Form, Json, Router};
use bytes::{Bytes, BytesMut};
use scrap::{Capturer, Display, Frame};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Size, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tokio::sync::{broadcast, oneshot};
use uuid::Uuid;

#[path = "screenshare_interaction.rs"]
mod screenshare_interaction;
use screenshare_interaction::{
    AnnotationDocument, AnnotationUpdatePayload, ClientEnvelope, ControlRequestInfo, ControlState,
    InteractionClientMetadata, InteractionConfig, InteractionState, NormalizedPoint,
    MAX_CLIENT_ID_BYTES, MAX_WS_MESSAGE_BYTES,
};
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
const PREVIEW_WINDOW_LABEL_PREFIX: &str = "screen-share-preview";
const DESKTOP_OVERLAY_WINDOW_LABEL_PREFIX: &str = "screen-share-desktop-overlay";

const VIEWER_IP_TTL: Duration = Duration::from_secs(12);
/// DXGI DuplicateOutput 偶发瞬时失败，创建时做 3 次短重试；
/// 长退避由捕获循环的暂停-重试机制负责，此处不需要更长的重试梯子。
const DXGI_CREATE_RETRY_DELAYS_MS: [u64; 3] = [0, 200, 400];
const CAPTURE_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const CAPTURE_RETRY_CANCEL_POLL_MS: u64 = 100;
/// 采集器重建的无限重试退避表；到顶后维持 30s 间隔直到会话被取消。
/// 锁屏可能持续数小时——共享必须活着等到解锁自动恢复。
const CAPTURE_RECREATE_BACKOFF_MS: [u64; 6] = [1000, 2000, 4000, 8000, 15000, 30000];
/// graceful shutdown 后允许连接 drain 的最长时间；超时直接丢弃 serve future
/// （连带 listener），确保端口一定释放——半死的 viewer 连接不能扣住端口。
const SERVER_DRAIN_DEADLINE: Duration = Duration::from_secs(3);
/// screen_share_stop 等待服务真正退出的上限（略大于 drain 上限）。
const STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(4);

fn capture_recreate_backoff(attempt: u32) -> Duration {
    let index = (attempt as usize).min(CAPTURE_RECREATE_BACKOFF_MS.len() - 1);
    Duration::from_millis(CAPTURE_RECREATE_BACKOFF_MS[index])
}

/// 帧饥饿看门狗参数。WouldBlock 本身是正常信号（画面无变化），但
/// "锁屏→解锁后帧迟迟不恢复"或"重建后拿不到首帧"说明采集源已静默死亡——
/// WGC 在锁屏/显示器休眠后偶发不再触发 FrameArrived 且不报任何错误，
/// 表现为观看端永久黑屏且刷新无效，必须由看门狗判定后强制重建。
const FRAME_STARVATION_MIN: Duration = Duration::from_secs(2);
/// 饥饿期间桌面锁定状态的采样间隔（OpenInputDesktop，开销可忽略）。
const STARVATION_DESKTOP_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
/// 解锁后帧必须在此宽限期内恢复，否则强制重建采集源。
const STARVATION_POST_UNLOCK_GRACE: Duration = Duration::from_secs(3);
/// (重)建成功后在桌面可用状态下拿首帧的最长等待；健康的 WGC/DXGI
/// 创建后必然立刻交付当前画面帧，超时即为僵尸源。
const STARVATION_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(10);
/// WGC 帧池主动探测：FrameArrived 事件通道静默死亡时，直接调用
/// TryGetNextFrame 能把"设备丢失/池已关闭"暴露为携带精确 HRESULT 的
/// 真实错误（事件死亡本身永远不报错）。饥饿超过 AFTER 后每 INTERVAL 探测一次。
#[cfg(target_os = "windows")]
const WGC_PROBE_AFTER: Duration = Duration::from_secs(2);
#[cfg(target_os = "windows")]
const WGC_PROBE_INTERVAL: Duration = Duration::from_secs(2);
const BLACK_FRAME_RECREATE_AFTER: Duration = Duration::from_millis(1500);
const BLACK_FRAME_RECOVERY_WINDOW: Duration = Duration::from_secs(8);
const BLACK_FRAME_DESKTOP_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const BLACK_FRAME_PRIVACY_RESCAN_DELAY: Duration = Duration::from_secs(10);
const BLACK_FRAME_BRIGHT_THRESHOLD: u8 = 12;
const BLACK_FRAME_MAX_BRIGHT_PIXELS_PER_10K: usize = 35;
const MEDIA_JPEG_SAMPLE_WINDOW: usize = 512;
const MEDIA_STREAM_SAMPLE_WINDOW: usize = 256;
const H264_STREAM_COOPERATIVE_DELAY: Duration = Duration::from_millis(1);
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

    fn alternate(self) -> Self {
        match self {
            Self::Dxgi => Self::Wgc,
            Self::Wgc => Self::Dxgi,
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
    /// Whether viewers may ask the host for mouse control. Disabled by default.
    #[serde(default)]
    pub control_requests_enabled: bool,
    /// Whether the approved controller may send the restricted keyboard whitelist.
    #[serde(default)]
    pub keyboard_control_enabled: bool,
    /// Media transport selector. P0/P1 currently resolve auto to MJPEG.
    #[serde(default)]
    pub transport: ScreenShareMediaTransport,
    #[serde(default = "default_true")]
    pub annotations_enabled: bool,
    #[serde(default = "default_true")]
    pub shared_freeze_enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareMediaTransport {
    Auto,
    Mjpeg,
    MseH264,
    WebRtc,
}

impl Default for ScreenShareMediaTransport {
    fn default() -> Self {
        Self::Auto
    }
}

impl ScreenShareMediaTransport {
    fn wants_h264(self) -> bool {
        matches!(self, Self::Auto | Self::MseH264)
    }

    fn resolved_label(self) -> &'static str {
        match self {
            Self::Auto | Self::Mjpeg => "mjpeg",
            Self::MseH264 => "mse_h264",
            Self::WebRtc => "webrtc",
        }
    }
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
    pub capture_issue: Option<ScreenShareCaptureIssue>,
    pub interaction_connected_count: u32,
    pub annotation_count: u32,
    pub view_mode: screenshare_interaction::ViewMode,
    pub source_epoch: u64,
    pub latest_frame_id: Option<u64>,
    pub frame_width: Option<u32>,
    pub frame_height: Option<u32>,
    pub transport: ScreenShareMediaTransport,
    pub h264_media: H264MediaMetricsSnapshot,
    pub control_state: ControlState,
    pub controller_ip: Option<String>,
    pub pending_control_request: Option<screenshare_interaction::ControlRequestInfo>,
    pub desktop_overlay_active: bool,
    pub media_metrics: ScreenShareMediaMetricsSnapshot,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ScreenShareMediaMetricsSnapshot {
    pub encoded_frame_count: u64,
    pub jpeg_sample_count: u32,
    pub jpeg_size_avg_bytes: u64,
    pub jpeg_size_p50_bytes: u64,
    pub jpeg_size_p95_bytes: u64,
    pub first_frame_delay_ms: Option<u64>,
    pub frame_age_ms: Option<u64>,
    pub slow_client_dropped_frames: u64,
    pub stream_connection_count: u64,
    pub stream_first_frame_sample_count: u32,
    pub stream_first_frame_avg_ms: Option<u64>,
    pub stream_first_frame_p95_ms: Option<u64>,
    pub stream_reconnect_count: u64,
    pub stream_reconnect_sample_count: u32,
    pub stream_reconnect_avg_ms: Option<u64>,
    pub stream_reconnect_p95_ms: Option<u64>,
    pub fps_actual: f32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Default)]
struct ScreenShareMediaSamples {
    jpeg_sizes: VecDeque<u32>,
    first_frame_ms: VecDeque<u64>,
    reconnect_ms: VecDeque<u64>,
}

#[derive(Debug)]
struct ScreenShareMediaMetrics {
    started_at: Instant,
    encoded_frame_count: AtomicU64,
    first_frame_delay_ms: AtomicU64,
    slow_client_dropped_frames: AtomicU64,
    stream_connection_count: AtomicU64,
    stream_reconnect_count: AtomicU64,
    fps_actual: AtomicU32,
    bitrate_kbps: AtomicU32,
    samples: Mutex<ScreenShareMediaSamples>,
}

impl ScreenShareMediaMetrics {
    fn new() -> Self {
        Self::new_at(Instant::now())
    }

    fn new_at(started_at: Instant) -> Self {
        Self {
            started_at,
            encoded_frame_count: AtomicU64::new(0),
            first_frame_delay_ms: AtomicU64::new(u64::MAX),
            slow_client_dropped_frames: AtomicU64::new(0),
            stream_connection_count: AtomicU64::new(0),
            stream_reconnect_count: AtomicU64::new(0),
            fps_actual: AtomicU32::new(0),
            bitrate_kbps: AtomicU32::new(0),
            samples: Mutex::new(ScreenShareMediaSamples::default()),
        }
    }

    fn record_encoded_frame(&self, jpeg_size: usize) {
        self.record_encoded_frame_at(jpeg_size, Instant::now());
    }

    fn record_encoded_frame_at(&self, jpeg_size: usize, captured_at: Instant) {
        self.encoded_frame_count.fetch_add(1, Ordering::Relaxed);
        let delay_ms = captured_at
            .checked_duration_since(self.started_at)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        let _ = self.first_frame_delay_ms.compare_exchange(
            u64::MAX,
            delay_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        if let Ok(mut samples) = self.samples.lock() {
            push_bounded(
                &mut samples.jpeg_sizes,
                jpeg_size.min(u32::MAX as usize) as u32,
                MEDIA_JPEG_SAMPLE_WINDOW,
            );
        }
    }

    fn record_stream_open(&self, reconnect: bool) {
        self.stream_connection_count.fetch_add(1, Ordering::Relaxed);
        if reconnect {
            self.stream_reconnect_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_stream_first_frame(&self, elapsed: Duration, reconnect: bool) {
        let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        if let Ok(mut samples) = self.samples.lock() {
            let target = if reconnect {
                &mut samples.reconnect_ms
            } else {
                &mut samples.first_frame_ms
            };
            push_bounded(target, elapsed_ms, MEDIA_STREAM_SAMPLE_WINDOW);
        }
    }

    fn record_lagged_frames(&self, skipped: u64) {
        self.slow_client_dropped_frames
            .fetch_add(skipped, Ordering::Relaxed);
    }

    fn update_rates(&self, fps_actual: u32, bitrate_kbps: u32) {
        self.fps_actual.store(fps_actual, Ordering::Relaxed);
        self.bitrate_kbps.store(bitrate_kbps, Ordering::Relaxed);
    }

    fn snapshot(
        &self,
        latest_frame_captured_at_ms: Option<u64>,
    ) -> ScreenShareMediaMetricsSnapshot {
        self.snapshot_at(latest_frame_captured_at_ms, unix_time_ms())
    }

    fn snapshot_at(
        &self,
        latest_frame_captured_at_ms: Option<u64>,
        now_ms: u64,
    ) -> ScreenShareMediaMetricsSnapshot {
        let (jpeg, first_frame, reconnect) = self
            .samples
            .lock()
            .map(|samples| {
                (
                    summarize_samples(&samples.jpeg_sizes),
                    summarize_samples(&samples.first_frame_ms),
                    summarize_samples(&samples.reconnect_ms),
                )
            })
            .unwrap_or_default();
        let first_frame_delay_ms = match self.first_frame_delay_ms.load(Ordering::Relaxed) {
            u64::MAX => None,
            value => Some(value),
        };
        ScreenShareMediaMetricsSnapshot {
            encoded_frame_count: self.encoded_frame_count.load(Ordering::Relaxed),
            jpeg_sample_count: jpeg.count,
            jpeg_size_avg_bytes: jpeg.average,
            jpeg_size_p50_bytes: jpeg.p50,
            jpeg_size_p95_bytes: jpeg.p95,
            first_frame_delay_ms,
            frame_age_ms: latest_frame_captured_at_ms
                .map(|captured_at| now_ms.saturating_sub(captured_at)),
            slow_client_dropped_frames: self.slow_client_dropped_frames.load(Ordering::Relaxed),
            stream_connection_count: self.stream_connection_count.load(Ordering::Relaxed),
            stream_first_frame_sample_count: first_frame.count,
            stream_first_frame_avg_ms: first_frame.optional_average(),
            stream_first_frame_p95_ms: first_frame.optional_p95(),
            stream_reconnect_count: self.stream_reconnect_count.load(Ordering::Relaxed),
            stream_reconnect_sample_count: reconnect.count,
            stream_reconnect_avg_ms: reconnect.optional_average(),
            stream_reconnect_p95_ms: reconnect.optional_p95(),
            fps_actual: self.fps_actual.load(Ordering::Relaxed) as f32,
            bitrate_kbps: self.bitrate_kbps.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default)]
struct SampleSummary {
    count: u32,
    average: u64,
    p50: u64,
    p95: u64,
}

impl SampleSummary {
    fn optional_average(&self) -> Option<u64> {
        (self.count > 0).then_some(self.average)
    }

    fn optional_p95(&self) -> Option<u64> {
        (self.count > 0).then_some(self.p95)
    }
}

fn push_bounded<T>(samples: &mut VecDeque<T>, value: T, limit: usize) {
    if samples.len() == limit {
        samples.pop_front();
    }
    samples.push_back(value);
}

fn summarize_samples<T>(samples: &VecDeque<T>) -> SampleSummary
where
    T: Copy + Into<u64>,
{
    if samples.is_empty() {
        return SampleSummary::default();
    }
    let mut sorted = samples.iter().copied().map(Into::into).collect::<Vec<_>>();
    sorted.sort_unstable();
    let sum = sorted.iter().copied().map(u128::from).sum::<u128>();
    let count = sorted.len();
    let average = ((sum + (count as u128 / 2)) / count as u128).min(u128::from(u64::MAX)) as u64;
    SampleSummary {
        count: count.min(u32::MAX as usize) as u32,
        average,
        p50: nearest_rank(&sorted, 50),
        p95: nearest_rank(&sorted, 95),
    }
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    let rank = (sorted.len() * percentile).div_ceil(100).max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareCaptureIssue {
    Retrying,
    PrivacyModeOrDisplayOff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScreenShareAccessUrls {
    server_url: String,
    all_urls: Vec<String>,
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
    capture_issue: Arc<Mutex<Option<ScreenShareCaptureIssue>>>,
    interaction: Mutex<Option<Arc<InteractionState>>>,
    transport: Arc<Mutex<ScreenShareMediaTransport>>,
    h264_media: Mutex<Option<Arc<H264MediaState>>>,
    input_worker: Mutex<Option<Arc<InputWorkerHandle>>>,
    active_monitor_index: Arc<AtomicUsize>,
    preview_token: Arc<Mutex<Option<String>>>,
    desktop_overlay_active: Arc<AtomicBool>,
    media_metrics: Mutex<Option<Arc<ScreenShareMediaMetrics>>>,
    /// Completed by the server watcher once the HTTP server has fully exited
    /// (port released); screen_share_stop awaits it so "stop returned" means
    /// "the port is immediately reusable".
    server_done_rx: Mutex<Option<oneshot::Receiver<()>>>,
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
            capture_issue: Arc::new(Mutex::new(None)),
            interaction: Mutex::new(None),
            transport: Arc::new(Mutex::new(ScreenShareMediaTransport::Mjpeg)),
            h264_media: Mutex::new(None),
            input_worker: Mutex::new(None),
            active_monitor_index: Arc::new(AtomicUsize::new(0)),
            preview_token: Arc::new(Mutex::new(None)),
            desktop_overlay_active: Arc::new(AtomicBool::new(false)),
            media_metrics: Mutex::new(None),
            server_done_rx: Mutex::new(None),
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
        capture_issue: None,
        interaction_connected_count: 0,
        annotation_count: 0,
        view_mode: screenshare_interaction::ViewMode::Live,
        source_epoch: 0,
        latest_frame_id: None,
        frame_width: None,
        frame_height: None,
        transport: ScreenShareMediaTransport::Mjpeg,
        h264_media: H264MediaMetricsSnapshot::default(),
        control_state: ControlState::Disabled,
        controller_ip: None,
        pending_control_request: None,
        desktop_overlay_active: false,
        media_metrics: ScreenShareMediaMetricsSnapshot::default(),
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
    *handle.capture_issue.lock().unwrap() = None;
    *handle.interaction.lock().unwrap() = None;
    *handle.media_metrics.lock().unwrap() = None;
    *handle.h264_media.lock().unwrap() = None;
    *handle.transport.lock().unwrap() = ScreenShareMediaTransport::Mjpeg;
    if let Ok(mut worker) = handle.input_worker.lock() {
        if let Some(worker) = worker.take() {
            worker.shutdown();
        }
    }
    *handle.preview_token.lock().unwrap() = None;
    handle.desktop_overlay_active.store(false, Ordering::SeqCst);
    *handle.server_done_rx.lock().unwrap() = None;
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

fn set_capture_issue(handle: &ScreenShareHandle, issue: Option<ScreenShareCaptureIssue>) {
    handle
        .capture_paused
        .store(issue.is_some(), Ordering::SeqCst);
    *handle.capture_issue.lock().unwrap() = issue;
    if issue.is_some() {
        if let Some(interaction) = handle.interaction.lock().unwrap().as_ref() {
            interaction.revoke_control("capture_paused");
        }
        if let Some(worker) = handle.input_worker.lock().unwrap().as_ref() {
            worker.revoke();
        }
    }
}

fn current_capture_issue(handle: &ScreenShareHandle) -> Option<ScreenShareCaptureIssue> {
    *handle.capture_issue.lock().unwrap()
}

fn invalidate_interaction_source(handle: &ScreenShareHandle, interaction: &InteractionState) {
    interaction.bump_source_epoch();
    if let Some(worker) = handle.input_worker.lock().unwrap().as_ref() {
        worker.revoke();
    }
}

fn interaction_event_updates_annotations(message_type: &str) -> bool {
    matches!(
        message_type,
        "annotation.applied" | "view.state" | "source.changed"
    )
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
    events: Arc<dyn ScreenShareEventSink>,
    broadcast_tx: broadcast::Sender<Arc<Bytes>>,
    interaction: Arc<InteractionState>,
    viewer_count: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    auth_hash: Option<String>,
    auth_username: Option<String>,
    bytes_sent: Arc<AtomicU64>,
    media_metrics: Arc<ScreenShareMediaMetrics>,
    h264_media: Arc<H264MediaState>,
    viewer_ips: Arc<Mutex<ViewerIpMap>>,
    /// Session epoch: viewers use it to detect a server-side restart and
    /// reconnect their stream without a manual page refresh.
    session_id: u64,
    capture_paused: Arc<AtomicBool>,
    capture_issue: Arc<Mutex<Option<ScreenShareCaptureIssue>>>,
    preview_token: Arc<Mutex<Option<String>>>,
    transport: Arc<Mutex<ScreenShareMediaTransport>>,
    input_worker: Option<Arc<InputWorkerHandle>>,
}

trait ScreenShareEventSink: Send + Sync {
    fn emit_tool_log(&self, message: &str, level: &str);
    fn emit_control_request(&self, request: ControlRequestInfo);
}

struct TauriScreenShareEventSink {
    app_handle: AppHandle,
}

impl ScreenShareEventSink for TauriScreenShareEventSink {
    fn emit_tool_log(&self, message: &str, level: &str) {
        let app_handle = self.app_handle.clone();
        let message = message.to_string();
        let level = level.to_string();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &message, &level);
        });
    }

    fn emit_control_request(&self, request: ControlRequestInfo) {
        crate::show_main_window(&self.app_handle, "screen-share-control-request");
        let _ = self
            .app_handle
            .emit("screen-share-control-request", request);
    }
}

/// RAII guard that decrements viewer count and removes IP on drop.
struct ViewerGuard {
    events: Arc<dyn ScreenShareEventSink>,
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
        self.events.emit_tool_log(
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
    handle
        .active_monitor_index
        .store(config.monitor_index, Ordering::SeqCst);

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

    // Bind listener BEFORE spawning so that bind errors propagate to the caller
    let bind_ip = config.bind_address.as_deref().unwrap_or("0.0.0.0");
    let addr = format!("{}:{}", bind_ip, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
        let msg = format!("Failed to bind port {}: {}", config.port, e);
        crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, &msg, "error");
        msg
    })?;
    log::info!("Screen share HTTP server listening on {}", addr);

    // Get local IPs (all non-loopback IPv4). When bound to a specific address,
    // only that address is reachable, so only publish that single URL.
    let all_ips = get_lan_ips();
    let access_urls =
        build_screen_share_access_urls(&all_ips, config.bind_address.as_deref(), config.port);
    let server_url = access_urls.server_url;
    let all_urls = access_urls.all_urls;

    // Broadcast channel for JPEG frames. Interaction state is per session and
    // is deliberately kept separate from the lossy MJPEG channel.
    let (broadcast_tx, _) = broadcast::channel::<Arc<Bytes>>(8);
    let media_metrics = Arc::new(ScreenShareMediaMetrics::new());
    let h264_media = Arc::new(H264MediaState::new());
    *handle.media_metrics.lock().unwrap() = Some(media_metrics.clone());
    *handle.h264_media.lock().unwrap() = Some(h264_media.clone());
    let effective_transport = ScreenShareMediaTransport::Mjpeg;
    let interaction = InteractionState::new_with_config(
        session_id,
        InteractionConfig {
            annotations_enabled: config.annotations_enabled,
            shared_freeze_enabled: config.shared_freeze_enabled,
            control_requests_enabled: config.control_requests_enabled,
            keyboard_control_enabled: config.control_requests_enabled
                && config.keyboard_control_enabled,
        },
    );
    *handle.interaction.lock().unwrap() = Some(interaction.clone());
    *handle.transport.lock().unwrap() = effective_transport;
    let h264_worker = if config.transport.wants_h264() {
        match H264EncoderWorker::spawn(h264_media.clone(), config.fps, config.quality) {
            Ok(worker) => Some(worker),
            Err(error) => {
                crate::scanner::emit_tool_log(
                    &app_handle,
                    TOOL_NAME,
                    &format!("H.264 编码器启动失败，继续使用 MJPEG: {error}"),
                    "warn",
                );
                None
            }
        }
    } else {
        None
    };
    if config.control_requests_enabled {
        let worker = InputWorkerHandle::spawn().map_err(|error| {
            reset_runtime_state(handle);
            error
        })?;
        *handle.input_worker.lock().unwrap() = Some(worker);
    }

    let auth_hash = config
        .password
        .as_ref()
        .map(|p| hash_credential(config.username.as_deref(), p));
    let auth_username = config.username.clone();

    // --- Spawn capture thread ---
    let capture_cancel = session_cancel.clone();
    let capture_fps = handle.fps_counter.clone();
    let capture_media_metrics = media_metrics.clone();
    let capture_viewers = handle.viewer_count.clone();
    let capture_handle = handle.clone();
    let monitor_index = config.monitor_index;
    let quality = config.quality;
    let fps = config.fps;
    let show_cursor = config.show_cursor;
    let backend_mode = config.capture_backend_mode;
    let capture_tx = broadcast_tx.clone();
    let capture_interaction = interaction.clone();
    let capture_h264_media = h264_media.clone();
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
                h264_worker,
                capture_h264_media,
                capture_tx,
                capture_interaction,
                capture_cancel,
                capture_fps,
                capture_media_metrics,
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

    // Viewer annotations are part of the shared session, so make the host
    // overlay available by default. The host can still close it explicitly
    // from the screen-share page for the remainder of this session.
    if config.annotations_enabled {
        schedule_desktop_overlay_window(app_handle.clone(), handle.clone(), session_id);
    }

    let mut annotation_events = interaction.subscribe();
    let annotation_app = app_handle.clone();
    let annotation_interaction = interaction.clone();
    let annotation_cancel = session_cancel.clone();
    tokio::spawn(async move {
        let _ = annotation_app.emit(
            "screen-share-annotation-state",
            annotation_interaction.snapshot(),
        );
        let mut cancel_tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                result = annotation_events.recv() => {
                    match result {
                        Ok(event) if interaction_event_updates_annotations(&event.message_type) => {
                            let _ = annotation_app.emit(
                                "screen-share-annotation-state",
                                annotation_interaction.snapshot(),
                            );
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let _ = annotation_app.emit(
                                "screen-share-annotation-state",
                                annotation_interaction.snapshot(),
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = cancel_tick.tick() => {
                    if annotation_cancel.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }
    });

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    *handle.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

    let server_state = Arc::new(HttpServerState {
        events: Arc::new(TauriScreenShareEventSink {
            app_handle: app_handle.clone(),
        }),
        broadcast_tx: broadcast_tx.clone(),
        interaction: interaction.clone(),
        viewer_count: handle.viewer_count.clone(),
        cancel: session_cancel.clone(),
        auth_hash,
        auth_username,
        bytes_sent: handle.bytes_sent.clone(),
        media_metrics: media_metrics.clone(),
        h264_media: h264_media.clone(),
        viewer_ips: handle.viewer_ips.clone(),
        session_id,
        capture_paused: handle.capture_paused.clone(),
        capture_issue: handle.capture_issue.clone(),
        preview_token: handle.preview_token.clone(),
        transport: handle.transport.clone(),
        input_worker: handle.input_worker.lock().unwrap().clone(),
    });

    // --- Spawn HTTP server ---
    let ss_server_active = handle.active.clone();
    let ss_server_app = app_handle.clone();
    let ss_runtime_handle = handle.clone();
    let ss_session_id = session_id;
    let server_join = tokio::spawn(async move {
        run_http_server(listener, server_state, shutdown_rx).await;
    });

    let (server_done_tx, server_done_rx) = oneshot::channel::<()>();
    *handle.server_done_rx.lock().unwrap() = Some(server_done_rx);

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
            close_preview_window(&ss_server_app, ss_session_id);
            close_desktop_overlay_window(&ss_server_app, &ss_runtime_handle, ss_session_id);
            reset_runtime_state(&ss_runtime_handle);
            crate::scanner::emit_tool_log(&ss_server_app, TOOL_NAME, "已停止", "info");
            let _ = ss_server_app.emit(
                "screen-share-log",
                serde_json::json!({ "level": "info", "message": "Screen share stopped" }),
            );
            emit_inactive_status(&ss_server_app);
        }

        let _ = server_done_tx.send(());
    });

    // --- Spawn status reporter ---
    let reporter_app = app_handle.clone();
    let reporter_active = handle.active.clone();
    let reporter_viewers = handle.viewer_count.clone();
    let reporter_fps = handle.fps_counter.clone();
    let reporter_bytes = handle.bytes_sent.clone();
    let reporter_media_metrics = media_metrics;
    let reporter_url = server_url.clone();
    let reporter_all_urls = all_urls.clone();
    let reporter_start = Instant::now();
    let reporter_ips = handle.viewer_ips.clone();
    let reporter_capture_paused = handle.capture_paused.clone();
    let reporter_capture_issue = handle.capture_issue.clone();
    let reporter_interaction = interaction.clone();
    let reporter_transport = handle.transport.clone();
    let reporter_input_worker = handle.input_worker.lock().unwrap().clone();
    let reporter_runtime_handle = handle.clone();
    let reporter_session_id = session_id;

    tokio::spawn(async move {
        status_reporter(
            reporter_app,
            reporter_active,
            reporter_viewers,
            reporter_fps,
            reporter_bytes,
            reporter_media_metrics,
            reporter_url,
            reporter_all_urls,
            reporter_start,
            reporter_ips,
            reporter_capture_paused,
            reporter_capture_issue,
            reporter_interaction,
            reporter_transport,
            reporter_input_worker,
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

    // Take the receiver BEFORE reset_runtime_state clears it.
    let done_rx = handle.server_done_rx.lock().unwrap().take();

    let session_id = handle.session_id.load(Ordering::SeqCst);
    close_preview_window(&app_handle, session_id);
    close_desktop_overlay_window(&app_handle, handle, session_id);
    reset_runtime_state(handle);

    crate::scanner::emit_tool_log(&app_handle, TOOL_NAME, "已停止", "info");

    let _ = app_handle.emit(
        "screen-share-log",
        serde_json::json!({ "level": "info", "message": "Screen share stopped" }),
    );
    emit_inactive_status(&app_handle);

    // 等待 HTTP 服务真正退出（graceful 或 drain 超时强制关闭），
    // 返回后端口保证已释放，前端可立即用同端口重启。
    if let Some(done_rx) = done_rx {
        let _ = tokio::time::timeout(STOP_WAIT_TIMEOUT, done_rx).await;
    }

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
    let interaction = handle.interaction.lock().unwrap().clone();
    let interaction_snapshot = interaction.as_ref().map(|interaction| {
        (
            interaction.client_count() as u32,
            interaction.snapshot(),
            interaction.control_snapshot(),
            interaction.latest_frame_info(),
        )
    });
    let latest_frame_captured_at_ms = interaction_snapshot
        .as_ref()
        .and_then(|(_, _, _, frame)| frame.as_ref().map(|frame| frame.captured_at_ms));
    let media_metrics = handle
        .media_metrics
        .lock()
        .unwrap()
        .as_ref()
        .map(|metrics| metrics.snapshot(latest_frame_captured_at_ms))
        .unwrap_or_default();
    let h264_media = handle
        .h264_media
        .lock()
        .unwrap()
        .as_ref()
        .map(|media| media.metrics())
        .unwrap_or_default();

    ScreenShareStatus {
        is_active: true,
        viewer_count: handle.viewer_count.load(Ordering::Relaxed),
        connection_count: connected_ips.len() as u32,
        fps_actual: media_metrics.fps_actual,
        bitrate_kbps: media_metrics.bitrate_kbps,
        uptime_secs: uptime,
        server_url: handle.server_url.lock().unwrap().clone(),
        all_urls: handle.all_urls.lock().unwrap().clone(),
        connected_ips,
        capture_paused: handle.capture_paused.load(Ordering::Relaxed),
        capture_issue: current_capture_issue(handle),
        interaction_connected_count: interaction_snapshot
            .as_ref()
            .map(|(count, _, _, _)| *count)
            .unwrap_or(0),
        annotation_count: interaction_snapshot
            .as_ref()
            .map(|(_, document, _, _)| document.shapes.len() as u32)
            .unwrap_or(0),
        view_mode: interaction_snapshot
            .as_ref()
            .map(|(_, document, _, _)| document.mode)
            .unwrap_or(screenshare_interaction::ViewMode::Live),
        source_epoch: interaction_snapshot
            .as_ref()
            .map(|(_, document, _, _)| document.source_epoch)
            .unwrap_or(0),
        latest_frame_id: interaction_snapshot
            .as_ref()
            .and_then(|(_, _, _, frame)| frame.as_ref().map(|frame| frame.frame_id)),
        frame_width: interaction_snapshot
            .as_ref()
            .and_then(|(_, _, _, frame)| frame.as_ref().map(|frame| frame.width)),
        frame_height: interaction_snapshot
            .as_ref()
            .and_then(|(_, _, _, frame)| frame.as_ref().map(|frame| frame.height)),
        transport: *handle.transport.lock().unwrap(),
        h264_media,
        control_state: interaction_snapshot
            .as_ref()
            .map(|(_, _, control, _)| control.state)
            .unwrap_or(ControlState::Disabled),
        controller_ip: interaction_snapshot
            .as_ref()
            .and_then(|(_, _, control, _)| control.controller_ip.clone()),
        pending_control_request: interaction
            .as_ref()
            .and_then(|interaction| interaction.pending_control_request()),
        desktop_overlay_active: handle.desktop_overlay_active.load(Ordering::Relaxed),
        media_metrics,
    }
}

/// Clear every annotation in the current session. The host is the only UI
/// that gets this command; viewers can only undo or clear their own marks.
#[tauri::command]
pub fn screen_share_clear_annotations(state: State<'_, crate::AppState>) -> Result<(), String> {
    let handle = &state.screen_share;
    let interaction = handle
        .interaction
        .lock()
        .map_err(|_| "Screen share interaction state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "Screen share is not active".to_string())?;
    interaction.clear_all();
    Ok(())
}

#[tauri::command]
pub fn screen_share_remove_annotation(
    state: State<'_, crate::AppState>,
    shape_id: String,
) -> Result<(), String> {
    let interaction = state
        .screen_share
        .interaction
        .lock()
        .map_err(|_| "Screen share interaction state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "Screen share is not active".to_string())?;
    interaction
        .remove_annotation(&shape_id)
        .map_err(|error| error.message)
}

#[tauri::command]
pub fn screen_share_update_annotation(
    state: State<'_, crate::AppState>,
    shape_id: String,
    points: Vec<NormalizedPoint>,
    color: String,
    width: f32,
) -> Result<(), String> {
    let interaction = state
        .screen_share
        .interaction
        .lock()
        .map_err(|_| "Screen share interaction state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "Screen share is not active".to_string())?;
    interaction
        .update_annotation(AnnotationUpdatePayload {
            shape_id,
            points,
            color,
            width,
        })
        .map_err(|error| error.message)
}

#[tauri::command]
pub fn screen_share_get_annotation_state(
    state: State<'_, crate::AppState>,
) -> Result<AnnotationDocument, String> {
    state
        .screen_share
        .interaction
        .lock()
        .map_err(|_| "Screen share interaction state is unavailable".to_string())?
        .as_ref()
        .map(|interaction| interaction.snapshot())
        .ok_or_else(|| "Screen share is not active".to_string())
}

#[tauri::command]
pub fn screen_share_respond_control_request(
    state: State<'_, crate::AppState>,
    request_id: String,
    allow: bool,
) -> Result<(), String> {
    let handle = &state.screen_share;
    let interaction = handle
        .interaction
        .lock()
        .map_err(|_| "Screen share interaction state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "Screen share is not active".to_string())?;
    let pending = interaction
        .pending_control_request()
        .filter(|request| request.request_id == request_id)
        .ok_or_else(|| "Control request is no longer pending".to_string())?;

    let grant = if allow {
        let (session_id, source_epoch, _) = interaction.identity();
        let frame = interaction
            .latest_frame_info()
            .ok_or_else(|| "Screen frame is not ready".to_string())?;
        if frame.source_epoch != source_epoch {
            return Err("Screen source changed; request control again".to_string());
        }
        let monitor_index = handle.active_monitor_index.load(Ordering::Relaxed);
        let source = source_rect_for_monitor(monitor_index, frame.width, frame.height)
            .ok_or_else(|| "Active monitor is unavailable".to_string())?;
        let worker = handle
            .input_worker
            .lock()
            .map_err(|_| "Remote input worker is unavailable".to_string())?
            .clone()
            .ok_or_else(|| "Remote input is not enabled for this session".to_string())?;
        Some((
            worker,
            InputContext::new(pending.client_id.clone(), session_id, source_epoch),
            source,
        ))
    } else {
        None
    };

    // Make the input path ready before publishing Granted. Otherwise the
    // viewer can race the broadcast with its first pointer event and have the
    // inactive worker mistaken for a saturated input queue.
    if let Some((worker, context, source)) = grant.as_ref() {
        if let Err(error) = worker.grant(context.clone(), *source) {
            worker.revoke();
            return Err(error);
        }
    } else if let Some(worker) = handle.input_worker.lock().unwrap().as_ref() {
        worker.revoke();
    }

    if let Err(error) = interaction.respond_control_request(&request_id, allow) {
        if let Some((worker, _, _)) = grant.as_ref() {
            worker.revoke();
        }
        return Err(error.message);
    }

    if let Some((worker, context, _)) = grant {
        if !interaction.is_controller(&context.client_id, context.session_id, context.source_epoch)
        {
            worker.revoke();
            return Err("Screen source changed; request control again".to_string());
        }
    }
    Ok(())
}

#[tauri::command]
pub fn screen_share_revoke_control(state: State<'_, crate::AppState>) -> Result<(), String> {
    let handle = &state.screen_share;
    let interaction = handle
        .interaction
        .lock()
        .map_err(|_| "Screen share interaction state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "Screen share is not active".to_string())?;
    interaction.revoke_control("host_revoked");
    if let Some(worker) = handle.input_worker.lock().unwrap().as_ref() {
        worker.revoke();
    }
    Ok(())
}

/// Create a short-lived local preview capability. The returned URL contains
/// only this random capability, never the configured sharing password. The
/// browser exchanges it for an HttpOnly cookie on the first page load.
#[tauri::command]
pub fn screen_share_open_local_preview(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let handle = &state.screen_share;
    if !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }
    let base = handle.server_url.lock().unwrap().clone();
    if base.is_empty() {
        return Err("Screen share URL is not ready".into());
    }
    let session_id = handle.session_id.load(Ordering::SeqCst);
    let window_label = preview_window_label(session_id);
    if let Some(window) = app_handle.get_webview_window(&window_label) {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }
    let token = Uuid::new_v4().simple().to_string();
    *handle.preview_token.lock().unwrap() = Some(token.clone());
    let preview_url = format!("{base}/?host_preview={token}")
        .parse()
        .map_err(|error| format!("Invalid local preview URL: {error}"))?;
    let preview_handle = state.screen_share.clone();
    let window = WebviewWindowBuilder::new(
        &app_handle,
        &window_label,
        WebviewUrl::External(preview_url),
    )
    .title("Screen Share Preview")
    .inner_size(1280.0, 800.0)
    .min_inner_size(640.0, 400.0)
    .resizable(true)
    .build()
    .map_err(|error| {
        *handle.preview_token.lock().unwrap() = None;
        format!("Failed to open local preview: {error}")
    })?;
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && preview_handle.session_id.load(Ordering::SeqCst) == session_id
        {
            *preview_handle.preview_token.lock().unwrap() = None;
        }
    });
    Ok(())
}

#[tauri::command]
pub fn screen_share_close_local_preview(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let session_id = state.screen_share.session_id.load(Ordering::SeqCst);
    close_preview_window(&app_handle, session_id);
    *state.screen_share.preview_token.lock().unwrap() = None;
    Ok(())
}

fn preview_window_label(session_id: u64) -> String {
    format!("{PREVIEW_WINDOW_LABEL_PREFIX}-{session_id}")
}

fn close_preview_window(app_handle: &AppHandle, session_id: u64) {
    if let Some(window) = app_handle.get_webview_window(&preview_window_label(session_id)) {
        if let Err(error) = window.close() {
            log::warn!("Failed to close screen share preview: {error}");
        }
    }
}

#[tauri::command]
pub fn screen_share_open_desktop_overlay(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let handle = state.screen_share.clone();
    if !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }

    let session_id = handle.session_id.load(Ordering::SeqCst);
    schedule_desktop_overlay_window(app_handle, handle, session_id);
    Ok(())
}

fn schedule_desktop_overlay_window(
    app_handle: AppHandle,
    handle: Arc<ScreenShareHandle>,
    session_id: u64,
) {
    tauri::async_runtime::spawn(async move {
        tokio::task::yield_now().await;
        let build_app = app_handle.clone();
        let error_app = app_handle.clone();
        if let Err(error) = app_handle.run_on_main_thread(move || {
            if let Err(error) = ensure_desktop_overlay_window(&build_app, handle, session_id) {
                emit_desktop_overlay_error(&error_app, &error);
            }
        }) {
            emit_desktop_overlay_error(
                &app_handle,
                &format!("Failed to schedule desktop annotation overlay: {error}"),
            );
        }
    });
}

fn emit_desktop_overlay_error(app_handle: &AppHandle, error: &str) {
    log::error!("Desktop annotation overlay failed: {error}");
    crate::scanner::emit_tool_log(
        app_handle,
        TOOL_NAME,
        &format!("Desktop annotation overlay failed: {error}"),
        "error",
    );
    let _ = app_handle.emit("screen-share-desktop-overlay-error", error.to_string());
}

fn ensure_desktop_overlay_window(
    app_handle: &AppHandle,
    handle: Arc<ScreenShareHandle>,
    session_id: u64,
) -> Result<(), String> {
    if !handle.active.load(Ordering::SeqCst)
        || handle.session_id.load(Ordering::SeqCst) != session_id
    {
        return Err("Screen share is not active".into());
    }

    let window_label = desktop_overlay_window_label(session_id);
    if let Some(window) = app_handle.get_webview_window(&window_label) {
        if handle.desktop_overlay_active.load(Ordering::SeqCst) {
            window.show().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    let overlay_handle = handle.clone();
    let window = WebviewWindowBuilder::new(
        app_handle,
        &window_label,
        WebviewUrl::App("index.html#/screen-share-overlay".into()),
    )
    .title("Screen Share Annotations")
    .inner_size(1.0, 1.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .focusable(false)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Failed to create desktop annotation overlay: {error}"))?;

    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && overlay_handle.session_id.load(Ordering::SeqCst) == session_id
        {
            overlay_handle
                .desktop_overlay_active
                .store(false, Ordering::SeqCst);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn screen_share_desktop_overlay_ready(
    app_handle: AppHandle,
    window: WebviewWindow,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let handle = &state.screen_share;
    if !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }
    let session_id = handle.session_id.load(Ordering::SeqCst);
    if window.label() != desktop_overlay_window_label(session_id) {
        return Err("Desktop annotation overlay belongs to a stale session".into());
    }
    let overlay_handle = state.screen_share.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if overlay_handle.session_id.load(Ordering::SeqCst) != session_id
            || !overlay_handle.active.load(Ordering::SeqCst)
        {
            let _ = window.close();
            return;
        }
        let show_result = window
            .set_always_on_top(true)
            .map_err(|error| error.to_string())
            .and_then(|_| show_desktop_overlay_without_activation(&window));
        if let Err(error) = show_result {
            emit_desktop_overlay_error(&app_handle, &error);
            let _ = window.close();
            return;
        }

        // A capture-excluded, full-screen transparent window makes both WGC
        // and DXGI return a black monitor frame on supported Windows builds.
        // Keep the host overlay in the captured stream instead. Viewers also
        // render the same normalized document as a crisp client-side layer,
        // so the two representations align without creating a feedback loop.
        match configure_desktop_overlay_window(&window, &overlay_handle) {
            Ok(()) => overlay_handle
                .desktop_overlay_active
                .store(true, Ordering::SeqCst),
            Err(error) => {
                emit_desktop_overlay_error(&app_handle, &error);
                let _ = window.close();
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub fn screen_share_close_desktop_overlay(
    app_handle: AppHandle,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let session_id = state.screen_share.session_id.load(Ordering::SeqCst);
    close_desktop_overlay_window(&app_handle, &state.screen_share, session_id);
    Ok(())
}

fn desktop_overlay_window_label(session_id: u64) -> String {
    format!("{DESKTOP_OVERLAY_WINDOW_LABEL_PREFIX}-{session_id}")
}

fn desktop_overlay_bounds(handle: &ScreenShareHandle) -> Result<ScreenRect, String> {
    let monitor_index = handle.active_monitor_index.load(Ordering::Relaxed);
    let (fallback_width, fallback_height) = handle
        .interaction
        .lock()
        .ok()
        .and_then(|interaction| interaction.clone())
        .and_then(|interaction| interaction.latest_frame_info())
        .map(|frame| (frame.width.max(1), frame.height.max(1)))
        .unwrap_or((1, 1));
    source_rect_for_monitor(monitor_index, fallback_width, fallback_height)
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .ok_or_else(|| "Active monitor is unavailable".to_string())
}

fn configure_desktop_overlay_window(
    window: &WebviewWindow,
    handle: &ScreenShareHandle,
) -> Result<(), String> {
    let bounds = desktop_overlay_bounds(handle)?;
    window
        .set_size(Size::Physical(PhysicalSize::new(
            bounds.width,
            bounds.height,
        )))
        .map_err(|error| format!("Failed to size desktop annotation overlay: {error}"))?;
    window
        .set_position(PhysicalPosition::new(bounds.left, bounds.top))
        .map_err(|error| format!("Failed to position desktop annotation overlay: {error}"))?;
    window
        .set_always_on_top(true)
        .map_err(|error| format!("Failed to keep desktop annotation overlay on top: {error}"))?;
    window.set_ignore_cursor_events(true).map_err(|error| {
        format!("Failed to make desktop annotation overlay click-through: {error}")
    })?;
    // `set_ignore_cursor_events` updates Tao's window flags and can clear the
    // raw WS_VISIBLE bit that was set by SW_SHOWNOACTIVATE. Restore visibility
    // without activating the overlay after every style update.
    show_desktop_overlay_without_activation(window)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn show_desktop_overlay_without_activation(window: &WebviewWindow) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};

    let hwnd = window
        .hwnd()
        .map(|hwnd| HWND(hwnd.0 as *mut _))
        .map_err(|error| error.to_string())?;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn show_desktop_overlay_without_activation(window: &WebviewWindow) -> Result<(), String> {
    window.show().map_err(|error| error.to_string())
}

fn close_desktop_overlay_window(
    app_handle: &AppHandle,
    handle: &ScreenShareHandle,
    session_id: u64,
) {
    handle.desktop_overlay_active.store(false, Ordering::SeqCst);
    if let Some(window) = app_handle.get_webview_window(&desktop_overlay_window_label(session_id)) {
        if let Err(error) = window.close() {
            log::warn!("Failed to close desktop annotation overlay: {error}");
        }
    }
}

fn sync_desktop_overlay_window(
    app_handle: &AppHandle,
    handle: &ScreenShareHandle,
    session_id: u64,
) -> Result<(), String> {
    let Some(window) = app_handle.get_webview_window(&desktop_overlay_window_label(session_id))
    else {
        handle.desktop_overlay_active.store(false, Ordering::SeqCst);
        return Ok(());
    };
    configure_desktop_overlay_window(&window, handle)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CaptureCandidate {
    monitor_index: usize,
    backend: CaptureBackendKind,
}

fn build_black_recovery_candidates(
    display_count: usize,
    active_monitor_index: usize,
    active_backend: CaptureBackendKind,
) -> VecDeque<CaptureCandidate> {
    let mut candidates = VecDeque::new();
    candidates.push_back(CaptureCandidate {
        monitor_index: active_monitor_index,
        backend: active_backend.alternate(),
    });

    for monitor_index in 0..display_count {
        if monitor_index == active_monitor_index {
            continue;
        }
        candidates.push_back(CaptureCandidate {
            monitor_index,
            backend: active_backend,
        });
        candidates.push_back(CaptureCandidate {
            monitor_index,
            backend: active_backend.alternate(),
        });
    }

    candidates
}

fn detected_display_count() -> usize {
    Display::all().map(|displays| displays.len()).unwrap_or(0)
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

/// 报告当前输入桌面：交互桌面为 "Default"；锁屏/UAC 期间为 "Winlogon" 或直接打不开。
/// 用于在采集创建失败时一条日志区分"锁屏/安全桌面"与"真实的采集冲突"。
#[cfg(target_os = "windows")]
fn describe_input_desktop() -> String {
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_CONTROL_FLAGS,
        DESKTOP_READOBJECTS, UOI_NAME,
    };

    unsafe {
        let desktop = match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(desktop) => desktop,
            Err(error) => {
                return format!(
                    "input_desktop=unavailable(likely lock screen or UAC secure desktop, error={})",
                    sanitize_log_field(&error.message())
                );
            }
        };

        let mut name_buf = [0u16; 128];
        let mut needed = 0u32;
        let name = if GetUserObjectInformationW(
            windows::Win32::Foundation::HANDLE(desktop.0),
            UOI_NAME,
            Some(name_buf.as_mut_ptr() as *mut _),
            (name_buf.len() * 2) as u32,
            Some(&mut needed),
        )
        .is_ok()
        {
            let len = name_buf
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(name_buf.len());
            String::from_utf16_lossy(&name_buf[..len])
        } else {
            "unknown".to_string()
        };
        let _ = CloseDesktop(desktop);
        format!("input_desktop={}", name)
    }
}

#[cfg(not(target_os = "windows"))]
fn describe_input_desktop() -> String {
    "input_desktop=n/a".to_string()
}

/// 输入桌面是否可用（机器可交互）：锁屏/UAC 安全桌面期间 OpenInputDesktop
/// 会失败。供帧饥饿看门狗区分"锁定中的正常静默"与"解锁后的异常静默"。
#[cfg(target_os = "windows")]
fn is_input_desktop_available() -> bool {
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
    };
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(desktop) => {
                let _ = CloseDesktop(desktop);
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn is_input_desktop_available() -> bool {
    true
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

// ─── Frame Starvation Watchdog ──────────────────────────────

/// 看门狗判定：采集源已静默死亡，须强制重建。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StarvationVerdict {
    /// (重)建采集源后，桌面可用却迟迟无首帧（僵尸源，典型于
    /// 锁屏期间重建"成功"的 WGC 会话，解锁后也不会自愈）。
    NoFrameSinceCreate { waited: Duration },
    /// 曾正常出帧，观察到锁屏后解锁，宽限期内帧仍未恢复
    /// （WGC 锁屏静默死亡的已知形态）。
    SilentAfterUnlock { since_unlock: Duration },
}

impl StarvationVerdict {
    fn describe(&self) -> String {
        match self {
            Self::NoFrameSinceCreate { waited } => format!(
                "capture source produced no frame for {}s after (re)create while desktop is available — treating as silently dead",
                waited.as_secs()
            ),
            Self::SilentAfterUnlock { since_unlock } => format!(
                "no frame within {}s after desktop unlock — capture source likely died during lock screen",
                since_unlock.as_secs()
            ),
        }
    }
}

#[derive(Debug, Default)]
struct StarvationTick {
    /// 本 tick 首次观察到桌面进入锁定/安全桌面
    lock_observed: bool,
    /// 本 tick 观察到桌面从锁定恢复可用
    unlock_observed: bool,
    force_recreate: Option<StarvationVerdict>,
}

/// 纯状态机：由捕获循环每 tick 驱动，桌面锁定状态由调用方按需采样传入。
/// 关键设计：静止画面（长时间 WouldBlock 但从未观察到锁屏）永远不触发
/// 重建——避免空闲机器上无意义的采集器重建（WGC 黄框闪烁）。
struct FrameStarvationWatchdog {
    created_at: Instant,
    last_frame_at: Option<Instant>,
    desktop_locked: Option<bool>,
    saw_lock_while_starved: bool,
    unlocked_at: Option<Instant>,
    last_desktop_sample_at: Option<Instant>,
}

impl FrameStarvationWatchdog {
    fn new(now: Instant) -> Self {
        Self {
            created_at: now,
            last_frame_at: None,
            desktop_locked: None,
            saw_lock_while_starved: false,
            unlocked_at: None,
            last_desktop_sample_at: None,
        }
    }

    /// 采集源（重）建成功后调用：全部状态复位，首帧计时重新开始。
    fn on_created(&mut self, now: Instant) {
        *self = Self::new(now);
    }

    /// 收到真实帧：饥饿与锁屏观察全部清零。
    fn on_frame(&mut self, now: Instant) {
        self.last_frame_at = Some(now);
        self.saw_lock_while_starved = false;
        self.unlocked_at = None;
    }

    fn starved_for(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_frame_at.unwrap_or(self.created_at))
    }

    fn is_starved(&self, now: Instant) -> bool {
        self.starved_for(now) >= FRAME_STARVATION_MIN
    }

    /// 是否应采样桌面锁定状态（仅饥饿期间，至多每秒一次）。
    fn should_sample_desktop(&self, now: Instant) -> bool {
        self.is_starved(now)
            && self.last_desktop_sample_at.map_or(true, |t| {
                now.saturating_duration_since(t) >= STARVATION_DESKTOP_SAMPLE_INTERVAL
            })
    }

    fn tick(&mut self, now: Instant, desktop_locked: Option<bool>) -> StarvationTick {
        let mut out = StarvationTick::default();
        if !self.is_starved(now) {
            return out;
        }

        if let Some(locked) = desktop_locked {
            self.last_desktop_sample_at = Some(now);
            let prev = self.desktop_locked;
            if locked {
                if prev != Some(true) {
                    out.lock_observed = true;
                }
                self.saw_lock_while_starved = true;
                self.unlocked_at = None;
            } else if prev == Some(true) {
                out.unlock_observed = true;
                self.unlocked_at = Some(now);
            }
            self.desktop_locked = Some(locked);
        }

        // 判定只在"当前桌面可用"时进行；锁定期间无帧是正常的，必须等待。
        if self.desktop_locked != Some(false) {
            return out;
        }

        match self.last_frame_at {
            // (重)建后从未出过帧：僵尸源。经历过锁屏则从解锁时刻起算。
            None => {
                let base = self.unlocked_at.unwrap_or(self.created_at);
                let waited = now.saturating_duration_since(base);
                if waited >= STARVATION_FIRST_FRAME_TIMEOUT {
                    out.force_recreate = Some(StarvationVerdict::NoFrameSinceCreate { waited });
                }
            }
            // 出过帧且本轮饥饿期间观察到锁屏：解锁后宽限期内必须恢复出帧。
            Some(_) if self.saw_lock_while_starved => {
                if let Some(unlocked_at) = self.unlocked_at {
                    let since_unlock = now.saturating_duration_since(unlocked_at);
                    if since_unlock >= STARVATION_POST_UNLOCK_GRACE {
                        out.force_recreate =
                            Some(StarvationVerdict::SilentAfterUnlock { since_unlock });
                    }
                }
            }
            _ => {}
        }
        out
    }
}

// ─── Screen Capture Loop ────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum BlackFrameDecision {
    Accept,
    Suppress,
    ForceRecreate { reason: String },
}

struct BlackFrameRecoveryWatchdog {
    had_content_frame: bool,
    recovery_armed_at: Option<Instant>,
    black_since: Option<Instant>,
    saw_black_while_desktop_unavailable: bool,
    last_desktop_sample_at: Option<Instant>,
}

impl BlackFrameRecoveryWatchdog {
    fn new() -> Self {
        Self {
            had_content_frame: false,
            recovery_armed_at: None,
            black_since: None,
            saw_black_while_desktop_unavailable: false,
            last_desktop_sample_at: None,
        }
    }

    fn arm_for_recovery(&mut self, now: Instant) {
        self.recovery_armed_at = Some(now);
        self.black_since = None;
    }

    fn should_sample_desktop(&mut self, now: Instant, near_black: bool) -> bool {
        if !near_black || (!self.had_content_frame && self.recovery_armed_at.is_none()) {
            return false;
        }
        if self.last_desktop_sample_at.map_or(false, |sampled_at| {
            now.saturating_duration_since(sampled_at) < BLACK_FRAME_DESKTOP_SAMPLE_INTERVAL
        }) {
            return false;
        }
        self.last_desktop_sample_at = Some(now);
        true
    }

    fn observe_frame(
        &mut self,
        now: Instant,
        bgra: &[u8],
        width: usize,
        height: usize,
        stride: usize,
        recovery_pending: bool,
        desktop_available: Option<bool>,
    ) -> BlackFrameDecision {
        let near_black = is_nearly_black_bgra_frame(bgra, width, height, stride);
        self.observe_frame_classification(now, near_black, recovery_pending, desktop_available)
    }

    fn observe_frame_classification(
        &mut self,
        now: Instant,
        near_black: bool,
        recovery_pending: bool,
        desktop_available: Option<bool>,
    ) -> BlackFrameDecision {
        if !near_black {
            self.had_content_frame = true;
            self.recovery_armed_at = None;
            self.black_since = None;
            self.saw_black_while_desktop_unavailable = false;
            return BlackFrameDecision::Accept;
        }

        if desktop_available == Some(false) {
            self.saw_black_while_desktop_unavailable = true;
            self.black_since = None;
            return BlackFrameDecision::Suppress;
        }

        let recovery_window_open = recovery_pending
            || self.recovery_armed_at.map_or(false, |armed_at| {
                now.saturating_duration_since(armed_at) <= BLACK_FRAME_RECOVERY_WINDOW
            });
        let returning_from_unavailable_desktop =
            self.saw_black_while_desktop_unavailable && desktop_available == Some(true);

        if !self.had_content_frame && !(recovery_window_open || returning_from_unavailable_desktop)
        {
            return BlackFrameDecision::Accept;
        }

        if !(recovery_window_open || returning_from_unavailable_desktop) {
            return BlackFrameDecision::Accept;
        }

        let black_since = *self.black_since.get_or_insert_with(|| {
            if recovery_window_open {
                self.recovery_armed_at.unwrap_or(now)
            } else {
                now
            }
        });
        if now.saturating_duration_since(black_since) >= BLACK_FRAME_RECREATE_AFTER {
            return BlackFrameDecision::ForceRecreate {
                reason: format!(
                    "near-black frames persisted for {}ms after capture recovery",
                    now.saturating_duration_since(black_since).as_millis()
                ),
            };
        }

        BlackFrameDecision::Suppress
    }
}

fn is_nearly_black_bgra_frame(bgra: &[u8], width: usize, height: usize, stride: usize) -> bool {
    if width == 0 || height == 0 || stride < width * 4 || bgra.len() < height * stride {
        return false;
    }

    let x_step = (width / 64).max(1);
    let y_step = (height / 64).max(1);
    let mut sampled = 0usize;
    let mut bright = 0usize;

    for y in (0..height).step_by(y_step) {
        let row_start = y * stride;
        for x in (0..width).step_by(x_step) {
            let offset = row_start + x * 4;
            if offset + 2 >= bgra.len() {
                continue;
            }
            sampled += 1;
            let max_channel = bgra[offset].max(bgra[offset + 1]).max(bgra[offset + 2]);
            if max_channel > BLACK_FRAME_BRIGHT_THRESHOLD {
                bright += 1;
            }
        }
    }

    sampled > 0 && bright * 10_000 <= sampled * BLACK_FRAME_MAX_BRIGHT_PIXELS_PER_10K
}

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
    h264_worker: Option<H264EncoderWorker>,
    h264_media: Arc<H264MediaState>,
    tx: broadcast::Sender<Arc<Bytes>>,
    interaction: Arc<InteractionState>,
    cancel: Arc<AtomicBool>,
    fps_counter: Arc<AtomicU32>,
    media_metrics: Arc<ScreenShareMediaMetrics>,
    viewer_count: Arc<AtomicU32>,
    runtime_handle: Arc<ScreenShareHandle>,
    session_id: u64,
    startup_tx: Option<oneshot::Sender<Result<(), String>>>,
    app_handle: AppHandle,
) {
    let mut startup_tx = startup_tx;
    let h264_worker = h264_worker;
    let mut h264_failure_logged = false;
    let mut h264_ready_logged = false;
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

    // 尺寸随采集源重建/分辨率变化而更新——保持不变会让 encode 守卫
    // 永远丢帧，观看端表现为无提示的永久黑屏。
    let mut width = source.width();
    let mut height = source.height();
    let mut active_monitor_index = monitor_index;
    let frame_interval = Duration::from_millis(1000 / fps.max(1) as u64);
    let mut first_real_frame = false;
    // 会话内是否推送过真实帧：决定 WouldBlock 时是否继续发占位帧
    //（仅初始预热阶段发；重建等待期让观看端保留最后一帧真实画面）。
    let mut session_ever_had_frame = false;
    // 重建成功后延迟到真实帧到达才清除 capture_paused，
    // 僵尸源期间观看端保持"画面中断，重试中"提示而不是无解释黑屏。
    let mut pending_resume = false;
    let mut starvation_watchdog = FrameStarvationWatchdog::new(Instant::now());
    let mut black_frame_watchdog = BlackFrameRecoveryWatchdog::new();
    black_frame_watchdog.arm_for_recovery(Instant::now());
    let mut black_frame_suppressed_logged = false;
    let mut forced_recreate_error: Option<(io::Error, bool)> = None;
    let mut candidate_scan_started = false;
    let mut recovery_candidates = VecDeque::new();
    let mut privacy_issue_logged = false;

    // Cursor overlay setup
    #[cfg(target_os = "windows")]
    let mut monitor_rect = if show_cursor {
        cursor_overlay::get_monitor_rect(active_monitor_index)
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
        let data = Arc::new(Bytes::from(placeholder));
        let _ = tx.send(data);
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

        // ── 帧饥饿看门狗：先于取帧判定采集源是否已静默死亡 ──
        // WGC 锁屏/显示器休眠后可能停止触发 FrameArrived 且不报错（永远
        // WouldBlock），旧逻辑会在"画面无变化"分支里死等——观看端黑屏且刷新无效。
        let desktop_sample = if starvation_watchdog.should_sample_desktop(tick_start) {
            Some(!is_input_desktop_available())
        } else {
            None
        };
        let starvation = starvation_watchdog.tick(tick_start, desktop_sample);
        let current_backend = source.backend_kind();
        if starvation.lock_observed {
            black_frame_watchdog.arm_for_recovery(tick_start);
            emit_capture_create_diagnostic(
                &app_handle,
                "info",
                format!(
                    "取帧饥饿期间检测到桌面锁定（锁屏/UAC），等待解锁后自动恢复: backend={}, starved_for={}s, first_real_frame={}",
                    current_backend.label(),
                    starvation_watchdog.starved_for(tick_start).as_secs(),
                    first_real_frame
                ),
            );
        }
        if starvation.unlock_observed {
            emit_capture_create_diagnostic(
                &app_handle,
                "info",
                format!(
                    "桌面已解锁，等待画面帧恢复（宽限 {}s）: backend={}, starved_for={}s",
                    STARVATION_POST_UNLOCK_GRACE.as_secs(),
                    current_backend.label(),
                    starvation_watchdog.starved_for(tick_start).as_secs()
                ),
            );
        }

        let mut scan_capture_candidates = false;
        let frame_result =
            if let Some((error, should_scan_candidates)) = forced_recreate_error.take() {
                scan_capture_candidates = should_scan_candidates;
                Err(error)
            } else {
                match starvation.force_recreate {
                    Some(verdict) => {
                        scan_capture_candidates = true;
                        Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!("frame starvation watchdog: {}", verdict.describe()),
                        ))
                    }
                    None => source.frame(),
                }
            };

        match frame_result {
            Ok(frame) => {
                let stride = frame.stride();
                let frame_pixels = frame.pixels();

                // WGC 分辨率变化时帧尺寸会静默改变（staging 缓冲已随帧重建），
                // 必须同步 loop 尺寸，否则 encode 守卫永远丢帧 → 黑屏。
                // DXGI 的 stride 含行对齐填充，不适用此推导；其分辨率变化
                // 会直接报错走重建路径，重建后尺寸在恢复分支同步。
                if current_backend == CaptureBackendKind::Wgc && stride >= 4 {
                    let (frame_w, frame_h) = (stride / 4, frame_pixels.len() / stride);
                    if frame_w > 0 && frame_h > 0 && (frame_w != width || frame_h != height) {
                        invalidate_interaction_source(&runtime_handle, &interaction);
                        emit_capture_create_diagnostic(
                            &app_handle,
                            "warn",
                            format!(
                                "画面尺寸变化，已同步编码尺寸: {}x{} -> {}x{}, backend={}",
                                width,
                                height,
                                frame_w,
                                frame_h,
                                current_backend.label()
                            ),
                        );
                        width = frame_w;
                        height = frame_h;
                    }
                }

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

                let near_black = is_nearly_black_bgra_frame(source_pixels, width, height, stride);
                let black_desktop_sample =
                    if black_frame_watchdog.should_sample_desktop(tick_start, near_black) {
                        Some(is_input_desktop_available())
                    } else {
                        None
                    };
                match black_frame_watchdog.observe_frame_classification(
                    tick_start,
                    near_black,
                    pending_resume,
                    black_desktop_sample,
                ) {
                    BlackFrameDecision::Accept => {
                        black_frame_suppressed_logged = false;
                        starvation_watchdog.on_frame(tick_start);
                        if current_capture_issue(&runtime_handle).is_some() {
                            set_capture_issue(&runtime_handle, None);
                        }
                        candidate_scan_started = false;
                        recovery_candidates.clear();
                        privacy_issue_logged = false;
                    }
                    BlackFrameDecision::Suppress => {
                        if is_current_session(&runtime_handle, session_id) {
                            if current_capture_issue(&runtime_handle)
                                != Some(ScreenShareCaptureIssue::PrivacyModeOrDisplayOff)
                            {
                                set_capture_issue(
                                    &runtime_handle,
                                    Some(ScreenShareCaptureIssue::Retrying),
                                );
                            }
                        }
                        if !black_frame_suppressed_logged {
                            black_frame_suppressed_logged = true;
                            emit_capture_create_diagnostic(
                                &app_handle,
                                "warn",
                                format!(
                                    "near-black screen frames suppressed while waiting for capture recovery: backend={}, pending_resume={}, desktop_available={:?}",
                                    current_backend.label(),
                                    pending_resume,
                                    black_desktop_sample
                                ),
                            );
                        }
                        let elapsed = tick_start.elapsed();
                        if elapsed < frame_interval {
                            std::thread::sleep(frame_interval - elapsed);
                        }
                        continue;
                    }
                    BlackFrameDecision::ForceRecreate { reason } => {
                        forced_recreate_error = Some((
                            io::Error::new(
                                io::ErrorKind::TimedOut,
                                format!("black frame watchdog: {}", reason),
                            ),
                            true,
                        ));
                        if is_current_session(&runtime_handle, session_id) {
                            if current_capture_issue(&runtime_handle)
                                != Some(ScreenShareCaptureIssue::PrivacyModeOrDisplayOff)
                            {
                                set_capture_issue(
                                    &runtime_handle,
                                    Some(ScreenShareCaptureIssue::Retrying),
                                );
                            }
                        }
                        emit_capture_create_diagnostic(
                            &app_handle,
                            "warn",
                            format!(
                                "near-black screen frames persisted; rebuilding capture source: backend={}, pending_resume={}, desktop_available={:?}, reason={}",
                                current_backend.label(),
                                pending_resume,
                                black_desktop_sample,
                                reason
                            ),
                        );
                        let elapsed = tick_start.elapsed();
                        if elapsed < frame_interval {
                            std::thread::sleep(frame_interval - elapsed);
                        }
                        continue;
                    }
                }

                // Submit the captured pixels to the low-latency encoder before
                // spending time on the compatibility JPEG path.
                if let Some(worker) = h264_worker.as_ref() {
                    let _ = worker.try_submit(source_pixels, width, height, stride);
                }

                let jpeg = encode_jpeg_reuse(
                    source_pixels,
                    width,
                    height,
                    stride,
                    quality,
                    &mut rgb_buf,
                    &mut jpeg_buf,
                );

                if h264_worker.is_some() {
                    let current_transport = *runtime_handle.transport.lock().unwrap();
                    if h264_media.is_ready() {
                        if current_transport != ScreenShareMediaTransport::MseH264 {
                            *runtime_handle.transport.lock().unwrap() =
                                ScreenShareMediaTransport::MseH264;
                        }
                        if !h264_ready_logged {
                            h264_ready_logged = true;
                            h264_failure_logged = false;
                            emit_capture_create_diagnostic(
                                &app_handle,
                                "success",
                                "H.264 媒体流已就绪，观看端将优先使用低带宽传输".to_string(),
                            );
                        }
                    } else if current_transport == ScreenShareMediaTransport::MseH264 {
                        *runtime_handle.transport.lock().unwrap() =
                            ScreenShareMediaTransport::Mjpeg;
                        h264_ready_logged = false;
                    }
                    if let Some(error) = h264_media.error() {
                        if !h264_failure_logged {
                            h264_failure_logged = true;
                            emit_capture_create_diagnostic(
                                &app_handle,
                                "warn",
                                format!("H.264 媒体流不可用，已回退 MJPEG: {error}"),
                            );
                        }
                    }
                }

                if !jpeg.is_empty() {
                    let data = Arc::new(Bytes::from(jpeg));
                    media_metrics.record_encoded_frame(data.len());
                    interaction.record_frame_with_metadata(
                        data.clone(),
                        width as u32,
                        height as u32,
                    );
                    let _ = tx.send(data);
                    fps_counter.fetch_add(1, Ordering::Relaxed);
                    first_real_frame = true;
                    session_ever_had_frame = true;
                    if pending_resume {
                        // 重建后的首个真实帧才是"恢复"——此前观看端一直
                        // 显示"画面中断，重试中"提示（capture_paused=true）。
                        pending_resume = false;
                        if is_current_session(&runtime_handle, session_id) {
                            set_capture_issue(&runtime_handle, None);
                        }
                        emit_capture_create_diagnostic(
                            &app_handle,
                            "success",
                            format!(
                                "画面推流已恢复: backend={}, size={}x{}",
                                current_backend.label(),
                                width,
                                height
                            ),
                        );
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !session_ever_had_frame {
                    // Still warming up the very first source — send placeholder every 500ms
                    // to keep stream alive. 重建后的预热不发占位帧：观看端保留最后
                    // 一帧真实画面比被刷成深色占位帧（视觉上就是黑屏）好得多。
                    if !wait_for_capture_retry_delay(
                        Duration::from_millis(500),
                        &cancel,
                        &runtime_handle,
                        session_id,
                    ) {
                        break;
                    }
                    let placeholder = make_placeholder_jpeg();
                    let data = Arc::new(Bytes::from(placeholder));
                    let _ = tx.send(data);
                } else {
                    // Screen unchanged; sleep briefly (not busy-wait)
                    std::thread::sleep(Duration::from_millis(5));
                }
                continue;
            }
            Err(e) => {
                let capture_error_detail = format!(
                    "捕获循环异常，进入暂停重试: monitor_index={}, backend={}, viewers={}, first_real_frame={}, error_kind={:?}, error={}, {}",
                    active_monitor_index,
                    current_backend.label(),
                    viewer_count.load(Ordering::Relaxed),
                    first_real_frame,
                    e.kind(),
                    e,
                    describe_input_desktop()
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
                    if current_capture_issue(&runtime_handle)
                        != Some(ScreenShareCaptureIssue::PrivacyModeOrDisplayOff)
                    {
                        set_capture_issue(&runtime_handle, Some(ScreenShareCaptureIssue::Retrying));
                    }
                }
                // The HTTP server and viewer connections stay alive during the pause;
                // viewers keep the last frame and see a "retrying" hint via /status.
                drop(source);

                let mut retry_attempt = 0u32;
                let recovered = if scan_capture_candidates {
                    if !candidate_scan_started {
                        recovery_candidates = build_black_recovery_candidates(
                            detected_display_count().max(1),
                            active_monitor_index,
                            current_backend,
                        );
                        candidate_scan_started = true;
                    }

                    loop {
                        if recovery_candidates.is_empty() {
                            if is_current_session(&runtime_handle, session_id) {
                                set_capture_issue(
                                    &runtime_handle,
                                    Some(ScreenShareCaptureIssue::PrivacyModeOrDisplayOff),
                                );
                            }
                            if !privacy_issue_logged {
                                privacy_issue_logged = true;
                                emit_capture_create_diagnostic(
                                    &app_handle,
                                    "error",
                                    "所有显示器与采集后端均持续输出黑屏；可能启用了远程控制隐私模式或显示器已被逻辑关闭。保持共享服务运行并继续定期检测"
                                        .to_string(),
                                );
                            }
                            if !wait_for_capture_retry_delay(
                                BLACK_FRAME_PRIVACY_RESCAN_DELAY,
                                &cancel,
                                &runtime_handle,
                                session_id,
                            ) {
                                break None;
                            }
                            recovery_candidates = build_black_recovery_candidates(
                                detected_display_count().max(1),
                                active_monitor_index,
                                current_backend,
                            );
                        } else if !wait_for_capture_retry_delay(
                            capture_recreate_backoff(retry_attempt),
                            &cancel,
                            &runtime_handle,
                            session_id,
                        ) {
                            break None;
                        }

                        let Some(candidate) = recovery_candidates.pop_front() else {
                            continue;
                        };
                        emit_capture_create_diagnostic(
                            &app_handle,
                            "warn",
                            format!(
                                "检测到持续黑屏，尝试其他采集候选: monitor_index={}, backend={}, remaining_candidates={}",
                                candidate.monitor_index,
                                candidate.backend.label(),
                                recovery_candidates.len()
                            ),
                        );
                        match create_capture_source_for_backend(
                            candidate.monitor_index,
                            show_cursor,
                            candidate.backend,
                            &cancel,
                            &runtime_handle,
                            session_id,
                            &app_handle,
                        ) {
                            Ok(new_source) => {
                                break Some((new_source, candidate.monitor_index));
                            }
                            Err(err) => {
                                retry_attempt = retry_attempt.saturating_add(1);
                                emit_capture_create_diagnostic(
                                    &app_handle,
                                    "warn",
                                    format!(
                                        "黑屏恢复候选创建失败: attempt={}, monitor_index={}, backend={}, cause={}",
                                        retry_attempt,
                                        candidate.monitor_index,
                                        candidate.backend.label(),
                                        err
                                    ),
                                );
                            }
                        }
                    }
                } else {
                    loop {
                        if !wait_for_capture_retry_delay(
                            capture_recreate_backoff(retry_attempt),
                            &cancel,
                            &runtime_handle,
                            session_id,
                        ) {
                            break None;
                        }
                        match create_capture_source(
                            active_monitor_index,
                            show_cursor,
                            backend_mode,
                            CaptureStartKind::RuntimeRecreate,
                            Some(current_backend),
                            &cancel,
                            &runtime_handle,
                            session_id,
                            &app_handle,
                        ) {
                            Ok(new_source) => {
                                break Some((new_source, active_monitor_index));
                            }
                            Err(err) => {
                                retry_attempt = retry_attempt.saturating_add(1);
                                let retry_msg = format!(
                                    "屏幕捕获器重建失败，{}s 后继续重试: attempt={}, monitor_index={}, viewers={}, cause={}",
                                    capture_recreate_backoff(retry_attempt).as_secs(),
                                    retry_attempt,
                                    active_monitor_index,
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
                    }
                };

                match recovered {
                    Some((new_source, recovered_monitor_index)) => {
                        // A recreated capture source is a new coordinate space,
                        // even when the dimensions happen to be unchanged.
                        invalidate_interaction_source(&runtime_handle, &interaction);
                        source = new_source;
                        active_monitor_index = recovered_monitor_index;
                        runtime_handle
                            .active_monitor_index
                            .store(active_monitor_index, Ordering::SeqCst);
                        // 注意：不在这里清除 capture_paused——重建"成功"可能是僵尸源
                        //（锁屏期间创建的 WGC 会话不出帧），等首个真实帧到达再清除。
                        pending_resume = true;
                        let recovered_at = Instant::now();
                        starvation_watchdog.on_created(recovered_at);
                        black_frame_watchdog.arm_for_recovery(recovered_at);
                        black_frame_suppressed_logged = false;
                        let (new_width, new_height) = (source.width(), source.height());
                        if new_width != width || new_height != height {
                            emit_capture_create_diagnostic(
                                &app_handle,
                                "warn",
                                format!(
                                    "重建后画面尺寸变化，已同步编码尺寸: {}x{} -> {}x{}",
                                    width, height, new_width, new_height
                                ),
                            );
                            width = new_width;
                            height = new_height;
                        }
                        #[cfg(target_os = "windows")]
                        if show_cursor {
                            monitor_rect = cursor_overlay::get_monitor_rect(active_monitor_index);
                        }
                        let resumed_msg = format!(
                            "屏幕捕获已恢复，等待首帧推流: retries={}, monitor_index={}, backend={}, size={}x{}",
                            retry_attempt,
                            active_monitor_index,
                            source.backend_kind().label(),
                            width,
                            height
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
        let result = create_capture_source_for_backend(
            monitor_index,
            show_cursor,
            *backend,
            cancel,
            runtime_handle,
            session_id,
            app_handle,
        );

        match result {
            Ok(source) => return Ok(source),
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

fn create_capture_source_for_backend(
    monitor_index: usize,
    show_cursor: bool,
    backend: CaptureBackendKind,
    cancel: &AtomicBool,
    runtime_handle: &ScreenShareHandle,
    session_id: u64,
    app_handle: &AppHandle,
) -> Result<CaptureSource, String> {
    let source = match backend {
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
    }?;

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
    Ok(source)
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
    /// GraphicsCaptureItem.Closed 触发后置位（显示器丢失/显示拓扑变化）——
    /// 事件死亡时帧循环永远收不到错误，靠此标志把"已关闭"暴露为可重建错误。
    closed: Arc<AtomicBool>,
    _closed_handler: TypedEventHandler<GraphicsCaptureItem, IInspectable>,
    closed_token: EventRegistrationToken,
    /// 最近一次真实交付帧的时刻；配合主动探测判定事件通道是否静默死亡。
    last_frame_delivered: Instant,
    last_probe_at: Option<Instant>,
    probe_recovery_logged: bool,
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

        let closed = Arc::new(AtomicBool::new(false));
        let closed_flag = closed.clone();
        let closed_handler =
            TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                closed_flag.store(true, Ordering::SeqCst);
                Ok(())
            });
        let closed_token = item
            .Closed(&closed_handler)
            .map_err(|error| format_windows_error("GraphicsCaptureItem::Closed", &error))?;

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
            closed,
            _closed_handler: closed_handler,
            closed_token,
            last_frame_delivered: Instant::now(),
            last_probe_at: None,
            probe_recovery_logged: false,
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

    /// 帧饥饿超过阈值后是否应主动探测帧池（限频）。
    fn probe_due(&mut self) -> bool {
        let now = Instant::now();
        if now.saturating_duration_since(self.last_frame_delivered) < WGC_PROBE_AFTER {
            return false;
        }
        if self
            .last_probe_at
            .is_some_and(|t| now.saturating_duration_since(t) < WGC_PROBE_INTERVAL)
        {
            return false;
        }
        self.last_probe_at = Some(now);
        true
    }

    fn frame(&mut self) -> io::Result<CapturedFrame<'_>> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "WGC capture item closed (monitor lost or display topology changed)",
            ));
        }
        let mut via_probe = false;
        match self.frame_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // FrameArrived 长时间静默时不能只报 WouldBlock：事件通道死亡
                // 是无声的（锁屏/设备丢失后的已知形态）。主动调用 TryGetNextFrame
                // 把真实故障暴露成带 HRESULT 的错误，触发上层重建。
                if !self.probe_due() {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "WGC frame not ready",
                    ));
                }
                via_probe = true;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "WGC frame signal disconnected",
                ));
            }
        }
        while self.frame_rx.try_recv().is_ok() {}

        let frame = match self.frame_pool.TryGetNextFrame() {
            Ok(frame) => frame,
            // 空帧（HRESULT=S_OK 且对象为 null）：池仍存活只是无新内容
            //（纯静止画面），维持 WouldBlock 语义，绝不触发重建。
            Err(error) if is_wgc_no_frame_error(&error) => {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "WGC frame pool has no new frame",
                ));
            }
            Err(error) => {
                return Err(windows_error_to_io(
                    if via_probe {
                        "Direct3D11CaptureFramePool::TryGetNextFrame(starvation probe)"
                    } else {
                        "Direct3D11CaptureFramePool::TryGetNextFrame"
                    },
                    error,
                ));
            }
        };
        if via_probe && !self.probe_recovery_logged {
            self.probe_recovery_logged = true;
            log::warn!(
                "WGC FrameArrived 事件静默但帧池仍在产帧，已通过主动探测恢复取帧（事件通道疑似失效）"
            );
        }
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

        self.last_frame_delivered = Instant::now();
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
        let _ = self._item.RemoveClosed(self.closed_token);
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

/// windows-rs 将"HRESULT=S_OK 但返回对象为 null"投影为 code==S_OK 的 Err；
/// TryGetNextFrame 的"当前无新帧"正是这种形态——池仍存活，不是故障。
/// 真正的设备丢失/池已关闭会携带非零错误码。
#[cfg(target_os = "windows")]
fn is_wgc_no_frame_error(error: &WindowsError) -> bool {
    error.code() == HRESULT(0)
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
            "屏幕捕获器创建开始: session_id={}, monitor_index={}, attempts={}, retry_delays_ms={:?}, startup_timeout_ms={}, {}, {}",
            session_id,
            monitor_index,
            total_attempts,
            retry_delays,
            CAPTURE_STARTUP_TIMEOUT.as_millis(),
            capture_runtime_state_summary(runtime_handle, session_id),
            describe_input_desktop()
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
                        "屏幕捕获器创建失败: session_id={}, attempt={}/{}, next_delay_ms={}, elapsed_ms={}, monitor_index={}, retryable={}, error_kind={:?}, cause={}, {}, {}",
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
                        capture_runtime_state_summary(runtime_handle, session_id),
                        describe_input_desktop()
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
    let app = screen_share_router(state);

    let (drain_started_tx, drain_started_rx) = oneshot::channel::<()>();
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_rx.await.ok();
        let _ = drain_started_tx.send(());
    });

    tokio::select! {
        result = serve => {
            if let Err(e) = result {
                log::error!("Screen share HTTP server error: {}", e);
            }
        }
        _ = async {
            drain_started_rx.await.ok();
            tokio::time::sleep(SERVER_DRAIN_DEADLINE).await;
        } => {
            // Deadline branch: the select drops the serve future — and with it the
            // listener — so the port becomes reusable immediately even if half-dead
            // viewer connections never finish draining.
            log::warn!(
                "Screen share drain deadline ({}s) exceeded; forcing listener close",
                SERVER_DRAIN_DEADLINE.as_secs()
            );
        }
    }

    log::info!("Screen share HTTP server stopped");
}

fn screen_share_router(state: Arc<HttpServerState>) -> Router {
    Router::new()
        .route("/", get(handler_index))
        .route("/assets/*path", get(handler_web_asset))
        .route("/stream", get(handler_stream))
        .route("/media/ws", get(handler_media_ws))
        .route("/auth", post(handler_auth))
        .route("/status", get(handler_status))
        .route("/session/ws", get(handler_session_ws))
        .route("/snapshot/:frame_id", get(handler_snapshot))
        .with_state(state)
}

// ─── HTTP Handlers ──────────────────────────────────────────

#[derive(Deserialize)]
struct IndexQuery {
    error: Option<u8>,
    host_preview: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamQuery {
    /// `single=1` returns the newest cached JPEG and closes the response. It
    /// is used by the viewer's optional refresh limiter; the default remains a
    /// long-lived MJPEG response for backwards compatibility.
    single: Option<u8>,
    /// Browser retries mark the replacement stream so reconnect first-frame
    /// latency can be measured separately from initial connections.
    reconnect: Option<u8>,
}

#[derive(Debug, Default, Deserialize)]
struct SessionQuery {
    /// Stable browser identity, scoped to one browser tab by sessionStorage.
    client_id: Option<String>,
}

fn requested_session_client_id(candidate: Option<String>) -> String {
    if candidate.as_deref().is_some_and(valid_session_client_id) {
        return candidate.expect("candidate was checked above");
    }
    Uuid::new_v4().to_string()
}

fn valid_session_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CLIENT_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn screen_share_asset_path(path: &str) -> String {
    format!("assets/{}", path.trim_start_matches('/'))
}

async fn handler_index(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(q): Query<IndexQuery>,
    headers: HeaderMap,
) -> Response {
    let preview_query_authorized = q.host_preview.as_deref().is_some_and(|candidate| {
        state
            .preview_token
            .lock()
            .ok()
            .and_then(|token| token.clone())
            .as_deref()
            == Some(candidate)
    });
    if preview_query_authorized {
        let cookie_token = Uuid::new_v4().simple().to_string();
        *state.preview_token.lock().unwrap() = Some(cookie_token.clone());
        return Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header("Location", "/")
            .header(
                "Set-Cookie",
                format!("ss_preview={cookie_token}; HttpOnly; SameSite=Strict; Path=/"),
            )
            .body(Body::empty())
            .unwrap();
    }
    let preview_authorized = preview_token_matches(&headers, None, &state.preview_token);
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) && !preview_authorized {
            let has_error = q.error.unwrap_or(0) == 1;
            let need_username = state.auth_username.is_some();
            return Html(login_html(has_error, need_username)).into_response();
        }
    }

    screenshare_web_assets::serve_index()
}

async fn handler_web_asset(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash)
            && !preview_token_matches(&headers, None, &state.preview_token)
        {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }
    let asset_path = screen_share_asset_path(&path);
    screenshare_web_assets::serve_asset(&asset_path)
        .unwrap_or_else(screenshare_web_assets::unavailable_response)
}

fn mjpeg_frame_chunk(frame: &Bytes) -> Bytes {
    let frame_len = frame.len();
    let mut buffer = BytesMut::with_capacity(frame_len + 128);
    buffer.extend_from_slice(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: ");
    buffer.extend_from_slice(frame_len.to_string().as_bytes());
    buffer.extend_from_slice(b"\r\n\r\n");
    buffer.extend_from_slice(frame);
    buffer.extend_from_slice(b"\r\n");
    buffer.freeze()
}

fn drain_to_latest_mjpeg_frame(
    receiver: &mut broadcast::Receiver<Arc<Bytes>>,
    initial: Arc<Bytes>,
) -> (Arc<Bytes>, u64) {
    let mut latest = initial;
    let mut skipped = 0u64;
    loop {
        match receiver.try_recv() {
            Ok(frame) => {
                latest = frame;
                skipped = skipped.saturating_add(1);
            }
            Err(broadcast::error::TryRecvError::Lagged(count)) => {
                skipped = skipped.saturating_add(count);
            }
            Err(broadcast::error::TryRecvError::Empty)
            | Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }
    (latest, skipped)
}

async fn handler_stream(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    // Auth check
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash)
            && !preview_token_matches(&headers, None, &state.preview_token)
        {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }

    let client_ip = addr.ip().to_string();
    let single = query.single == Some(1);
    record_viewer_ip(&state.viewer_ips, client_ip.clone());

    // A rate-limited viewer must still work when the desktop is static and
    // the capture loop has no new broadcast frame to deliver. Reuse the
    // server-side cached JPEG and avoid counting this short request as a
    // long-lived viewer connection.
    if single {
        if state.cancel.load(Ordering::Relaxed) {
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("Cache-Control", "no-store")
                .body(Body::from("Screen share is not active"))
                .unwrap();
        }
        let Some(frame) = state.interaction.latest_frame_bytes() else {
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("Cache-Control", "no-store")
                .body(Body::from("Screen frame is not ready"))
                .unwrap();
        };
        state
            .bytes_sent
            .fetch_add(frame.len() as u64, Ordering::Relaxed);
        return Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "image/jpeg")
            .header("Content-Length", frame.len())
            .header("Cache-Control", "no-store, no-cache")
            .body(Body::from(frame.as_ref().clone()))
            .unwrap();
    }

    let viewer_total = state.viewer_count.fetch_add(1, Ordering::Relaxed) + 1;
    let is_reconnect = query.reconnect == Some(1);
    state.media_metrics.record_stream_open(is_reconnect);
    state.events.emit_tool_log(
        &format!(
            "Viewer connected: ip={}, viewers={}, user_agent={}",
            client_ip,
            viewer_total,
            summarize_user_agent(&headers)
        ),
        "info",
    );
    let viewer_guard = ViewerGuard {
        events: state.events.clone(),
        count: state.viewer_count.clone(),
        ips: state.viewer_ips.clone(),
        ip: client_ip,
    };
    let bytes_sent = state.bytes_sent.clone();
    let media_metrics = state.media_metrics.clone();
    let interaction = state.interaction.clone();
    let broadcast_tx = state.broadcast_tx.clone();
    let cancel = state.cancel.clone();
    let initial_frame = interaction.latest_frame_bytes();
    let mut rx = broadcast_tx.subscribe();
    let stream_started = Instant::now();

    let stream = async_stream::stream! {
        let _guard = viewer_guard;
        let mut first_frame_sent = false;

        if let Some(frame) = initial_frame {
            let chunk = mjpeg_frame_chunk(&frame);
            bytes_sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            media_metrics.record_stream_first_frame(stream_started.elapsed(), is_reconnect);
            first_frame_sent = true;
            yield Ok::<_, Infallible>(chunk);
        }

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
                    let (frame, skipped) = drain_to_latest_mjpeg_frame(&mut rx, frame);
                    if skipped > 0 {
                        media_metrics.record_lagged_frames(skipped);
                    }
                    let chunk = mjpeg_frame_chunk(&frame);
                    bytes_sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                    if !first_frame_sent {
                        media_metrics.record_stream_first_frame(stream_started.elapsed(), is_reconnect);
                        first_frame_sent = true;
                    }
                    yield Ok::<_, Infallible>(chunk);
                }
                Ok(Err(broadcast::error::RecvError::Lagged(skipped))) => {
                    media_metrics.record_lagged_frames(skipped);
                    // Reset the receiver after yielding the cached newest frame,
                    // instead of walking through the remainder of the stale queue.
                    rx = broadcast_tx.subscribe();
                    if let Some(frame) = interaction.latest_frame_bytes() {
                        let chunk = mjpeg_frame_chunk(&frame);
                        bytes_sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                        if !first_frame_sent {
                            media_metrics.record_stream_first_frame(stream_started.elapsed(), is_reconnect);
                            first_frame_sent = true;
                        }
                        yield Ok::<_, Infallible>(chunk);
                    }
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

async fn handler_media_ws(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    websocket: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash)
            && !preview_token_matches(&headers, None, &state.preview_token)
        {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }
    if state.h264_media.snapshot().is_none() {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Cache-Control", "no-store")
            .body(Body::from("H.264 media stream is not ready"))
            .unwrap();
    }
    let websocket = match websocket {
        Ok(websocket) => websocket,
        Err(rejection) => return rejection.into_response(),
    };

    let client_ip = addr.ip().to_string();
    record_viewer_ip(&state.viewer_ips, client_ip.clone());
    let viewer_total = state.viewer_count.fetch_add(1, Ordering::Relaxed) + 1;
    let is_reconnect = query.reconnect == Some(1);
    state.media_metrics.record_stream_open(is_reconnect);
    state.events.emit_tool_log(
        &format!(
            "Viewer connected: ip={}, viewers={}, transport=mse_h264, user_agent={}",
            client_ip,
            viewer_total,
            summarize_user_agent(&headers)
        ),
        "info",
    );
    let viewer_guard = ViewerGuard {
        events: state.events.clone(),
        count: state.viewer_count.clone(),
        ips: state.viewer_ips.clone(),
        ip: client_ip,
    };
    let media = state.h264_media.clone();
    let cancel = state.cancel.clone();
    let bytes_sent = state.bytes_sent.clone();
    let media_metrics = state.media_metrics.clone();
    websocket
        .max_message_size(64 * 1024)
        .on_upgrade(move |socket| {
            run_h264_media_socket(
                socket,
                media,
                cancel,
                bytes_sent,
                media_metrics,
                is_reconnect,
                viewer_guard,
            )
        })
        .into_response()
}

async fn run_h264_media_socket(
    mut socket: WebSocket,
    media: Arc<H264MediaState>,
    cancel: Arc<AtomicBool>,
    bytes_sent: Arc<AtomicU64>,
    media_metrics: Arc<ScreenShareMediaMetrics>,
    is_reconnect: bool,
    viewer_guard: ViewerGuard,
) {
    let _viewer_guard = viewer_guard;
    let started_at = Instant::now();
    let mut first_frame_sent = false;
    let mut events = media.subscribe();
    let mut generation = 0u64;
    let mut sequence = 0u64;
    if let Some(snapshot) = media.snapshot() {
        match send_h264_snapshot(&mut socket, &snapshot, &bytes_sent).await {
            Ok((sent_generation, sent_sequence, sent_frame)) => {
                generation = sent_generation;
                sequence = sent_sequence;
                first_frame_sent = sent_frame;
                if sent_frame {
                    media_metrics.record_stream_first_frame(started_at.elapsed(), is_reconnect);
                }
            }
            Err(_) => return,
        }
    }

    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
            }
            event = events.recv() => {
                match event {
                    Ok(event) => match event.as_ref() {
                        H264MediaEvent::Reset(descriptor) => {
                            if send_h264_descriptor(&mut socket, descriptor, &bytes_sent).await.is_err() {
                                break;
                            }
                            generation = descriptor.generation;
                            sequence = 0;
                            first_frame_sent = false;
                        }
                        H264MediaEvent::Segment(segment)
                            if segment.generation == generation && segment.sequence > sequence =>
                        {
                            let payload = segment.bytes.as_ref().clone();
                            let length = payload.len();
                            if socket.send(Message::Binary(payload.to_vec())).await.is_err() {
                                break;
                            }
                            bytes_sent.fetch_add(length as u64, Ordering::Relaxed);
                            sequence = segment.sequence;
                            tokio::time::sleep(H264_STREAM_COOPERATIVE_DELAY).await;
                            if !first_frame_sent {
                                first_frame_sent = true;
                                media_metrics.record_stream_first_frame(started_at.elapsed(), is_reconnect);
                            }
                        }
                        H264MediaEvent::Segment(_) => {}
                        H264MediaEvent::Unavailable { generation: next_generation, error } => {
                            generation = *next_generation;
                            sequence = 0;
                            first_frame_sent = false;
                            let message = serde_json::json!({
                                "v": 1,
                                "type": "media.unavailable",
                                "generation": next_generation,
                                "error": error,
                            })
                            .to_string();
                            if socket.send(Message::Text(message)).await.is_err() {
                                break;
                            }
                        }
                    },
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        media_metrics.record_lagged_frames(skipped);
                        if let Some(snapshot) = media.snapshot() {
                            match send_h264_snapshot(&mut socket, &snapshot, &bytes_sent).await {
                                Ok((sent_generation, sent_sequence, sent_frame)) => {
                                    generation = sent_generation;
                                    sequence = sent_sequence;
                                    first_frame_sent = sent_frame;
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Ping(payload) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Text(_) | Message::Binary(_) | Message::Pong(_) => {}
                }
            }
        }
    }
}

async fn send_h264_snapshot(
    socket: &mut WebSocket,
    snapshot: &H264StreamSnapshot,
    bytes_sent: &AtomicU64,
) -> Result<(u64, u64, bool), axum::Error> {
    send_h264_descriptor(socket, &snapshot.descriptor, bytes_sent).await?;
    let mut sequence = 0;
    let mut sent_frame = false;
    for segment in &snapshot.segments {
        let payload = segment.bytes.as_ref().clone();
        let length = payload.len();
        socket.send(Message::Binary(payload.to_vec())).await?;
        bytes_sent.fetch_add(length as u64, Ordering::Relaxed);
        sequence = segment.sequence;
        sent_frame = true;
        tokio::time::sleep(H264_STREAM_COOPERATIVE_DELAY).await;
    }
    Ok((snapshot.descriptor.generation, sequence, sent_frame))
}

async fn send_h264_descriptor(
    socket: &mut WebSocket,
    descriptor: &H264StreamDescriptor,
    bytes_sent: &AtomicU64,
) -> Result<(), axum::Error> {
    let message = serde_json::json!({
        "v": 1,
        "type": "media.hello",
        "transport": "mse_h264",
        "generation": descriptor.generation,
        "codec": descriptor.codec,
        "mime_type": format!("video/mp4; codecs=\"{}\"", descriptor.codec),
        "width": descriptor.width,
        "height": descriptor.height,
        "fps": descriptor.fps,
        "bitrate_bps": descriptor.bitrate_bps,
    })
    .to_string();
    socket.send(Message::Text(message)).await?;
    let init = descriptor.init_segment.as_ref().clone();
    let length = init.len();
    socket.send(Message::Binary(init.to_vec())).await?;
    bytes_sent.fetch_add(length as u64, Ordering::Relaxed);
    tokio::time::sleep(H264_STREAM_COOPERATIVE_DELAY).await;
    Ok(())
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
    let interaction_document = state.interaction.snapshot();
    let latest_frame = state.interaction.latest_frame_info();
    let control = state.interaction.control_snapshot();
    let media_metrics = state
        .media_metrics
        .snapshot(latest_frame.as_ref().map(|frame| frame.captured_at_ms));
    Json(serde_json::json!({
        "active": !state.cancel.load(Ordering::Relaxed),
        "viewers": state.viewer_count.load(Ordering::Relaxed),
        "session_id": state.session_id,
        "source_epoch": interaction_document.source_epoch,
        "annotation_count": interaction_document.shapes.len(),
        "view_mode": interaction_document.mode,
        "frozen_frame_id": interaction_document.frozen_frame_id,
        "interaction_connected_count": state.interaction.client_count(),
        "latest_frame_id": latest_frame.as_ref().map(|frame| frame.frame_id),
        "frame_width": latest_frame.as_ref().map(|frame| frame.width),
        "frame_height": latest_frame.as_ref().map(|frame| frame.height),
        "frame_captured_at_ms": latest_frame.as_ref().map(|frame| frame.captured_at_ms),
        "frame_age_ms": media_metrics.frame_age_ms,
        "fps_actual": media_metrics.fps_actual,
        "bitrate_kbps": media_metrics.bitrate_kbps,
        "media_metrics": media_metrics,
        "transport": state.transport.lock().unwrap().resolved_label(),
        "h264_media": state.h264_media.metrics(),
        "control_state": control.state,
        "controller_ip": control.controller_ip,
        "pending_control_request": state.interaction.pending_control_request(),
        "capture_paused": state.capture_paused.load(Ordering::Relaxed),
        "capture_issue": *state.capture_issue.lock().unwrap(),
    }))
}

async fn handler_snapshot(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Path(frame_id): Path<u64>,
    headers: HeaderMap,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash)
            && !preview_token_matches(&headers, None, &state.preview_token)
        {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }

    let Some(frame) = state.interaction.frozen_frame(frame_id) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Frozen frame is not available"))
            .unwrap();
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/jpeg")
        .header("Cache-Control", "no-store, no-cache")
        .header("Content-Length", frame.len())
        .body(Body::from(frame.as_ref().clone()))
        .unwrap()
}

async fn handler_session_ws(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<SessionQuery>,
    websocket: WebSocketUpgrade,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash)
            && !preview_token_matches(&headers, None, &state.preview_token)
        {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }

    let interaction = state.interaction.clone();
    let cancel = state.cancel.clone();
    let client_id = requested_session_client_id(query.client_id);
    let client_ip = addr.ip().to_string();
    let user_agent = summarize_user_agent(&headers);
    let events = state.events.clone();
    let input_worker = state.input_worker.clone();
    websocket
        .max_message_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| {
            run_interaction_socket(
                socket,
                interaction,
                cancel,
                client_id,
                client_ip,
                user_agent,
                events,
                input_worker,
            )
        })
        .into_response()
}

async fn run_interaction_socket(
    mut socket: WebSocket,
    interaction: Arc<InteractionState>,
    cancel: Arc<AtomicBool>,
    client_id: String,
    client_ip: String,
    user_agent: String,
    events: Arc<dyn ScreenShareEventSink>,
    input_worker: Option<Arc<InputWorkerHandle>>,
) {
    if let Err(error) = interaction.register_client_with_metadata(
        &client_id,
        InteractionClientMetadata::new(client_ip.clone(), user_agent),
    ) {
        let _ = send_interaction_message(&mut socket, error.to_message(&interaction)).await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    }

    let mut interaction_events = interaction.subscribe();
    let hello = match interaction.hello(&client_id) {
        Ok(message) => message,
        Err(error) => {
            let _ = send_interaction_message(&mut socket, error.to_message(&interaction)).await;
            interaction.unregister_client(&client_id);
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
    };
    if send_interaction_message(&mut socket, hello).await.is_err()
        || send_interaction_message(&mut socket, interaction.snapshot_message())
            .await
            .is_err()
    {
        interaction.unregister_client(&client_id);
        return;
    }

    log::info!("Interaction client connected: ip={client_ip}");

    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                // Snapshot() also expires old laser points. The client-side
                // expiry timestamp provides smooth rendering between ticks.
                let _ = interaction.snapshot();
            }
            event = interaction_events.recv() => {
                match event {
                    Ok(message) => {
                        if send_interaction_message(&mut socket, message).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if send_interaction_message(&mut socket, interaction.snapshot_message()).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Text(text) => {
                        if text.len() > MAX_WS_MESSAGE_BYTES {
                            let error = screenshare_interaction::ProtocolError::new(
                                "message_too_large",
                                format!("message exceeds {MAX_WS_MESSAGE_BYTES} bytes"),
                            );
                            if send_interaction_message(&mut socket, error.to_message(&interaction)).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        let envelope = match serde_json::from_str::<ClientEnvelope>(&text) {
                            Ok(envelope) => envelope,
                            Err(error) => {
                                let protocol_error = screenshare_interaction::ProtocolError::new(
                                    "invalid_json",
                                    format!("invalid interaction message: {error}"),
                                );
                                if send_interaction_message(&mut socket, protocol_error.to_message(&interaction)).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                        };
                        let message_type = envelope.message_type.clone();
                        if message_type.starts_with("input.") {
                            let context = InputContext::new(
                                client_id.clone(),
                                envelope.session_id,
                                envelope.source_epoch,
                            );
                            let input_result = interaction
                                .authorize_input(&client_id, &envelope)
                                .and_then(|_| {
                                    let input = parse_input_event(
                                        &message_type,
                                        envelope.payload.clone(),
                                    )
                                    .map_err(|message| {
                                        screenshare_interaction::ProtocolError::new(
                                            "invalid_input",
                                            message,
                                        )
                                    })?;
                                    let worker = input_worker.as_ref().ok_or_else(|| {
                                        screenshare_interaction::ProtocolError::new(
                                            "input_unavailable",
                                            "remote input is not enabled for this session",
                                        )
                                    })?;
                                    let queued = if matches!(&input, InputEvent::ReleaseAll) {
                                        worker.release_all(&context).map(|_| ())
                                    } else {
                                        worker
                                            .enqueue(QueuedInput::new(context, input))
                                            .map(|_| ())
                                    };
                                    queued
                                        .map_err(|_| {
                                            screenshare_interaction::ProtocolError::new(
                                                "input_queue_full",
                                                "remote input queue is full; control was revoked",
                                            )
                                        })?;
                                    Ok::<(), screenshare_interaction::ProtocolError>(())
                                });
                            if let Err(error) = input_result {
                                if error.code == "input_queue_full" {
                                    if let Some(worker) = input_worker.as_ref() {
                                        worker.revoke();
                                    }
                                    interaction.revoke_control("input_queue_full");
                                }
                                if send_interaction_message(
                                    &mut socket,
                                    error.to_message(&interaction),
                                )
                                .await
                                .is_err()
                                {
                                    break;
                                }
                            }
                            continue;
                        }
                        match interaction.process(&client_id, envelope) {
                            Ok(Some(event)) if event.message_type == "control.requested" => {
                                if let Some(request) = interaction.pending_control_request() {
                                    events.emit_control_request(request);
                                }
                            }
                            Ok(Some(event)) if message_type == "control.release" || message_type == "view.freeze" => {
                                if let Some(worker) = input_worker.as_ref() {
                                    worker.revoke();
                                }
                                let _ = event;
                            }
                            Ok(_) => {}
                            Err(error) => {
                                if send_interaction_message(&mut socket, error.to_message(&interaction)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Message::Binary(bytes) => {
                        let protocol_error = screenshare_interaction::ProtocolError::new(
                            "binary_not_supported",
                            "interaction messages must be UTF-8 JSON text",
                        );
                        if bytes.len() > MAX_WS_MESSAGE_BYTES || send_interaction_message(&mut socket, protocol_error.to_message(&interaction)).await.is_err() {
                            break;
                        }
                    }
                    Message::Ping(bytes) => {
                        if socket.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => break,
                }
            }
        }
    }

    let (_, source_epoch, _) = interaction.identity();
    let was_controller =
        interaction.is_controller(&client_id, interaction.identity().0, source_epoch);
    interaction.unregister_client(&client_id);
    if was_controller {
        if let Some(worker) = input_worker.as_ref() {
            worker.revoke();
        }
    }
}

async fn send_interaction_message(
    socket: &mut WebSocket,
    message: screenshare_interaction::ServerEnvelope,
) -> Result<(), axum::Error> {
    let serialized = serde_json::to_string(&message)
        .map_err(|error| axum::Error::new(std::io::Error::new(std::io::ErrorKind::Other, error)))?;
    socket.send(Message::Text(serialized)).await
}

// ─── Status Reporter ────────────────────────────────────────

async fn status_reporter(
    app_handle: AppHandle,
    active: Arc<AtomicBool>,
    viewer_count: Arc<AtomicU32>,
    fps_counter: Arc<AtomicU32>,
    bytes_sent: Arc<AtomicU64>,
    media_metrics: Arc<ScreenShareMediaMetrics>,
    server_url: String,
    all_urls: Vec<String>,
    start_time: Instant,
    viewer_ips: Arc<Mutex<ViewerIpMap>>,
    capture_paused: Arc<AtomicBool>,
    capture_issue: Arc<Mutex<Option<ScreenShareCaptureIssue>>>,
    interaction: Arc<InteractionState>,
    transport: Arc<Mutex<ScreenShareMediaTransport>>,
    input_worker: Option<Arc<InputWorkerHandle>>,
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

        if input_worker.as_ref().is_some_and(|worker| worker.failed()) {
            interaction.revoke_control("input_worker_failed");
            if let Some(worker) = input_worker.as_ref() {
                worker.revoke();
            }
        }

        if runtime_handle
            .desktop_overlay_active
            .load(Ordering::Relaxed)
        {
            let _ = sync_desktop_overlay_window(&app_handle, &runtime_handle, session_id);
        }

        let fps_count = fps_counter.swap(0, Ordering::Relaxed);
        let current_bytes = bytes_sent.load(Ordering::Relaxed);
        let bytes_delta = current_bytes.saturating_sub(last_bytes);
        last_bytes = current_bytes;
        let bitrate_kbps = (bytes_delta * 8 / 1024).min(u64::from(u32::MAX)) as u32;
        media_metrics.update_rates(fps_count, bitrate_kbps);

        let connected_ips = snapshot_viewer_ips(&viewer_ips);
        let interaction_document = interaction.snapshot();
        let control = interaction.control_snapshot();
        let latest_frame = interaction.latest_frame_info();
        let media_snapshot =
            media_metrics.snapshot(latest_frame.as_ref().map(|frame| frame.captured_at_ms));

        let status = ScreenShareStatus {
            is_active: true,
            viewer_count: viewer_count.load(Ordering::Relaxed),
            connection_count: connected_ips.len() as u32,
            fps_actual: media_snapshot.fps_actual,
            bitrate_kbps: media_snapshot.bitrate_kbps,
            uptime_secs: start_time.elapsed().as_secs(),
            server_url: server_url.clone(),
            all_urls: all_urls.clone(),
            connected_ips,
            capture_paused: capture_paused.load(Ordering::Relaxed),
            capture_issue: *capture_issue.lock().unwrap(),
            interaction_connected_count: interaction.client_count() as u32,
            annotation_count: interaction_document.shapes.len() as u32,
            view_mode: interaction_document.mode,
            source_epoch: interaction_document.source_epoch,
            latest_frame_id: latest_frame.as_ref().map(|frame| frame.frame_id),
            frame_width: latest_frame.as_ref().map(|frame| frame.width),
            frame_height: latest_frame.as_ref().map(|frame| frame.height),
            transport: *transport.lock().unwrap(),
            h264_media: runtime_handle
                .h264_media
                .lock()
                .unwrap()
                .as_ref()
                .map(|media| media.metrics())
                .unwrap_or_default(),
            control_state: control.state,
            controller_ip: control.controller_ip,
            pending_control_request: interaction.pending_control_request(),
            desktop_overlay_active: runtime_handle
                .desktop_overlay_active
                .load(Ordering::Relaxed),
            media_metrics: media_snapshot,
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

fn preview_token_matches(
    headers: &HeaderMap,
    query_token: Option<&str>,
    expected_token: &Arc<Mutex<Option<String>>>,
) -> bool {
    let Some(expected) = expected_token.lock().ok().and_then(|token| token.clone()) else {
        return false;
    };
    if query_token.is_some_and(|token| token == expected) {
        return true;
    }
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .map(|cookies| {
            cookies
                .split(';')
                .any(|cookie| cookie.trim().strip_prefix("ss_preview=") == Some(expected.as_str()))
        })
        .unwrap_or(false)
}

fn build_screen_share_access_urls(
    lan_ips: &[String],
    bind_address: Option<&str>,
    port: u16,
) -> ScreenShareAccessUrls {
    let bind_address = bind_address
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("0.0.0.0");

    let display_ips: Vec<String> = if bind_address == "0.0.0.0" {
        if lan_ips.is_empty() {
            vec!["127.0.0.1".to_string()]
        } else {
            lan_ips.to_vec()
        }
    } else {
        vec![bind_address.to_string()]
    };

    let all_urls: Vec<String> = display_ips
        .iter()
        .map(|ip| format!("http://{}:{}", ip, port))
        .collect();
    let server_url = all_urls
        .first()
        .cloned()
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", port));

    ScreenShareAccessUrls {
        server_url,
        all_urls,
    }
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

#[cfg(test)]
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
.capture-alert{display:none;position:absolute;top:12px;left:50%;transform:translateX(-50%);z-index:6;align-items:flex-start;gap:8px;max-width:min(92vw,720px);background:rgba(245,158,11,.15);border:1px solid rgba(245,158,11,.35);color:#fbbf24;padding:9px 16px;border-radius:10px;font-size:13px;font-weight:500;line-height:1.45;backdrop-filter:blur(6px)}
.capture-alert.privacy{background:rgba(239,68,68,.16);border-color:rgba(248,113,113,.45);color:#fecaca}
.capture-alert.privacy .dot{background:#ef4444;box-shadow:0 0 6px #ef444460}
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
    <div id="captureRetry" class="capture-alert" role="status" aria-live="polite">
      <span class="dot dot-retry"></span><span id="captureRetryText"></span>
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
  serverRetrying:isZh?'画面中断，服务端自动重试中':'Capture interrupted — server is retrying',
  privacyMode:isZh?'检测到所有屏幕持续黑屏。请关闭远程控制软件的隐私模式，或保持显示器处于逻辑开启状态；服务端会继续自动检测。':'All screens remain black. Disable privacy mode in the remote-control app or keep the display logically enabled; the server will keep checking automatically.',
  pause:isZh?'暂停':'Pause',
  resume:isZh?'继续':'Resume',
  refresh:isZh?'刷新率':'Refresh',
  original:isZh?'原始':'Original',
  fullscreen:isZh?'全屏':'Fullscreen',
  viewer:isZh?'位观看者':'viewer',viewers:isZh?'位观看者':'viewers',
};
// Apply i18n to static elements
document.getElementById('pausedText').textContent=T.paused;
document.getElementById('captureRetryText').textContent=T.serverRetrying;
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
      // 会话纪元变化 = 服务端重启过共享（旧流已死但 TCP 可能还挂着）→ 主动重连
      if(typeof d.session_id!=='undefined'){
        if(window.__ssSession!==undefined&&window.__ssSession!==d.session_id&&!paused&&d.active){
          window.__ssSession=d.session_id;
          holdCurrentFrame();
          tryReconnect();
        } else {
          window.__ssSession=d.session_id;
        }
      }
      // 服务端采集暂停（锁屏等）→ 显示重试提示条；恢复后自动隐藏
      const captureRetryEl=document.getElementById('captureRetry');
      if(captureRetryEl){
        const privacy=d.capture_issue==='privacy_mode_or_display_off';
        captureRetryEl.classList.toggle('privacy',privacy);
        document.getElementById('captureRetryText').textContent=privacy?T.privacyMode:T.serverRetrying;
        captureRetryEl.style.display=(d.capture_paused&&!paused)?'flex':'none';
      }
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
    use axum::body::to_bytes;
    use axum::http::Request;
    use futures_lite::StreamExt;
    use tower::ServiceExt;

    #[derive(Default)]
    struct TestScreenShareEvents;

    impl ScreenShareEventSink for TestScreenShareEvents {
        fn emit_tool_log(&self, _message: &str, _level: &str) {}

        fn emit_control_request(&self, _request: ControlRequestInfo) {}
    }

    #[test]
    fn media_metrics_keep_bounded_jpeg_samples_and_report_percentiles() {
        let started_at = Instant::now();
        let metrics = ScreenShareMediaMetrics::new_at(started_at);

        for index in 1..=(MEDIA_JPEG_SAMPLE_WINDOW + 40) {
            metrics.record_encoded_frame_at(
                index as usize,
                started_at + Duration::from_millis(index as u64),
            );
        }

        let snapshot = metrics.snapshot_at(None, 10_000);
        assert_eq!(snapshot.encoded_frame_count, 552);
        assert_eq!(snapshot.jpeg_sample_count, MEDIA_JPEG_SAMPLE_WINDOW as u32);
        assert_eq!(snapshot.jpeg_size_avg_bytes, 297);
        assert_eq!(snapshot.jpeg_size_p50_bytes, 296);
        assert_eq!(snapshot.jpeg_size_p95_bytes, 527);
        assert_eq!(snapshot.first_frame_delay_ms, Some(1));
    }

    #[test]
    fn mjpeg_viewers_skip_queued_frames_and_send_the_latest() {
        let (sender, _) = broadcast::channel(8);
        let mut receiver = sender.subscribe();
        let first = Arc::new(Bytes::from_static(b"first"));
        let second = Arc::new(Bytes::from_static(b"second"));
        let latest = Arc::new(Bytes::from_static(b"latest"));
        sender.send(first.clone()).unwrap();
        sender.send(second).unwrap();
        sender.send(latest.clone()).unwrap();

        let initial = receiver.try_recv().unwrap();
        let (drained, skipped) = drain_to_latest_mjpeg_frame(&mut receiver, initial);
        assert_eq!(drained.as_ref(), latest.as_ref());
        assert_eq!(skipped, 2);
    }

    #[test]
    fn media_metrics_report_stream_first_frame_reconnect_age_and_lag() {
        let started_at = Instant::now();
        let metrics = ScreenShareMediaMetrics::new_at(started_at);
        metrics.record_stream_open(false);
        metrics.record_stream_first_frame(Duration::from_millis(80), false);
        metrics.record_stream_open(true);
        metrics.record_stream_first_frame(Duration::from_millis(140), true);
        metrics.record_lagged_frames(7);
        metrics.update_rates(15, 8_192);

        let snapshot = metrics.snapshot_at(Some(9_950), 10_000);
        assert_eq!(snapshot.frame_age_ms, Some(50));
        assert_eq!(snapshot.slow_client_dropped_frames, 7);
        assert_eq!(snapshot.stream_connection_count, 2);
        assert_eq!(snapshot.stream_first_frame_avg_ms, Some(80));
        assert_eq!(snapshot.stream_first_frame_p95_ms, Some(80));
        assert_eq!(snapshot.stream_reconnect_count, 1);
        assert_eq!(snapshot.stream_reconnect_avg_ms, Some(140));
        assert_eq!(snapshot.stream_reconnect_p95_ms, Some(140));
        assert_eq!(snapshot.fps_actual, 15.0);
        assert_eq!(snapshot.bitrate_kbps, 8_192);
    }

    fn test_http_state() -> Arc<HttpServerState> {
        let (broadcast_tx, _) = broadcast::channel(8);
        Arc::new(HttpServerState {
            events: Arc::new(TestScreenShareEvents),
            broadcast_tx,
            interaction: InteractionState::new(77),
            viewer_count: Arc::new(AtomicU32::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            auth_hash: None,
            auth_username: None,
            bytes_sent: Arc::new(AtomicU64::new(0)),
            media_metrics: Arc::new(ScreenShareMediaMetrics::new()),
            h264_media: Arc::new(H264MediaState::new()),
            viewer_ips: Arc::new(Mutex::new(HashMap::new())),
            session_id: 77,
            capture_paused: Arc::new(AtomicBool::new(false)),
            capture_issue: Arc::new(Mutex::new(None)),
            preview_token: Arc::new(Mutex::new(None)),
            transport: Arc::new(Mutex::new(ScreenShareMediaTransport::Mjpeg)),
            input_worker: None,
        })
    }

    fn http_request(path: &str) -> Request<Body> {
        let mut request = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("test request");
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:41234"
                .parse::<SocketAddr>()
                .expect("test socket address"),
        ));
        request
    }

    #[tokio::test]
    async fn screen_share_router_serves_every_asset_referenced_by_index() {
        let app = screen_share_router(test_http_state());
        let index = app
            .clone()
            .oneshot(http_request("/"))
            .await
            .expect("index response");
        assert_eq!(index.status(), StatusCode::OK);
        let html = String::from_utf8(
            to_bytes(index.into_body(), usize::MAX)
                .await
                .expect("index body")
                .to_vec(),
        )
        .expect("utf-8 index");
        let asset_paths: Vec<String> = regex::Regex::new(r#"(?:src|href)=\"(/assets/[^\"]+)\""#)
            .unwrap()
            .captures_iter(&html)
            .map(|capture| capture[1].to_string())
            .collect();
        assert!(!asset_paths.is_empty(), "built index must reference assets");

        for path in asset_paths {
            let response = app
                .clone()
                .oneshot(http_request(&path))
                .await
                .expect("asset response");
            assert_eq!(response.status(), StatusCode::OK, "asset {path}");
        }
    }

    #[tokio::test]
    async fn screen_share_router_snapshot_status_stream_and_websocket_paths_are_isolated() {
        let state = test_http_state();
        let app = screen_share_router(state.clone());

        let missing_snapshot = app
            .clone()
            .oneshot(http_request("/snapshot/1"))
            .await
            .unwrap();
        assert_eq!(missing_snapshot.status(), StatusCode::NOT_FOUND);

        state
            .interaction
            .register_client("freezer")
            .expect("register freezer");
        state
            .interaction
            .record_frame(Arc::new(Bytes::from_static(b"jpeg-frame")));
        state
            .interaction
            .process(
                "freezer",
                ClientEnvelope {
                    v: screenshare_interaction::PROTOCOL_VERSION,
                    message_type: "view.freeze".to_string(),
                    session_id: 77,
                    source_epoch: 1,
                    client_seq: Some(1),
                    revision: None,
                    payload: None,
                },
            )
            .expect("freeze request");

        let snapshot = app
            .clone()
            .oneshot(http_request("/snapshot/1"))
            .await
            .unwrap();
        assert_eq!(snapshot.status(), StatusCode::OK);
        assert_eq!(
            to_bytes(snapshot.into_body(), usize::MAX).await.unwrap(),
            Bytes::from_static(b"jpeg-frame")
        );

        let single_frame = app
            .clone()
            .oneshot(http_request("/stream?single=1"))
            .await
            .unwrap();
        assert_eq!(single_frame.status(), StatusCode::OK);
        assert_eq!(single_frame.headers()["content-type"], "image/jpeg");

        let status = app.clone().oneshot(http_request("/status")).await.unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let status_json: serde_json::Value = serde_json::from_slice(
            &to_bytes(status.into_body(), usize::MAX)
                .await
                .expect("status body"),
        )
        .expect("status JSON");
        assert_eq!(status_json["session_id"], 77);
        assert_eq!(status_json["source_epoch"], 1);
        assert_eq!(status_json["latest_frame_id"], 1);
        assert_eq!(status_json["frozen_frame_id"], 1);
        assert_eq!(status_json["view_mode"], "frozen");
        assert_eq!(status_json["transport"], "mjpeg");
        assert_eq!(status_json["h264_media"]["ready"], false);

        let media_without_encoder = app
            .clone()
            .oneshot(http_request("/media/ws"))
            .await
            .unwrap();
        assert_eq!(
            media_without_encoder.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        let websocket_without_upgrade = app
            .clone()
            .oneshot(http_request("/session/ws"))
            .await
            .unwrap();
        assert_ne!(websocket_without_upgrade.status(), StatusCode::NOT_FOUND);

        let unknown = app.oneshot(http_request("/not-a-route")).await.unwrap();
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn long_lived_mjpeg_stream_sends_cached_frame_without_waiting_for_broadcast() {
        let state = test_http_state();
        state
            .interaction
            .record_frame(Arc::new(Bytes::from_static(b"cached-jpeg")));
        let app = screen_share_router(state.clone());

        let response = app.oneshot(http_request("/stream")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body().into_data_stream();
        let first_chunk = tokio::time::timeout(Duration::from_millis(100), body.next())
            .await
            .expect("cached frame should be immediate")
            .expect("stream should yield a chunk")
            .expect("chunk should be readable");
        assert!(first_chunk
            .windows(b"cached-jpeg".len())
            .any(|window| window == b"cached-jpeg"));

        let metrics = state.media_metrics.snapshot(None);
        assert_eq!(metrics.stream_connection_count, 1);
        assert_eq!(metrics.stream_first_frame_sample_count, 1);

        let reconnect_response = screen_share_router(state.clone())
            .oneshot(http_request("/stream?reconnect=1"))
            .await
            .unwrap();
        let mut reconnect_body = reconnect_response.into_body().into_data_stream();
        tokio::time::timeout(Duration::from_millis(100), reconnect_body.next())
            .await
            .expect("reconnect cached frame should be immediate")
            .expect("reconnect stream should yield a chunk")
            .expect("reconnect chunk should be readable");
        let metrics = state.media_metrics.snapshot(None);
        assert_eq!(metrics.stream_connection_count, 2);
        assert_eq!(metrics.stream_reconnect_count, 1);
        assert_eq!(metrics.stream_reconnect_sample_count, 1);
    }

    #[test]
    fn screen_share_embedded_pages_include_svg_favicon() {
        let viewer = viewer_html();
        let login = login_html(false, false);

        for html in [&viewer, &login] {
            assert!(html.contains(r#"<link rel="icon" type="image/svg+xml""#));
            assert!(html.contains("data:image/svg+xml"));
        }
        assert!(viewer.contains("privacy_mode_or_display_off"));
        assert!(viewer.contains("检测到所有屏幕持续黑屏"));
    }

    #[test]
    fn screen_share_asset_route_restores_embedded_assets_prefix() {
        assert_eq!(screen_share_asset_path("index.js"), "assets/index.js");
        assert_eq!(screen_share_asset_path("/index.css"), "assets/index.css");
    }

    #[test]
    fn legacy_config_defaults_preserve_existing_viewing_behavior() {
        let config: ScreenShareConfig = serde_json::from_value(serde_json::json!({
            "port": 9870,
            "username": null,
            "password": null,
            "monitor_index": 0,
            "quality": 70,
            "fps": 15,
            "show_cursor": true
        }))
        .expect("legacy screen-share config");

        assert!(config.annotations_enabled);
        assert!(config.shared_freeze_enabled);
        assert!(!config.control_requests_enabled);
        assert!(!config.keyboard_control_enabled);
        assert_eq!(config.transport, ScreenShareMediaTransport::Auto);
    }

    #[test]
    fn h264_selector_preserves_explicit_mjpeg_and_webrtc_fallback() {
        assert!(ScreenShareMediaTransport::Auto.wants_h264());
        assert!(ScreenShareMediaTransport::MseH264.wants_h264());
        assert!(!ScreenShareMediaTransport::Mjpeg.wants_h264());
        assert!(!ScreenShareMediaTransport::WebRtc.wants_h264());
        assert_eq!(ScreenShareMediaTransport::Mjpeg.resolved_label(), "mjpeg");
    }

    #[test]
    fn desktop_overlay_identity_and_annotation_events_are_session_scoped() {
        assert_eq!(
            desktop_overlay_window_label(42),
            "screen-share-desktop-overlay-42"
        );
        assert!(interaction_event_updates_annotations("annotation.applied"));
        assert!(interaction_event_updates_annotations("view.state"));
        assert!(interaction_event_updates_annotations("source.changed"));
        assert!(!interaction_event_updates_annotations("control.state"));
        assert!(!interaction_event_updates_annotations("input.pointer_move"));
    }

    #[test]
    fn access_urls_respect_specific_bind_address() {
        let ips = vec![
            "192.168.1.15".to_string(),
            "192.168.2.15".to_string(),
            "10.222.88.140".to_string(),
        ];

        let urls = build_screen_share_access_urls(&ips, Some("192.168.2.15"), 9870);

        assert_eq!(urls.server_url, "http://192.168.2.15:9870");
        assert_eq!(urls.all_urls, vec!["http://192.168.2.15:9870"]);
    }

    #[test]
    fn access_urls_keep_all_lan_ips_when_binding_all_interfaces() {
        let ips = vec!["192.168.1.15".to_string(), "10.222.88.140".to_string()];

        let urls = build_screen_share_access_urls(&ips, Some("0.0.0.0"), 9870);

        assert_eq!(urls.server_url, "http://192.168.1.15:9870");
        assert_eq!(
            urls.all_urls,
            vec!["http://192.168.1.15:9870", "http://10.222.88.140:9870"]
        );
    }

    #[test]
    fn host_preview_capability_accepts_only_matching_query_or_cookie() {
        let token = Arc::new(Mutex::new(Some("preview-token".to_string())));
        let mut headers = HeaderMap::new();
        assert!(preview_token_matches(
            &headers,
            Some("preview-token"),
            &token
        ));
        assert!(!preview_token_matches(&headers, Some("wrong"), &token));

        headers.insert(
            "cookie",
            "other=1; ss_preview=preview-token".parse().unwrap(),
        );
        assert!(preview_token_matches(&headers, None, &token));
        *token.lock().unwrap() = None;
        assert!(!preview_token_matches(&headers, None, &token));
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
        *handle.server_done_rx.lock().unwrap() = Some(oneshot::channel::<()>().1);
        *handle.interaction.lock().unwrap() = Some(InteractionState::new(99));
        *handle.media_metrics.lock().unwrap() = Some(Arc::new(ScreenShareMediaMetrics::new()));
        *handle.preview_token.lock().unwrap() = Some("stale-preview".into());
        handle.desktop_overlay_active.store(true, Ordering::SeqCst);
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
        assert!(handle.server_done_rx.lock().unwrap().is_none());
        assert!(handle.interaction.lock().unwrap().is_none());
        assert!(handle.media_metrics.lock().unwrap().is_none());
        assert!(handle.preview_token.lock().unwrap().is_none());
        assert!(!handle.desktop_overlay_active.load(Ordering::SeqCst));
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
        *handle.server_done_rx.lock().unwrap() = Some(oneshot::channel::<()>().1);
        *handle.interaction.lock().unwrap() = Some(InteractionState::new(100));
        *handle.media_metrics.lock().unwrap() = Some(Arc::new(ScreenShareMediaMetrics::new()));
        *handle.preview_token.lock().unwrap() = Some("active-preview".into());
        handle.desktop_overlay_active.store(true, Ordering::SeqCst);
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
        assert!(handle.server_done_rx.lock().unwrap().is_none());
        assert!(handle.interaction.lock().unwrap().is_none());
        assert!(handle.media_metrics.lock().unwrap().is_none());
        assert!(handle.preview_token.lock().unwrap().is_none());
        assert!(!handle.desktop_overlay_active.load(Ordering::SeqCst));
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
    fn describe_input_desktop_reports_a_desktop_state() {
        let described = describe_input_desktop();
        assert!(described.starts_with("input_desktop="), "got: {described}");
        assert!(described.len() > "input_desktop=".len(), "got: {described}");
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

    // ─── FrameStarvationWatchdog（锁屏解锁后采集静默死亡的判定） ───

    #[test]
    fn starvation_watchdog_stays_quiet_with_steady_frames() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        for s in 1..=60 {
            let now = t0 + Duration::from_secs(s);
            w.on_frame(now);
            let check_at = now + Duration::from_millis(100);
            assert!(!w.should_sample_desktop(check_at));
            let tick = w.tick(check_at, None);
            assert!(tick.force_recreate.is_none());
        }
    }

    #[test]
    fn starvation_watchdog_never_fires_on_static_screen_without_lock() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        w.on_frame(t0); // 有过真实帧，此后画面纯静止一小时
        for s in 1..=3600u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(false); // 桌面始终未锁定
            let tick = w.tick(now, sample);
            assert!(tick.force_recreate.is_none(), "fired at +{}s", s);
        }
    }

    #[test]
    fn starvation_watchdog_fires_when_frames_stay_silent_after_unlock() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        w.on_frame(t0);
        // 锁屏 60 秒：只允许等待，绝不重建
        let mut lock_seen = false;
        for s in 3..=60u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(true);
            let tick = w.tick(now, sample);
            lock_seen |= tick.lock_observed;
            assert!(
                tick.force_recreate.is_none(),
                "must wait during lock, fired at +{}s",
                s
            );
        }
        assert!(lock_seen);
        // 解锁后帧仍未恢复 → 宽限期后强制重建
        let mut unlock_seen = false;
        let mut fired = None;
        for s in 61..=70u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(false);
            let tick = w.tick(now, sample);
            unlock_seen |= tick.unlock_observed;
            if let Some(verdict) = tick.force_recreate {
                fired = Some((s, verdict));
                break;
            }
        }
        assert!(unlock_seen);
        let (fired_s, verdict) = fired.expect("watchdog must force recreate after unlock silence");
        assert!(fired_s >= 61 + STARVATION_POST_UNLOCK_GRACE.as_secs());
        assert!(matches!(
            verdict,
            StarvationVerdict::SilentAfterUnlock { .. }
        ));
    }

    #[test]
    fn starvation_watchdog_resets_when_frame_arrives_after_unlock() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        w.on_frame(t0);
        for s in 3..=30u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(true);
            let _ = w.tick(now, sample);
        }
        // 解锁 1 秒后画面恢复 → 看门狗复位，此后长时间静止画面不误报
        let _ = w.tick(t0 + Duration::from_secs(31), Some(false));
        w.on_frame(t0 + Duration::from_secs(32));
        for s in 33..=120u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(false);
            let tick = w.tick(now, sample);
            assert!(
                tick.force_recreate.is_none(),
                "fired at +{}s after recovery",
                s
            );
        }
    }

    #[test]
    fn starvation_watchdog_fires_when_rebuilt_source_never_delivers_first_frame() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        w.on_created(t0);
        let mut fired = None;
        for s in 1..=30u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(false);
            let tick = w.tick(now, sample);
            if let Some(verdict) = tick.force_recreate {
                fired = Some((s, verdict));
                break;
            }
        }
        let (fired_s, verdict) = fired.expect("zombie source must trigger recreate");
        assert!(fired_s >= STARVATION_FIRST_FRAME_TIMEOUT.as_secs());
        assert!(fired_s <= STARVATION_FIRST_FRAME_TIMEOUT.as_secs() + 1);
        assert!(matches!(
            verdict,
            StarvationVerdict::NoFrameSinceCreate { .. }
        ));
    }

    #[test]
    fn starvation_watchdog_waits_out_lock_before_first_frame_timeout() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        w.on_created(t0); // 锁屏期间重建成功但拿不到首帧（僵尸源）
        for s in 1..=120u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(true);
            let tick = w.tick(now, sample);
            assert!(
                tick.force_recreate.is_none(),
                "must not fire while locked, +{}s",
                s
            );
        }
        let mut fired = None;
        for s in 121..=140u64 {
            let now = t0 + Duration::from_secs(s);
            let sample = w.should_sample_desktop(now).then_some(false);
            let tick = w.tick(now, sample);
            if let Some(verdict) = tick.force_recreate {
                fired = Some((s, verdict));
                break;
            }
        }
        let (fired_s, verdict) = fired.expect("must fire after unlock + first-frame timeout");
        assert!(
            fired_s >= 120 + STARVATION_FIRST_FRAME_TIMEOUT.as_secs(),
            "fired too early at +{}s",
            fired_s
        );
        assert!(matches!(
            verdict,
            StarvationVerdict::NoFrameSinceCreate { .. }
        ));
    }

    #[test]
    fn starvation_watchdog_samples_desktop_at_one_second_cadence_only_when_starved() {
        let t0 = Instant::now();
        let mut w = FrameStarvationWatchdog::new(t0);
        w.on_frame(t0);
        // 未达饥饿阈值：不采样桌面状态
        assert!(!w.should_sample_desktop(t0 + Duration::from_secs(1)));
        // 饥饿后：至多 1 秒一次
        let now = t0 + Duration::from_secs(3);
        assert!(w.should_sample_desktop(now));
        let _ = w.tick(now, Some(false));
        assert!(!w.should_sample_desktop(now + Duration::from_millis(300)));
        assert!(w.should_sample_desktop(now + Duration::from_secs(1)));
    }

    #[test]
    fn black_frame_watchdog_forces_recreate_after_recovery_black_frames() {
        let t0 = Instant::now();
        let mut watchdog = BlackFrameRecoveryWatchdog::new();
        let content = solid_bgra_frame(16, 16, [64, 80, 96, 255]);
        let black = solid_bgra_frame(16, 16, [0, 0, 0, 255]);

        assert_eq!(
            watchdog.observe_frame(t0, &content, 16, 16, 16 * 4, false, None),
            BlackFrameDecision::Accept
        );

        watchdog.arm_for_recovery(t0 + Duration::from_secs(1));

        assert_eq!(
            watchdog.observe_frame(
                t0 + Duration::from_secs(1) + BLACK_FRAME_RECREATE_AFTER / 2,
                &black,
                16,
                16,
                16 * 4,
                true,
                Some(true),
            ),
            BlackFrameDecision::Suppress
        );

        assert!(matches!(
            watchdog.observe_frame(
                t0 + Duration::from_secs(1) + BLACK_FRAME_RECREATE_AFTER,
                &black,
                16,
                16,
                16 * 4,
                true,
                Some(true),
            ),
            BlackFrameDecision::ForceRecreate { .. }
        ));
    }

    #[test]
    fn black_frame_watchdog_allows_initial_black_desktop() {
        let t0 = Instant::now();
        let mut watchdog = BlackFrameRecoveryWatchdog::new();
        let black = solid_bgra_frame(16, 16, [0, 0, 0, 255]);

        assert_eq!(
            watchdog.observe_frame(
                t0 + BLACK_FRAME_RECREATE_AFTER * 2,
                &black,
                16,
                16,
                16 * 4,
                false,
                Some(true),
            ),
            BlackFrameDecision::Accept
        );
    }

    #[test]
    fn black_frame_watchdog_rejects_initial_black_when_capture_validation_is_armed() {
        let t0 = Instant::now();
        let mut watchdog = BlackFrameRecoveryWatchdog::new();
        let black = solid_bgra_frame(16, 16, [0, 0, 0, 255]);
        watchdog.arm_for_recovery(t0);

        assert_eq!(
            watchdog.observe_frame(
                t0 + BLACK_FRAME_RECREATE_AFTER / 2,
                &black,
                16,
                16,
                16 * 4,
                false,
                Some(true),
            ),
            BlackFrameDecision::Suppress
        );
        assert!(matches!(
            watchdog.observe_frame(
                t0 + BLACK_FRAME_RECREATE_AFTER,
                &black,
                16,
                16,
                16 * 4,
                false,
                Some(true),
            ),
            BlackFrameDecision::ForceRecreate { .. }
        ));
    }

    #[test]
    fn black_recovery_candidates_try_alternate_backend_then_other_monitors() {
        let candidates = build_black_recovery_candidates(3, 1, CaptureBackendKind::Wgc)
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(
            candidates,
            vec![
                CaptureCandidate {
                    monitor_index: 1,
                    backend: CaptureBackendKind::Dxgi,
                },
                CaptureCandidate {
                    monitor_index: 0,
                    backend: CaptureBackendKind::Wgc,
                },
                CaptureCandidate {
                    monitor_index: 0,
                    backend: CaptureBackendKind::Dxgi,
                },
                CaptureCandidate {
                    monitor_index: 2,
                    backend: CaptureBackendKind::Wgc,
                },
                CaptureCandidate {
                    monitor_index: 2,
                    backend: CaptureBackendKind::Dxgi,
                },
            ]
        );
    }

    fn solid_bgra_frame(width: usize, height: usize, pixel: [u8; 4]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(width * height * 4);
        for _ in 0..width * height {
            frame.extend_from_slice(&pixel);
        }
        frame
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wgc_null_frame_probe_error_is_recognized_as_no_frame() {
        assert!(is_wgc_no_frame_error(&WindowsError::from(HRESULT(0))));
        assert!(!is_wgc_no_frame_error(&WindowsError::from(HRESULT(
            0x80004005u32 as i32
        ))));
    }
}
