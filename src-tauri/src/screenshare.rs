#![allow(clippy::too_many_arguments)]

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::future::Future;
use std::io;
#[cfg(target_os = "windows")]
use std::mem;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use crate::screenshare_gpu::{
    GpuFallbackCode, GpuFallbackReason, GpuNv12Surface, GpuPreprocessConfig, GpuVideoPreprocessor,
};
use crate::screenshare_input::{
    parse_input_event, source_rect_for_monitor, work_rect_for_monitor, AppliedInputSnapshot,
    InputContext, InputEvent, InputMetricsSnapshot, InputWorkerHandle, QueuePushOutcome,
    QueuedInput, ScreenRect,
};
use crate::screenshare_media::{
    H264EncoderWorker, H264MediaEvent, H264MediaMetricsSnapshot, H264MediaSegment, H264MediaState,
    H264StreamDescriptor,
};
use crate::screenshare_web_assets;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State as AxumState};
use axum::http::{header::USER_AGENT, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{body::Body, Form, Json, Router};
use bytes::{Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use scrap::{Capturer, Display, Frame};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Size, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tokio::sync::{broadcast, oneshot, Notify};
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
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Resource,
    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
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
const WEBCODECS_AU_HEADER_BYTES: usize = 40;
const WEBCODECS_MAX_ACCESS_UNIT_BYTES: usize = 32 * 1024 * 1024;
#[cfg(feature = "screen-share-webrtc-prototype")]
const WEBRTC_SIGNALING_MAX_BYTES: usize = 256 * 1024;
const DESKTOP_OVERLAY_WINDOW_LABEL_PREFIX: &str = "screen-share-desktop-overlay";
const ANNOTATION_BAR_WINDOW_LABEL_PREFIX: &str = "screen-share-annotation-bar";
/// Logical size of the host annotation action bar. It is fixed so the Rust side
/// can place it without waiting for the page to report a measured size.
const ANNOTATION_BAR_LOGICAL_WIDTH: f64 = 360.0;
const ANNOTATION_BAR_LOGICAL_HEIGHT: f64 = 60.0;
const ANNOTATION_BAR_LOGICAL_MARGIN: f64 = 16.0;

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
/// MJPEG is a compatibility fallback, not a second full-rate encode pipeline.
/// Ten frames per second keeps recovery usable while bounding CPU and bandwidth.
const MJPEG_FALLBACK_FRAME_INTERVAL: Duration = Duration::from_millis(100);

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
const MEDIA_PIPELINE_SAMPLE_WINDOW: usize = 1024;
const MEDIA_SAMPLE_MEASUREMENT_SCOPE: &str = "rolling_last_n_samples";
const H264_STREAM_SEND_TIMEOUT: Duration = Duration::from_secs(1);
const MJPEG_STREAM_SEND_TIMEOUT: Duration = Duration::from_secs(1);
const MJPEG_BODY_CHANNEL_CAPACITY: usize = 1;
const MAX_MEDIA_VIEWERS: u32 = 40;

#[derive(Debug, Clone, Copy)]
struct ViewerIpEntry {
    active_media_connections: u32,
}

type ViewerIpMap = HashMap<String, ViewerIpEntry>;

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
}

fn default_true() -> bool {
    true
}

/// 采集帧率下限。
const MIN_CAPTURE_FPS: u8 = 1;
/// 采集帧率上限，同时是界面 60 FPS 实验档的取值。30 FPS 及以下是默认档位；
/// 60 FPS 必须与 30 FPS 分别采集 capture-to-display、资源和扇出数据后才能决定
/// 默认值，因此这里只放开取值范围，不改变界面默认值。
const MAX_CAPTURE_FPS: u8 = 60;

fn validate_capture_fps(fps: u8) -> Result<(), String> {
    if fps < MIN_CAPTURE_FPS || fps > MAX_CAPTURE_FPS {
        return Err(format!("FPS must be {MIN_CAPTURE_FPS}-{MAX_CAPTURE_FPS}"));
    }
    Ok(())
}

/// 采集节拍。使用微秒而不是毫秒，避免 1000/fps 的整数除法把 30 FPS 抬到
/// 30.3 FPS、把 60 FPS 抬到 62.5 FPS，从而污染两档的对比数据。
fn capture_frame_interval(fps: u8) -> Duration {
    let fps = u64::from(fps.clamp(MIN_CAPTURE_FPS, MAX_CAPTURE_FPS));
    Duration::from_micros(1_000_000 / fps)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareMediaTransport {
    Auto,
    Mjpeg,
    MseH264,
    WebCodecs,
    WebRtc,
}

impl Default for ScreenShareMediaTransport {
    fn default() -> Self {
        Self::Auto
    }
}

impl ScreenShareMediaTransport {
    fn wants_h264(self) -> bool {
        matches!(
            self,
            Self::Auto | Self::MseH264 | Self::WebCodecs | Self::WebRtc
        )
    }

    fn resolved_label(self) -> &'static str {
        match self {
            Self::Auto | Self::Mjpeg => "mjpeg",
            Self::MseH264 => "mse_h264",
            Self::WebCodecs => "web_codecs",
            Self::WebRtc => "web_rtc",
        }
    }

    fn resolved_h264_transport(self) -> Self {
        match self {
            Self::Auto => Self::MseH264,
            other => other,
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
    /// Number of active media leases represented in the per-IP reference map.
    pub viewer_ip_reference_count: u32,
    /// Number of live media producer/socket tasks that still own a viewer lease.
    pub active_media_task_count: u32,
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
    pub input_metrics: Option<InputMetricsSnapshot>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ScreenShareMediaMetricsSnapshot {
    pub capture_frame_count: u64,
    pub mjpeg_encoded_frame_count: u64,
    pub mjpeg_encoded_bytes: u64,
    pub outbound_bytes_total: u64,
    pub capture_fps_actual: f32,
    pub mjpeg_fps_actual: f32,
    pub mjpeg_consumer_count: u32,
    pub jpeg_active: bool,
    pub jpeg_enable_count: u64,
    pub jpeg_disable_count: u64,
    pub jpeg_fallback_reason: Option<String>,
    pub latest_capture: Option<LatestCaptureMetadataSnapshot>,
    pub frame_wait: DurationMetricsSnapshot,
    pub capture_queue_age: DurationMetricsSnapshot,
    pub gpu_readback: DurationMetricsSnapshot,
    pub black_frame_classification: DurationMetricsSnapshot,
    pub jpeg_color_conversion: DurationMetricsSnapshot,
    pub jpeg_encode: DurationMetricsSnapshot,
    pub stream_send_wait: DurationMetricsSnapshot,
    pub outbound_100ms: ByteWindowMetricsSnapshot,
    pub outbound_1s: ByteWindowMetricsSnapshot,
    pub stream_send_timeout_count: u64,
    pub stream_disconnect_count: u64,
    /// Compatibility alias for `mjpeg_encoded_frame_count`.
    pub encoded_frame_count: u64,
    /// Compatibility alias for `jpeg_retained_sample_count`.
    pub jpeg_sample_count: u32,
    pub jpeg_total_sample_count: u64,
    pub jpeg_retained_sample_count: u32,
    pub jpeg_sample_window_capacity: u32,
    pub jpeg_measurement_scope: &'static str,
    pub jpeg_size_avg_bytes: u64,
    pub jpeg_size_p50_bytes: u64,
    pub jpeg_size_p95_bytes: u64,
    pub jpeg_size_p99_bytes: u64,
    pub jpeg_size_max_bytes: u64,
    pub first_frame_delay_ms: Option<u64>,
    pub frame_age_ms: Option<u64>,
    pub slow_client_dropped_frames: u64,
    pub stream_connection_count: u64,
    /// Compatibility alias for `stream_first_frame_retained_sample_count`.
    pub stream_first_frame_sample_count: u32,
    pub stream_first_frame_total_sample_count: u64,
    pub stream_first_frame_retained_sample_count: u32,
    pub stream_first_frame_sample_window_capacity: u32,
    pub stream_first_frame_measurement_scope: &'static str,
    pub stream_first_frame_avg_ms: Option<u64>,
    pub stream_first_frame_p50_ms: Option<u64>,
    pub stream_first_frame_p95_ms: Option<u64>,
    pub stream_first_frame_p99_ms: Option<u64>,
    pub stream_first_frame_max_ms: Option<u64>,
    pub stream_reconnect_count: u64,
    /// Compatibility alias for `stream_reconnect_retained_sample_count`.
    pub stream_reconnect_sample_count: u32,
    pub stream_reconnect_total_sample_count: u64,
    pub stream_reconnect_retained_sample_count: u32,
    pub stream_reconnect_sample_window_capacity: u32,
    pub stream_reconnect_measurement_scope: &'static str,
    pub stream_reconnect_avg_ms: Option<u64>,
    pub stream_reconnect_p50_ms: Option<u64>,
    pub stream_reconnect_p95_ms: Option<u64>,
    pub stream_reconnect_p99_ms: Option<u64>,
    pub stream_reconnect_max_ms: Option<u64>,
    pub fps_actual: f32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct DurationMetricsSnapshot {
    /// Compatibility alias for `retained_sample_count`.
    pub sample_count: u32,
    pub total_sample_count: u64,
    pub retained_sample_count: u32,
    pub sample_window_capacity: u32,
    pub measurement_scope: &'static str,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct ByteWindowMetricsSnapshot {
    /// Compatibility alias for `retained_sample_count`.
    pub sample_count: u32,
    pub total_sample_count: u64,
    pub retained_sample_count: u32,
    pub sample_window_capacity: u32,
    pub measurement_scope: &'static str,
    /// Compatibility alias for `retained_total_bytes`.
    pub total_bytes: u64,
    pub retained_total_bytes: u64,
    pub p50_bytes: u64,
    pub p95_bytes: u64,
    pub p99_bytes: u64,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LatestCaptureMetadataSnapshot {
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub system_relative_time_100ns: Option<i64>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Default)]
struct ScreenShareMediaSamples {
    jpeg_sizes: VecDeque<u32>,
    jpeg_sizes_total: u64,
    first_frame_ms: VecDeque<u64>,
    first_frame_ms_total: u64,
    reconnect_ms: VecDeque<u64>,
    reconnect_ms_total: u64,
    frame_wait_us: VecDeque<u64>,
    frame_wait_us_total: u64,
    capture_queue_age_us: VecDeque<u64>,
    capture_queue_age_us_total: u64,
    gpu_readback_us: VecDeque<u64>,
    gpu_readback_us_total: u64,
    black_frame_classification_us: VecDeque<u64>,
    black_frame_classification_us_total: u64,
    jpeg_color_conversion_us: VecDeque<u64>,
    jpeg_color_conversion_us_total: u64,
    jpeg_encode_us: VecDeque<u64>,
    jpeg_encode_us_total: u64,
    stream_send_wait_us: VecDeque<u64>,
    stream_send_wait_us_total: u64,
    outbound_100ms_bytes: VecDeque<u64>,
    outbound_100ms_bytes_total: u64,
    outbound_1s_bytes: VecDeque<u64>,
    outbound_1s_bytes_total: u64,
}

#[derive(Debug)]
struct ScreenShareMediaMetrics {
    started_at: Instant,
    capture_frame_count: AtomicU64,
    capture_interval_frames: AtomicU32,
    mjpeg_interval_frames: AtomicU32,
    mjpeg_encoded_bytes: AtomicU64,
    outbound_bytes_total: AtomicU64,
    latest_capture_sequence: AtomicU64,
    latest_capture: Mutex<Option<LatestCaptureMetadataSnapshot>>,
    encoded_frame_count: AtomicU64,
    first_frame_delay_ms: AtomicU64,
    slow_client_dropped_frames: AtomicU64,
    stream_connection_count: AtomicU64,
    stream_reconnect_count: AtomicU64,
    stream_send_timeout_count: AtomicU64,
    stream_disconnect_count: AtomicU64,
    fps_actual: AtomicU32,
    mjpeg_fps_actual: AtomicU32,
    bitrate_kbps: AtomicU32,
    mjpeg_consumer_count: AtomicU32,
    jpeg_state_initialized: AtomicBool,
    jpeg_active: AtomicBool,
    jpeg_enable_count: AtomicU64,
    jpeg_disable_count: AtomicU64,
    jpeg_fallback_reason: Mutex<Option<String>>,
    samples: Mutex<ScreenShareMediaSamples>,
}

impl ScreenShareMediaMetrics {
    fn new() -> Self {
        Self::new_at(Instant::now())
    }

    fn new_at(started_at: Instant) -> Self {
        Self {
            started_at,
            capture_frame_count: AtomicU64::new(0),
            capture_interval_frames: AtomicU32::new(0),
            mjpeg_interval_frames: AtomicU32::new(0),
            mjpeg_encoded_bytes: AtomicU64::new(0),
            outbound_bytes_total: AtomicU64::new(0),
            latest_capture_sequence: AtomicU64::new(0),
            latest_capture: Mutex::new(None),
            encoded_frame_count: AtomicU64::new(0),
            first_frame_delay_ms: AtomicU64::new(u64::MAX),
            slow_client_dropped_frames: AtomicU64::new(0),
            stream_connection_count: AtomicU64::new(0),
            stream_reconnect_count: AtomicU64::new(0),
            stream_send_timeout_count: AtomicU64::new(0),
            stream_disconnect_count: AtomicU64::new(0),
            fps_actual: AtomicU32::new(0),
            mjpeg_fps_actual: AtomicU32::new(0),
            bitrate_kbps: AtomicU32::new(0),
            mjpeg_consumer_count: AtomicU32::new(0),
            jpeg_state_initialized: AtomicBool::new(false),
            jpeg_active: AtomicBool::new(false),
            jpeg_enable_count: AtomicU64::new(0),
            jpeg_disable_count: AtomicU64::new(0),
            jpeg_fallback_reason: Mutex::new(None),
            samples: Mutex::new(ScreenShareMediaSamples::default()),
        }
    }

    fn record_encoded_frame(&self, jpeg_size: usize) {
        self.record_encoded_frame_at(jpeg_size, Instant::now());
    }

    fn record_encoded_frame_at(&self, jpeg_size: usize, captured_at: Instant) {
        self.encoded_frame_count.fetch_add(1, Ordering::Relaxed);
        self.mjpeg_interval_frames.fetch_add(1, Ordering::Relaxed);
        self.mjpeg_encoded_bytes
            .fetch_add(jpeg_size as u64, Ordering::Relaxed);
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
            samples.jpeg_sizes_total = samples.jpeg_sizes_total.saturating_add(1);
            push_bounded(
                &mut samples.jpeg_sizes,
                jpeg_size.min(u32::MAX as usize) as u32,
                MEDIA_JPEG_SAMPLE_WINDOW,
            );
        }
    }

    fn record_capture_frame(
        &self,
        width: u32,
        height: u32,
        system_relative_time_100ns: Option<i64>,
        frame_wait: Duration,
        capture_queue_age: Option<Duration>,
        gpu_readback: Option<Duration>,
    ) -> (u64, u64) {
        let sequence = self
            .latest_capture_sequence
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.capture_frame_count.fetch_add(1, Ordering::Relaxed);
        self.capture_interval_frames.fetch_add(1, Ordering::Relaxed);
        let observed_at_ms = unix_time_ms();
        // WGC's SystemRelativeTime is monotonic rather than wall-clock time.
        // The measured delivery queue age is therefore the best wall-clock
        // approximation of when this frame was captured. DXGI has no source
        // timestamp, so its capture time is the observation time.
        let captured_at_unix_ms = observed_at_ms.saturating_sub(
            capture_queue_age
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );
        if let Ok(mut latest) = self.latest_capture.lock() {
            *latest = Some(LatestCaptureMetadataSnapshot {
                sequence,
                observed_at_ms,
                system_relative_time_100ns,
                width,
                height,
            });
        }
        let delay_ms = Instant::now()
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
            samples.frame_wait_us_total = samples.frame_wait_us_total.saturating_add(1);
            push_duration_sample(&mut samples.frame_wait_us, frame_wait);
            if let Some(duration) = capture_queue_age {
                samples.capture_queue_age_us_total =
                    samples.capture_queue_age_us_total.saturating_add(1);
                push_duration_sample(&mut samples.capture_queue_age_us, duration);
            }
            if let Some(duration) = gpu_readback {
                samples.gpu_readback_us_total = samples.gpu_readback_us_total.saturating_add(1);
                push_duration_sample(&mut samples.gpu_readback_us, duration);
            }
        }
        (sequence, captured_at_unix_ms)
    }

    fn record_black_frame_classification(&self, elapsed: Duration) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.black_frame_classification_us_total = samples
                .black_frame_classification_us_total
                .saturating_add(1);
            push_duration_sample(&mut samples.black_frame_classification_us, elapsed);
        }
    }

    fn record_jpeg_timings(&self, color_conversion: Duration, encode: Duration) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.jpeg_color_conversion_us_total =
                samples.jpeg_color_conversion_us_total.saturating_add(1);
            samples.jpeg_encode_us_total = samples.jpeg_encode_us_total.saturating_add(1);
            push_duration_sample(&mut samples.jpeg_color_conversion_us, color_conversion);
            push_duration_sample(&mut samples.jpeg_encode_us, encode);
        }
    }

    fn update_jpeg_state(&self, active: bool, consumer_count: u32, reason: &str) -> bool {
        self.mjpeg_consumer_count
            .store(consumer_count, Ordering::Relaxed);
        let previous = self.jpeg_active.swap(active, Ordering::Relaxed);
        let initialized = self.jpeg_state_initialized.swap(true, Ordering::Relaxed);
        let changed = !initialized || previous != active;
        if !initialized && active || initialized && !previous && active {
            self.jpeg_enable_count.fetch_add(1, Ordering::Relaxed);
        } else if initialized && previous && !active {
            self.jpeg_disable_count.fetch_add(1, Ordering::Relaxed);
        }
        if changed {
            if let Ok(mut current_reason) = self.jpeg_fallback_reason.lock() {
                *current_reason = Some(reason.to_owned());
            }
        }
        changed
    }

    fn record_stream_send(&self, elapsed: Duration, timed_out: bool) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.stream_send_wait_us_total = samples.stream_send_wait_us_total.saturating_add(1);
            push_duration_sample(&mut samples.stream_send_wait_us, elapsed);
        }
        if timed_out {
            self.stream_send_timeout_count
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_stream_disconnect(&self) {
        self.stream_disconnect_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_outbound_window(&self, window: Duration, bytes: u64) {
        if let Ok(mut samples) = self.samples.lock() {
            if window == Duration::from_millis(100) {
                samples.outbound_100ms_bytes_total =
                    samples.outbound_100ms_bytes_total.saturating_add(1);
                push_bounded(
                    &mut samples.outbound_100ms_bytes,
                    bytes,
                    MEDIA_PIPELINE_SAMPLE_WINDOW,
                );
            } else {
                samples.outbound_1s_bytes_total = samples.outbound_1s_bytes_total.saturating_add(1);
                push_bounded(
                    &mut samples.outbound_1s_bytes,
                    bytes,
                    MEDIA_PIPELINE_SAMPLE_WINDOW,
                );
            }
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
            if reconnect {
                samples.reconnect_ms_total = samples.reconnect_ms_total.saturating_add(1);
                push_bounded(
                    &mut samples.reconnect_ms,
                    elapsed_ms,
                    MEDIA_STREAM_SAMPLE_WINDOW,
                );
            } else {
                samples.first_frame_ms_total = samples.first_frame_ms_total.saturating_add(1);
                push_bounded(
                    &mut samples.first_frame_ms,
                    elapsed_ms,
                    MEDIA_STREAM_SAMPLE_WINDOW,
                );
            }
        }
    }

    fn record_lagged_frames(&self, skipped: u64) {
        self.slow_client_dropped_frames
            .fetch_add(skipped, Ordering::Relaxed);
    }

    fn update_rates(&self, fps_actual: u32, bitrate_kbps: u32, outbound_bytes_total: u64) {
        self.fps_actual.store(fps_actual, Ordering::Relaxed);
        self.mjpeg_fps_actual.store(
            self.mjpeg_interval_frames.swap(0, Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.bitrate_kbps.store(bitrate_kbps, Ordering::Relaxed);
        self.outbound_bytes_total
            .store(outbound_bytes_total, Ordering::Relaxed);
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
        let (
            jpeg,
            first_frame,
            reconnect,
            pipeline,
            outbound,
            jpeg_total,
            first_frame_total,
            reconnect_total,
        ) = self
            .samples
            .lock()
            .map(|samples| {
                (
                    summarize_samples(&samples.jpeg_sizes),
                    summarize_samples(&samples.first_frame_ms),
                    summarize_samples(&samples.reconnect_ms),
                    [
                        duration_snapshot(&samples.frame_wait_us, samples.frame_wait_us_total),
                        duration_snapshot(
                            &samples.capture_queue_age_us,
                            samples.capture_queue_age_us_total,
                        ),
                        duration_snapshot(&samples.gpu_readback_us, samples.gpu_readback_us_total),
                        duration_snapshot(
                            &samples.black_frame_classification_us,
                            samples.black_frame_classification_us_total,
                        ),
                        duration_snapshot(
                            &samples.jpeg_color_conversion_us,
                            samples.jpeg_color_conversion_us_total,
                        ),
                        duration_snapshot(&samples.jpeg_encode_us, samples.jpeg_encode_us_total),
                        duration_snapshot(
                            &samples.stream_send_wait_us,
                            samples.stream_send_wait_us_total,
                        ),
                    ],
                    [
                        byte_window_snapshot(
                            &samples.outbound_100ms_bytes,
                            samples.outbound_100ms_bytes_total,
                        ),
                        byte_window_snapshot(
                            &samples.outbound_1s_bytes,
                            samples.outbound_1s_bytes_total,
                        ),
                    ],
                    samples.jpeg_sizes_total,
                    samples.first_frame_ms_total,
                    samples.reconnect_ms_total,
                )
            })
            .unwrap_or_default();
        let first_frame_delay_ms = match self.first_frame_delay_ms.load(Ordering::Relaxed) {
            u64::MAX => None,
            value => Some(value),
        };
        let latest_capture = self
            .latest_capture
            .lock()
            .ok()
            .and_then(|value| value.clone());
        let frame_age_ms = latest_capture
            .as_ref()
            .map(|capture| now_ms.saturating_sub(capture.observed_at_ms))
            .or_else(|| {
                latest_frame_captured_at_ms.map(|captured_at| now_ms.saturating_sub(captured_at))
            });
        ScreenShareMediaMetricsSnapshot {
            capture_frame_count: self.capture_frame_count.load(Ordering::Relaxed),
            mjpeg_encoded_frame_count: self.encoded_frame_count.load(Ordering::Relaxed),
            mjpeg_encoded_bytes: self.mjpeg_encoded_bytes.load(Ordering::Relaxed),
            outbound_bytes_total: self.outbound_bytes_total.load(Ordering::Relaxed),
            capture_fps_actual: self.fps_actual.load(Ordering::Relaxed) as f32,
            mjpeg_fps_actual: self.mjpeg_fps_actual.load(Ordering::Relaxed) as f32,
            mjpeg_consumer_count: self.mjpeg_consumer_count.load(Ordering::Relaxed),
            jpeg_active: self.jpeg_active.load(Ordering::Relaxed),
            jpeg_enable_count: self.jpeg_enable_count.load(Ordering::Relaxed),
            jpeg_disable_count: self.jpeg_disable_count.load(Ordering::Relaxed),
            jpeg_fallback_reason: self
                .jpeg_fallback_reason
                .lock()
                .ok()
                .and_then(|value| value.clone()),
            latest_capture,
            frame_wait: pipeline[0].clone(),
            capture_queue_age: pipeline[1].clone(),
            gpu_readback: pipeline[2].clone(),
            black_frame_classification: pipeline[3].clone(),
            jpeg_color_conversion: pipeline[4].clone(),
            jpeg_encode: pipeline[5].clone(),
            stream_send_wait: pipeline[6].clone(),
            outbound_100ms: outbound[0].clone(),
            outbound_1s: outbound[1].clone(),
            stream_send_timeout_count: self.stream_send_timeout_count.load(Ordering::Relaxed),
            stream_disconnect_count: self.stream_disconnect_count.load(Ordering::Relaxed),
            encoded_frame_count: self.encoded_frame_count.load(Ordering::Relaxed),
            jpeg_sample_count: jpeg.count,
            jpeg_total_sample_count: jpeg_total,
            jpeg_retained_sample_count: jpeg.count,
            jpeg_sample_window_capacity: MEDIA_JPEG_SAMPLE_WINDOW as u32,
            jpeg_measurement_scope: MEDIA_SAMPLE_MEASUREMENT_SCOPE,
            jpeg_size_avg_bytes: jpeg.average,
            jpeg_size_p50_bytes: jpeg.p50,
            jpeg_size_p95_bytes: jpeg.p95,
            jpeg_size_p99_bytes: jpeg.p99,
            jpeg_size_max_bytes: jpeg.max,
            first_frame_delay_ms,
            frame_age_ms,
            slow_client_dropped_frames: self.slow_client_dropped_frames.load(Ordering::Relaxed),
            stream_connection_count: self.stream_connection_count.load(Ordering::Relaxed),
            stream_first_frame_sample_count: first_frame.count,
            stream_first_frame_total_sample_count: first_frame_total,
            stream_first_frame_retained_sample_count: first_frame.count,
            stream_first_frame_sample_window_capacity: MEDIA_STREAM_SAMPLE_WINDOW as u32,
            stream_first_frame_measurement_scope: MEDIA_SAMPLE_MEASUREMENT_SCOPE,
            stream_first_frame_avg_ms: first_frame.optional_average(),
            stream_first_frame_p50_ms: first_frame.optional_p50(),
            stream_first_frame_p95_ms: first_frame.optional_p95(),
            stream_first_frame_p99_ms: first_frame.optional_p99(),
            stream_first_frame_max_ms: first_frame.optional_max(),
            stream_reconnect_count: self.stream_reconnect_count.load(Ordering::Relaxed),
            stream_reconnect_sample_count: reconnect.count,
            stream_reconnect_total_sample_count: reconnect_total,
            stream_reconnect_retained_sample_count: reconnect.count,
            stream_reconnect_sample_window_capacity: MEDIA_STREAM_SAMPLE_WINDOW as u32,
            stream_reconnect_measurement_scope: MEDIA_SAMPLE_MEASUREMENT_SCOPE,
            stream_reconnect_avg_ms: reconnect.optional_average(),
            stream_reconnect_p50_ms: reconnect.optional_p50(),
            stream_reconnect_p95_ms: reconnect.optional_p95(),
            stream_reconnect_p99_ms: reconnect.optional_p99(),
            stream_reconnect_max_ms: reconnect.optional_max(),
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
    p99: u64,
    max: u64,
}

impl SampleSummary {
    fn optional_average(&self) -> Option<u64> {
        (self.count > 0).then_some(self.average)
    }

    fn optional_p95(&self) -> Option<u64> {
        (self.count > 0).then_some(self.p95)
    }

    fn optional_p50(&self) -> Option<u64> {
        (self.count > 0).then_some(self.p50)
    }

    fn optional_p99(&self) -> Option<u64> {
        (self.count > 0).then_some(self.p99)
    }

    fn optional_max(&self) -> Option<u64> {
        (self.count > 0).then_some(self.max)
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
        p99: nearest_rank(&sorted, 99),
        max: sorted.last().copied().unwrap_or_default(),
    }
}

fn push_duration_sample(samples: &mut VecDeque<u64>, duration: Duration) {
    push_bounded(
        samples,
        duration.as_micros().min(u128::from(u64::MAX)) as u64,
        MEDIA_PIPELINE_SAMPLE_WINDOW,
    );
}

fn duration_snapshot(samples: &VecDeque<u64>, total_sample_count: u64) -> DurationMetricsSnapshot {
    let summary = summarize_samples(samples);
    DurationMetricsSnapshot {
        sample_count: summary.count,
        total_sample_count,
        retained_sample_count: summary.count,
        sample_window_capacity: MEDIA_PIPELINE_SAMPLE_WINDOW as u32,
        measurement_scope: MEDIA_SAMPLE_MEASUREMENT_SCOPE,
        p50_us: summary.p50,
        p95_us: summary.p95,
        p99_us: summary.p99,
        max_us: summary.max,
    }
}

fn byte_window_snapshot(
    samples: &VecDeque<u64>,
    total_sample_count: u64,
) -> ByteWindowMetricsSnapshot {
    let summary = summarize_samples(samples);
    let total = samples
        .iter()
        .copied()
        .map(u128::from)
        .sum::<u128>()
        .min(u128::from(u64::MAX)) as u64;
    ByteWindowMetricsSnapshot {
        sample_count: summary.count,
        total_sample_count,
        retained_sample_count: summary.count,
        sample_window_capacity: MEDIA_PIPELINE_SAMPLE_WINDOW as u32,
        measurement_scope: MEDIA_SAMPLE_MEASUREMENT_SCOPE,
        total_bytes: total,
        retained_total_bytes: total,
        p50_bytes: summary.p50,
        p95_bytes: summary.p95,
        p99_bytes: summary.p99,
        max_bytes: summary.max,
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

/// A frame cannot visibly contain input that was injected after that frame was captured.
/// Keep this check at the capture boundary so all downstream transports share the same
/// causal lower bound. This remains a controlled-scene proxy; it does not inspect pixels.
fn applied_input_visible_at_capture(
    input: Option<AppliedInputSnapshot>,
    captured_at_unix_ms: u64,
) -> Option<AppliedInputSnapshot> {
    input.filter(|snapshot| snapshot.applied_at_server_unix_ms <= captured_at_unix_ms)
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
    viewer_ip_reference_count: Arc<AtomicU32>,
    active_media_task_count: Arc<AtomicU32>,
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
    desktop_overlay_active: Arc<AtomicBool>,
    /// True while the host annotation action bar is on screen. The bar page
    /// drives this from the annotation document, so it is not part of the
    /// session start config.
    annotation_bar_visible: Arc<AtomicBool>,
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
            viewer_ip_reference_count: Arc::new(AtomicU32::new(0)),
            active_media_task_count: Arc::new(AtomicU32::new(0)),
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
            desktop_overlay_active: Arc::new(AtomicBool::new(false)),
            annotation_bar_visible: Arc::new(AtomicBool::new(false)),
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
        viewer_ip_reference_count: 0,
        active_media_task_count: 0,
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
        input_metrics: None,
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
    handle.viewer_ip_reference_count.store(0, Ordering::Relaxed);
    handle.active_media_task_count.store(0, Ordering::Relaxed);
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
    handle.desktop_overlay_active.store(false, Ordering::SeqCst);
    handle.annotation_bar_visible.store(false, Ordering::SeqCst);
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

fn record_viewer_connection(viewer_ips: &Arc<Mutex<ViewerIpMap>>, ip: &str) {
    if let Ok(mut ips) = viewer_ips.lock() {
        let entry = ips.entry(ip.to_owned()).or_insert(ViewerIpEntry {
            active_media_connections: 0,
        });
        entry.active_media_connections = entry.active_media_connections.saturating_add(1);
    }
}

fn release_viewer_connection(viewer_ips: &Arc<Mutex<ViewerIpMap>>, ip: &str) {
    if let Ok(mut ips) = viewer_ips.lock() {
        let should_remove = ips.get_mut(ip).is_some_and(|entry| {
            entry.active_media_connections = entry.active_media_connections.saturating_sub(1);
            entry.active_media_connections == 0
        });
        if should_remove {
            ips.remove(ip);
        }
    }
}

fn snapshot_viewer_ips(viewer_ips: &Arc<Mutex<ViewerIpMap>>) -> Vec<String> {
    let mut ips: Vec<String> = viewer_ips
        .lock()
        .map(|map| map.keys().cloned().collect())
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
    viewer_ip_reference_count: Arc<AtomicU32>,
    active_media_task_count: Arc<AtomicU32>,
    mjpeg_viewer_count: Arc<AtomicU32>,
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
    transport: Arc<Mutex<ScreenShareMediaTransport>>,
    input_worker: Option<Arc<InputWorkerHandle>>,
    #[cfg(feature = "screen-share-webrtc-prototype")]
    webrtc: Option<Arc<crate::screenshare_webrtc::WebRtcTransportState>>,
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
    ip_reference_count: Arc<AtomicU32>,
    active_task_count: Arc<AtomicU32>,
    transport_count: Option<Arc<AtomicU32>>,
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
        if let Some(transport_count) = self.transport_count.as_ref() {
            decrement_nonzero(transport_count);
        }
        release_viewer_connection(&self.ips, &self.ip);
        decrement_nonzero(&self.ip_reference_count);
        decrement_nonzero(&self.active_task_count);
        self.events.emit_tool_log(
            &format!(
                "Viewer disconnected: ip={}, remaining_viewers={}",
                self.ip, updated_count
            ),
            "info",
        );
    }
}

fn decrement_nonzero(counter: &AtomicU32) -> u32 {
    loop {
        let current = counter.load(Ordering::Relaxed);
        if current == 0 {
            return 0;
        }
        if counter
            .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return current - 1;
        }
    }
}

fn try_reserve_media_viewer(counter: &AtomicU32) -> Option<u32> {
    loop {
        let current = counter.load(Ordering::Relaxed);
        if current >= MAX_MEDIA_VIEWERS {
            return None;
        }
        let next = current + 1;
        if counter
            .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return Some(next);
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

    // Validate
    if config.port < 1024 {
        return Err("Port must be >= 1024".into());
    }
    if config.quality < 10 || config.quality > 100 {
        return Err("Quality must be 10-100".into());
    }
    validate_capture_fps(config.fps)?;
    #[cfg(not(feature = "screen-share-webrtc-prototype"))]
    if config.transport == ScreenShareMediaTransport::WebRtc {
        return Err(
            "WebRTC experimental transport is unavailable in this build; rebuild with feature 'screen-share-webrtc-prototype' or choose MSE/MJPEG"
                .into(),
        );
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
    let mjpeg_viewer_count = Arc::new(AtomicU32::new(0));
    let media_metrics = Arc::new(ScreenShareMediaMetrics::new());
    let h264_media = Arc::new(H264MediaState::new());
    *handle.media_metrics.lock().unwrap() = Some(media_metrics.clone());
    *handle.h264_media.lock().unwrap() = Some(h264_media.clone());
    let effective_transport = ScreenShareMediaTransport::Mjpeg;
    let interaction = InteractionState::new_with_config(
        session_id,
        InteractionConfig {
            annotations_enabled: config.annotations_enabled,
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
    #[cfg(feature = "screen-share-webrtc-prototype")]
    let webrtc = (config.transport == ScreenShareMediaTransport::WebRtc)
        .then(|| crate::screenshare_webrtc::WebRtcTransportState::new(h264_media.clone()));
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
    let capture_mjpeg_viewers = mjpeg_viewer_count.clone();
    let capture_handle = handle.clone();
    let monitor_index = config.monitor_index;
    let quality = config.quality;
    let fps = config.fps;
    let show_cursor = config.show_cursor;
    let backend_mode = config.capture_backend_mode;
    let capture_tx = broadcast_tx.clone();
    let capture_interaction = interaction.clone();
    let capture_h264_media = h264_media.clone();
    let capture_input_worker = handle.input_worker.lock().unwrap().clone();
    let capture_app = app_handle.clone();
    let requested_transport = config.transport;
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
                requested_transport,
                h264_worker,
                capture_h264_media,
                capture_tx,
                capture_interaction,
                capture_cancel,
                capture_fps,
                capture_media_metrics,
                capture_viewers,
                capture_mjpeg_viewers,
                capture_input_worker,
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
        schedule_annotation_bar_window(app_handle.clone(), handle.clone(), session_id);
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
        viewer_ip_reference_count: handle.viewer_ip_reference_count.clone(),
        active_media_task_count: handle.active_media_task_count.clone(),
        mjpeg_viewer_count,
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
        transport: handle.transport.clone(),
        input_worker: handle.input_worker.lock().unwrap().clone(),
        #[cfg(feature = "screen-share-webrtc-prototype")]
        webrtc,
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
            close_desktop_overlay_window(&ss_server_app, &ss_runtime_handle, ss_session_id);
            close_annotation_bar_window(&ss_server_app, &ss_runtime_handle, ss_session_id);
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
    let outbound_active = handle.active.clone();
    let outbound_cancel = session_cancel.clone();
    let outbound_bytes = handle.bytes_sent.clone();
    let outbound_media_metrics = media_metrics.clone();
    let outbound_runtime_handle = handle.clone();
    tokio::spawn(async move {
        outbound_metrics_sampler(
            outbound_active,
            outbound_cancel,
            outbound_bytes,
            outbound_media_metrics,
            outbound_runtime_handle,
            session_id,
        )
        .await;
    });
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
    close_desktop_overlay_window(&app_handle, handle, session_id);
    close_annotation_bar_window(&app_handle, handle, session_id);
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
    let input_metrics = handle.input_worker.lock().ok().and_then(|worker| {
        worker
            .as_ref()
            .and_then(|worker| worker.metrics_snapshot().ok())
    });
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
        viewer_ip_reference_count: handle.viewer_ip_reference_count.load(Ordering::Relaxed),
        active_media_task_count: handle.active_media_task_count.load(Ordering::Relaxed),
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
        input_metrics,
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
            .and_then(|_| show_window_without_activation(&window));
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
    show_window_without_activation(window)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn show_window_without_activation(window: &WebviewWindow) -> Result<(), String> {
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
fn show_window_without_activation(window: &WebviewWindow) -> Result<(), String> {
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

// ─── Host annotation action bar ──────────────────────────────
//
// The click-through overlay above can only draw. Clearing a viewer annotation
// used to mean switching back to the screen-share page, so the host also gets a
// small interactive bar in the corner of the shared monitor. It is built hidden
// at session start and shown only while persistent annotations exist, which the
// bar page decides from the same annotation document the overlay renders.
//
// It cannot be excluded from capture for the reason recorded on
// `screen_share_desktop_overlay_ready`, so viewers see it too.

fn annotation_bar_window_label(session_id: u64) -> String {
    format!("{ANNOTATION_BAR_WINDOW_LABEL_PREFIX}-{session_id}")
}

fn schedule_annotation_bar_window(
    app_handle: AppHandle,
    handle: Arc<ScreenShareHandle>,
    session_id: u64,
) {
    tauri::async_runtime::spawn(async move {
        tokio::task::yield_now().await;
        let build_app = app_handle.clone();
        if let Err(error) = app_handle.run_on_main_thread(move || {
            if let Err(error) = ensure_annotation_bar_window(&build_app, &handle, session_id) {
                // A missing bar only costs the host the shortcut; the overlay,
                // the capture and the screen-share page all keep working.
                log::warn!("Host annotation action bar unavailable: {error}");
            }
        }) {
            log::warn!("Failed to schedule host annotation action bar: {error}");
        }
    });
}

fn ensure_annotation_bar_window(
    app_handle: &AppHandle,
    handle: &Arc<ScreenShareHandle>,
    session_id: u64,
) -> Result<(), String> {
    if !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }

    let window_label = annotation_bar_window_label(session_id);
    if app_handle.get_webview_window(&window_label).is_some() {
        return Ok(());
    }

    let bar_handle = handle.clone();
    let window = WebviewWindowBuilder::new(
        app_handle,
        &window_label,
        WebviewUrl::App("index.html#/screen-share-annotation-bar".into()),
    )
    .title("Screen Share Annotations")
    .inner_size(ANNOTATION_BAR_LOGICAL_WIDTH, ANNOTATION_BAR_LOGICAL_HEIGHT)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Failed to create host annotation action bar: {error}"))?;

    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed)
            && bar_handle.session_id.load(Ordering::SeqCst) == session_id
        {
            bar_handle
                .annotation_bar_visible
                .store(false, Ordering::SeqCst);
        }
    });
    Ok(())
}

/// Bottom-right corner of the shared monitor's work area, so the bar sits above
/// the taskbar rather than on top of the clock.
fn annotation_bar_placement(bounds: ScreenRect, scale_factor: f64) -> PhysicalPosition<i32> {
    let scale = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let width = (ANNOTATION_BAR_LOGICAL_WIDTH * scale).round() as i32;
    let height = (ANNOTATION_BAR_LOGICAL_HEIGHT * scale).round() as i32;
    let margin = (ANNOTATION_BAR_LOGICAL_MARGIN * scale).round() as i32;

    // Clamp to the monitor origin so a bar wider than the work area stays
    // reachable instead of being pushed off the left edge.
    let x = (bounds.left + bounds.width as i32 - width - margin).max(bounds.left);
    let y = (bounds.top + bounds.height as i32 - height - margin).max(bounds.top);
    PhysicalPosition::new(x, y)
}

fn annotation_bar_position(
    handle: &ScreenShareHandle,
    scale_factor: f64,
) -> Result<PhysicalPosition<i32>, String> {
    let monitor_index = handle.active_monitor_index.load(Ordering::Relaxed);
    // The work area keeps the bar above the taskbar; the full monitor bounds are
    // only a fallback for a transient topology change.
    let bounds = work_rect_for_monitor(monitor_index)
        .or_else(|| desktop_overlay_bounds(handle).ok())
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .ok_or_else(|| "Active monitor is unavailable".to_string())?;
    Ok(annotation_bar_placement(bounds, scale_factor))
}

fn configure_annotation_bar_window(
    window: &WebviewWindow,
    handle: &ScreenShareHandle,
) -> Result<(), String> {
    let scale_factor = window.scale_factor().unwrap_or(1.0);
    window
        .set_position(annotation_bar_position(handle, scale_factor)?)
        .map_err(|error| format!("Failed to position host annotation action bar: {error}"))?;
    window
        .set_always_on_top(true)
        .map_err(|error| format!("Failed to keep host annotation action bar on top: {error}"))
}

#[tauri::command]
pub fn screen_share_annotation_bar_ready(
    window: WebviewWindow,
    state: State<'_, crate::AppState>,
) -> Result<(), String> {
    let handle = &state.screen_share;
    if !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }
    let session_id = handle.session_id.load(Ordering::SeqCst);
    if window.label() != annotation_bar_window_label(session_id) {
        return Err("Host annotation action bar belongs to a stale session".into());
    }

    // Place the bar off the page-load path, the same way the overlay does, and
    // leave it hidden: the page shows it once it sees the first persistent
    // annotation. `screen_share_set_annotation_bar_visible` re-positions before
    // showing, so nothing depends on this landing first.
    let bar_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if bar_handle.session_id.load(Ordering::SeqCst) != session_id
            || !bar_handle.active.load(Ordering::SeqCst)
        {
            let _ = window.close();
            return;
        }
        if let Err(error) = configure_annotation_bar_window(&window, &bar_handle) {
            log::warn!("Host annotation action bar could not be placed: {error}");
        }
    });
    Ok(())
}

/// Show or hide the bar without ever taking focus from whatever the host is
/// presenting. Called by the bar page when the persistent annotation count
/// crosses zero in either direction.
#[tauri::command]
pub fn screen_share_set_annotation_bar_visible(
    window: WebviewWindow,
    state: State<'_, crate::AppState>,
    visible: bool,
) -> Result<(), String> {
    let handle = &state.screen_share;
    let session_id = handle.session_id.load(Ordering::SeqCst);
    if window.label() != annotation_bar_window_label(session_id) {
        return Err("Host annotation action bar belongs to a stale session".into());
    }
    if visible && !handle.active.load(Ordering::SeqCst) {
        return Err("Screen share is not active".into());
    }

    if visible {
        configure_annotation_bar_window(&window, handle)?;
        show_window_without_activation(&window)?;
    } else {
        window
            .hide()
            .map_err(|error| format!("Failed to hide host annotation action bar: {error}"))?;
    }
    handle
        .annotation_bar_visible
        .store(visible, Ordering::SeqCst);
    Ok(())
}

fn close_annotation_bar_window(
    app_handle: &AppHandle,
    handle: &ScreenShareHandle,
    session_id: u64,
) {
    handle.annotation_bar_visible.store(false, Ordering::SeqCst);
    if let Some(window) = app_handle.get_webview_window(&annotation_bar_window_label(session_id)) {
        if let Err(error) = window.close() {
            log::warn!("Failed to close host annotation action bar: {error}");
        }
    }
}

fn sync_annotation_bar_window(
    app_handle: &AppHandle,
    handle: &ScreenShareHandle,
    session_id: u64,
) -> Result<(), String> {
    let Some(window) = app_handle.get_webview_window(&annotation_bar_window_label(session_id))
    else {
        handle.annotation_bar_visible.store(false, Ordering::SeqCst);
        return Ok(());
    };
    configure_annotation_bar_window(&window, handle)
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
    Dxgi {
        frame: Frame<'a>,
        stride: usize,
    },
    #[cfg(target_os = "windows")]
    Wgc {
        pixels: Option<&'a [u8]>,
        stride: usize,
        width: usize,
        height: usize,
        gpu_surface: Option<GpuNv12Surface>,
        near_black: Option<bool>,
        gpu_preprocess_elapsed: Option<Duration>,
        gpu_backpressure: bool,
        gpu_pipeline_active: bool,
        gpu_fallback_reason: Option<String>,
        system_relative_time_100ns: Option<i64>,
        capture_queue_age: Option<Duration>,
        gpu_readback: Option<Duration>,
    },
}

impl CapturedFrame<'_> {
    fn pixels(&self) -> Option<&[u8]> {
        match self {
            Self::Dxgi { frame, .. } => Some(frame),
            #[cfg(target_os = "windows")]
            Self::Wgc { pixels, .. } => *pixels,
        }
    }

    fn stride(&self) -> usize {
        match self {
            Self::Dxgi { stride, .. } => *stride,
            #[cfg(target_os = "windows")]
            Self::Wgc { stride, .. } => *stride,
        }
    }

    fn dimensions(&self, fallback_width: usize, fallback_height: usize) -> (usize, usize) {
        match self {
            Self::Dxgi { .. } => (fallback_width, fallback_height),
            #[cfg(target_os = "windows")]
            Self::Wgc { width, height, .. } => (*width, *height),
        }
    }

    #[cfg(target_os = "windows")]
    fn take_gpu_surface(&mut self) -> Option<GpuNv12Surface> {
        match self {
            Self::Dxgi { .. } => None,
            Self::Wgc { gpu_surface, .. } => gpu_surface.take(),
        }
    }

    fn probed_near_black(&self) -> Option<bool> {
        match self {
            Self::Dxgi { .. } => None,
            #[cfg(target_os = "windows")]
            Self::Wgc { near_black, .. } => *near_black,
        }
    }

    fn report_gpu_metrics(&self, media: &H264MediaState) {
        #[cfg(target_os = "windows")]
        if let Self::Wgc {
            gpu_preprocess_elapsed,
            gpu_backpressure,
            gpu_pipeline_active,
            gpu_fallback_reason,
            ..
        } = self
        {
            if let Some(elapsed) = gpu_preprocess_elapsed {
                media.record_gpu_preprocess(*elapsed);
            }
            if *gpu_backpressure {
                media.record_gpu_backpressure_drop();
            }
            if *gpu_pipeline_active {
                media.set_gpu_pipeline_active();
            }
            if let Some(reason) = gpu_fallback_reason {
                media.record_gpu_fallback(reason.clone());
            }
        }
    }

    fn system_relative_time_100ns(&self) -> Option<i64> {
        match self {
            Self::Dxgi { .. } => None,
            #[cfg(target_os = "windows")]
            Self::Wgc {
                system_relative_time_100ns,
                ..
            } => *system_relative_time_100ns,
        }
    }

    fn capture_queue_age(&self) -> Option<Duration> {
        match self {
            Self::Dxgi { .. } => None,
            #[cfg(target_os = "windows")]
            Self::Wgc {
                capture_queue_age, ..
            } => *capture_queue_age,
        }
    }

    fn gpu_readback(&self) -> Option<Duration> {
        match self {
            Self::Dxgi { .. } => None,
            #[cfg(target_os = "windows")]
            Self::Wgc { gpu_readback, .. } => *gpu_readback,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CaptureFrameOptions {
    gpu_h264: bool,
    cpu_pixels: bool,
    fps: u8,
}

fn select_capture_frame_options(
    backend: CaptureBackendKind,
    h264_available: bool,
    gpu_input_allowed: bool,
    mjpeg_consumers: u32,
    fps: u8,
) -> CaptureFrameOptions {
    let gpu_h264 = backend == CaptureBackendKind::Wgc && h264_available && gpu_input_allowed;
    CaptureFrameOptions {
        gpu_h264,
        cpu_pixels: backend != CaptureBackendKind::Wgc || !gpu_h264 || mjpeg_consumers > 0,
        fps,
    }
}

fn select_jpeg_state(
    consumers: u32,
    cpu_pixels_available: bool,
    h264_ready: bool,
    h264_available: bool,
) -> (bool, &'static str) {
    if consumers == 0 {
        return (false, "no_mjpeg_consumers");
    }
    if !cpu_pixels_available {
        return (false, "mjpeg_cpu_readback_pending");
    }
    (
        true,
        if h264_ready {
            "mjpeg_compatibility_viewer"
        } else if h264_available {
            "h264_not_ready"
        } else {
            "h264_unavailable"
        },
    )
}

fn relative_capture_queue_age(
    anchor: &mut Option<(i64, Instant)>,
    system_time_100ns: i64,
    observed_at: Instant,
) -> Option<Duration> {
    let reset_anchor = anchor.is_none_or(|(base, _)| system_time_100ns < base);
    if reset_anchor {
        *anchor = Some((system_time_100ns, observed_at));
        return Some(Duration::ZERO);
    }
    let (base_system_time, base_instant) = *anchor.as_ref()?;
    let elapsed_ticks = u64::try_from(system_time_100ns - base_system_time).ok()?;
    let elapsed = Duration::from_nanos(elapsed_ticks.saturating_mul(100));
    let captured_at = base_instant.checked_add(elapsed)?;
    Some(observed_at.saturating_duration_since(captured_at))
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

    fn frame(&mut self, options: CaptureFrameOptions) -> io::Result<CapturedFrame<'_>> {
        match self {
            Self::Dxgi(capturer) => {
                let height = capturer.height();
                let frame: Frame<'_> = capturer.frame()?;
                let stride = frame.len() / height;
                Ok(CapturedFrame::Dxgi { frame, stride })
            }
            #[cfg(target_os = "windows")]
            Self::Wgc(capturer) => capturer.frame(options),
        }
    }
}

fn capture_loop(
    monitor_index: usize,
    quality: u8,
    fps: u8,
    show_cursor: bool,
    backend_mode: ScreenShareBackendMode,
    requested_transport: ScreenShareMediaTransport,
    h264_worker: Option<H264EncoderWorker>,
    h264_media: Arc<H264MediaState>,
    tx: broadcast::Sender<Arc<Bytes>>,
    interaction: Arc<InteractionState>,
    cancel: Arc<AtomicBool>,
    fps_counter: Arc<AtomicU32>,
    media_metrics: Arc<ScreenShareMediaMetrics>,
    viewer_count: Arc<AtomicU32>,
    mjpeg_viewer_count: Arc<AtomicU32>,
    input_worker: Option<Arc<InputWorkerHandle>>,
    runtime_handle: Arc<ScreenShareHandle>,
    session_id: u64,
    startup_tx: Option<oneshot::Sender<Result<(), String>>>,
    app_handle: AppHandle,
) {
    let mut startup_tx = startup_tx;
    let h264_worker = h264_worker;
    let mut h264_failure_logged = false;
    let mut h264_ready_logged = false;
    let mut last_jpeg_encoded_at: Option<Instant> = None;
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
    let frame_interval = capture_frame_interval(fps);
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
        let frame_wait_started = Instant::now();
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
                    None => {
                        #[cfg(target_os = "windows")]
                        let gpu_h264 = current_backend == CaptureBackendKind::Wgc
                            && h264_worker
                                .as_ref()
                                .is_some_and(H264EncoderWorker::gpu_input_allowed);
                        #[cfg(not(target_os = "windows"))]
                        let gpu_h264 = false;
                        source.frame(select_capture_frame_options(
                            current_backend,
                            h264_worker.is_some(),
                            gpu_h264,
                            mjpeg_viewer_count.load(Ordering::Relaxed),
                            fps,
                        ))
                    }
                }
            };

        match frame_result {
            Ok(mut frame) => {
                frame.report_gpu_metrics(&h264_media);
                let frame_wait = frame_wait_started.elapsed();
                let system_relative_time_100ns = frame.system_relative_time_100ns();
                let capture_queue_age = frame.capture_queue_age();
                let gpu_readback = frame.gpu_readback();
                let stride = frame.stride();
                #[cfg(target_os = "windows")]
                let mut gpu_surface = frame.take_gpu_surface();
                let frame_pixels = frame.pixels();

                // WGC 分辨率变化时帧尺寸会静默改变（staging 缓冲已随帧重建），
                // 必须同步 loop 尺寸，否则 encode 守卫永远丢帧 → 黑屏。
                // DXGI 的 stride 含行对齐填充，不适用此推导；其分辨率变化
                // 会直接报错走重建路径，重建后尺寸在恢复分支同步。
                if current_backend == CaptureBackendKind::Wgc && stride >= 4 {
                    let (frame_w, frame_h) = frame.dimensions(width, height);
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
                let source_pixels: Option<&[u8]> =
                    if show_cursor && current_backend == CaptureBackendKind::Dxgi {
                        let frame_pixels = frame_pixels.expect("DXGI frame must expose CPU pixels");
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
                            Some(&frame_scratch)
                        } else {
                            Some(frame_pixels)
                        }
                    } else {
                        frame_pixels
                    };

                #[cfg(not(target_os = "windows"))]
                let source_pixels: Option<&[u8]> = frame_pixels;

                let (capture_sequence, captured_at_unix_ms) = media_metrics.record_capture_frame(
                    width as u32,
                    height as u32,
                    system_relative_time_100ns,
                    frame_wait,
                    capture_queue_age,
                    gpu_readback,
                );
                let black_frame_started = Instant::now();
                let near_black = frame.probed_near_black().unwrap_or_else(|| {
                    source_pixels.is_some_and(|pixels| {
                        is_nearly_black_bgra_frame(pixels, width, height, stride)
                    })
                });
                media_metrics.record_black_frame_classification(black_frame_started.elapsed());
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
                        #[cfg(target_os = "windows")]
                        if let Some(surface) = gpu_surface.take() {
                            let _ = surface.release_after_encoder_done();
                        }
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
                        #[cfg(target_os = "windows")]
                        if let Some(surface) = gpu_surface.take() {
                            let _ = surface.release_after_encoder_done();
                        }
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

                fps_counter.fetch_add(1, Ordering::Relaxed);
                first_real_frame = true;
                session_ever_had_frame = true;
                let interaction_frame_id =
                    interaction.record_frame_metadata(width as u32, height as u32);
                if pending_resume {
                    // Recovery is complete when a real capture frame is accepted;
                    // it must not depend on the optional compatibility JPEG path.
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

                // Submit the captured pixels to the low-latency encoder before
                // spending time on the compatibility JPEG path.
                if let Some(worker) = h264_worker.as_ref() {
                    let applied_input = applied_input_visible_at_capture(
                        input_worker
                            .as_ref()
                            .and_then(|input_worker| input_worker.latest_applied_input()),
                        captured_at_unix_ms,
                    );
                    let visible_input_sequence =
                        applied_input.as_ref().map(|input| input.client_sequence);
                    let input_applied_at_server_unix_ms = applied_input
                        .as_ref()
                        .map(|input| input.applied_at_server_unix_ms);
                    #[cfg(target_os = "windows")]
                    let gpu_submitted = gpu_surface.is_some_and(|surface| {
                        worker.try_submit_gpu_with_metadata(
                            surface,
                            capture_sequence,
                            captured_at_unix_ms,
                            visible_input_sequence,
                            input_applied_at_server_unix_ms,
                        )
                    });
                    #[cfg(not(target_os = "windows"))]
                    let gpu_submitted = false;
                    if !gpu_submitted {
                        if let Some(source_pixels) = source_pixels {
                            let _ = worker.try_submit_with_metadata(
                                source_pixels,
                                width,
                                height,
                                stride,
                                capture_sequence,
                                captured_at_unix_ms,
                                visible_input_sequence,
                                input_applied_at_server_unix_ms,
                            );
                        }
                    }
                }

                if h264_worker.is_some() {
                    let current_transport = *runtime_handle.transport.lock().unwrap();
                    if h264_media.is_ready() {
                        let ready_transport = requested_transport.resolved_h264_transport();
                        if current_transport != ready_transport {
                            *runtime_handle.transport.lock().unwrap() = ready_transport;
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
                    } else if current_transport != ScreenShareMediaTransport::Mjpeg {
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

                let mjpeg_consumers = mjpeg_viewer_count.load(Ordering::Relaxed);
                // Viewer demand can race with the frame-options snapshot taken
                // before WGC capture. If a viewer arrived mid-frame, defer JPEG
                // by one frame instead of assuming a CPU readback exists.
                let (jpeg_active, jpeg_reason) = select_jpeg_state(
                    mjpeg_consumers,
                    source_pixels.is_some(),
                    h264_media.is_ready(),
                    h264_worker.is_some(),
                );
                if media_metrics.update_jpeg_state(jpeg_active, mjpeg_consumers, jpeg_reason) {
                    emit_capture_create_diagnostic(
                        &app_handle,
                        "info",
                        format!(
                            "MJPEG compatibility encoder {}: consumers={}, reason={}, max_fps={}",
                            if jpeg_active { "enabled" } else { "disabled" },
                            mjpeg_consumers,
                            jpeg_reason,
                            1000 / MJPEG_FALLBACK_FRAME_INTERVAL.as_millis()
                        ),
                    );
                }

                let jpeg_due = last_jpeg_encoded_at.is_none_or(|last| {
                    tick_start.saturating_duration_since(last) >= MJPEG_FALLBACK_FRAME_INTERVAL
                });
                let jpeg = (jpeg_active && jpeg_due)
                    .then_some(source_pixels)
                    .flatten()
                    .map(|source_pixels| {
                        last_jpeg_encoded_at = Some(tick_start);
                        let encoded = encode_jpeg_reuse(
                            source_pixels,
                            width,
                            height,
                            stride,
                            quality,
                            &mut rgb_buf,
                            &mut jpeg_buf,
                        );
                        media_metrics.record_jpeg_timings(encoded.color_conversion, encoded.encode);
                        encoded
                    });

                if let Some(jpeg) = jpeg.filter(|jpeg| !jpeg.bytes.is_empty()) {
                    let data = Arc::new(Bytes::from(jpeg.bytes));
                    media_metrics.record_encoded_frame(data.len());
                    let _ = interaction.record_frame_bytes(interaction_frame_id, data.clone());
                    let _ = tx.send(data);
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
    /// Maps WGC's monotonic SystemRelativeTime clock to this process's
    /// `Instant` clock without pretending either value is a wall-clock timestamp.
    system_time_anchor: Option<(i64, Instant)>,
    gpu_preprocessor: Option<GpuVideoPreprocessor>,
    gpu_generation: u64,
    gpu_pipeline_disabled: bool,
    pending_gpu_fallback: Option<String>,
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
        let capturer = Self {
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
            system_time_anchor: None,
            gpu_preprocessor: None,
            gpu_generation: 1,
            gpu_pipeline_disabled: false,
            pending_gpu_fallback: None,
            staging: None,
            frame_buf: Vec::with_capacity(width * height * 4),
            stride: width * 4,
            width,
            height,
        };
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

    fn frame(&mut self, options: CaptureFrameOptions) -> io::Result<CapturedFrame<'_>> {
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

        let system_relative_time_100ns = frame
            .SystemRelativeTime()
            .ok()
            .map(|time| time.Duration)
            .filter(|ticks| *ticks >= 0);

        let surface = frame
            .Surface()
            .map_err(|error| windows_error_to_io("Direct3D11CaptureFrame::Surface", error))?;
        let texture = wgc_surface_texture(&surface)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        let mut descriptor = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut descriptor) };
        self.width = descriptor.Width as usize;
        self.height = descriptor.Height as usize;
        self.stride = self.width.saturating_mul(4);

        let gpu_attempted = options.gpu_h264 && !self.gpu_pipeline_disabled;
        let gpu_preprocess_started = Instant::now();
        let (gpu_surface, near_black, gpu_backpressure) = if gpu_attempted {
            self.preprocess_gpu_frame(&texture, options.fps)
        } else {
            (None, None, false)
        };
        let gpu_preprocess_elapsed = gpu_attempted.then_some(gpu_preprocess_started.elapsed());
        let cpu_readback_required = options.cpu_pixels
            || (options.gpu_h264 && gpu_surface.is_none() && self.gpu_pipeline_disabled);
        let gpu_readback = if cpu_readback_required {
            let readback_started = Instant::now();
            self.copy_texture_to_frame_buffer(&texture)
                .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
            Some(readback_started.elapsed())
        } else {
            None
        };
        let _ = frame.Close();

        let delivered_at = Instant::now();
        let capture_queue_age = system_relative_time_100ns.and_then(|ticks| {
            relative_capture_queue_age(&mut self.system_time_anchor, ticks, delivered_at)
        });
        self.last_frame_delivered = delivered_at;
        Ok(CapturedFrame::Wgc {
            pixels: cpu_readback_required.then_some(self.frame_buf.as_slice()),
            stride: self.stride,
            width: self.width,
            height: self.height,
            gpu_surface,
            near_black,
            gpu_preprocess_elapsed,
            gpu_backpressure,
            // A configured preprocessor is not evidence that the live encoder
            // still accepts DXGI surfaces. Once the encoder disables GPU input,
            // later CPU fallback frames must not clear the recorded failure and
            // falsely advertise the GPU path as active.
            gpu_pipeline_active: gpu_attempted
                && self.gpu_preprocessor.is_some()
                && !self.gpu_pipeline_disabled,
            gpu_fallback_reason: self.pending_gpu_fallback.take(),
            system_relative_time_100ns,
            capture_queue_age,
            gpu_readback,
        })
    }

    fn preprocess_gpu_frame(
        &mut self,
        texture: &ID3D11Texture2D,
        fps: u8,
    ) -> (Option<GpuNv12Surface>, Option<bool>, bool) {
        let output_width = (self.width as u32) & !1;
        let output_height = (self.height as u32) & !1;
        let generation = self
            .gpu_preprocessor
            .as_ref()
            .filter(|preprocessor| {
                let active = preprocessor.config();
                active.input_width != self.width as u32
                    || active.input_height != self.height as u32
                    || active.output_width != output_width
                    || active.output_height != output_height
                    || active.frame_rate_numerator != u32::from(fps.max(1))
            })
            .map_or(self.gpu_generation, |_| {
                self.gpu_generation.saturating_add(1)
            });
        let config = GpuPreprocessConfig {
            input_width: self.width as u32,
            input_height: self.height as u32,
            output_width,
            output_height,
            frame_rate_numerator: u32::from(fps.max(1)),
            frame_rate_denominator: 1,
            generation,
        };
        if config.output_width < 2 || config.output_height < 2 {
            self.disable_gpu_pipeline(GpuFallbackReason {
                code: GpuFallbackCode::InvalidConfiguration,
                operation: "WGC GPU pipeline dimensions",
                hresult: None,
                detail: format!("invalid WGC size {}x{}", self.width, self.height),
            });
            return (None, None, false);
        }

        let setup_result = match self.gpu_preprocessor.as_mut() {
            Some(preprocessor) if preprocessor.config() != config => {
                preprocessor.reconfigure(config)
            }
            Some(_) => Ok(()),
            None => match GpuVideoPreprocessor::new(
                self._d3d_device.clone(),
                self.context.clone(),
                config,
            ) {
                Ok(preprocessor) => {
                    let capabilities = preprocessor.capabilities();
                    log::info!(
                        "screen_share_gpu_preprocessor enabled=true input={}x{} output={}x{} pool={} bgra_input={} nv12_output={} rate_modes={}",
                        config.input_width,
                        config.input_height,
                        config.output_width,
                        config.output_height,
                        capabilities.pool_size,
                        capabilities.bgra_input,
                        capabilities.nv12_output,
                        capabilities.rate_conversion_modes,
                    );
                    self.gpu_preprocessor = Some(preprocessor);
                    Ok(())
                }
                Err(error) => Err(error),
            },
        };
        if let Err(error) = setup_result {
            if is_transient_gpu_backpressure(&error) {
                return (None, None, true);
            }
            self.disable_gpu_pipeline(error);
            return (None, None, false);
        }
        self.gpu_generation = generation;

        let Some(preprocessor) = self.gpu_preprocessor.as_mut() else {
            return (None, None, false);
        };
        let near_black = match preprocessor.poll_black_frame_probe(texture, Instant::now()) {
            Ok(value) => value,
            Err(error) => {
                self.disable_gpu_pipeline(error);
                return (None, None, false);
            }
        };
        match preprocessor.preprocess(texture) {
            Ok(surface) => (Some(surface), near_black, false),
            Err(error) if is_transient_gpu_backpressure(&error) => (None, near_black, true),
            Err(error) => {
                self.disable_gpu_pipeline(error);
                (None, near_black, false)
            }
        }
    }

    fn disable_gpu_pipeline(&mut self, error: GpuFallbackReason) {
        if !self.gpu_pipeline_disabled {
            log::warn!(
                "screen_share_gpu_preprocessor enabled=false code={:?} operation={} hresult={:?} detail={}",
                error.code,
                error.operation,
                error.hresult,
                sanitize_log_field(&error.detail),
            );
        }
        self.gpu_pipeline_disabled = true;
        self.pending_gpu_fallback = Some(format!(
            "code={:?}; operation={}; hresult={:?}; detail={}",
            error.code,
            error.operation,
            error.hresult,
            sanitize_log_field(&error.detail),
        ));
        self.gpu_preprocessor = None;
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

    fn copy_texture_to_frame_buffer(&mut self, texture: &ID3D11Texture2D) -> Result<(), String> {
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
fn wgc_surface_texture(surface: &IDirect3DSurface) -> Result<ID3D11Texture2D, String> {
    let access: IDirect3DDxgiInterfaceAccess = surface
        .cast()
        .map_err(|error| format_windows_error("IDirect3DSurface::cast", &error))?;
    unsafe {
        access.GetInterface().map_err(|error| {
            format_windows_error("IDirect3DDxgiInterfaceAccess::GetInterface", &error)
        })
    }
}

#[cfg(target_os = "windows")]
fn is_transient_gpu_backpressure(error: &GpuFallbackReason) -> bool {
    matches!(
        error.code,
        GpuFallbackCode::PoolExhausted | GpuFallbackCode::PoolBusy
    )
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
    let video_device_result = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            Some(&mut d3d_context),
        )
    };
    if let Err(video_error) = video_device_result {
        log::warn!(
            "screen_share_wgc_video_device enabled=false operation=D3D11CreateDevice(VIDEO_SUPPORT) hresult=0x{:08X} detail={}",
            video_error.code().0 as u32,
            sanitize_log_field(&video_error.message()),
        );
        d3d_device = None;
        d3d_context = None;
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
            .map_err(|error| format_windows_error("D3D11CreateDevice(WGC fallback)", &error))?;
        }
    }

    let d3d_device =
        d3d_device.ok_or_else(|| "D3D11CreateDevice returned no device".to_string())?;
    let d3d_context =
        d3d_context.ok_or_else(|| "D3D11CreateDevice returned no immediate context".to_string())?;
    match d3d_context.cast::<ID3D11Multithread>() {
        Ok(multithread) => unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        },
        Err(error) => log::warn!(
            "screen_share_wgc_multithread_protection enabled=false hresult=0x{:08X} detail={}",
            error.code().0 as u32,
            sanitize_log_field(&error.message()),
        ),
    }
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
    .bytes
}

#[derive(Debug, Default)]
struct JpegEncodeResult {
    bytes: Vec<u8>,
    color_conversion: Duration,
    encode: Duration,
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
) -> JpegEncodeResult {
    if width == 0 || height == 0 || stride < width * 4 || bgra.len() < height * stride {
        return JpegEncodeResult::default();
    }

    // Convert BGRA (with stride padding) to packed RGB — reusing buffer
    let color_started = Instant::now();
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
    let color_conversion = color_started.elapsed();

    jpeg_buf.clear();
    let encode_started = Instant::now();
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
            return JpegEncodeResult {
                bytes: Vec::new(),
                color_conversion,
                encode: encode_started.elapsed(),
            };
        }
    }
    JpegEncodeResult {
        bytes: jpeg_buf.clone(),
        color_conversion,
        encode: encode_started.elapsed(),
    }
}

// ─── HTTP Server ────────────────────────────────────────────

async fn run_http_server(
    listener: tokio::net::TcpListener,
    state: Arc<HttpServerState>,
    shutdown_rx: oneshot::Receiver<()>,
) {
    #[cfg(feature = "screen-share-webrtc-prototype")]
    let webrtc = state.webrtc.clone();
    let app = screen_share_router(state);

    let (drain_started_tx, drain_started_rx) = oneshot::channel::<()>();
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .tcp_nodelay(true)
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

    #[cfg(feature = "screen-share-webrtc-prototype")]
    if let Some(webrtc) = webrtc {
        webrtc.shutdown_all().await;
    }

    log::info!("Screen share HTTP server stopped");
}

fn screen_share_router(state: Arc<HttpServerState>) -> Router {
    let router = Router::new()
        .route("/", get(handler_index))
        .route("/assets/*path", get(handler_web_asset))
        .route("/stream", get(handler_stream))
        .route("/media/ws", get(handler_media_ws))
        .route("/media/webcodecs/ws", get(handler_webcodecs_ws))
        .route("/auth", post(handler_auth))
        .route("/time", get(handler_time))
        .route("/status", get(handler_status))
        .route("/session/ws", get(handler_session_ws));
    #[cfg(feature = "screen-share-webrtc-prototype")]
    let router = router.route("/api/screenshare/webrtc/offer", post(handler_webrtc_offer));
    router.with_state(state)
}

async fn handler_time() -> impl IntoResponse {
    Json(serde_json::json!({ "server_unix_ms": unix_time_ms() }))
}

// ─── HTTP Handlers ──────────────────────────────────────────

#[derive(Deserialize)]
struct IndexQuery {
    error: Option<u8>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamQuery {
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
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
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
        if !check_auth_cookie(&headers, hash) {
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

fn media_viewer_limit_response() -> Response {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("Retry-After", "5")
        .header("Cache-Control", "no-store")
        .body(Body::from(format!(
            "Screen-share viewer limit reached ({MAX_MEDIA_VIEWERS})"
        )))
        .unwrap()
}

async fn handler_stream(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(query): Query<StreamQuery>,
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
    let Some(viewer_total) = try_reserve_media_viewer(&state.viewer_count) else {
        return media_viewer_limit_response();
    };
    record_viewer_connection(&state.viewer_ips, &client_ip);
    state
        .viewer_ip_reference_count
        .fetch_add(1, Ordering::Relaxed);
    state
        .active_media_task_count
        .fetch_add(1, Ordering::Relaxed);
    state.mjpeg_viewer_count.fetch_add(1, Ordering::Relaxed);
    let is_reconnect = query.reconnect == Some(1);
    state.media_metrics.record_stream_open(is_reconnect);
    state.events.emit_tool_log(
        &format!(
            "Viewer connected: ip={}, viewers={}, transport=mjpeg, user_agent={}",
            client_ip,
            viewer_total,
            summarize_user_agent(&headers)
        ),
        "info",
    );
    let viewer_guard = ViewerGuard {
        events: state.events.clone(),
        count: state.viewer_count.clone(),
        ip_reference_count: state.viewer_ip_reference_count.clone(),
        active_task_count: state.active_media_task_count.clone(),
        transport_count: Some(state.mjpeg_viewer_count.clone()),
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

    let (body_sender, body_receiver) =
        tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(MJPEG_BODY_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        let _guard = viewer_guard;
        let mut first_frame_sent = false;

        if let Some(frame) = initial_frame {
            let chunk = mjpeg_frame_chunk(&frame);
            let chunk_len = chunk.len();
            if send_mjpeg_body_chunk(&body_sender, chunk, &media_metrics)
                .await
                .is_err()
            {
                media_metrics.record_stream_disconnect();
                return;
            }
            bytes_sent.fetch_add(chunk_len as u64, Ordering::Relaxed);
            media_metrics.record_stream_first_frame(stream_started.elapsed(), is_reconnect);
            first_frame_sent = true;
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
                    let chunk_len = chunk.len();
                    if send_mjpeg_body_chunk(&body_sender, chunk, &media_metrics)
                        .await
                        .is_err()
                    {
                        break;
                    }
                    bytes_sent.fetch_add(chunk_len as u64, Ordering::Relaxed);
                    if !first_frame_sent {
                        media_metrics
                            .record_stream_first_frame(stream_started.elapsed(), is_reconnect);
                        first_frame_sent = true;
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(skipped))) => {
                    media_metrics.record_lagged_frames(skipped);
                    // Reset the receiver after yielding the cached newest frame,
                    // instead of walking through the remainder of the stale queue.
                    rx = broadcast_tx.subscribe();
                    if let Some(frame) = interaction.latest_frame_bytes() {
                        let chunk = mjpeg_frame_chunk(&frame);
                        let chunk_len = chunk.len();
                        if send_mjpeg_body_chunk(&body_sender, chunk, &media_metrics)
                            .await
                            .is_err()
                        {
                            break;
                        }
                        bytes_sent.fetch_add(chunk_len as u64, Ordering::Relaxed);
                        if !first_frame_sent {
                            media_metrics
                                .record_stream_first_frame(stream_started.elapsed(), is_reconnect);
                            first_frame_sent = true;
                        }
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
        media_metrics.record_stream_disconnect();
    });
    let stream = futures_util::stream::unfold(body_receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });

    Response::builder()
        .header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
        .header("Cache-Control", "no-cache, no-store")
        .header("Connection", "keep-alive")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn send_mjpeg_body_chunk(
    sender: &tokio::sync::mpsc::Sender<Result<Bytes, Infallible>>,
    chunk: Bytes,
    media_metrics: &ScreenShareMediaMetrics,
) -> Result<(), ()> {
    send_mjpeg_body_chunk_with_timeout(MJPEG_STREAM_SEND_TIMEOUT, sender, chunk, media_metrics)
        .await
}

async fn send_mjpeg_body_chunk_with_timeout(
    timeout: Duration,
    sender: &tokio::sync::mpsc::Sender<Result<Bytes, Infallible>>,
    chunk: Bytes,
    media_metrics: &ScreenShareMediaMetrics,
) -> Result<(), ()> {
    let started_at = Instant::now();
    match tokio::time::timeout(timeout, sender.send(Ok(chunk))).await {
        Ok(Ok(())) => {
            media_metrics.record_stream_send(started_at.elapsed(), false);
            Ok(())
        }
        Ok(Err(_)) => {
            media_metrics.record_stream_send(started_at.elapsed(), false);
            Err(())
        }
        Err(_) => {
            media_metrics.record_stream_send(started_at.elapsed(), true);
            Err(())
        }
    }
}

async fn handler_media_ws(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    websocket: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }
    if state.h264_media.descriptor().is_none() {
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
    let Some(viewer_total) = try_reserve_media_viewer(&state.viewer_count) else {
        return media_viewer_limit_response();
    };
    record_viewer_connection(&state.viewer_ips, &client_ip);
    state
        .viewer_ip_reference_count
        .fetch_add(1, Ordering::Relaxed);
    state
        .active_media_task_count
        .fetch_add(1, Ordering::Relaxed);
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
        ip_reference_count: state.viewer_ip_reference_count.clone(),
        active_task_count: state.active_media_task_count.clone(),
        transport_count: None,
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

async fn handler_webcodecs_ws(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    websocket: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }
    if *state.transport.lock().unwrap() != ScreenShareMediaTransport::WebCodecs {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Cache-Control", "no-store")
            .body(Body::from(
                "WebCodecs experimental transport is not selected for this session",
            ))
            .unwrap();
    }
    let Some(descriptor) = state.h264_media.descriptor() else {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Cache-Control", "no-store")
            .body(Body::from("H.264 media stream is not ready"))
            .unwrap();
    };
    if descriptor.decoder_configuration.is_empty() {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Cache-Control", "no-store")
            .body(Body::from("H.264 decoder configuration is unavailable"))
            .unwrap();
    }
    let websocket = match websocket {
        Ok(websocket) => websocket,
        Err(rejection) => return rejection.into_response(),
    };
    let Some(viewer_total) = try_reserve_media_viewer(&state.viewer_count) else {
        return media_viewer_limit_response();
    };
    let client_ip = addr.ip().to_string();
    record_viewer_connection(&state.viewer_ips, &client_ip);
    state
        .viewer_ip_reference_count
        .fetch_add(1, Ordering::Relaxed);
    state
        .active_media_task_count
        .fetch_add(1, Ordering::Relaxed);
    let is_reconnect = query.reconnect == Some(1);
    state.media_metrics.record_stream_open(is_reconnect);
    state.events.emit_tool_log(
        &format!(
            "Viewer connected: ip={}, viewers={}, transport=web_codecs, user_agent={}",
            client_ip,
            viewer_total,
            summarize_user_agent(&headers)
        ),
        "info",
    );
    let viewer_guard = ViewerGuard {
        events: state.events.clone(),
        count: state.viewer_count.clone(),
        ip_reference_count: state.viewer_ip_reference_count.clone(),
        active_task_count: state.active_media_task_count.clone(),
        transport_count: None,
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
            run_webcodecs_media_socket(
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

async fn run_webcodecs_media_socket(
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
    let Some(initial_descriptor) = media.descriptor() else {
        media_metrics.record_stream_disconnect();
        return;
    };
    let mut generation = initial_descriptor.generation;
    let mut sequence = 0u64;
    let mut waiting_for_keyframe = true;
    if send_webcodecs_descriptor(
        &mut socket,
        &initial_descriptor,
        &bytes_sent,
        &media_metrics,
    )
    .await
    .is_err()
    {
        media_metrics.record_stream_disconnect();
        return;
    }
    let _ = media.request_keyframe(generation);
    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
            }
            event = events.recv() => match event {
                Ok(event) => match event.as_ref() {
                    H264MediaEvent::Reset(descriptor) => {
                        if send_webcodecs_descriptor(
                            &mut socket,
                            descriptor,
                            &bytes_sent,
                            &media_metrics,
                        ).await.is_err() {
                            break;
                        }
                        generation = descriptor.generation;
                        sequence = 0;
                        waiting_for_keyframe = true;
                        first_frame_sent = false;
                        let _ = media.request_keyframe(generation);
                    }
                    H264MediaEvent::Segment(segment) if segment.generation == generation => {
                        let gap = sequence != 0 && segment.sequence != sequence.saturating_add(1);
                        if gap {
                            waiting_for_keyframe = true;
                            let _ = media.request_keyframe(generation);
                        }
                        if waiting_for_keyframe && !segment.keyframe {
                            continue;
                        }
                        let payload = match webcodecs_access_unit_message(segment, waiting_for_keyframe) {
                            Ok(payload) => payload,
                            Err(_) => break,
                        };
                        let trace = h264_media_trace_message(segment);
                        let wire_bytes = payload.len().saturating_add(trace.len());
                        if send_h264_message(
                            &mut socket,
                            Message::Text(trace),
                            &media_metrics,
                        ).await.is_err() {
                            break;
                        }
                        if send_h264_message(
                            &mut socket,
                            Message::Binary(payload.to_vec()),
                            &media_metrics,
                        ).await.is_err() {
                            break;
                        }
                        bytes_sent.fetch_add(wire_bytes as u64, Ordering::Relaxed);
                        sequence = segment.sequence;
                        waiting_for_keyframe = false;
                        if !first_frame_sent {
                            first_frame_sent = true;
                            media_metrics.record_stream_first_frame(started_at.elapsed(), is_reconnect);
                        }
                    }
                    H264MediaEvent::Segment(_) => {}
                    H264MediaEvent::Unavailable { generation: next_generation, error } => {
                        generation = *next_generation;
                        sequence = 0;
                        waiting_for_keyframe = true;
                        first_frame_sent = false;
                        let message = serde_json::json!({
                            "v": 1,
                            "type": "media.unavailable",
                            "generation": next_generation,
                            "error": error,
                        }).to_string();
                        if send_h264_message(
                            &mut socket,
                            Message::Text(message),
                            &media_metrics,
                        ).await.is_err() {
                            break;
                        }
                    }
                },
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    media_metrics.record_lagged_frames(skipped);
                    sequence = 0;
                    waiting_for_keyframe = true;
                    // The one-shot Reset event may itself have been overwritten. Refresh the
                    // descriptor from authoritative state instead of draining retained events;
                    // otherwise this socket can wait forever for an IDR from an old generation.
                    if let Some(descriptor) =
                        webcodecs_newer_descriptor(generation, media.descriptor())
                    {
                        if send_webcodecs_descriptor(
                            &mut socket,
                            &descriptor,
                            &bytes_sent,
                            &media_metrics,
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                        generation = descriptor.generation;
                        first_frame_sent = false;
                    }
                    let _ = media.request_keyframe(generation);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Ping(payload) => {
                        if send_h264_message(
                            &mut socket,
                            Message::Pong(payload),
                            &media_metrics,
                        ).await.is_err() {
                            break;
                        }
                    }
                    Message::Text(text) => {
                        let request = serde_json::from_str::<serde_json::Value>(&text).ok();
                        if request.as_ref().is_some_and(|request| {
                            request["v"] == 1 && request["type"] == "media.keyframe.request"
                        }) {
                            let _ = media.request_keyframe(generation);
                        }
                    }
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Pong(_) => {}
                }
            }
        }
    }
    media_metrics.record_stream_disconnect();
}

fn webcodecs_newer_descriptor(
    current_generation: u64,
    descriptor: Option<Arc<H264StreamDescriptor>>,
) -> Option<Arc<H264StreamDescriptor>> {
    descriptor.filter(|descriptor| descriptor.generation != current_generation)
}

fn webcodecs_access_unit_message(
    segment: &H264MediaSegment,
    discontinuity: bool,
) -> Result<Bytes, String> {
    let payload = segment.access_unit_avcc.as_ref();
    if segment.generation == 0
        || segment.sequence == 0
        || segment.duration_us == 0
        || segment.duration_us > u64::from(u32::MAX)
        || payload.is_empty()
        || payload.len() > WEBCODECS_MAX_ACCESS_UNIT_BYTES
        || payload.len() > u32::MAX as usize
    {
        return Err("invalid WebCodecs access-unit metadata".to_owned());
    }
    let mut bytes = BytesMut::with_capacity(WEBCODECS_AU_HEADER_BYTES + payload.len());
    bytes.extend_from_slice(b"FSTW");
    bytes.extend_from_slice(&[1]);
    let mut flags = if segment.keyframe { 1u8 } else { 2u8 };
    if discontinuity {
        flags |= 4;
    }
    bytes.extend_from_slice(&[flags]);
    bytes.extend_from_slice(&(WEBCODECS_AU_HEADER_BYTES as u16).to_be_bytes());
    bytes.extend_from_slice(&segment.generation.to_be_bytes());
    bytes.extend_from_slice(&segment.sequence.to_be_bytes());
    bytes.extend_from_slice(&segment.timestamp_us.to_be_bytes());
    bytes.extend_from_slice(&(segment.duration_us as u32).to_be_bytes());
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes.freeze())
}

async fn send_webcodecs_descriptor(
    socket: &mut WebSocket,
    descriptor: &H264StreamDescriptor,
    bytes_sent: &AtomicU64,
    media_metrics: &ScreenShareMediaMetrics,
) -> Result<(), axum::Error> {
    let message = webcodecs_descriptor_message(descriptor)
        .map_err(|error| axum::Error::new(io::Error::new(io::ErrorKind::InvalidData, error)))?;
    let message_length = message.len();
    send_h264_message(socket, Message::Text(message), media_metrics).await?;
    bytes_sent.fetch_add(message_length as u64, Ordering::Relaxed);
    Ok(())
}

fn webcodecs_descriptor_message(descriptor: &H264StreamDescriptor) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    if descriptor.decoder_configuration.is_empty() {
        return Err("H.264 decoder configuration is empty".to_owned());
    }
    Ok(serde_json::json!({
        "v": 1,
        "type": "media.hello",
        "transport": "webcodecs_h264",
        "generation": descriptor.generation,
        "codec": descriptor.codec,
        "description_base64": STANDARD.encode(descriptor.decoder_configuration.as_ref()),
        "width": descriptor.width,
        "height": descriptor.height,
        "fps": descriptor.fps,
        "bitrate_bps": descriptor.bitrate_bps,
        "color_space": null,
    })
    .to_string())
}

#[cfg(feature = "screen-share-webrtc-prototype")]
async fn handler_webrtc_offer(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("Cache-Control", "no-store")
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    }
    if body.len() > WEBRTC_SIGNALING_MAX_BYTES {
        return Response::builder()
            .status(StatusCode::PAYLOAD_TOO_LARGE)
            .header("Cache-Control", "no-store")
            .body(Body::from("WebRTC SDP offer is too large"))
            .unwrap();
    }
    let offer = match serde_json::from_slice::<
        webrtc::peer_connection::sdp::session_description::RTCSessionDescription,
    >(&body)
    {
        Ok(offer) => offer,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("Cache-Control", "no-store")
                .body(Body::from("Invalid WebRTC SDP offer"))
                .unwrap();
        }
    };
    let Some(webrtc) = state.webrtc.as_ref() else {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Cache-Control", "no-store")
            .body(Body::from(
                "WebRTC prototype is not selected for this screen-share session",
            ))
            .unwrap();
    };
    let Some(viewer_total) = try_reserve_media_viewer(&state.viewer_count) else {
        return media_viewer_limit_response();
    };
    let client_ip = addr.ip().to_string();
    record_viewer_connection(&state.viewer_ips, &client_ip);
    state
        .viewer_ip_reference_count
        .fetch_add(1, Ordering::Relaxed);
    state
        .active_media_task_count
        .fetch_add(1, Ordering::Relaxed);
    state.events.emit_tool_log(
        &format!(
            "Viewer connected: ip={}, viewers={}, transport=web_rtc, user_agent={}",
            client_ip,
            viewer_total,
            summarize_user_agent(&headers)
        ),
        "info",
    );
    let viewer_guard = ViewerGuard {
        events: state.events.clone(),
        count: state.viewer_count.clone(),
        ip_reference_count: state.viewer_ip_reference_count.clone(),
        active_task_count: state.active_media_task_count.clone(),
        transport_count: None,
        ips: state.viewer_ips.clone(),
        ip: client_ip,
    };
    match webrtc
        .answer_offer_with_lease(offer, Some(Box::new(viewer_guard)))
        .await
    {
        Ok(answer) => Json(answer).into_response(),
        Err(error) => Response::builder()
            .status(error.status())
            .header("Cache-Control", "no-store")
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::json!({ "error": error.message() }).to_string(),
            ))
            .unwrap(),
    }
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
    let Some(initial_descriptor) = media.descriptor() else {
        media_metrics.record_stream_disconnect();
        return;
    };
    let mut generation = initial_descriptor.generation;
    let mut sequence = 0u64;
    let mut waiting_for_keyframe = true;
    if send_h264_descriptor(
        &mut socket,
        &initial_descriptor,
        &bytes_sent,
        &media_metrics,
    )
    .await
    .is_err()
    {
        media_metrics.record_stream_disconnect();
        return;
    }
    let _ = media.request_keyframe(generation);

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
                            if send_h264_descriptor(
                                &mut socket,
                                descriptor,
                                &bytes_sent,
                                &media_metrics,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                            generation = descriptor.generation;
                            sequence = 0;
                            first_frame_sent = false;
                            waiting_for_keyframe = true;
                            let _ = media.request_keyframe(generation);
                        }
                        H264MediaEvent::Segment(segment)
                            if segment.generation == generation =>
                        {
                            if waiting_for_keyframe && !segment.keyframe {
                                continue;
                            }
                            if !waiting_for_keyframe
                                && segment.sequence != sequence.saturating_add(1)
                            {
                                // Never continue a decoder dependency chain across a gap.
                                break;
                            }
                            let payload = segment.bytes.as_ref().clone();
                            let length = payload.len();
                            let trace = h264_media_trace_message(segment);
                            let trace_length = trace.len();
                            if send_h264_message(
                                &mut socket,
                                Message::Text(trace),
                                &media_metrics,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                            if send_h264_message(
                                &mut socket,
                                Message::Binary(payload.to_vec()),
                                &media_metrics,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                            bytes_sent.fetch_add(
                                length.saturating_add(trace_length) as u64,
                                Ordering::Relaxed,
                            );
                            sequence = segment.sequence;
                            waiting_for_keyframe = false;
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
                            waiting_for_keyframe = true;
                            let message = serde_json::json!({
                                "v": 1,
                                "type": "media.unavailable",
                                "generation": next_generation,
                                "error": error,
                            })
                            .to_string();
                            if send_h264_message(
                                &mut socket,
                                Message::Text(message),
                                &media_metrics,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                    },
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        media_metrics.record_lagged_frames(skipped);
                        // A lagged client is already slower than the live stream. Disconnect it
                        // so its normal reconnect can consume one cached snapshot, rather than
                        // replaying a GOP here and making this socket fall even farther behind.
                        break;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Ping(payload) => {
                        if send_h264_message(
                            &mut socket,
                            Message::Pong(payload),
                            &media_metrics,
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Text(_) | Message::Binary(_) | Message::Pong(_) => {}
                }
            }
        }
    }
    media_metrics.record_stream_disconnect();
}

fn h264_media_trace_message(segment: &H264MediaSegment) -> String {
    serde_json::json!({
        "v": 1,
        "type": "media.trace",
        "generation": segment.generation,
        "sequence": segment.sequence,
        "keyframe": segment.keyframe,
        "timestamp_us": segment.timestamp_us,
        "duration_us": segment.duration_us,
        "capture_sequence": segment.capture_sequence,
        "captured_at_unix_ms": segment.captured_at_unix_ms,
        "visible_input_sequence": segment.visible_input_sequence,
        "input_applied_at_server_unix_ms": segment.input_applied_at_server_unix_ms,
    })
    .to_string()
}

async fn send_h264_descriptor(
    socket: &mut WebSocket,
    descriptor: &H264StreamDescriptor,
    bytes_sent: &AtomicU64,
    media_metrics: &ScreenShareMediaMetrics,
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
    let message_length = message.len();
    send_h264_message(socket, Message::Text(message), media_metrics).await?;
    let init = descriptor.init_segment.as_ref().clone();
    let length = init.len();
    send_h264_message(socket, Message::Binary(init.to_vec()), media_metrics).await?;
    bytes_sent.fetch_add(
        length.saturating_add(message_length) as u64,
        Ordering::Relaxed,
    );
    Ok(())
}

async fn send_h264_message(
    socket: &mut WebSocket,
    message: Message,
    media_metrics: &ScreenShareMediaMetrics,
) -> Result<(), axum::Error> {
    let started_at = Instant::now();
    match tokio::time::timeout(H264_STREAM_SEND_TIMEOUT, socket.send(message)).await {
        Ok(result) => {
            media_metrics.record_stream_send(started_at.elapsed(), false);
            result
        }
        Err(error) => {
            media_metrics.record_stream_send(started_at.elapsed(), true);
            Err(axum::Error::new(error))
        }
    }
}

#[cfg(test)]
async fn with_h264_send_timeout<F>(timeout: Duration, send: F) -> Result<(), axum::Error>
where
    F: Future<Output = Result<(), axum::Error>>,
{
    tokio::time::timeout(timeout, send)
        .await
        .map_err(axum::Error::new)?
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
    let interaction_document = state.interaction.snapshot();
    let latest_frame = state.interaction.latest_frame_info();
    let control = state.interaction.control_snapshot();
    let media_metrics = state
        .media_metrics
        .snapshot(latest_frame.as_ref().map(|frame| frame.captured_at_ms));
    let input_metrics = state
        .input_worker
        .as_ref()
        .and_then(|worker| worker.metrics_snapshot().ok());
    #[cfg(feature = "screen-share-webrtc-prototype")]
    let webrtc_metrics = state
        .webrtc
        .as_ref()
        .map(|webrtc| webrtc.metrics_snapshot());
    #[cfg(not(feature = "screen-share-webrtc-prototype"))]
    let webrtc_metrics: Option<serde_json::Value> = None;
    Json(serde_json::json!({
        "active": !state.cancel.load(Ordering::Relaxed),
        "viewers": state.viewer_count.load(Ordering::Relaxed),
        "viewer_ip_reference_count": state.viewer_ip_reference_count.load(Ordering::Relaxed),
        "active_media_task_count": state.active_media_task_count.load(Ordering::Relaxed),
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
        "input_metrics": input_metrics,
        "transport": state.transport.lock().unwrap().resolved_label(),
        "h264_media": state.h264_media.metrics(),
        "webrtc": webrtc_metrics,
        "control_state": control.state,
        "controller_ip": control.controller_ip,
        "pending_control_request": state.interaction.pending_control_request(),
        "capture_paused": state.capture_paused.load(Ordering::Relaxed),
        "capture_issue": *state.capture_issue.lock().unwrap(),
    }))
}

const INTERACTION_CRITICAL_QUEUE_CAPACITY: usize = 32;
const INTERACTION_BULK_QUEUE_CAPACITY: usize = 128;
const INTERACTION_SEND_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionMessagePriority {
    Critical,
    Bulk,
}

fn interaction_message_priority(message_type: &str) -> InteractionMessagePriority {
    match message_type {
        // Annotation deltas can recover from an authoritative snapshot after a
        // revision gap. Control, session state, errors, and input ACKs must not
        // compete with the high-frequency drawing stream.
        "annotation.applied" => InteractionMessagePriority::Bulk,
        _ => InteractionMessagePriority::Critical,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionQueuePushOutcome {
    Queued,
    BulkDroppedOldest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InteractionQueuePushError {
    Closed,
    CriticalFull,
}

struct InteractionOutboundQueueState {
    critical: VecDeque<Message>,
    bulk: VecDeque<Message>,
    closed: bool,
}

struct InteractionOutboundQueue {
    state: Mutex<InteractionOutboundQueueState>,
    notify: Notify,
    critical_capacity: usize,
    bulk_capacity: usize,
}

impl InteractionOutboundQueue {
    fn new(critical_capacity: usize, bulk_capacity: usize) -> Self {
        Self {
            state: Mutex::new(InteractionOutboundQueueState {
                critical: VecDeque::with_capacity(critical_capacity),
                bulk: VecDeque::with_capacity(bulk_capacity),
                closed: false,
            }),
            notify: Notify::new(),
            critical_capacity,
            bulk_capacity,
        }
    }

    fn push(
        &self,
        message: Message,
        priority: InteractionMessagePriority,
    ) -> Result<InteractionQueuePushOutcome, InteractionQueuePushError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| InteractionQueuePushError::Closed)?;
        if state.closed {
            return Err(InteractionQueuePushError::Closed);
        }
        let outcome = match priority {
            InteractionMessagePriority::Critical => {
                if state.critical.len() >= self.critical_capacity {
                    return Err(InteractionQueuePushError::CriticalFull);
                }
                state.critical.push_back(message);
                InteractionQueuePushOutcome::Queued
            }
            InteractionMessagePriority::Bulk => {
                let dropped = if state.bulk.len() >= self.bulk_capacity {
                    state.bulk.pop_front();
                    true
                } else {
                    false
                };
                state.bulk.push_back(message);
                if dropped {
                    InteractionQueuePushOutcome::BulkDroppedOldest
                } else {
                    InteractionQueuePushOutcome::Queued
                }
            }
        };
        drop(state);
        self.notify.notify_one();
        Ok(outcome)
    }

    #[cfg(test)]
    fn pop_now(&self) -> Option<Message> {
        let mut state = self.state.lock().ok()?;
        state
            .critical
            .pop_front()
            .or_else(|| state.bulk.pop_front())
    }

    async fn next(&self) -> Option<Message> {
        loop {
            let notified = self.notify.notified();
            if let Ok(mut state) = self.state.lock() {
                if let Some(message) = state
                    .critical
                    .pop_front()
                    .or_else(|| state.bulk.pop_front())
                {
                    return Some(message);
                }
                if state.closed {
                    return None;
                }
            } else {
                return None;
            }
            notified.await;
        }
    }

    fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
        }
        self.notify.notify_waiters();
    }
}

fn serialize_interaction_message(
    message: screenshare_interaction::ServerEnvelope,
) -> Result<Message, String> {
    serde_json::to_string(&message)
        .map(Message::Text)
        .map_err(|error| format!("failed to serialize interaction message: {error}"))
}

fn queue_interaction_message(
    queue: &InteractionOutboundQueue,
    message: screenshare_interaction::ServerEnvelope,
) -> Result<InteractionQueuePushOutcome, InteractionQueuePushError> {
    let priority = interaction_message_priority(&message.message_type);
    queue_interaction_message_with_priority(queue, message, priority)
}

fn queue_interaction_message_with_priority(
    queue: &InteractionOutboundQueue,
    message: screenshare_interaction::ServerEnvelope,
    priority: InteractionMessagePriority,
) -> Result<InteractionQueuePushOutcome, InteractionQueuePushError> {
    let serialized = serialize_interaction_message(message)
        .map_err(|_| InteractionQueuePushError::CriticalFull)?;
    queue.push(serialized, priority)
}

async fn handler_session_ws(
    AxumState(state): AxumState<Arc<HttpServerState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<SessionQuery>,
    websocket: WebSocketUpgrade,
) -> Response {
    if let Some(hash) = &state.auth_hash {
        if !check_auth_cookie(&headers, hash) {
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
        let _ = with_interaction_send_timeout(socket.send(Message::Close(None))).await;
        return;
    }

    let mut interaction_events = interaction.subscribe();
    let hello = match interaction.hello(&client_id) {
        Ok(message) => message,
        Err(error) => {
            let _ = send_interaction_message(&mut socket, error.to_message(&interaction)).await;
            interaction.unregister_client(&client_id);
            let _ = with_interaction_send_timeout(socket.send(Message::Close(None))).await;
            return;
        }
    };

    let (sender, mut receiver) = socket.split();
    let outbound = Arc::new(InteractionOutboundQueue::new(
        INTERACTION_CRITICAL_QUEUE_CAPACITY,
        INTERACTION_BULK_QUEUE_CAPACITY,
    ));
    if queue_interaction_message(&outbound, hello).is_err()
        || queue_interaction_message(&outbound, interaction.snapshot_message()).is_err()
    {
        interaction.unregister_client(&client_id);
        return;
    }
    let writer_queue = outbound.clone();
    let mut writer_task =
        tokio::spawn(async move { run_interaction_writer(sender, writer_queue).await });
    log::info!("Interaction client connected: ip={client_ip}");

    let mut ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            writer_result = &mut writer_task => {
                match writer_result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => log::warn!("Interaction writer closed for {client_ip}: {error}"),
                    Err(error) => log::warn!("Interaction writer task failed for {client_ip}: {error}"),
                }
                break;
            }
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
                        match queue_interaction_message(&outbound, message) {
                            Ok(InteractionQueuePushOutcome::BulkDroppedOldest) => {
                                log::debug!("Dropped oldest bulk interaction update for slow client {client_ip}");
                            }
                            Ok(InteractionQueuePushOutcome::Queued) => {}
                            Err(_) => break,
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if queue_interaction_message(&outbound, interaction.snapshot_message()).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = receiver.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Text(text) => {
                        let message_received_at = Instant::now();
                        let message_received_at_ms = unix_time_ms();
                        if text.len() > MAX_WS_MESSAGE_BYTES {
                            let error = screenshare_interaction::ProtocolError::new(
                                "message_too_large",
                                format!("message exceeds {MAX_WS_MESSAGE_BYTES} bytes"),
                            );
                            if queue_interaction_message(&outbound, error.to_message(&interaction)).is_err() {
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
                                if queue_interaction_message(&outbound, protocol_error.to_message(&interaction)).is_err() {
                                    break;
                                }
                                continue;
                            }
                        };
                        let message_type = envelope.message_type.clone();
                        if message_type.starts_with("input.") {
                            let client_seq = envelope.client_seq;
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
                                        worker.release_all(&context).map(|_| "release_all")
                                    } else {
                                        worker
                                            .enqueue(QueuedInput::with_client_sequence_received_at(
                                                context,
                                                input,
                                                client_seq,
                                                message_received_at,
                                            ))
                                            .map(|outcome| match outcome {
                                                QueuePushOutcome::Queued => "queued",
                                                QueuePushOutcome::Coalesced => "coalesced",
                                            })
                                    };
                                    queued.map_err(|_| {
                                        screenshare_interaction::ProtocolError::new(
                                            "input_queue_full",
                                            "remote input queue is full; control was revoked",
                                        )
                                    })
                                });
                            match input_result {
                                Ok(queue_outcome) => {
                                    let server_enqueued_at_ms = unix_time_ms();
                                    let receive_to_enqueue_us = message_received_at
                                        .elapsed()
                                        .as_micros()
                                        .min(u128::from(u64::MAX)) as u64;
                                    log::trace!(
                                        "Remote input accepted: client={client_id} seq={} receive_to_enqueue_us={receive_to_enqueue_us} outcome={queue_outcome}",
                                        client_seq.unwrap_or_default(),
                                    );
                                    let ack = input_ack_message(
                                        &message_type,
                                        envelope.session_id,
                                        envelope.source_epoch,
                                        client_seq,
                                        message_received_at_ms,
                                        server_enqueued_at_ms,
                                        receive_to_enqueue_us,
                                        queue_outcome,
                                    );
                                    let ack_priority = if message_type == "input.pointer_move" {
                                        InteractionMessagePriority::Bulk
                                    } else {
                                        InteractionMessagePriority::Critical
                                    };
                                    if queue_interaction_message_with_priority(
                                        &outbound,
                                        ack,
                                        ack_priority,
                                    )
                                    .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    if error.code == "input_queue_full" {
                                        if let Some(worker) = input_worker.as_ref() {
                                            worker.revoke();
                                        }
                                        interaction.revoke_control("input_queue_full");
                                    }
                                    let mut error_message = error.to_message(&interaction);
                                    error_message.client_seq = client_seq;
                                    if queue_interaction_message(&outbound, error_message).is_err() {
                                        break;
                                    }
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
                            Ok(Some(event)) if message_type == "control.release" => {
                                if let Some(worker) = input_worker.as_ref() {
                                    worker.revoke();
                                }
                                let _ = event;
                            }
                            Ok(_) => {}
                            Err(error) => {
                                if queue_interaction_message(&outbound, error.to_message(&interaction)).is_err() {
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
                        if bytes.len() > MAX_WS_MESSAGE_BYTES || queue_interaction_message(&outbound, protocol_error.to_message(&interaction)).is_err() {
                            break;
                        }
                    }
                    Message::Ping(bytes) => {
                        if outbound.push(Message::Pong(bytes), InteractionMessagePriority::Critical).is_err() {
                            break;
                        }
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => break,
                }
            }
        }
    }

    outbound.close();
    if !writer_task.is_finished() {
        match tokio::time::timeout(Duration::from_millis(100), &mut writer_task).await {
            Ok(_) => {}
            Err(_) => writer_task.abort(),
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
) -> Result<(), String> {
    let serialized = serialize_interaction_message(message)?;
    with_interaction_send_timeout(socket.send(serialized)).await
}

async fn with_interaction_send_timeout<F>(send: F) -> Result<(), String>
where
    F: Future<Output = Result<(), axum::Error>>,
{
    with_interaction_send_deadline(send, INTERACTION_SEND_TIMEOUT).await
}

async fn with_interaction_send_deadline<F>(send: F, timeout: Duration) -> Result<(), String>
where
    F: Future<Output = Result<(), axum::Error>>,
{
    match tokio::time::timeout(timeout, send).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(format!("interaction send failed: {error}")),
        Err(_) => Err(format!(
            "interaction send exceeded {}ms",
            timeout.as_millis()
        )),
    }
}

async fn run_interaction_writer(
    mut sender: futures_util::stream::SplitSink<WebSocket, Message>,
    outbound: Arc<InteractionOutboundQueue>,
) -> Result<(), String> {
    while let Some(message) = outbound.next().await {
        with_interaction_send_timeout(sender.send(message)).await?;
    }
    with_interaction_send_timeout(sender.close()).await
}

fn input_ack_message(
    input_type: &str,
    session_id: u64,
    source_epoch: u64,
    client_seq: Option<u64>,
    server_received_at_ms: u64,
    server_enqueued_at_ms: u64,
    receive_to_enqueue_us: u64,
    queue_outcome: &str,
) -> screenshare_interaction::ServerEnvelope {
    screenshare_interaction::ServerEnvelope {
        v: 1,
        message_type: "input.ack".to_string(),
        session_id,
        source_epoch,
        client_seq,
        revision: None,
        payload: Some(serde_json::json!({
            "input_type": input_type,
            "server_received_at_ms": server_received_at_ms,
            "server_enqueued_at_ms": server_enqueued_at_ms,
            "server_ack_queued_at_ms": unix_time_ms(),
            "receive_to_enqueue_us": receive_to_enqueue_us,
            "queue_outcome": queue_outcome,
        })),
    }
}

// ─── Status Reporter ────────────────────────────────────────

async fn outbound_metrics_sampler(
    active: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    bytes_sent: Arc<AtomicU64>,
    media_metrics: Arc<ScreenShareMediaMetrics>,
    runtime_handle: Arc<ScreenShareHandle>,
    session_id: u64,
) {
    const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
    const ONE_SECOND_BUCKETS: u8 = 10;

    let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    let mut last_bytes = bytes_sent.load(Ordering::Relaxed);
    let mut one_second_bytes = 0_u64;
    let mut bucket_count = 0_u8;

    loop {
        interval.tick().await;
        if cancel.load(Ordering::Relaxed)
            || !active.load(Ordering::Relaxed)
            || !is_current_session(&runtime_handle, session_id)
        {
            break;
        }

        let current_bytes = bytes_sent.load(Ordering::Relaxed);
        let delta = current_bytes.saturating_sub(last_bytes);
        last_bytes = current_bytes;
        media_metrics.record_outbound_window(SAMPLE_INTERVAL, delta);
        one_second_bytes = one_second_bytes.saturating_add(delta);
        bucket_count = bucket_count.saturating_add(1);
        if bucket_count == ONE_SECOND_BUCKETS {
            media_metrics.record_outbound_window(Duration::from_secs(1), one_second_bytes);
            one_second_bytes = 0;
            bucket_count = 0;
        }
    }
}

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

        if runtime_handle
            .annotation_bar_visible
            .load(Ordering::Relaxed)
        {
            let _ = sync_annotation_bar_window(&app_handle, &runtime_handle, session_id);
        }

        let fps_count = fps_counter.swap(0, Ordering::Relaxed);
        let current_bytes = bytes_sent.load(Ordering::Relaxed);
        let bytes_delta = current_bytes.saturating_sub(last_bytes);
        last_bytes = current_bytes;
        let bitrate_kbps = (bytes_delta * 8 / 1024).min(u64::from(u32::MAX)) as u32;
        media_metrics.update_rates(fps_count, bitrate_kbps, current_bytes);

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
            viewer_ip_reference_count: runtime_handle
                .viewer_ip_reference_count
                .load(Ordering::Relaxed),
            active_media_task_count: runtime_handle
                .active_media_task_count
                .load(Ordering::Relaxed),
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
            input_metrics: runtime_handle.input_worker.lock().ok().and_then(|worker| {
                worker
                    .as_ref()
                    .and_then(|worker| worker.metrics_snapshot().ok())
            }),
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
    use tower::ServiceExt;

    #[test]
    fn annotation_bar_anchors_to_the_bottom_right_of_the_work_area() {
        let work_area = ScreenRect {
            left: 0,
            top: 0,
            width: 1920,
            // 1080 monitor minus a 40 px taskbar: the bar must stay above it.
            height: 1040,
        };
        let position = annotation_bar_placement(work_area, 1.0);
        assert_eq!(position.x, 1920 - 360 - 16);
        assert_eq!(position.y, 1040 - 60 - 16);
    }

    #[test]
    fn annotation_bar_scales_its_own_size_and_margin_for_dpi() {
        let work_area = ScreenRect {
            left: 0,
            top: 0,
            width: 3840,
            height: 2160,
        };
        let position = annotation_bar_placement(work_area, 2.0);
        assert_eq!(position.x, 3840 - 720 - 32);
        assert_eq!(position.y, 2160 - 120 - 32);
    }

    #[test]
    fn annotation_bar_follows_a_secondary_monitor_origin() {
        let work_area = ScreenRect {
            left: -1920,
            top: 120,
            width: 1920,
            height: 1000,
        };
        let position = annotation_bar_placement(work_area, 1.0);
        assert_eq!(position.x, -1920 + 1920 - 360 - 16);
        assert_eq!(position.y, 120 + 1000 - 60 - 16);
    }

    #[test]
    fn annotation_bar_stays_reachable_on_a_work_area_narrower_than_itself() {
        let work_area = ScreenRect {
            left: 40,
            top: 60,
            width: 200,
            height: 40,
        };
        let position = annotation_bar_placement(work_area, 1.0);
        assert_eq!(position.x, 40);
        assert_eq!(position.y, 60);
    }

    #[test]
    fn annotation_bar_treats_a_missing_scale_factor_as_one() {
        let work_area = ScreenRect {
            left: 0,
            top: 0,
            width: 1280,
            height: 720,
        };
        assert_eq!(
            annotation_bar_placement(work_area, 0.0),
            annotation_bar_placement(work_area, 1.0)
        );
    }

    #[test]
    fn wgc_gpu_path_avoids_full_frame_readback_without_mjpeg_demand() {
        let options = select_capture_frame_options(CaptureBackendKind::Wgc, true, true, 0, 30);
        assert!(options.gpu_h264);
        assert!(!options.cpu_pixels);
        assert_eq!(options.fps, 30);
    }

    #[test]
    fn capture_fps_range_admits_the_60_fps_experiment_tier() {
        assert!(validate_capture_fps(MIN_CAPTURE_FPS).is_ok());
        assert!(validate_capture_fps(30).is_ok());
        // 界面实验档发送的就是上限值。
        assert_eq!(MAX_CAPTURE_FPS, 60);
        assert!(validate_capture_fps(MAX_CAPTURE_FPS).is_ok());

        let too_low = validate_capture_fps(0).unwrap_err();
        assert!(too_low.contains("1-60"), "unexpected message: {too_low}");
        assert!(validate_capture_fps(MAX_CAPTURE_FPS + 1).is_err());
    }

    #[test]
    fn capture_frame_interval_does_not_inflate_either_fps_tier() {
        // 毫秒整数除法会把 30 FPS 变成 33 ms（30.3 FPS）、60 FPS 变成 16 ms（62.5 FPS），
        // 两档对比数据必须使用真实节拍。
        assert_eq!(capture_frame_interval(30), Duration::from_micros(33_333));
        assert_eq!(
            capture_frame_interval(MAX_CAPTURE_FPS),
            Duration::from_micros(16_666)
        );
        assert_eq!(capture_frame_interval(1), Duration::from_secs(1));
        // 0 与超限值仍必须产生有限节拍，不能除零或退化成忙循环。
        assert_eq!(capture_frame_interval(0), Duration::from_secs(1));
        assert_eq!(
            capture_frame_interval(u8::MAX),
            Duration::from_micros(16_666)
        );
    }

    #[test]
    fn capture_selection_preserves_cpu_fallback_and_mjpeg_compatibility() {
        let mjpeg = select_capture_frame_options(CaptureBackendKind::Wgc, true, true, 1, 60);
        assert!(mjpeg.gpu_h264);
        assert!(mjpeg.cpu_pixels);

        let gpu_disabled =
            select_capture_frame_options(CaptureBackendKind::Wgc, true, false, 0, 30);
        assert!(!gpu_disabled.gpu_h264);
        assert!(gpu_disabled.cpu_pixels);

        let dxgi = select_capture_frame_options(CaptureBackendKind::Dxgi, true, true, 0, 30);
        assert!(!dxgi.gpu_h264);
        assert!(dxgi.cpu_pixels);
    }

    #[test]
    fn mid_frame_mjpeg_arrival_defers_until_cpu_pixels_are_available() {
        assert_eq!(
            select_jpeg_state(1, false, true, true),
            (false, "mjpeg_cpu_readback_pending")
        );
        assert_eq!(
            select_jpeg_state(1, true, true, true),
            (true, "mjpeg_compatibility_viewer")
        );
    }

    #[derive(Default)]
    struct TestScreenShareEvents;

    impl ScreenShareEventSink for TestScreenShareEvents {
        fn emit_tool_log(&self, _message: &str, _level: &str) {}

        fn emit_control_request(&self, _request: ControlRequestInfo) {}
    }

    fn interaction_text(message: Message) -> String {
        match message {
            Message::Text(text) => text,
            other => panic!("expected text interaction message, got {other:?}"),
        }
    }

    #[test]
    fn interaction_outbound_queue_reserves_priority_for_critical_state() {
        let queue = InteractionOutboundQueue::new(1, 1);
        queue
            .push(
                Message::Text("annotation-1".to_string()),
                InteractionMessagePriority::Bulk,
            )
            .unwrap();
        assert_eq!(
            queue.push(
                Message::Text("annotation-2".to_string()),
                InteractionMessagePriority::Bulk,
            ),
            Ok(InteractionQueuePushOutcome::BulkDroppedOldest)
        );
        queue
            .push(
                Message::Text("input-ack".to_string()),
                InteractionMessagePriority::Critical,
            )
            .unwrap();

        assert_eq!(interaction_text(queue.pop_now().unwrap()), "input-ack");
        assert_eq!(interaction_text(queue.pop_now().unwrap()), "annotation-2");
        assert!(queue.pop_now().is_none());
    }

    #[test]
    fn interaction_queue_rejects_only_a_saturated_critical_lane() {
        let queue = InteractionOutboundQueue::new(1, 4);
        queue
            .push(
                Message::Text("control-state".to_string()),
                InteractionMessagePriority::Critical,
            )
            .unwrap();

        assert_eq!(
            queue.push(
                Message::Text("input-ack".to_string()),
                InteractionMessagePriority::Critical,
            ),
            Err(InteractionQueuePushError::CriticalFull)
        );
        assert_eq!(
            queue.push(
                Message::Text("annotation".to_string()),
                InteractionMessagePriority::Bulk,
            ),
            Ok(InteractionQueuePushOutcome::Queued)
        );
    }

    #[tokio::test]
    async fn interaction_slow_writer_send_is_bounded_by_deadline() {
        let send = std::future::pending::<Result<(), axum::Error>>();
        let started = Instant::now();

        let error = with_interaction_send_deadline(send, Duration::from_millis(10))
            .await
            .unwrap_err();

        assert!(error.contains("exceeded 10ms"), "got: {error}");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn input_ack_correlates_sequence_and_server_enqueue_timing() {
        let ack = input_ack_message(
            "input.pointer_move",
            42,
            7,
            Some(99),
            1_000,
            1_001,
            750,
            "coalesced",
        );

        assert_eq!(ack.message_type, "input.ack");
        assert_eq!(ack.client_seq, Some(99));
        assert_eq!(ack.session_id, 42);
        assert_eq!(ack.source_epoch, 7);
        let payload = ack.payload.unwrap();
        assert_eq!(payload["input_type"], "input.pointer_move");
        assert_eq!(payload["server_received_at_ms"], 1_000);
        assert_eq!(payload["server_enqueued_at_ms"], 1_001);
        assert_eq!(payload["receive_to_enqueue_us"], 750);
        assert_eq!(payload["queue_outcome"], "coalesced");
        assert!(payload["server_ack_queued_at_ms"].as_u64().is_some());
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
        metrics.update_rates(15, 8_192, 1_048_576);

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
        assert_eq!(snapshot.outbound_bytes_total, 1_048_576);
    }

    #[test]
    fn media_metrics_separate_capture_jpeg_pipeline_and_send_timings() {
        let metrics = ScreenShareMediaMetrics::new();
        metrics.record_capture_frame(
            1_920,
            1_080,
            Some(42_000_000),
            Duration::from_millis(2),
            Some(Duration::from_millis(3)),
            Some(Duration::from_millis(4)),
        );
        metrics.record_black_frame_classification(Duration::from_micros(500));
        metrics.record_jpeg_timings(Duration::from_millis(5), Duration::from_millis(6));
        metrics.record_encoded_frame(12_345);
        metrics.record_stream_send(Duration::from_millis(7), true);
        metrics.record_stream_disconnect();
        metrics.update_rates(30, 4_096, 99_999);

        let snapshot = metrics.snapshot(None);
        assert_eq!(snapshot.capture_frame_count, 1);
        assert_eq!(snapshot.mjpeg_encoded_frame_count, 1);
        assert_eq!(snapshot.encoded_frame_count, 1);
        assert_eq!(snapshot.mjpeg_encoded_bytes, 12_345);
        assert_eq!(snapshot.capture_fps_actual, 30.0);
        assert_eq!(snapshot.mjpeg_fps_actual, 1.0);
        assert_eq!(snapshot.outbound_bytes_total, 99_999);
        assert_eq!(snapshot.latest_capture.as_ref().unwrap().sequence, 1);
        assert_eq!(snapshot.latest_capture.as_ref().unwrap().width, 1_920);
        assert_eq!(
            snapshot
                .latest_capture
                .as_ref()
                .unwrap()
                .system_relative_time_100ns,
            Some(42_000_000)
        );
        assert_eq!(snapshot.frame_wait.p99_us, 2_000);
        assert_eq!(snapshot.capture_queue_age.max_us, 3_000);
        assert_eq!(snapshot.gpu_readback.p95_us, 4_000);
        assert_eq!(snapshot.black_frame_classification.p50_us, 500);
        assert_eq!(snapshot.jpeg_color_conversion.max_us, 5_000);
        assert_eq!(snapshot.jpeg_encode.max_us, 6_000);
        assert_eq!(snapshot.stream_send_wait.max_us, 7_000);
        assert_eq!(snapshot.stream_send_timeout_count, 1);
        assert_eq!(snapshot.stream_disconnect_count, 1);
    }

    #[test]
    fn media_metrics_report_bounded_outbound_windows() {
        let metrics = ScreenShareMediaMetrics::new();
        for bytes in 1_u64..=1_100 {
            metrics.record_outbound_window(Duration::from_millis(100), bytes);
        }
        metrics.record_outbound_window(Duration::from_secs(1), 4_000);
        metrics.record_outbound_window(Duration::from_secs(1), 8_000);

        let snapshot = metrics.snapshot(None);
        assert_eq!(snapshot.outbound_100ms.sample_count, 1_024);
        assert_eq!(snapshot.outbound_100ms.max_bytes, 1_100);
        assert_eq!(snapshot.outbound_100ms.p50_bytes, 588);
        assert_eq!(
            snapshot.outbound_100ms.total_bytes,
            (77_u64..=1_100).sum::<u64>()
        );
        assert_eq!(snapshot.outbound_1s.sample_count, 2);
        assert_eq!(snapshot.outbound_1s.total_bytes, 12_000);
        assert_eq!(snapshot.outbound_1s.p95_bytes, 8_000);
    }

    #[test]
    fn wgc_system_relative_clock_reports_queue_age_without_wall_clock_assumptions() {
        let base = Instant::now();
        let mut anchor = None;
        assert_eq!(
            relative_capture_queue_age(&mut anchor, 1_000_000, base),
            Some(Duration::ZERO)
        );
        // The capture clock advanced 10 ms while processing observed 14 ms.
        assert_eq!(
            relative_capture_queue_age(&mut anchor, 1_100_000, base + Duration::from_millis(14),),
            Some(Duration::from_millis(4))
        );
        // A capture-clock reset establishes a new anchor instead of underflowing.
        assert_eq!(
            relative_capture_queue_age(&mut anchor, 10, base + Duration::from_millis(20)),
            Some(Duration::ZERO)
        );
    }

    #[tokio::test]
    async fn h264_send_timeout_allows_ready_sends_and_stops_stalled_sends() {
        let ready = with_h264_send_timeout(Duration::from_millis(10), async {
            Ok::<(), axum::Error>(())
        })
        .await;
        assert!(ready.is_ok());

        let stalled = with_h264_send_timeout(
            Duration::from_millis(1),
            std::future::pending::<Result<(), axum::Error>>(),
        )
        .await;
        assert!(stalled.is_err());
    }

    #[tokio::test]
    async fn mjpeg_body_backpressure_is_bounded_and_counted() {
        let metrics = ScreenShareMediaMetrics::new();
        let (sender, _receiver) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(1);
        assert!(send_mjpeg_body_chunk_with_timeout(
            Duration::from_millis(10),
            &sender,
            Bytes::from_static(b"first"),
            &metrics,
        )
        .await
        .is_ok());
        assert!(send_mjpeg_body_chunk_with_timeout(
            Duration::from_millis(1),
            &sender,
            Bytes::from_static(b"blocked"),
            &metrics,
        )
        .await
        .is_err());
        let snapshot = metrics.snapshot(None);
        assert_eq!(snapshot.stream_send_timeout_count, 1);
        assert_eq!(snapshot.stream_send_wait.total_sample_count, 2);
    }

    fn test_http_state() -> Arc<HttpServerState> {
        let (broadcast_tx, _) = broadcast::channel(8);
        Arc::new(HttpServerState {
            events: Arc::new(TestScreenShareEvents),
            broadcast_tx,
            interaction: InteractionState::new(77),
            viewer_count: Arc::new(AtomicU32::new(0)),
            viewer_ip_reference_count: Arc::new(AtomicU32::new(0)),
            active_media_task_count: Arc::new(AtomicU32::new(0)),
            mjpeg_viewer_count: Arc::new(AtomicU32::new(0)),
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
            transport: Arc::new(Mutex::new(ScreenShareMediaTransport::Mjpeg)),
            input_worker: None,
            #[cfg(feature = "screen-share-webrtc-prototype")]
            webrtc: None,
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
    async fn screen_share_router_removed_snapshot_and_single_paths_stay_removed() {
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
            .record_frame(Arc::new(Bytes::from_static(b"jpeg-frame")));

        let single_frame = app
            .clone()
            .oneshot(http_request("/stream?single=1"))
            .await
            .unwrap();
        assert_eq!(single_frame.status(), StatusCode::BAD_REQUEST);

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
        assert_eq!(status_json["frozen_frame_id"], serde_json::Value::Null);
        assert_eq!(status_json["view_mode"], "live");
        assert_eq!(status_json["transport"], "mjpeg");
        assert_eq!(status_json["h264_media"]["ready"], false);

        let time = app.clone().oneshot(http_request("/time")).await.unwrap();
        assert_eq!(time.status(), StatusCode::OK);
        let time_json: serde_json::Value = serde_json::from_slice(
            &to_bytes(time.into_body(), usize::MAX)
                .await
                .expect("time body"),
        )
        .expect("time JSON");
        assert!(time_json["server_unix_ms"].as_u64().is_some());

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

    #[test]
    fn capture_trace_rejects_input_applied_after_capture() {
        let applied = AppliedInputSnapshot {
            client_sequence: 17,
            applied_at_server_unix_ms: 1_001,
        };

        assert_eq!(
            applied_input_visible_at_capture(Some(applied.clone()), 1_001),
            Some(applied.clone())
        );
        assert_eq!(applied_input_visible_at_capture(Some(applied), 1_000), None);
        assert_eq!(applied_input_visible_at_capture(None, 1_001), None);
    }

    #[test]
    fn h264_media_trace_sidecar_preserves_capture_and_media_identity() {
        let segment = H264MediaSegment {
            generation: 3,
            sequence: 44,
            keyframe: true,
            timestamp_us: 500_000,
            duration_us: 33_333,
            capture_sequence: 91,
            captured_at_unix_ms: 1_700_000_000_123,
            visible_input_sequence: Some(17),
            input_applied_at_server_unix_ms: Some(1_700_000_000_100),
            access_unit_avcc: Arc::new(Bytes::from_static(b"access-unit")),
            bytes: Arc::new(Bytes::from_static(b"fragment")),
        };

        let message: serde_json::Value =
            serde_json::from_str(&h264_media_trace_message(&segment)).unwrap();
        assert_eq!(message["type"], "media.trace");
        assert_eq!(message["generation"], 3);
        assert_eq!(message["sequence"], 44);
        assert_eq!(message["keyframe"], true);
        assert_eq!(message["timestamp_us"], 500_000);
        assert_eq!(message["duration_us"], 33_333);
        assert_eq!(message["capture_sequence"], 91);
        assert_eq!(message["captured_at_unix_ms"], 1_700_000_000_123_u64);
        assert_eq!(message["visible_input_sequence"], 17);
        assert_eq!(
            message["input_applied_at_server_unix_ms"],
            1_700_000_000_100_u64
        );
    }

    #[test]
    fn webcodecs_lag_recovery_refreshes_only_changed_generation() {
        let descriptor = Arc::new(H264StreamDescriptor {
            generation: 4,
            codec: "avc1.42C028".to_owned(),
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_bps: 3_000_000,
            init_segment: Arc::new(Bytes::new()),
            decoder_configuration: Arc::new(Bytes::from_static(&[1, 0x42, 0xc0, 0x28])),
        });

        assert!(webcodecs_newer_descriptor(4, Some(descriptor.clone())).is_none());
        assert_eq!(
            webcodecs_newer_descriptor(3, Some(descriptor))
                .expect("changed generation")
                .generation,
            4
        );
        assert!(webcodecs_newer_descriptor(3, None).is_none());
    }

    #[test]
    fn webcodecs_wire_preserves_complete_access_unit_and_exact_header() {
        let segment = H264MediaSegment {
            generation: 7,
            sequence: 99,
            keyframe: true,
            timestamp_us: 1_234_567,
            duration_us: 16_667,
            capture_sequence: 5,
            captured_at_unix_ms: 10,
            visible_input_sequence: None,
            input_applied_at_server_unix_ms: None,
            access_unit_avcc: Arc::new(Bytes::from_static(&[
                0, 0, 0, 2, 0x67, 0x64, 0, 0, 0, 3, 0x65, 1, 2,
            ])),
            bytes: Arc::new(Bytes::new()),
        };
        let wire = webcodecs_access_unit_message(&segment, true).unwrap();
        assert_eq!(&wire[0..4], b"FSTW");
        assert_eq!(wire[4], 1);
        assert_eq!(wire[5], 0b0101);
        assert_eq!(u16::from_be_bytes(wire[6..8].try_into().unwrap()), 40);
        assert_eq!(u64::from_be_bytes(wire[8..16].try_into().unwrap()), 7);
        assert_eq!(u64::from_be_bytes(wire[16..24].try_into().unwrap()), 99);
        assert_eq!(
            u64::from_be_bytes(wire[24..32].try_into().unwrap()),
            1_234_567
        );
        assert_eq!(u32::from_be_bytes(wire[32..36].try_into().unwrap()), 16_667);
        assert_eq!(
            u32::from_be_bytes(wire[36..40].try_into().unwrap()) as usize,
            segment.access_unit_avcc.len()
        );
        assert_eq!(&wire[40..], segment.access_unit_avcc.as_ref());

        let mut invalid = segment.clone();
        invalid.generation = 0;
        assert!(webcodecs_access_unit_message(&invalid, false).is_err());
        invalid = segment.clone();
        invalid.sequence = 0;
        assert!(webcodecs_access_unit_message(&invalid, false).is_err());
        invalid = segment.clone();
        invalid.duration_us = 0;
        assert!(webcodecs_access_unit_message(&invalid, false).is_err());
        invalid = segment.clone();
        invalid.access_unit_avcc = Arc::new(Bytes::new());
        assert!(webcodecs_access_unit_message(&invalid, false).is_err());
        invalid.access_unit_avcc =
            Arc::new(Bytes::from(vec![0; WEBCODECS_MAX_ACCESS_UNIT_BYTES + 1]));
        assert!(webcodecs_access_unit_message(&invalid, false).is_err());
    }

    #[test]
    fn webcodecs_descriptor_contains_avcc_configuration() {
        let descriptor = H264StreamDescriptor {
            generation: 4,
            codec: "avc1.42C028".to_owned(),
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_bps: 5_000_000,
            init_segment: Arc::new(Bytes::new()),
            decoder_configuration: Arc::new(Bytes::from_static(&[1, 0x42, 0xc0, 0x28])),
        };
        let message: serde_json::Value =
            serde_json::from_str(&webcodecs_descriptor_message(&descriptor).unwrap()).unwrap();
        assert_eq!(message["transport"], "webcodecs_h264");
        assert_eq!(message["generation"], 4);
        assert_eq!(message["codec"], "avc1.42C028");
        assert_eq!(message["description_base64"], "AULAKA==");
        assert!(message["color_space"].is_null());

        let mut invalid = descriptor;
        invalid.decoder_configuration = Arc::new(Bytes::new());
        assert!(webcodecs_descriptor_message(&invalid).is_err());
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
    fn screen_share_login_page_includes_svg_favicon() {
        let login = login_html(false, false);

        assert!(login.contains(r#"<link rel="icon" type="image/svg+xml""#));
        assert!(login.contains("data:image/svg+xml"));
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
        assert!(!config.control_requests_enabled);
        assert!(!config.keyboard_control_enabled);
        assert_eq!(config.transport, ScreenShareMediaTransport::Auto);
    }

    #[test]
    fn h264_selector_preserves_explicit_mjpeg_and_webrtc_fallback() {
        assert!(ScreenShareMediaTransport::Auto.wants_h264());
        assert!(ScreenShareMediaTransport::MseH264.wants_h264());
        assert!(ScreenShareMediaTransport::WebCodecs.wants_h264());
        assert!(ScreenShareMediaTransport::WebRtc.wants_h264());
        assert!(!ScreenShareMediaTransport::Mjpeg.wants_h264());
        assert_eq!(ScreenShareMediaTransport::Mjpeg.resolved_label(), "mjpeg");
        assert_eq!(
            ScreenShareMediaTransport::WebCodecs.resolved_label(),
            "web_codecs"
        );
        assert_eq!(
            ScreenShareMediaTransport::WebRtc.resolved_label(),
            "web_rtc"
        );
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
        handle.desktop_overlay_active.store(true, Ordering::SeqCst);
        record_viewer_connection(&handle.viewer_ips, "10.0.0.1");

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
        handle.desktop_overlay_active.store(true, Ordering::SeqCst);
        record_viewer_connection(&handle.viewer_ips, "10.0.0.2");

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
    fn snapshot_viewer_ips_contains_only_sorted_active_media_leases() {
        let ips = Arc::new(Mutex::new(std::collections::HashMap::new()));
        record_viewer_connection(&ips, "10.0.0.2");
        record_viewer_connection(&ips, "10.0.0.1");
        assert_eq!(
            snapshot_viewer_ips(&ips),
            vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()]
        );
    }

    #[test]
    fn active_viewer_ip_survives_partial_disconnect_and_is_removed_after_last_connection() {
        let ips = Arc::new(Mutex::new(std::collections::HashMap::new()));
        record_viewer_connection(&ips, "10.0.0.1");
        record_viewer_connection(&ips, "10.0.0.1");

        release_viewer_connection(&ips, "10.0.0.1");
        assert_eq!(snapshot_viewer_ips(&ips), vec!["10.0.0.1".to_string()]);

        release_viewer_connection(&ips, "10.0.0.1");
        assert!(snapshot_viewer_ips(&ips).is_empty());
    }

    #[test]
    fn viewer_guard_releases_all_runtime_accounting() {
        let viewers = Arc::new(AtomicU32::new(1));
        let ip_references = Arc::new(AtomicU32::new(1));
        let tasks = Arc::new(AtomicU32::new(1));
        let transport = Arc::new(AtomicU32::new(1));
        let ips = Arc::new(Mutex::new(std::collections::HashMap::new()));
        record_viewer_connection(&ips, "10.0.0.1");
        drop(ViewerGuard {
            events: Arc::new(TestScreenShareEvents),
            count: viewers.clone(),
            ip_reference_count: ip_references.clone(),
            active_task_count: tasks.clone(),
            transport_count: Some(transport.clone()),
            ips: ips.clone(),
            ip: "10.0.0.1".to_string(),
        });
        assert_eq!(viewers.load(Ordering::Relaxed), 0);
        assert_eq!(ip_references.load(Ordering::Relaxed), 0);
        assert_eq!(tasks.load(Ordering::Relaxed), 0);
        assert_eq!(transport.load(Ordering::Relaxed), 0);
        assert!(snapshot_viewer_ips(&ips).is_empty());
    }

    #[test]
    fn media_viewer_limit_is_atomic_and_releases_capacity() {
        let viewers = AtomicU32::new(0);
        for expected in 1..=MAX_MEDIA_VIEWERS {
            assert_eq!(try_reserve_media_viewer(&viewers), Some(expected));
        }
        assert_eq!(try_reserve_media_viewer(&viewers), None);
        assert_eq!(decrement_nonzero(&viewers), MAX_MEDIA_VIEWERS - 1);
        assert_eq!(try_reserve_media_viewer(&viewers), Some(MAX_MEDIA_VIEWERS));
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
