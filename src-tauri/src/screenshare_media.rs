use bytes::Bytes;
use mse_fmp4::avc::AvcDecoderConfigurationRecord;
use mse_fmp4::fmp4::{
    AvcConfigurationBox, AvcSampleEntry, FileTypeBox, InitializationSegment, MovieBox,
    MovieExtendsBox, MovieHeaderBox, SampleEntry, TrackBox, TrackExtendsBox,
};
use mse_fmp4::io::WriteTo;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

#[cfg(target_os = "windows")]
use crate::screenshare_gpu::{create_mf_dxgi_device_manager, GpuNv12Surface, MfDxgiDeviceManager};
#[cfg(target_os = "windows")]
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncCallback_Impl, IMFAsyncResult,
};

const H264_EVENT_CAPACITY: usize = 96;
const H264_INPUT_CAPACITY: usize = 2;
const H264_GOP_CACHE_LIMIT: usize = 180;
const H264_TIMESCALE: u32 = 90_000;
const H264_KEYFRAME_INTERVAL_100NS: i64 = 20_000_000;
const H264_KEYFRAME_REQUEST_MERGE_WINDOW: Duration = Duration::from_millis(200);
const H264_KEYFRAME_REQUEST_MIN_INTERVAL: Duration = Duration::from_millis(500);
const H264_METRIC_SAMPLE_LIMIT: usize = 512;
const H264_METRIC_MEASUREMENT_SCOPE: &str = "cumulative_count_with_rolling_distribution";
const H264_ENCODER_SELF_TEST_TIMEOUT: Duration = Duration::from_millis(1_800);

#[derive(Debug, Clone)]
pub struct H264StreamDescriptor {
    pub generation: u64,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u8,
    pub bitrate_bps: u32,
    pub init_segment: Arc<Bytes>,
    /// AVCDecoderConfigurationRecord (`avcC` payload), without the MP4 box header.
    pub decoder_configuration: Arc<Bytes>,
}

#[derive(Debug, Clone)]
pub struct H264MediaSegment {
    pub generation: u64,
    pub sequence: u64,
    pub keyframe: bool,
    pub timestamp_us: u64,
    pub duration_us: u64,
    pub capture_sequence: u64,
    pub captured_at_unix_ms: u64,
    pub visible_input_sequence: Option<u64>,
    pub input_applied_at_server_unix_ms: Option<u64>,
    /// One complete AVC-format access unit (4-byte length-prefixed NAL units).
    pub access_unit_avcc: Arc<Bytes>,
    pub bytes: Arc<Bytes>,
}

#[derive(Debug, Clone)]
pub enum H264MediaEvent {
    Reset(Arc<H264StreamDescriptor>),
    Segment(Arc<H264MediaSegment>),
    Unavailable { generation: u64, error: String },
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct H264StreamSnapshot {
    pub descriptor: Arc<H264StreamDescriptor>,
    pub segments: Vec<Arc<H264MediaSegment>>,
}

/// Result of asking the encoder for a keyframe for a specific codec generation.
///
/// A scheduled request may be delayed by the merge window or minimum dispatch
/// interval. Callers for the same generation share the pending request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264KeyframeRequestResult {
    Scheduled,
    Coalesced,
    StaleGeneration,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264MediaMetricsSnapshot {
    pub ready: bool,
    pub codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub target_bitrate_bps: Option<u32>,
    pub encoded_frame_count: u64,
    pub encoded_bytes: u64,
    pub keyframe_count: u64,
    pub cached_segment_count: u32,
    pub cached_bytes: u64,
    pub dropped_input_frames: u64,
    pub encoder_name: Option<String>,
    pub encoder_hardware: Option<bool>,
    pub encoder_async_mode: Option<bool>,
    pub encoder_adapter_luid: Option<String>,
    pub encoder_hardware_url: Option<String>,
    pub encoder_driver_version: Option<String>,
    pub encoder_fallback_reason: Option<String>,
    pub encoder_input_width: Option<u32>,
    pub encoder_input_height: Option<u32>,
    pub encoder_fps: Option<u8>,
    pub software_fallback_limited: bool,
    pub encoder_self_test: H264EncoderSelfTestSnapshot,
    pub capabilities: H264EncoderCapabilitiesSnapshot,
    pub encoder_candidate_report_total_count: u32,
    pub encoder_candidate_report_capacity: u32,
    pub encoder_candidate_reports: Vec<H264EncoderCandidateReport>,
    pub runtime_bitrate_update_count: u64,
    pub runtime_bitrate_update_failure_count: u64,
    pub runtime_bitrate_update_error: Option<String>,
    pub input_queue_age: H264DistributionSnapshot,
    pub bgra_to_nv12: H264DistributionSnapshot,
    pub gpu_preprocess: H264DistributionSnapshot,
    pub gpu_backpressure_dropped_frames: u64,
    pub gpu_fallback_count: u64,
    pub gpu_pipeline_active: bool,
    pub gpu_fallback_reason: Option<String>,
    pub mft_encode: H264DistributionSnapshot,
    pub mux: H264DistributionSnapshot,
    pub idr_size_bytes: H264DistributionSnapshot,
    pub idr_request_scheduled_count: u64,
    pub idr_request_coalesced_count: u64,
    pub idr_request_rate_limited_count: u64,
    pub idr_request_stale_count: u64,
    pub idr_request_dispatch_count: u64,
    pub idr_force_failure_count: u64,
    pub error: Option<String>,
}

/// Evidence collected before an encoder candidate is admitted to the live
/// pipeline. A candidate that fails this check is discarded; consequently a
/// live encoder normally reports `passed = true`.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264EncoderSelfTestSnapshot {
    pub attempted: bool,
    pub passed: bool,
    pub duration_ms: u64,
    pub produced_access_units: u32,
    pub found_sps: bool,
    pub found_pps: bool,
    pub found_idr: bool,
    pub timeline_monotonic: bool,
    pub timestamps_from_encoder: bool,
    pub durations_from_encoder: bool,
    pub baseline_profile_confirmed: bool,
    pub b_slice_count: u32,
    pub decoder_frame_count: u32,
    pub gpu_surface_input: bool,
    pub dynamic_pattern_input: bool,
    pub failure_reason: Option<String>,
}

/// Bounded rolling distribution. Timing distributions are expressed in
/// microseconds; `idr_size_bytes` uses the same shape with byte values.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264DistributionSnapshot {
    pub sample_count: u64,
    pub retained_sample_count: u32,
    pub retained_sample_capacity: u32,
    pub measurement_scope: &'static str,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

/// Result of negotiating one optional Media Foundation encoder feature.
///
/// `supported` reports `ICodecAPI::IsSupported`; the other fields distinguish
/// an advertised-but-unwritable property from a value that the MFT accepted
/// and returned unchanged. Optional feature failure is deliberately visible
/// here instead of making the whole encoder unavailable.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264EncoderCapabilitySnapshot {
    pub supported: bool,
    pub modifiable: bool,
    pub set_succeeded: bool,
    pub readback_succeeded: bool,
    pub value_matches: bool,
    pub requested_value: Option<String>,
    pub final_value: Option<String>,
    pub hresult: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264EncoderCapabilitiesSnapshot {
    pub low_latency: H264EncoderCapabilitySnapshot,
    pub rate_control: H264EncoderCapabilitySnapshot,
    pub rate_control_attempts: Vec<H264EncoderCapabilitySnapshot>,
    pub buffer_size: H264EncoderCapabilitySnapshot,
    pub max_bitrate: H264EncoderCapabilitySnapshot,
    pub reference_frames: H264EncoderCapabilitySnapshot,
    pub cabac: H264EncoderCapabilitySnapshot,
    pub b_frames_disabled: H264EncoderCapabilitySnapshot,
    pub dynamic_bitrate: H264EncoderCapabilitySnapshot,
    pub force_keyframe: H264EncoderCapabilitySnapshot,
    pub degradation_reasons: Vec<String>,
}

/// One encoder admission attempt. Keeping rejected candidates visible is
/// essential on mixed Intel driver generations: a bounded fallback string is
/// useful for the UI, but is not sufficient evidence for a capability matrix.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264DxgiAdapterIdentity {
    pub description: Option<String>,
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub luid: Option<String>,
    pub driver_version: Option<String>,
    pub pnp_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct H264EncoderCandidateReport {
    pub name: String,
    pub hardware: bool,
    pub adapter_luid: Option<String>,
    pub activation_adapter_luid: Option<String>,
    pub input_adapter: Option<H264DxgiAdapterIdentity>,
    pub luid_match: Option<bool>,
    pub gpu_surface_pool_recycled: Option<bool>,
    pub hardware_url: Option<String>,
    pub driver_version: Option<String>,
    pub input_width: u32,
    pub input_height: u32,
    pub fps: u8,
    pub gpu_surface_input: bool,
    pub activation_succeeded: bool,
    pub configuration_succeeded: bool,
    pub admitted: bool,
    pub failure_stage: Option<String>,
    pub failure_reason: Option<String>,
    pub self_test: H264EncoderSelfTestSnapshot,
    pub capabilities: H264EncoderCapabilitiesSnapshot,
}

#[derive(Debug, Clone, Default)]
struct H264EncoderDiagnostics {
    name: String,
    hardware: bool,
    async_mode: bool,
    adapter_luid: Option<String>,
    hardware_url: Option<String>,
    driver_version: Option<String>,
    fallback_reason: Option<String>,
    input_width: u32,
    input_height: u32,
    fps: u8,
    software_fallback_limited: bool,
    self_test: H264EncoderSelfTestSnapshot,
    capabilities: H264EncoderCapabilitiesSnapshot,
    candidate_report_total_count: u32,
    candidate_reports: Vec<H264EncoderCandidateReport>,
}

struct H264PendingKeyframeRequest {
    generation: u64,
    not_before: Instant,
}

#[derive(Default)]
struct H264BoundedSamples {
    values: Mutex<VecDeque<u64>>,
    total_count: AtomicU64,
}

impl H264BoundedSamples {
    fn record(&self, value: u64) {
        self.total_count.fetch_add(1, Ordering::Relaxed);
        let Ok(mut values) = self.values.lock() else {
            return;
        };
        if values.len() >= H264_METRIC_SAMPLE_LIMIT {
            values.pop_front();
        }
        values.push_back(value);
    }

    fn snapshot(&self) -> H264DistributionSnapshot {
        let mut values = self
            .values
            .lock()
            .map(|values| values.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        if values.is_empty() {
            return H264DistributionSnapshot {
                sample_count: self.total_count.load(Ordering::Relaxed),
                retained_sample_capacity: H264_METRIC_SAMPLE_LIMIT as u32,
                measurement_scope: H264_METRIC_MEASUREMENT_SCOPE,
                ..Default::default()
            };
        }
        values.sort_unstable();
        H264DistributionSnapshot {
            sample_count: self.total_count.load(Ordering::Relaxed),
            retained_sample_count: values.len().min(u32::MAX as usize) as u32,
            retained_sample_capacity: H264_METRIC_SAMPLE_LIMIT as u32,
            measurement_scope: H264_METRIC_MEASUREMENT_SCOPE,
            p50: percentile(&values, 50),
            p95: percentile(&values, 95),
            p99: percentile(&values, 99),
            max: values.last().copied().unwrap_or_default(),
        }
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = sorted
        .len()
        .saturating_mul(percentile)
        .saturating_add(99)
        .saturating_div(100)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

#[derive(Default)]
struct H264MediaInner {
    generation: u64,
    descriptor: Option<Arc<H264StreamDescriptor>>,
    segments: VecDeque<Arc<H264MediaSegment>>,
    error: Option<String>,
    pending_keyframe_request: Option<H264PendingKeyframeRequest>,
    last_keyframe_request_dispatch: Option<Instant>,
    encoder_diagnostics: Option<H264EncoderDiagnostics>,
    runtime_bitrate_update_error: Option<String>,
    gpu_pipeline_active: bool,
    gpu_fallback_reason: Option<String>,
}

pub struct H264MediaState {
    inner: Mutex<H264MediaInner>,
    events: broadcast::Sender<Arc<H264MediaEvent>>,
    encoded_frames: AtomicU64,
    encoded_bytes: AtomicU64,
    keyframes: AtomicU64,
    dropped_input_frames: AtomicU64,
    runtime_bitrate_updates: AtomicU64,
    runtime_bitrate_update_failures: AtomicU64,
    input_queue_age_us: H264BoundedSamples,
    bgra_to_nv12_us: H264BoundedSamples,
    gpu_preprocess_us: H264BoundedSamples,
    gpu_backpressure_dropped_frames: AtomicU64,
    gpu_fallback_count: AtomicU64,
    mft_encode_us: H264BoundedSamples,
    mux_us: H264BoundedSamples,
    idr_size_bytes: H264BoundedSamples,
    idr_request_scheduled: AtomicU64,
    idr_request_coalesced: AtomicU64,
    idr_request_rate_limited: AtomicU64,
    idr_request_stale: AtomicU64,
    idr_request_dispatch: AtomicU64,
    idr_force_failures: AtomicU64,
}

impl H264MediaState {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(H264_EVENT_CAPACITY);
        Self {
            inner: Mutex::new(H264MediaInner::default()),
            events,
            encoded_frames: AtomicU64::new(0),
            encoded_bytes: AtomicU64::new(0),
            keyframes: AtomicU64::new(0),
            dropped_input_frames: AtomicU64::new(0),
            runtime_bitrate_updates: AtomicU64::new(0),
            runtime_bitrate_update_failures: AtomicU64::new(0),
            input_queue_age_us: H264BoundedSamples::default(),
            bgra_to_nv12_us: H264BoundedSamples::default(),
            gpu_preprocess_us: H264BoundedSamples::default(),
            gpu_backpressure_dropped_frames: AtomicU64::new(0),
            gpu_fallback_count: AtomicU64::new(0),
            mft_encode_us: H264BoundedSamples::default(),
            mux_us: H264BoundedSamples::default(),
            idr_size_bytes: H264BoundedSamples::default(),
            idr_request_scheduled: AtomicU64::new(0),
            idr_request_coalesced: AtomicU64::new(0),
            idr_request_rate_limited: AtomicU64::new(0),
            idr_request_stale: AtomicU64::new(0),
            idr_request_dispatch: AtomicU64::new(0),
            idr_force_failures: AtomicU64::new(0),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<H264MediaEvent>> {
        self.events.subscribe()
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> Option<H264StreamSnapshot> {
        let inner = self.inner.lock().ok()?;
        let descriptor = inner.descriptor.clone()?;
        let first = inner.segments.front()?;
        if !first.keyframe
            || first.generation != descriptor.generation
            || inner
                .segments
                .iter()
                .any(|segment| segment.generation != descriptor.generation)
            || inner
                .segments
                .iter()
                .zip(inner.segments.iter().skip(1))
                .any(|(previous, current)| {
                    current.sequence != previous.sequence.saturating_add(1)
                        || current.sequence <= previous.sequence
                })
        {
            return None;
        }
        Some(H264StreamSnapshot {
            descriptor,
            segments: inner.segments.iter().cloned().collect(),
        })
    }

    /// Returns the current codec descriptor independently of GOP cache state.
    ///
    /// A transport can subscribe first, read this descriptor, and request a
    /// keyframe even when there is deliberately no replayable snapshot.
    pub fn descriptor(&self) -> Option<Arc<H264StreamDescriptor>> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.descriptor.clone())
    }

    /// Coalesces and rate-limits a keyframe request for `generation`.
    pub fn request_keyframe(&self, generation: u64) -> H264KeyframeRequestResult {
        self.request_keyframe_at(generation, Instant::now())
    }

    fn request_keyframe_at(&self, generation: u64, now: Instant) -> H264KeyframeRequestResult {
        let mut inner = self.inner.lock().unwrap();
        if inner
            .descriptor
            .as_ref()
            .map_or(true, |descriptor| descriptor.generation != generation)
        {
            self.idr_request_stale.fetch_add(1, Ordering::Relaxed);
            return H264KeyframeRequestResult::StaleGeneration;
        }
        if inner
            .pending_keyframe_request
            .as_ref()
            .is_some_and(|request| request.generation == generation)
        {
            self.idr_request_coalesced.fetch_add(1, Ordering::Relaxed);
            return H264KeyframeRequestResult::Coalesced;
        }
        let merge_deadline = now + H264_KEYFRAME_REQUEST_MERGE_WINDOW;
        let rate_limit_deadline = inner
            .last_keyframe_request_dispatch
            .and_then(|last| last.checked_add(H264_KEYFRAME_REQUEST_MIN_INTERVAL));
        if rate_limit_deadline.is_some_and(|deadline| deadline > merge_deadline) {
            self.idr_request_rate_limited
                .fetch_add(1, Ordering::Relaxed);
        }
        inner.pending_keyframe_request = Some(H264PendingKeyframeRequest {
            generation,
            not_before: rate_limit_deadline
                .map_or(merge_deadline, |deadline| deadline.max(merge_deadline)),
        });
        self.idr_request_scheduled.fetch_add(1, Ordering::Relaxed);
        H264KeyframeRequestResult::Scheduled
    }

    fn take_keyframe_request(&self, generation: u64) -> bool {
        self.take_keyframe_request_at(generation, Instant::now())
    }

    fn take_keyframe_request_at(&self, generation: u64, now: Instant) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner
            .descriptor
            .as_ref()
            .map_or(true, |descriptor| descriptor.generation != generation)
        {
            return false;
        }
        let Some(request) = inner.pending_keyframe_request.as_ref() else {
            return false;
        };
        if request.generation != generation || now < request.not_before {
            return false;
        }
        inner.pending_keyframe_request = None;
        inner.last_keyframe_request_dispatch = Some(now);
        self.idr_request_dispatch.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn is_ready(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.descriptor.is_some())
            .unwrap_or(false)
    }

    pub fn metrics(&self) -> H264MediaMetricsSnapshot {
        let inner = self.inner.lock().ok();
        let descriptor = inner.as_ref().and_then(|inner| inner.descriptor.as_ref());
        let diagnostics = inner
            .as_ref()
            .and_then(|inner| inner.encoder_diagnostics.as_ref());
        H264MediaMetricsSnapshot {
            ready: descriptor.is_some(),
            codec: descriptor.map(|descriptor| descriptor.codec.clone()),
            width: descriptor.map(|descriptor| descriptor.width),
            height: descriptor.map(|descriptor| descriptor.height),
            target_bitrate_bps: descriptor.map(|descriptor| descriptor.bitrate_bps),
            encoded_frame_count: self.encoded_frames.load(Ordering::Relaxed),
            encoded_bytes: self.encoded_bytes.load(Ordering::Relaxed),
            keyframe_count: self.keyframes.load(Ordering::Relaxed),
            cached_segment_count: inner
                .as_ref()
                .map(|inner| inner.segments.len().min(u32::MAX as usize) as u32)
                .unwrap_or(0),
            cached_bytes: inner
                .as_ref()
                .map(|inner| {
                    inner
                        .segments
                        .iter()
                        .map(|segment| segment.bytes.len() as u64)
                        .sum()
                })
                .unwrap_or(0),
            dropped_input_frames: self.dropped_input_frames.load(Ordering::Relaxed),
            encoder_name: diagnostics.map(|diagnostics| diagnostics.name.clone()),
            encoder_hardware: diagnostics.map(|diagnostics| diagnostics.hardware),
            encoder_async_mode: diagnostics.map(|diagnostics| diagnostics.async_mode),
            encoder_adapter_luid: diagnostics
                .and_then(|diagnostics| diagnostics.adapter_luid.clone()),
            encoder_hardware_url: diagnostics
                .and_then(|diagnostics| diagnostics.hardware_url.clone()),
            encoder_driver_version: diagnostics
                .and_then(|diagnostics| diagnostics.driver_version.clone()),
            encoder_fallback_reason: diagnostics
                .and_then(|diagnostics| diagnostics.fallback_reason.clone()),
            encoder_input_width: diagnostics.map(|diagnostics| diagnostics.input_width),
            encoder_input_height: diagnostics.map(|diagnostics| diagnostics.input_height),
            encoder_fps: diagnostics.map(|diagnostics| diagnostics.fps),
            software_fallback_limited: diagnostics
                .is_some_and(|diagnostics| diagnostics.software_fallback_limited),
            encoder_self_test: diagnostics
                .map(|diagnostics| diagnostics.self_test.clone())
                .unwrap_or_default(),
            capabilities: diagnostics
                .map(|diagnostics| diagnostics.capabilities.clone())
                .unwrap_or_default(),
            encoder_candidate_report_total_count: diagnostics
                .map(|diagnostics| diagnostics.candidate_report_total_count)
                .unwrap_or_default(),
            encoder_candidate_report_capacity: H264_ENCODER_CANDIDATE_REPORT_LIMIT as u32,
            encoder_candidate_reports: diagnostics
                .map(|diagnostics| diagnostics.candidate_reports.clone())
                .unwrap_or_default(),
            runtime_bitrate_update_count: self.runtime_bitrate_updates.load(Ordering::Relaxed),
            runtime_bitrate_update_failure_count: self
                .runtime_bitrate_update_failures
                .load(Ordering::Relaxed),
            runtime_bitrate_update_error: inner
                .as_ref()
                .and_then(|inner| inner.runtime_bitrate_update_error.clone()),
            input_queue_age: self.input_queue_age_us.snapshot(),
            bgra_to_nv12: self.bgra_to_nv12_us.snapshot(),
            gpu_preprocess: self.gpu_preprocess_us.snapshot(),
            gpu_backpressure_dropped_frames: self
                .gpu_backpressure_dropped_frames
                .load(Ordering::Relaxed),
            gpu_fallback_count: self.gpu_fallback_count.load(Ordering::Relaxed),
            gpu_pipeline_active: inner
                .as_ref()
                .is_some_and(|inner| inner.gpu_pipeline_active),
            gpu_fallback_reason: inner
                .as_ref()
                .and_then(|inner| inner.gpu_fallback_reason.clone()),
            mft_encode: self.mft_encode_us.snapshot(),
            mux: self.mux_us.snapshot(),
            idr_size_bytes: self.idr_size_bytes.snapshot(),
            idr_request_scheduled_count: self.idr_request_scheduled.load(Ordering::Relaxed),
            idr_request_coalesced_count: self.idr_request_coalesced.load(Ordering::Relaxed),
            idr_request_rate_limited_count: self.idr_request_rate_limited.load(Ordering::Relaxed),
            idr_request_stale_count: self.idr_request_stale.load(Ordering::Relaxed),
            idr_request_dispatch_count: self.idr_request_dispatch.load(Ordering::Relaxed),
            idr_force_failure_count: self.idr_force_failures.load(Ordering::Relaxed),
            error: inner.and_then(|inner| inner.error.clone()),
        }
    }

    fn set_encoder_diagnostics(&self, diagnostics: H264EncoderDiagnostics) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.encoder_diagnostics = Some(diagnostics);
            inner.runtime_bitrate_update_error = None;
        }
    }

    fn update_stream_bitrate(&self, generation: u64, bitrate_bps: u32) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let Some(current) = inner.descriptor.as_ref() else {
            return;
        };
        if current.generation != generation || current.bitrate_bps == bitrate_bps {
            return;
        }
        inner.descriptor = Some(Arc::new(H264StreamDescriptor {
            generation: current.generation,
            codec: current.codec.clone(),
            width: current.width,
            height: current.height,
            fps: current.fps,
            bitrate_bps,
            init_segment: current.init_segment.clone(),
            decoder_configuration: current.decoder_configuration.clone(),
        }));
    }

    fn record_runtime_bitrate_update(&self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.runtime_bitrate_updates.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut inner) = self.inner.lock() {
                    inner.runtime_bitrate_update_error = None;
                }
            }
            Err(error) => {
                self.runtime_bitrate_update_failures
                    .fetch_add(1, Ordering::Relaxed);
                if let Ok(mut inner) = self.inner.lock() {
                    inner.runtime_bitrate_update_error = Some(error);
                }
            }
        }
    }

    fn record_input_queue_age(&self, elapsed: Duration) {
        self.input_queue_age_us
            .record(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
    }

    fn record_bgra_to_nv12(&self, elapsed: Duration) {
        self.bgra_to_nv12_us
            .record(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
    }

    pub fn record_gpu_preprocess(&self, elapsed: Duration) {
        self.gpu_preprocess_us
            .record(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
    }

    pub fn record_gpu_backpressure_drop(&self) {
        self.gpu_backpressure_dropped_frames
            .fetch_add(1, Ordering::Relaxed);
        self.record_dropped_input();
    }

    pub fn set_gpu_pipeline_active(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.gpu_pipeline_active = true;
            inner.gpu_fallback_reason = None;
        }
    }

    pub fn record_gpu_fallback(&self, reason: String) {
        self.gpu_fallback_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut inner) = self.inner.lock() {
            inner.gpu_pipeline_active = false;
            inner.gpu_fallback_reason = Some(reason);
        }
    }

    fn record_mft_encode(&self, elapsed: Duration) {
        self.mft_encode_us
            .record(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
    }

    fn record_mux(&self, elapsed: Duration) {
        self.mux_us
            .record(elapsed.as_micros().min(u128::from(u64::MAX)) as u64);
    }

    fn record_idr_size(&self, size: usize) {
        self.idr_size_bytes.record(size as u64);
    }

    fn record_idr_force_failure(&self) {
        self.idr_force_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn install_stream(
        &self,
        codec: String,
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
        init_segment: Vec<u8>,
    ) -> u64 {
        let decoder_configuration = extract_avcc_decoder_configuration(&init_segment)
            .map(Bytes::from)
            .unwrap_or_default();
        let descriptor = {
            let mut inner = self.inner.lock().unwrap();
            inner.generation = inner.generation.saturating_add(1).max(1);
            inner.error = None;
            inner.segments.clear();
            inner.pending_keyframe_request = None;
            inner.last_keyframe_request_dispatch = None;
            let descriptor = Arc::new(H264StreamDescriptor {
                generation: inner.generation,
                codec,
                width,
                height,
                fps,
                bitrate_bps,
                init_segment: Arc::new(Bytes::from(init_segment)),
                decoder_configuration: Arc::new(decoder_configuration),
            });
            inner.descriptor = Some(descriptor.clone());
            descriptor
        };
        let generation = descriptor.generation;
        let _ = self
            .events
            .send(Arc::new(H264MediaEvent::Reset(descriptor)));
        generation
    }

    fn publish_segment(&self, segment: H264MediaSegment) {
        let segment = Arc::new(segment);
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.descriptor.as_ref().map_or(true, |descriptor| {
                descriptor.generation != segment.generation
            }) {
                return;
            }
            if segment.keyframe {
                inner.segments.clear();
                inner.segments.push_back(segment.clone());
            } else if let Some(previous) = inner.segments.back() {
                let is_contiguous = segment.sequence == previous.sequence.saturating_add(1)
                    && segment.sequence > previous.sequence;
                if !is_contiguous || inner.segments.len() >= H264_GOP_CACHE_LIMIT {
                    // A replay cache is useful only while it contains the whole
                    // dependency chain. Keep broadcasting live segments, but do
                    // not expose a truncated GOP to a newly joined viewer.
                    inner.segments.clear();
                } else {
                    inner.segments.push_back(segment.clone());
                }
            }
        }
        self.encoded_frames.fetch_add(1, Ordering::Relaxed);
        self.encoded_bytes
            .fetch_add(segment.bytes.len() as u64, Ordering::Relaxed);
        if segment.keyframe {
            self.keyframes.fetch_add(1, Ordering::Relaxed);
        }
        let _ = self.events.send(Arc::new(H264MediaEvent::Segment(segment)));
    }

    fn mark_unavailable(&self, error: String) {
        let generation = {
            let mut inner = self.inner.lock().unwrap();
            if inner.descriptor.is_none() && inner.error.as_deref() == Some(error.as_str()) {
                return;
            }
            inner.generation = inner.generation.saturating_add(1).max(1);
            inner.descriptor = None;
            inner.segments.clear();
            inner.error = Some(error.clone());
            inner.pending_keyframe_request = None;
            inner.last_keyframe_request_dispatch = None;
            inner.generation
        };
        let _ = self
            .events
            .send(Arc::new(H264MediaEvent::Unavailable { generation, error }));
    }

    fn record_dropped_input(&self) {
        self.dropped_input_frames.fetch_add(1, Ordering::Relaxed);
    }
}

enum H264InputPayload {
    Cpu {
        pixels: Vec<u8>,
        stride: usize,
    },
    #[cfg(target_os = "windows")]
    Gpu(GpuNv12Surface),
}

struct H264InputFrame {
    payload: H264InputPayload,
    width: usize,
    height: usize,
    captured_at_100ns: i64,
    enqueued_at: Instant,
    capture_sequence: u64,
    captured_at_unix_ms: u64,
    visible_input_sequence: Option<u64>,
    input_applied_at_server_unix_ms: Option<u64>,
}

pub struct H264EncoderWorker {
    sender: SyncSender<H264InputFrame>,
    state: Arc<H264MediaState>,
    origin: Instant,
    pending_bitrate_bps: Arc<AtomicU64>,
    submit_sequence: AtomicU64,
    gpu_input_state: Arc<AtomicU8>,
}

const GPU_INPUT_PROBING: u8 = 0;
const GPU_INPUT_ACTIVE: u8 = 1;
const GPU_INPUT_DISABLED: u8 = 2;

impl H264EncoderWorker {
    pub fn spawn(state: Arc<H264MediaState>, fps: u8, quality: u8) -> Result<Self, String> {
        let (sender, receiver) = sync_channel(H264_INPUT_CAPACITY);
        let thread_state = state.clone();
        let pending_bitrate_bps = Arc::new(AtomicU64::new(0));
        let thread_pending_bitrate_bps = pending_bitrate_bps.clone();
        let gpu_input_state = Arc::new(AtomicU8::new(GPU_INPUT_PROBING));
        let thread_gpu_input_state = gpu_input_state.clone();
        std::thread::Builder::new()
            .name("screen-h264-encoder".into())
            .spawn(move || {
                run_encoder_worker(
                    receiver,
                    thread_state,
                    fps,
                    quality,
                    thread_pending_bitrate_bps,
                    thread_gpu_input_state,
                )
            })
            .map_err(|error| format!("Failed to spawn H.264 encoder thread: {error}"))?;
        Ok(Self {
            sender,
            state,
            origin: Instant::now(),
            pending_bitrate_bps,
            submit_sequence: AtomicU64::new(0),
            gpu_input_state,
        })
    }

    /// Schedules a best-effort in-place bitrate change on the encoder thread.
    ///
    /// The latest request wins. The negotiated dynamic-bitrate capability and
    /// the eventual success/failure are exposed through media metrics.
    pub fn request_runtime_bitrate_update(&self, bitrate_bps: u32) -> Result<(), String> {
        if !(100_000..=50_000_000).contains(&bitrate_bps) {
            return Err("H.264 runtime bitrate must be between 100 kbps and 50 Mbps".to_string());
        }
        self.pending_bitrate_bps
            .store(u64::from(bitrate_bps), Ordering::Release);
        Ok(())
    }

    pub fn try_submit(&self, bgra: &[u8], width: usize, height: usize, stride: usize) -> bool {
        let capture_sequence = self.submit_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let captured_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        self.try_submit_with_metadata(
            bgra,
            width,
            height,
            stride,
            capture_sequence,
            captured_at_unix_ms,
            None,
            None,
        )
    }

    pub fn try_submit_with_metadata(
        &self,
        bgra: &[u8],
        width: usize,
        height: usize,
        stride: usize,
        capture_sequence: u64,
        captured_at_unix_ms: u64,
        visible_input_sequence: Option<u64>,
        input_applied_at_server_unix_ms: Option<u64>,
    ) -> bool {
        if width < 2 || height < 2 || stride < width.saturating_mul(4) {
            return false;
        }
        let required = match height.checked_mul(stride) {
            Some(required) if required <= bgra.len() => required,
            _ => return false,
        };
        let captured_at_100ns =
            self.origin.elapsed().as_nanos().min(i64::MAX as u128 * 100) as i64 / 100;
        let frame = H264InputFrame {
            payload: H264InputPayload::Cpu {
                pixels: bgra[..required].to_vec(),
                stride,
            },
            width,
            height,
            captured_at_100ns,
            enqueued_at: Instant::now(),
            capture_sequence,
            captured_at_unix_ms,
            visible_input_sequence,
            input_applied_at_server_unix_ms,
        };
        match self.sender.try_send(frame) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.state.record_dropped_input();
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                self.state
                    .mark_unavailable("H.264 encoder worker stopped".to_string());
                false
            }
        }
    }

    #[cfg(target_os = "windows")]
    pub fn gpu_input_allowed(&self) -> bool {
        self.gpu_input_state.load(Ordering::Acquire) != GPU_INPUT_DISABLED
    }

    #[cfg(target_os = "windows")]
    pub fn try_submit_gpu_with_metadata(
        &self,
        surface: GpuNv12Surface,
        capture_sequence: u64,
        captured_at_unix_ms: u64,
        visible_input_sequence: Option<u64>,
        input_applied_at_server_unix_ms: Option<u64>,
    ) -> bool {
        if !self.gpu_input_allowed() {
            let _ = surface.release_after_encoder_done();
            return false;
        }
        let captured_at_100ns =
            self.origin.elapsed().as_nanos().min(i64::MAX as u128 * 100) as i64 / 100;
        let frame = H264InputFrame {
            width: surface.width() as usize,
            height: surface.height() as usize,
            payload: H264InputPayload::Gpu(surface),
            captured_at_100ns,
            enqueued_at: Instant::now(),
            capture_sequence,
            captured_at_unix_ms,
            visible_input_sequence,
            input_applied_at_server_unix_ms,
        };
        match self.sender.try_send(frame) {
            Ok(()) => true,
            Err(TrySendError::Full(frame)) => {
                release_unsubmitted_input(frame);
                self.state.record_dropped_input();
                false
            }
            Err(TrySendError::Disconnected(frame)) => {
                release_unsubmitted_input(frame);
                self.state
                    .mark_unavailable("H.264 encoder worker stopped".to_string());
                false
            }
        }
    }
}

fn release_unsubmitted_input(frame: H264InputFrame) {
    #[cfg(target_os = "windows")]
    if let H264InputPayload::Gpu(surface) = frame.payload {
        let _ = surface.release_after_encoder_done();
    }
}

fn run_encoder_worker(
    receiver: Receiver<H264InputFrame>,
    state: Arc<H264MediaState>,
    fps: u8,
    quality: u8,
    pending_bitrate_bps: Arc<AtomicU64>,
    gpu_input_state: Arc<AtomicU8>,
) {
    #[cfg(target_os = "windows")]
    run_windows_encoder_worker(
        receiver,
        state,
        fps,
        quality,
        pending_bitrate_bps,
        gpu_input_state,
    );

    #[cfg(not(target_os = "windows"))]
    {
        let _ = receiver;
        let _ = fps;
        let _ = quality;
        let _ = pending_bitrate_bps;
        let _ = gpu_input_state;
        state.mark_unavailable("H.264/MSE is only available on Windows".to_string());
    }
}

#[derive(Debug)]
struct ParsedAccessUnit {
    avcc: Vec<u8>,
    keyframe: bool,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
}

fn parse_annex_b_access_unit(bytes: &[u8]) -> Result<ParsedAccessUnit, String> {
    let units = annex_b_units(bytes);
    if units.is_empty() {
        return Err("H.264 access unit has no NAL units".to_string());
    }
    let mut avcc = Vec::with_capacity(bytes.len());
    let mut keyframe = false;
    let mut sps = None;
    let mut pps = None;
    for unit in units {
        if unit.is_empty() || unit.len() > u32::MAX as usize {
            continue;
        }
        match unit[0] & 0x1f {
            5 => keyframe = true,
            7 => sps = Some(unit.to_vec()),
            8 => pps = Some(unit.to_vec()),
            _ => {}
        }
        avcc.extend_from_slice(&(unit.len() as u32).to_be_bytes());
        avcc.extend_from_slice(unit);
    }
    if avcc.is_empty() {
        return Err("H.264 access unit contains only empty NAL units".to_string());
    }
    Ok(ParsedAccessUnit {
        avcc,
        keyframe,
        sps,
        pps,
    })
}

fn annex_b_units(bytes: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let prefix = if index + 4 <= bytes.len() && bytes[index..index + 4] == [0, 0, 0, 1] {
            4
        } else if bytes[index..index + 3] == [0, 0, 1] {
            3
        } else {
            index += 1;
            continue;
        };
        starts.push((index, prefix));
        index += prefix;
    }
    let mut units = Vec::new();
    for (position, (start, prefix)) in starts.iter().copied().enumerate() {
        let unit_start = start + prefix;
        let unit_end = starts
            .get(position + 1)
            .map(|(next, _)| *next)
            .unwrap_or(bytes.len());
        let mut trimmed_end = unit_end;
        while trimmed_end > unit_start && bytes[trimmed_end - 1] == 0 {
            trimmed_end -= 1;
        }
        if unit_start < trimmed_end {
            units.push(&bytes[unit_start..trimmed_end]);
        }
    }
    units
}

fn codec_from_sps(sps: &[u8]) -> Result<String, String> {
    if sps.len() < 4 || sps[0] & 0x1f != 7 {
        return Err("Invalid H.264 SPS".to_string());
    }
    Ok(format!("avc1.{:02X}{:02X}{:02X}", sps[1], sps[2], sps[3]))
}

fn build_init_segment(
    width: u32,
    height: u32,
    fps: u8,
    sps: &[u8],
    pps: &[u8],
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err("Invalid H.264 dimensions".to_string());
    }
    let codec = codec_from_sps(sps)?;
    let _ = codec;
    let mut track = TrackBox::new(true);
    track.tkhd_box.duration = 0;
    track.tkhd_box.width = width << 16;
    track.tkhd_box.height = height << 16;
    track.mdia_box.mdhd_box.timescale = H264_TIMESCALE;
    track.mdia_box.mdhd_box.duration = 0;
    track
        .mdia_box
        .minf_box
        .stbl_box
        .stsd_box
        .sample_entries
        .push(SampleEntry::Avc(AvcSampleEntry {
            width: width as u16,
            height: height as u16,
            avcc_box: AvcConfigurationBox {
                configuration: AvcDecoderConfigurationRecord {
                    profile_idc: sps[1],
                    constraint_set_flag: sps[2],
                    level_idc: sps[3],
                    sequence_parameter_set: sps.to_vec(),
                    picture_parameter_set: pps.to_vec(),
                },
            },
        }));
    let mut extends = TrackExtendsBox::new(true);
    extends.default_sample_duration = H264_TIMESCALE / u32::from(fps.max(1));
    extends.default_sample_flags = 0x0101_0000;
    let segment = InitializationSegment {
        ftyp_box: FileTypeBox,
        moov_box: MovieBox {
            mvhd_box: MovieHeaderBox {
                timescale: H264_TIMESCALE,
                duration: 0,
            },
            trak_boxes: vec![track],
            mvex_box: MovieExtendsBox {
                mehd_box: None,
                trex_boxes: vec![extends],
            },
        },
    };
    let mut bytes = Vec::new();
    segment
        .write_to(&mut bytes)
        .map_err(|error| format!("Failed to build H.264 init segment: {error}"))?;
    Ok(bytes)
}

fn extract_avcc_decoder_configuration(init_segment: &[u8]) -> Option<Vec<u8>> {
    let marker = init_segment
        .windows(4)
        .position(|window| window == b"avcC")?;
    let box_start = marker.checked_sub(4)?;
    let size = u32::from_be_bytes(init_segment.get(box_start..marker)?.try_into().ok()?) as usize;
    if size < 9 {
        return None;
    }
    let box_end = box_start.checked_add(size)?;
    let payload = init_segment.get(marker + 4..box_end)?;
    (payload.first() == Some(&1)).then(|| payload.to_vec())
}

fn build_media_segment(
    sequence: u32,
    base_decode_time: u64,
    duration: u32,
    keyframe: bool,
    sample: &[u8],
) -> Result<Vec<u8>, String> {
    if sample.is_empty() || sample.len() > u32::MAX as usize {
        return Err("Invalid H.264 media sample".to_string());
    }
    let mut moof = Vec::new();
    let mut data_offset_position = None;
    append_box(&mut moof, *b"moof", |moof| {
        append_full_box(moof, *b"mfhd", 0, 0, |payload| {
            payload.extend_from_slice(&sequence.to_be_bytes());
        });
        append_box(moof, *b"traf", |traf| {
            append_full_box(traf, *b"tfhd", 0, 0x02_0000, |payload| {
                payload.extend_from_slice(&1u32.to_be_bytes());
            });
            if base_decode_time <= u32::MAX as u64 {
                append_full_box(traf, *b"tfdt", 0, 0, |payload| {
                    payload.extend_from_slice(&(base_decode_time as u32).to_be_bytes());
                });
            } else {
                append_full_box(traf, *b"tfdt", 1, 0, |payload| {
                    payload.extend_from_slice(&base_decode_time.to_be_bytes());
                });
            }
            append_full_box(traf, *b"trun", 0, 0x00_0701, |payload| {
                payload.extend_from_slice(&1u32.to_be_bytes());
                data_offset_position = Some(payload.len());
                payload.extend_from_slice(&0i32.to_be_bytes());
                payload.extend_from_slice(&duration.max(1).to_be_bytes());
                payload.extend_from_slice(&(sample.len() as u32).to_be_bytes());
                let flags = if keyframe {
                    0x0200_0000u32
                } else {
                    0x0101_0000u32
                };
                payload.extend_from_slice(&flags.to_be_bytes());
            });
        });
    });
    let offset = i32::try_from(moof.len().saturating_add(8))
        .map_err(|_| "H.264 media segment is too large".to_string())?;
    let position = data_offset_position.ok_or_else(|| "Missing trun data offset".to_string())?;
    moof[position..position + 4].copy_from_slice(&offset.to_be_bytes());
    append_box(&mut moof, *b"mdat", |payload| {
        payload.extend_from_slice(sample)
    });
    Ok(moof)
}

fn append_box(output: &mut Vec<u8>, box_type: [u8; 4], build: impl FnOnce(&mut Vec<u8>)) {
    let start = output.len();
    output.extend_from_slice(&0u32.to_be_bytes());
    output.extend_from_slice(&box_type);
    build(output);
    let size = (output.len() - start) as u32;
    output[start..start + 4].copy_from_slice(&size.to_be_bytes());
}

fn append_full_box(
    output: &mut Vec<u8>,
    box_type: [u8; 4],
    version: u8,
    flags: u32,
    build: impl FnOnce(&mut Vec<u8>),
) {
    append_box(output, box_type, |payload| {
        payload.push(version);
        payload.extend_from_slice(&(flags & 0x00ff_ffff).to_be_bytes()[1..]);
        build(payload);
    });
}

fn target_bitrate_bps(width: u32, height: u32, fps: u8, quality: u8) -> u32 {
    let quality = quality.clamp(10, 100) as u64;
    let pixels_per_second = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(u64::from(fps.max(1)));
    let scaled = pixels_per_second
        .saturating_mul(70 + quality)
        .saturating_div(1_000);
    scaled.clamp(1_200_000, 12_000_000) as u32
}

/// 软件回退的分辨率/帧率上限（规格 §5.3 要求的显式策略）。
///
/// 上限保持 1920x1080@30。曾经下调到 720p30 以缓解一台 Intel 目标机上的卡顿，
/// 但那次测量里同时存在一路 MJPEG 观看端（每帧约 1.2 MB、强制全帧 CPU 回读、
/// 外加 JPEG 编码），卡顿的主因是它而不是 1080p 软编；而屏幕共享的内容以文字
/// 为主，降采样——尤其是配合最近邻缩放——会让正文直接不可读。
///
/// 因此这里只保留"超出 1080p 才等比缩小"的硬上限。真正需要降级时应当由实测的
/// 编码耗时驱动自适应降档，而不是对最常见的 1080p 桌面无条件降低清晰度。
fn software_encoder_limits(width: usize, height: usize, fps: u8) -> (usize, usize, u8) {
    const MAX_WIDTH: usize = 1920;
    const MAX_HEIGHT: usize = 1080;

    let width = width & !1;
    let height = height & !1;
    let (limited_width, limited_height) = if width <= MAX_WIDTH && height <= MAX_HEIGHT {
        (width, height)
    } else if width.saturating_mul(MAX_HEIGHT) >= height.saturating_mul(MAX_WIDTH) {
        let limited_height = height
            .saturating_mul(MAX_WIDTH)
            .saturating_div(width.max(1))
            .min(MAX_HEIGHT)
            & !1;
        (MAX_WIDTH, limited_height.max(2))
    } else {
        let limited_width = width
            .saturating_mul(MAX_HEIGHT)
            .saturating_div(height.max(1))
            .min(MAX_WIDTH)
            & !1;
        (limited_width.max(2), MAX_HEIGHT)
    };
    (
        limited_width.max(2),
        limited_height.max(2),
        fps.min(30).max(1),
    )
}

fn scale_bgra_nearest(
    source: &[u8],
    source_width: usize,
    source_height: usize,
    source_stride: usize,
    target_width: usize,
    target_height: usize,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    validate_bgra_frame(source, source_width, source_height, source_stride)?;
    if target_width < 2 || target_height < 2 || target_width % 2 != 0 || target_height % 2 != 0 {
        return Err("Invalid software H.264 scaler target dimensions".to_string());
    }
    output.resize(
        target_width.saturating_mul(target_height).saturating_mul(4),
        0,
    );
    for target_y in 0..target_height {
        let source_y = target_y.saturating_mul(source_height) / target_height;
        for target_x in 0..target_width {
            let source_x = target_x.saturating_mul(source_width) / target_width;
            let source_offset = source_y * source_stride + source_x * 4;
            let target_offset = (target_y * target_width + target_x) * 4;
            output[target_offset..target_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
    Ok(())
}

fn bgra_to_nv12(
    bgra: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    output: &mut Vec<u8>,
) -> Result<(usize, usize), String> {
    let dimensions = validate_bgra_frame(bgra, width, height, stride)?;
    prepare_nv12_output(output, dimensions.0, dimensions.1);

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    if std::is_x86_feature_detected!("ssse3") {
        // SAFETY: the feature is checked at runtime and validate_bgra_frame
        // proves every row read and output write performed by this routine.
        unsafe {
            bgra_to_nv12_ssse3_into(bgra, dimensions.0, dimensions.1, stride, output);
        }
        return Ok(dimensions);
    }

    bgra_to_nv12_scalar_into(bgra, dimensions.0, dimensions.1, stride, output);
    Ok(dimensions)
}

#[cfg(test)]
fn bgra_to_nv12_scalar(
    bgra: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    output: &mut Vec<u8>,
) -> Result<(usize, usize), String> {
    let dimensions = validate_bgra_frame(bgra, width, height, stride)?;
    prepare_nv12_output(output, dimensions.0, dimensions.1);
    bgra_to_nv12_scalar_into(bgra, dimensions.0, dimensions.1, stride, output);
    Ok(dimensions)
}

fn validate_bgra_frame(
    bgra: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> Result<(usize, usize), String> {
    let encoded_width = width & !1;
    let encoded_height = height & !1;
    if encoded_width < 2
        || encoded_height < 2
        || stride < width.saturating_mul(4)
        || bgra.len() < height.saturating_mul(stride)
    {
        return Err("Invalid BGRA frame for H.264 encoding".to_string());
    }
    Ok((encoded_width, encoded_height))
}

fn prepare_nv12_output(output: &mut Vec<u8>, width: usize, height: usize) {
    let y_size = width * height;
    output.clear();
    output.resize(y_size + y_size / 2, 0);
}

fn bgra_to_nv12_scalar_into(
    bgra: &[u8],
    encoded_width: usize,
    encoded_height: usize,
    stride: usize,
    output: &mut [u8],
) {
    let y_size = encoded_width * encoded_height;
    for y in 0..encoded_height {
        for x in 0..encoded_width {
            let offset = y * stride + x * 4;
            let blue = i32::from(bgra[offset]);
            let green = i32::from(bgra[offset + 1]);
            let red = i32::from(bgra[offset + 2]);
            output[y * encoded_width + x] = rgb_to_y(red, green, blue);
        }
    }
    let uv_start = y_size;
    for y in (0..encoded_height).step_by(2) {
        for x in (0..encoded_width).step_by(2) {
            let mut red = 0i32;
            let mut green = 0i32;
            let mut blue = 0i32;
            for row in 0..2 {
                for column in 0..2 {
                    let offset = (y + row) * stride + (x + column) * 4;
                    blue += i32::from(bgra[offset]);
                    green += i32::from(bgra[offset + 1]);
                    red += i32::from(bgra[offset + 2]);
                }
            }
            red /= 4;
            green /= 4;
            blue /= 4;
            let uv_offset = uv_start + (y / 2) * encoded_width + x;
            output[uv_offset] = rgb_to_u(red, green, blue);
            output[uv_offset + 1] = rgb_to_v(red, green, blue);
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
unsafe fn bgra_to_nv12_ssse3_into(
    bgra: &[u8],
    encoded_width: usize,
    encoded_height: usize,
    stride: usize,
    output: &mut [u8],
) {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    let zero = _mm_setzero_si128();
    // Lane order matches two BGRA pixels after unpacking bytes to u16.
    let coefficients = _mm_set_epi16(0, 66, 129, 25, 0, 66, 129, 25);
    let rounding = _mm_set1_epi32(128);
    let video_range_offset = _mm_set1_epi32(16);

    for y in 0..encoded_height {
        let input_row = y * stride;
        let output_row = y * encoded_width;
        let mut x = 0;
        while x + 4 <= encoded_width {
            // SAFETY: validation proves stride covers width * 4 bytes and this
            // loop loads exactly four pixels wholly inside the current row.
            let pixels =
                unsafe { _mm_loadu_si128(bgra.as_ptr().add(input_row + x * 4).cast::<__m128i>()) };
            let low_words = _mm_unpacklo_epi8(pixels, zero);
            let high_words = _mm_unpackhi_epi8(pixels, zero);
            let low_pairs = _mm_madd_epi16(low_words, coefficients);
            let high_pairs = _mm_madd_epi16(high_words, coefficients);
            let sums = _mm_hadd_epi32(low_pairs, high_pairs);
            let scaled = _mm_add_epi32(
                _mm_srli_epi32(_mm_add_epi32(sums, rounding), 8),
                video_range_offset,
            );
            let packed_words = _mm_packs_epi32(scaled, zero);
            let packed_bytes = _mm_packus_epi16(packed_words, zero);
            let four_y = _mm_cvtsi128_si32(packed_bytes) as u32;
            output[output_row + x..output_row + x + 4].copy_from_slice(&four_y.to_le_bytes());
            x += 4;
        }
        while x < encoded_width {
            let offset = input_row + x * 4;
            output[output_row + x] = rgb_to_y(
                i32::from(bgra[offset + 2]),
                i32::from(bgra[offset + 1]),
                i32::from(bgra[offset]),
            );
            x += 1;
        }
    }

    // Chroma is one sample per 2x2 block and therefore only one quarter as
    // frequent as luma. Keep this lower-cost portion scalar so its signed
    // division semantics stay exactly identical to the reference algorithm.
    let uv_start = encoded_width * encoded_height;
    for y in (0..encoded_height).step_by(2) {
        for x in (0..encoded_width).step_by(2) {
            let mut red = 0i32;
            let mut green = 0i32;
            let mut blue = 0i32;
            for row in 0..2 {
                for column in 0..2 {
                    let offset = (y + row) * stride + (x + column) * 4;
                    blue += i32::from(bgra[offset]);
                    green += i32::from(bgra[offset + 1]);
                    red += i32::from(bgra[offset + 2]);
                }
            }
            let uv_offset = uv_start + (y / 2) * encoded_width + x;
            output[uv_offset] = rgb_to_u(red / 4, green / 4, blue / 4);
            output[uv_offset + 1] = rgb_to_v(red / 4, green / 4, blue / 4);
        }
    }
}

fn rgb_to_y(red: i32, green: i32, blue: i32) -> u8 {
    ((66 * red + 129 * green + 25 * blue + 128) / 256 + 16).clamp(0, 255) as u8
}

fn rgb_to_u(red: i32, green: i32, blue: i32) -> u8 {
    ((-38 * red - 74 * green + 112 * blue + 128) / 256 + 128).clamp(0, 255) as u8
}

fn rgb_to_v(red: i32, green: i32, blue: i32) -> u8 {
    ((112 * red - 94 * green - 18 * blue + 128) / 256 + 128).clamp(0, 255) as u8
}

#[cfg(target_os = "windows")]
fn run_windows_encoder_worker(
    receiver: Receiver<H264InputFrame>,
    state: Arc<H264MediaState>,
    fps: u8,
    quality: u8,
    pending_bitrate_bps: Arc<AtomicU64>,
    gpu_input_state: Arc<AtomicU8>,
) {
    use windows::core::HRESULT;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
    let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let com_initialized = if com_result.is_ok() {
        true
    } else if com_result == RPC_E_CHANGED_MODE {
        false
    } else {
        state.mark_unavailable(format!("CoInitializeEx failed: {com_result:?}"));
        return;
    };
    let runtime = match MediaFoundationRuntime::startup(com_initialized) {
        Ok(runtime) => runtime,
        Err(error) => {
            state.mark_unavailable(error);
            if com_initialized {
                unsafe { CoUninitialize() };
            }
            return;
        }
    };
    let mut encoder: Option<WindowsH264Encoder> = None;
    let mut nv12 = Vec::new();
    let mut scaled_bgra = Vec::new();
    let mut retry_after = Instant::now();
    let mut last_software_encode_at: Option<Instant> = None;
    let mut last_dimensions = (0usize, 0usize);
    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;
    let mut active_parameter_sets: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut generation = 0u64;
    let mut sequence = 0u64;
    let mut pending_frame_metadata = VecDeque::<PendingH264FrameMetadata>::new();
    let mut hardware_allowed = true;
    let mut hardware_runtime_failure: Option<String> = None;
    let mut consecutive_empty_outputs = 0u32;

    while let Ok(mut frame) = receiver.recv() {
        // Encoding an old queued frame only increases glass-to-glass latency.
        // Drain the tiny bounded queue and keep the newest available capture.
        while let Ok(newer) = receiver.try_recv() {
            release_unsubmitted_input(frame);
            frame = newer;
            state.record_dropped_input();
        }
        state.record_input_queue_age(frame.enqueued_at.elapsed());
        let gpu_frame = matches!(&frame.payload, H264InputPayload::Gpu(_));
        let (width, height) = match &frame.payload {
            H264InputPayload::Cpu { pixels, stride } => {
                match validate_bgra_frame(pixels, frame.width, frame.height, *stride) {
                    Ok(dimensions) => dimensions,
                    Err(error) => {
                        state.mark_unavailable(error);
                        continue;
                    }
                }
            }
            H264InputPayload::Gpu(surface) => (surface.width() as usize, surface.height() as usize),
        };
        let input_mode_changed = encoder
            .as_ref()
            .is_some_and(|active| active.uses_gpu_surfaces != gpu_frame);
        if encoder.is_none() || last_dimensions != (width, height) || input_mode_changed {
            if Instant::now() < retry_after {
                release_unsubmitted_input(frame);
                continue;
            }
            if let Some(active) = encoder.as_mut() {
                let _ = active.flush();
            }
            pending_frame_metadata.clear();
            encoder = None;
            let bitrate = target_bitrate_bps(width as u32, height as u32, fps, quality);
            let create_result = match &frame.payload {
                H264InputPayload::Cpu { .. } => WindowsH264Encoder::new(
                    width as u32,
                    height as u32,
                    fps,
                    bitrate,
                    hardware_allowed,
                    hardware_runtime_failure.clone(),
                ),
                H264InputPayload::Gpu(surface) => WindowsH264Encoder::new_for_gpu_surface(
                    surface,
                    width as u32,
                    height as u32,
                    fps,
                    bitrate,
                ),
            };
            match create_result {
                Ok(created) => {
                    if created.uses_gpu_surfaces {
                        gpu_input_state.store(GPU_INPUT_ACTIVE, Ordering::Release);
                    }
                    state.set_encoder_diagnostics(created.diagnostics.clone());
                    encoder = Some(created);
                    last_dimensions = (width, height);
                    sps = None;
                    pps = None;
                    active_parameter_sets = None;
                    sequence = 0;
                    last_software_encode_at = None;
                    consecutive_empty_outputs = 0;
                }
                Err(error) => {
                    if gpu_frame {
                        gpu_input_state.store(GPU_INPUT_DISABLED, Ordering::Release);
                        state.record_gpu_fallback(format!(
                            "GPU H.264 encoder candidate/self-test rejected: {error}"
                        ));
                    }
                    release_unsubmitted_input(frame);
                    state.mark_unavailable(error);
                    retry_after = Instant::now()
                        + if gpu_frame {
                            Duration::from_millis(50)
                        } else {
                            Duration::from_secs(5)
                        };
                    continue;
                }
            }
        }
        let Some(active_encoder) = encoder.as_mut() else {
            continue;
        };
        if !active_encoder.diagnostics.hardware && active_encoder.fps < fps {
            let minimum_interval = Duration::from_secs_f64(1.0 / f64::from(active_encoder.fps));
            let now = Instant::now();
            if last_software_encode_at
                .is_some_and(|last| now.saturating_duration_since(last) < minimum_interval)
            {
                state.record_dropped_input();
                continue;
            }
            last_software_encode_at = Some(now);
        }
        let requested_bitrate = pending_bitrate_bps.swap(0, Ordering::AcqRel);
        if requested_bitrate != 0 {
            let requested_bitrate = requested_bitrate.min(u64::from(u32::MAX)) as u32;
            let result = active_encoder.update_bitrate(requested_bitrate);
            if result.is_ok() {
                state.update_stream_bitrate(generation, requested_bitrate);
            }
            state.record_runtime_bitrate_update(result);
        }
        if generation != 0
            && active_parameter_sets.is_some()
            && state.take_keyframe_request(generation)
        {
            // A failed on-demand request must not tear down an otherwise usable
            // encoder; the bounded periodic GOP remains the recovery fallback.
            match active_encoder.request_next_keyframe() {
                Ok(true) => {}
                Ok(false) | Err(_) => state.record_idr_force_failure(),
            }
        }
        let encode_started = Instant::now();
        let encode_result = match frame.payload {
            H264InputPayload::Cpu { pixels, stride } => {
                let conversion_started = Instant::now();
                let conversion_result = if active_encoder.input_width as usize == width
                    && active_encoder.input_height as usize == height
                {
                    bgra_to_nv12(&pixels, width, height, stride, &mut nv12)
                } else {
                    scale_bgra_nearest(
                        &pixels,
                        width,
                        height,
                        stride,
                        active_encoder.input_width as usize,
                        active_encoder.input_height as usize,
                        &mut scaled_bgra,
                    )
                    .and_then(|()| {
                        bgra_to_nv12(
                            &scaled_bgra,
                            active_encoder.input_width as usize,
                            active_encoder.input_height as usize,
                            active_encoder.input_width as usize * 4,
                            &mut nv12,
                        )
                    })
                };
                state.record_bgra_to_nv12(conversion_started.elapsed());
                match conversion_result {
                    Ok(_) => active_encoder.encode(&nv12, frame.captured_at_100ns),
                    Err(error) => Err(error),
                }
            }
            H264InputPayload::Gpu(surface) => {
                active_encoder.encode_surface(surface, frame.captured_at_100ns)
            }
        };
        state.record_mft_encode(encode_started.elapsed());
        let outputs = match encode_result {
            Ok(outputs) => {
                if pending_frame_metadata.len() >= H264_EVENT_CAPACITY {
                    let _ = active_encoder.flush();
                    pending_frame_metadata.clear();
                }
                pending_frame_metadata.push_back(PendingH264FrameMetadata {
                    sample_time_100ns: frame.captured_at_100ns,
                    capture_sequence: frame.capture_sequence,
                    captured_at_unix_ms: frame.captured_at_unix_ms,
                    visible_input_sequence: frame.visible_input_sequence,
                    input_applied_at_server_unix_ms: frame.input_applied_at_server_unix_ms,
                });
                outputs
            }
            Err(error) => {
                let _ = active_encoder.flush();
                pending_frame_metadata.clear();
                if active_encoder.diagnostics.hardware {
                    hardware_allowed = false;
                    hardware_runtime_failure = Some(format!(
                        "hardware encoder failed during ProcessInput/ProcessOutput: {error}"
                    ));
                }
                if active_encoder.uses_gpu_surfaces {
                    gpu_input_state.store(GPU_INPUT_DISABLED, Ordering::Release);
                    state.record_gpu_fallback(format!(
                        "GPU H.264 encoder ProcessInput/ProcessOutput failed: {error}"
                    ));
                }
                state.mark_unavailable(error);
                encoder = None;
                retry_after = Instant::now() + Duration::from_secs(2);
                continue;
            }
        };
        if outputs.is_empty() {
            consecutive_empty_outputs = consecutive_empty_outputs.saturating_add(1);
            if active_encoder.diagnostics.hardware
                && consecutive_empty_outputs >= u32::from(fps.max(1)).saturating_mul(2)
            {
                let error = "hardware encoder produced no output for two seconds".to_string();
                hardware_allowed = false;
                hardware_runtime_failure = Some(error.clone());
                let was_gpu = active_encoder.uses_gpu_surfaces;
                let _ = active_encoder.flush();
                pending_frame_metadata.clear();
                if was_gpu {
                    gpu_input_state.store(GPU_INPUT_DISABLED, Ordering::Release);
                    state.record_gpu_fallback(error.clone());
                }
                state.mark_unavailable(error);
                encoder = None;
                retry_after = Instant::now() + Duration::from_millis(250);
            }
            continue;
        }
        consecutive_empty_outputs = 0;
        let mut latest_output_time = None;
        for output in outputs {
            latest_output_time = Some(
                latest_output_time.map_or(output.sample_time_100ns, |latest: i64| {
                    latest.max(output.sample_time_100ns)
                }),
            );
            let source_metadata = pending_frame_metadata
                .iter()
                .rev()
                .find(|metadata| metadata.sample_time_100ns <= output.sample_time_100ns)
                .map(|metadata| H264FrameMetadataValues {
                    capture_sequence: metadata.capture_sequence,
                    captured_at_unix_ms: metadata.captured_at_unix_ms,
                    visible_input_sequence: metadata.visible_input_sequence,
                    input_applied_at_server_unix_ms: metadata.input_applied_at_server_unix_ms,
                })
                .unwrap_or(H264FrameMetadataValues {
                    capture_sequence: frame.capture_sequence,
                    captured_at_unix_ms: frame.captured_at_unix_ms,
                    visible_input_sequence: frame.visible_input_sequence,
                    input_applied_at_server_unix_ms: frame.input_applied_at_server_unix_ms,
                });
            let parsed = match parse_annex_b_access_unit(&output.bytes) {
                Ok(parsed) => parsed,
                Err(error) => {
                    state.mark_unavailable(error);
                    continue;
                }
            };
            if let Some(value) = parsed.sps.clone() {
                sps = Some(value);
            }
            if let Some(value) = parsed.pps.clone() {
                pps = Some(value);
            }
            let parameter_sets_changed = match (&sps, &pps, &active_parameter_sets) {
                (Some(sps), Some(pps), Some((active_sps, active_pps))) => {
                    sps != active_sps || pps != active_pps
                }
                (Some(_), Some(_), None) => true,
                _ => false,
            };
            if parsed.keyframe && parameter_sets_changed {
                let current_sps = sps.clone().unwrap();
                let current_pps = pps.clone().unwrap();
                let codec = match codec_from_sps(&current_sps) {
                    Ok(codec) => codec,
                    Err(error) => {
                        state.mark_unavailable(error);
                        continue;
                    }
                };
                let init = match build_init_segment(
                    active_encoder.input_width,
                    active_encoder.input_height,
                    active_encoder.fps,
                    &current_sps,
                    &current_pps,
                ) {
                    Ok(init) => init,
                    Err(error) => {
                        state.mark_unavailable(error);
                        continue;
                    }
                };
                generation = state.install_stream(
                    codec,
                    active_encoder.input_width,
                    active_encoder.input_height,
                    active_encoder.fps,
                    active_encoder.bitrate_bps,
                    init,
                );
                active_parameter_sets = Some((current_sps, current_pps));
                sequence = 0;
            }
            if generation == 0 || active_parameter_sets.is_none() {
                continue;
            }
            sequence = sequence.saturating_add(1);
            let decode_time = output
                .sample_time_100ns
                .max(0)
                .saturating_mul(i64::from(H264_TIMESCALE))
                / 10_000_000;
            let duration = output
                .sample_duration_100ns
                .max(1)
                .saturating_mul(i64::from(H264_TIMESCALE))
                / 10_000_000;
            let mux_started = Instant::now();
            let fragment = match build_media_segment(
                sequence.min(u64::from(u32::MAX)) as u32,
                decode_time.max(0) as u64,
                duration.max(1).min(i64::from(u32::MAX)) as u32,
                parsed.keyframe,
                &parsed.avcc,
            ) {
                Ok(fragment) => fragment,
                Err(error) => {
                    state.mark_unavailable(error);
                    continue;
                }
            };
            state.record_mux(mux_started.elapsed());
            if parsed.keyframe {
                state.record_idr_size(fragment.len());
            }
            state.publish_segment(H264MediaSegment {
                generation,
                sequence,
                keyframe: parsed.keyframe,
                timestamp_us: output.sample_time_100ns.max(0) as u64 / 10,
                duration_us: output.sample_duration_100ns.max(1) as u64 / 10,
                capture_sequence: source_metadata.capture_sequence,
                captured_at_unix_ms: source_metadata.captured_at_unix_ms,
                visible_input_sequence: source_metadata.visible_input_sequence,
                input_applied_at_server_unix_ms: source_metadata.input_applied_at_server_unix_ms,
                access_unit_avcc: Arc::new(Bytes::from(parsed.avcc)),
                bytes: Arc::new(Bytes::from(fragment)),
            });
        }
        let _ = latest_output_time;
    }
    if let Some(active) = encoder.as_mut() {
        let _ = active.flush();
    }
    pending_frame_metadata.clear();
    drop(encoder);
    drop(runtime);
}

#[cfg(target_os = "windows")]
struct MediaFoundationRuntime {
    com_initialized: bool,
}

#[cfg(target_os = "windows")]
impl MediaFoundationRuntime {
    fn startup(com_initialized: bool) -> Result<Self, String> {
        use windows::Win32::Media::MediaFoundation::{MFStartup, MFSTARTUP_FULL, MF_VERSION};
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|error| format!("Media Foundation startup failed: {error}"))?;
        Ok(Self { com_initialized })
    }
}

#[cfg(target_os = "windows")]
impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        use windows::Win32::Media::MediaFoundation::MFShutdown;
        use windows::Win32::System::Com::CoUninitialize;
        unsafe {
            let _ = MFShutdown();
            if self.com_initialized {
                CoUninitialize();
            }
        }
    }
}

#[cfg(target_os = "windows")]
struct EncodedH264Output {
    bytes: Vec<u8>,
    sample_time_100ns: i64,
    sample_duration_100ns: i64,
    sample_time_from_encoder: bool,
    sample_duration_from_encoder: bool,
}

/// `ProcessOutput` 的两类结果：真正的失败，以及"MFT 要求重新协商输出类型"这个
/// 正常协议事件。分开是为了让后者能被重新协商后重试，而不是把候选编码器否掉。
#[cfg(target_os = "windows")]
enum H264ProcessOutputError {
    StreamChange,
    Failed(String),
}

#[cfg(target_os = "windows")]
impl From<String> for H264ProcessOutputError {
    fn from(value: String) -> Self {
        Self::Failed(value)
    }
}

/// 一次取输出的结果。`Renegotiated` 表示本次事件被输出类型变更消耗掉了：
/// 异步 MFT 必须等下一个 `METransformHaveOutput`，不能当成"缺输入"报错。
#[cfg(target_os = "windows")]
enum H264OutputOutcome {
    Produced(EncodedH264Output),
    NeedMoreInput,
    Renegotiated,
}

#[cfg(target_os = "windows")]
struct PendingH264FrameMetadata {
    sample_time_100ns: i64,
    capture_sequence: u64,
    captured_at_unix_ms: u64,
    visible_input_sequence: Option<u64>,
    input_applied_at_server_unix_ms: Option<u64>,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Default)]
struct GpuSurfaceReleaseErrorSink {
    errors: Arc<Mutex<VecDeque<String>>>,
}

#[cfg(target_os = "windows")]
impl GpuSurfaceReleaseErrorSink {
    fn record(&self, error: String) {
        log::error!("screen_share_gpu_tracked_sample_release failed detail={error}");
        let Ok(mut errors) = self.errors.lock() else {
            log::error!(
                "screen_share_gpu_tracked_sample_release failed detail=release error queue poisoned"
            );
            return;
        };
        const ERROR_LIMIT: usize = 8;
        if errors.len() >= ERROR_LIMIT {
            errors.pop_front();
        }
        errors.push_back(error);
    }

    fn take(&self) -> Option<String> {
        self.errors.lock().ok()?.pop_front()
    }
}

#[cfg(target_os = "windows")]
struct GpuSurfaceLeaseHolder {
    surface: Mutex<Option<GpuNv12Surface>>,
    errors: GpuSurfaceReleaseErrorSink,
}

#[cfg(target_os = "windows")]
impl GpuSurfaceLeaseHolder {
    fn new(surface: GpuNv12Surface, errors: GpuSurfaceReleaseErrorSink) -> Self {
        Self {
            surface: Mutex::new(Some(surface)),
            errors,
        }
    }

    fn release(&self, operation: &'static str) {
        let surface = match self.surface.lock() {
            Ok(mut surface) => surface.take(),
            Err(poisoned) => {
                self.errors.record(format!(
                    "{operation}: tracked surface holder mutex was poisoned"
                ));
                poisoned.into_inner().take()
            }
        };
        if let Some(surface) = surface {
            if let Err(error) = surface.release_after_encoder_done() {
                self.errors.record(format!(
                    "{operation}: code={:?}; operation={}; hresult={:?}; detail={}",
                    error.code, error.operation, error.hresult, error.detail
                ));
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for GpuSurfaceLeaseHolder {
    fn drop(&mut self) {
        self.release("tracked sample callback drop");
    }
}

#[cfg(target_os = "windows")]
#[windows::core::implement(IMFAsyncCallback)]
struct GpuSurfaceReleaseCallback {
    holder: Arc<GpuSurfaceLeaseHolder>,
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
impl IMFAsyncCallback_Impl for GpuSurfaceReleaseCallback_Impl {
    fn GetParameters(&self, _flags: *mut u32, _queue: *mut u32) -> windows::core::Result<()> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }

    fn Invoke(&self, _result: Option<&IMFAsyncResult>) -> windows::core::Result<()> {
        self.holder.release("IMFTrackedSample allocator callback");
        Ok(())
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
struct H264FrameMetadataValues {
    capture_sequence: u64,
    captured_at_unix_ms: u64,
    visible_input_sequence: Option<u64>,
    input_applied_at_server_unix_ms: Option<u64>,
}

const H264_ASYNC_EVENT_CREDIT_LIMIT: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum H264AsyncMftEvent {
    NeedInput,
    HaveOutput,
    DrainComplete,
    Other(u32),
}

/// Platform-independent accounting for the event-driven MFT contract. Credits
/// are deliberately bounded so a broken transform cannot grow an unbounded
/// in-memory event queue or make us call ProcessInput/ProcessOutput blindly.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct H264AsyncMftState {
    input_credits: u8,
    output_credits: u8,
    draining: bool,
    drain_complete: bool,
    ignored_events: u32,
}

impl H264AsyncMftState {
    fn observe(&mut self, event: H264AsyncMftEvent) -> Result<(), String> {
        match event {
            H264AsyncMftEvent::NeedInput => {
                self.input_credits = self
                    .input_credits
                    .checked_add(1)
                    .filter(|value| *value <= H264_ASYNC_EVENT_CREDIT_LIMIT)
                    .ok_or_else(|| {
                        "async H.264 MFT exceeded the NeedInput credit limit".to_string()
                    })?;
            }
            H264AsyncMftEvent::HaveOutput => {
                self.output_credits = self
                    .output_credits
                    .checked_add(1)
                    .filter(|value| *value <= H264_ASYNC_EVENT_CREDIT_LIMIT)
                    .ok_or_else(|| {
                        "async H.264 MFT exceeded the HaveOutput credit limit".to_string()
                    })?;
            }
            H264AsyncMftEvent::DrainComplete => {
                if !self.draining {
                    return Err(
                        "async H.264 MFT reported DrainComplete outside a drain".to_string()
                    );
                }
                self.drain_complete = true;
            }
            H264AsyncMftEvent::Other(_) => {
                self.ignored_events = self.ignored_events.saturating_add(1);
            }
        }
        Ok(())
    }

    fn take_input_credit(&mut self) -> bool {
        if self.input_credits == 0 {
            return false;
        }
        self.input_credits -= 1;
        true
    }

    fn take_output_credit(&mut self) -> bool {
        if self.output_credits == 0 {
            return false;
        }
        self.output_credits -= 1;
        true
    }

    #[cfg(test)]
    fn begin_drain(&mut self) {
        self.draining = true;
        self.drain_complete = false;
        self.input_credits = 0;
    }

    fn reset_after_flush(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy)]
struct H264SelfTestAccessUnit<'a> {
    bytes: &'a [u8],
    sample_time_100ns: i64,
    sample_duration_100ns: i64,
}

fn inspect_h264_self_test_access_units(
    access_units: &[H264SelfTestAccessUnit<'_>],
) -> H264EncoderSelfTestSnapshot {
    let mut snapshot = H264EncoderSelfTestSnapshot {
        attempted: true,
        produced_access_units: access_units.len().min(u32::MAX as usize) as u32,
        timeline_monotonic: true,
        timestamps_from_encoder: true,
        durations_from_encoder: true,
        ..Default::default()
    };
    let mut previous_time = None;
    for access_unit in access_units {
        if access_unit.sample_duration_100ns <= 0
            || previous_time.is_some_and(|previous| access_unit.sample_time_100ns <= previous)
        {
            snapshot.timeline_monotonic = false;
        }
        previous_time = Some(access_unit.sample_time_100ns);
        for unit in annex_b_units(access_unit.bytes) {
            match unit.first().copied().unwrap_or_default() & 0x1f {
                5 => snapshot.found_idr = true,
                7 => snapshot.found_sps = true,
                8 => snapshot.found_pps = true,
                1 => {
                    if h264_slice_type(unit).is_some_and(|slice_type| slice_type % 5 == 1) {
                        snapshot.b_slice_count = snapshot.b_slice_count.saturating_add(1);
                    }
                }
                _ => {}
            }
        }
    }
    snapshot.failure_reason = if access_units.is_empty() {
        Some("encoder self-test produced no access units".to_string())
    } else if !snapshot.found_sps || !snapshot.found_pps || !snapshot.found_idr {
        Some(format!(
            "encoder self-test parameter/keyframe evidence incomplete (SPS={}, PPS={}, IDR={})",
            snapshot.found_sps, snapshot.found_pps, snapshot.found_idr
        ))
    } else if !snapshot.timeline_monotonic {
        Some("encoder self-test output timestamps or durations are invalid".to_string())
    } else if snapshot.b_slice_count != 0 {
        Some(format!(
            "encoder self-test observed {} B slices although the fMP4 mux assumes DTS=PTS",
            snapshot.b_slice_count
        ))
    } else {
        None
    };
    snapshot
}

#[cfg(target_os = "windows")]
fn inspect_encoded_h264_self_test_outputs(
    outputs: &[EncodedH264Output],
) -> H264EncoderSelfTestSnapshot {
    let access_units = outputs
        .iter()
        .map(|output| H264SelfTestAccessUnit {
            bytes: &output.bytes,
            sample_time_100ns: output.sample_time_100ns,
            sample_duration_100ns: output.sample_duration_100ns,
        })
        .collect::<Vec<_>>();
    let mut snapshot = inspect_h264_self_test_access_units(&access_units);
    snapshot.timestamps_from_encoder = outputs.iter().all(|output| output.sample_time_from_encoder);
    snapshot.durations_from_encoder = outputs
        .iter()
        .all(|output| output.sample_duration_from_encoder);
    if snapshot.failure_reason.is_none() && !snapshot.timestamps_from_encoder {
        snapshot.failure_reason = Some(
            "encoder self-test output omitted a sample timestamp; synthesized timestamps cannot prove a legal timeline"
                .to_string(),
        );
    } else if snapshot.failure_reason.is_none() && !snapshot.durations_from_encoder {
        snapshot.failure_reason = Some(
            "encoder self-test output omitted a sample duration; synthesized durations are not accepted"
                .to_string(),
        );
    }
    snapshot
}

fn h264_self_test_minimum_observation_frames(fps: u8) -> usize {
    // Cover at least half a second of submitted presentation time, while still
    // exercising several frames at very low configured frame rates.
    usize::from(fps.max(1)).div_ceil(2).max(4)
}

fn fill_h264_self_test_pattern(nv12: &mut [u8], width: usize, height: usize, frame_index: u32) {
    let Some(luma_len) = width.checked_mul(height) else {
        return;
    };
    if width == 0 || height == 0 || nv12.len() < luma_len.saturating_add(luma_len / 2) {
        return;
    }
    // Moving high-contrast bars exercise inter-frame prediction. Chroma also
    // changes per frame so the test cannot accidentally become a black/static
    // fast path in a driver.
    let moving_x = (frame_index as usize * 17) % width;
    let moving_y = (frame_index as usize * 11) % height;
    for y in 0..height {
        for x in 0..width {
            let bar = x.abs_diff(moving_x) < width.clamp(8, 96) / 8
                || y.abs_diff(moving_y) < height.clamp(8, 96) / 8;
            nv12[y * width + x] = if bar {
                220
            } else {
                24u8.saturating_add(((x / 8 + y / 8 + frame_index as usize) % 96) as u8)
            };
        }
    }
    for (index, chroma) in nv12[luma_len..luma_len + luma_len / 2]
        .chunks_exact_mut(2)
        .enumerate()
    {
        let phase = ((index + frame_index as usize * 13) & 31) as u8;
        chroma[0] = 96u8.saturating_add(phase);
        chroma[1] = 160u8.saturating_sub(phase);
    }
}

fn h264_slice_type(nal_unit: &[u8]) -> Option<u32> {
    if nal_unit.len() < 2 || !matches!(nal_unit[0] & 0x1f, 1 | 5) {
        return None;
    }
    let mut rbsp = Vec::with_capacity(nal_unit.len() - 1);
    let mut zero_count = 0u8;
    for &byte in &nal_unit[1..] {
        if zero_count >= 2 && byte == 0x03 {
            zero_count = 0;
            continue;
        }
        rbsp.push(byte);
        zero_count = if byte == 0 {
            zero_count.saturating_add(1)
        } else {
            0
        };
    }
    let mut reader = H264BitReader::new(&rbsp);
    let _first_mb_in_slice = reader.read_unsigned_exp_golomb()?;
    reader.read_unsigned_exp_golomb()
}

struct H264BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> H264BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.bit_offset / 8)?;
        let bit = (byte >> (7 - (self.bit_offset % 8))) & 1;
        self.bit_offset += 1;
        Some(bit)
    }

    fn read_unsigned_exp_golomb(&mut self) -> Option<u32> {
        let mut leading_zero_bits = 0u32;
        while self.read_bit()? == 0 {
            leading_zero_bits = leading_zero_bits.checked_add(1)?;
            if leading_zero_bits > 31 {
                return None;
            }
        }
        let mut suffix = 0u32;
        for _ in 0..leading_zero_bits {
            suffix = (suffix << 1) | u32::from(self.read_bit()?);
        }
        ((1u32 << leading_zero_bits) - 1).checked_add(suffix)
    }
}

#[cfg(target_os = "windows")]
#[windows::core::implement(IMFAsyncCallback)]
struct WindowsAsyncMftEventCallback {
    sender: std::sync::mpsc::Sender<Result<H264AsyncMftEvent, String>>,
}

#[cfg(target_os = "windows")]
#[allow(non_snake_case)]
impl IMFAsyncCallback_Impl for WindowsAsyncMftEventCallback_Impl {
    fn GetParameters(&self, _flags: *mut u32, _queue: *mut u32) -> windows::core::Result<()> {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }

    fn Invoke(&self, result: Option<&IMFAsyncResult>) -> windows::core::Result<()> {
        use windows::core::Interface;
        use windows::Win32::Media::MediaFoundation::IMFMediaEventGenerator;

        let event = (|| -> Result<H264AsyncMftEvent, String> {
            let result = result
                .ok_or_else(|| "async H.264 MFT callback received no IMFAsyncResult".to_string())?;
            let state = unsafe { result.GetState() }
                .map_err(|error| format!("async H.264 MFT callback state failed: {error}"))?;
            let generator: IMFMediaEventGenerator = state.cast().map_err(|error| {
                format!("async H.264 MFT callback state is not an event generator: {error}")
            })?;
            let event = unsafe { generator.EndGetEvent(result) }
                .map_err(|error| format!("async H.264 MFT EndGetEvent failed: {error}"))?;
            classify_async_mft_event(&event)
        })();
        let _ = self.sender.send(event);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn classify_async_mft_event(
    event: &windows::Win32::Media::MediaFoundation::IMFMediaEvent,
) -> Result<H264AsyncMftEvent, String> {
    use windows::Win32::Media::MediaFoundation::{
        METransformDrainComplete, METransformHaveOutput, METransformNeedInput,
    };

    let status = unsafe { event.GetStatus() }
        .map_err(|error| format!("async H.264 MFT event status unavailable: {error}"))?;
    status
        .ok()
        .map_err(|error| format!("async H.264 MFT event reported failure: {error}"))?;
    let event_type = unsafe { event.GetType() }
        .map_err(|error| format!("async H.264 MFT event type unavailable: {error}"))?;
    Ok(if event_type == METransformNeedInput.0 as u32 {
        H264AsyncMftEvent::NeedInput
    } else if event_type == METransformHaveOutput.0 as u32 {
        H264AsyncMftEvent::HaveOutput
    } else if event_type == METransformDrainComplete.0 as u32 {
        H264AsyncMftEvent::DrainComplete
    } else {
        H264AsyncMftEvent::Other(event_type)
    })
}

#[cfg(target_os = "windows")]
struct WindowsAsyncMftAdapter {
    generator: windows::Win32::Media::MediaFoundation::IMFMediaEventGenerator,
    callback: IMFAsyncCallback,
    receiver: Receiver<Result<H264AsyncMftEvent, String>>,
    request_pending: bool,
    state: H264AsyncMftState,
}

#[cfg(target_os = "windows")]
impl WindowsAsyncMftAdapter {
    fn for_transform(
        transform: &windows::Win32::Media::MediaFoundation::IMFTransform,
    ) -> Result<Option<Self>, String> {
        use windows::core::Interface;
        use windows::Win32::Media::MediaFoundation::{
            IMFMediaEventGenerator, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK,
        };

        // Synchronous inbox transforms are allowed to return E_NOTIMPL here.
        // Async MFTs are required to expose MF_TRANSFORM_ASYNC in attributes.
        let Ok(attributes) = (unsafe { transform.GetAttributes() }) else {
            return Ok(None);
        };
        let is_async = unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
        if !is_async {
            return Ok(None);
        }
        unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
            .map_err(|error| format!("async H.264 MFT unlock failed: {error}"))?;
        let generator = transform
            .cast::<IMFMediaEventGenerator>()
            .map_err(|error| {
                format!("async H.264 MFT does not expose IMFMediaEventGenerator: {error}")
            })?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let callback: IMFAsyncCallback = WindowsAsyncMftEventCallback { sender }.into();
        let mut adapter = Self {
            generator,
            callback,
            receiver,
            request_pending: false,
            state: H264AsyncMftState::default(),
        };
        adapter.request_next_event()?;
        Ok(Some(adapter))
    }

    fn request_next_event(&mut self) -> Result<(), String> {
        if self.request_pending {
            return Ok(());
        }
        unsafe {
            self.generator
                .BeginGetEvent(&self.callback, &self.generator)
        }
        .map_err(|error| format!("async H.264 MFT BeginGetEvent failed: {error}"))?;
        self.request_pending = true;
        Ok(())
    }

    fn observe_received(&mut self, event: Result<H264AsyncMftEvent, String>) -> Result<(), String> {
        self.request_pending = false;
        self.state.observe(event?)?;
        self.request_next_event()
    }

    fn poll_available(&mut self) -> Result<(), String> {
        use std::sync::mpsc::TryRecvError;

        for _ in 0..32 {
            match self.receiver.try_recv() {
                Ok(event) => self.observe_received(event)?,
                Err(TryRecvError::Empty) => {
                    self.request_next_event()?;
                    return Ok(());
                }
                Err(TryRecvError::Disconnected) => {
                    return Err("async H.264 MFT event callback disconnected".to_string())
                }
            }
        }
        Err("async H.264 MFT produced more than 32 events in one poll cycle".to_string())
    }

    fn wait_for_event(&mut self, timeout: Duration) -> Result<(), String> {
        use std::sync::mpsc::RecvTimeoutError;

        self.request_next_event()?;
        match self.receiver.recv_timeout(timeout) {
            Ok(event) => self.observe_received(event),
            Err(RecvTimeoutError::Timeout) => Err(format!(
                "async H.264 MFT event callback timed out after {} ms",
                timeout.as_millis()
            )),
            Err(RecvTimeoutError::Disconnected) => {
                Err("async H.264 MFT event callback disconnected".to_string())
            }
        }
    }

    fn wait_for_input_credit(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.state.take_input_credit() {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(format!(
                    "async H.264 MFT did not report NeedInput within {} ms",
                    timeout.as_millis()
                ));
            }
            self.wait_for_event(deadline.saturating_duration_since(now))?;
        }
    }

    fn wait_for_output_or_next_input(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            self.poll_available()?;
            if self.state.output_credits != 0 || self.state.input_credits != 0 {
                return Ok(());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(format!(
                    "async H.264 MFT reported neither HaveOutput nor NeedInput within {} ms",
                    timeout.as_millis()
                ));
            }
            self.wait_for_event(deadline.saturating_duration_since(now))?;
        }
    }

    fn take_output_credit(&mut self) -> bool {
        self.state.take_output_credit()
    }

    fn reset_after_flush(&mut self) {
        self.state.reset_after_flush();
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsAsyncMftAdapter {
    fn drop(&mut self) {
        use windows::core::Interface;
        use windows::Win32::Media::MediaFoundation::IMFShutdown;

        if let Ok(shutdown) = self.generator.cast::<IMFShutdown>() {
            unsafe {
                let _ = shutdown.Shutdown();
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn decode_h264_self_test_access_units(
    width: u32,
    height: u32,
    fps: u8,
    access_units: &[EncodedH264Output],
    deadline: Instant,
) -> Result<u32, String> {
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

    if access_units.is_empty() {
        return Ok(0);
    }
    let decoder: IMFTransform =
        unsafe { CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("independent H.264 decoder activation failed: {error}"))?;
    let frame_size = (u64::from(width) << 32) | u64::from(height);
    let frame_rate = (u64::from(fps.max(1)) << 32) | 1;
    let input_type = unsafe { MFCreateMediaType() }
        .map_err(|error| format!("independent decoder input media type failed: {error}"))?;
    unsafe {
        win_result(input_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
        win_result(input_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))?;
        win_result(input_type.SetUINT64(&MF_MT_FRAME_SIZE, frame_size))?;
        win_result(input_type.SetUINT64(&MF_MT_FRAME_RATE, frame_rate))?;
        win_result(
            input_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
        )?;
        decoder
            .SetInputType(0, &input_type, 0)
            .map_err(|error| format!("independent decoder rejected H.264 input: {error}"))?;
    }
    select_h264_self_test_decoder_output(&decoder)?;
    unsafe {
        decoder
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .map_err(|error| format!("independent decoder begin streaming failed: {error}"))?;
        decoder
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
            .map_err(|error| format!("independent decoder start of stream failed: {error}"))?;
    }

    let mut decoded_frames = 0u32;
    for access_unit in access_units {
        if Instant::now() >= deadline {
            return Err(format!(
                "independent decoder exceeded the {} ms encoder self-test deadline",
                H264_ENCODER_SELF_TEST_TIMEOUT.as_millis()
            ));
        }
        let input_buffer = unsafe { MFCreateMemoryBuffer(access_unit.bytes.len() as u32) }
            .map_err(|error| format!("independent decoder input buffer failed: {error}"))?;
        let mut pointer = std::ptr::null_mut();
        unsafe {
            input_buffer
                .Lock(&mut pointer, None, None)
                .map_err(|error| format!("independent decoder input lock failed: {error}"))?;
            std::ptr::copy_nonoverlapping(
                access_unit.bytes.as_ptr(),
                pointer,
                access_unit.bytes.len(),
            );
            input_buffer
                .Unlock()
                .map_err(|error| format!("independent decoder input unlock failed: {error}"))?;
            input_buffer
                .SetCurrentLength(access_unit.bytes.len() as u32)
                .map_err(|error| format!("independent decoder input length failed: {error}"))?;
        }
        let input_sample = unsafe { MFCreateSample() }
            .map_err(|error| format!("independent decoder input sample failed: {error}"))?;
        unsafe {
            win_result(input_sample.AddBuffer(&input_buffer))?;
            win_result(input_sample.SetSampleTime(access_unit.sample_time_100ns))?;
            win_result(input_sample.SetSampleDuration(access_unit.sample_duration_100ns))?;
        }
        let submit = unsafe { decoder.ProcessInput(0, &input_sample, 0) };
        if let Err(error) = submit {
            if error.code() != MF_E_NOTACCEPTING {
                return Err(format!("independent decoder ProcessInput failed: {error}"));
            }
            decoded_frames = decoded_frames.saturating_add(drain_h264_self_test_decoder_outputs(
                &decoder, width, height, deadline,
            )?);
            unsafe { decoder.ProcessInput(0, &input_sample, 0) }.map_err(|retry_error| {
                format!("independent decoder rejected input after output drain: {retry_error}")
            })?;
        }
        decoded_frames = decoded_frames.saturating_add(drain_h264_self_test_decoder_outputs(
            &decoder, width, height, deadline,
        )?);
        if decoded_frames != 0 {
            break;
        }
    }
    if decoded_frames == 0 {
        unsafe {
            let _ = decoder.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            decoder
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .map_err(|error| format!("independent decoder drain command failed: {error}"))?;
        }
        decoded_frames = decoded_frames.saturating_add(drain_h264_self_test_decoder_outputs(
            &decoder, width, height, deadline,
        )?);
    }
    unsafe {
        let _ = decoder.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
    }
    Ok(decoded_frames)
}

#[cfg(target_os = "windows")]
fn select_h264_self_test_decoder_output(
    decoder: &windows::Win32::Media::MediaFoundation::IMFTransform,
) -> Result<(), String> {
    use windows::Win32::Media::MediaFoundation::{
        MFVideoFormat_NV12, MF_E_NO_MORE_TYPES, MF_MT_SUBTYPE,
    };

    for index in 0..64u32 {
        let output_type = match unsafe { decoder.GetOutputAvailableType(0, index) } {
            Ok(output_type) => output_type,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
            Err(error) => {
                return Err(format!(
                    "independent decoder output type enumeration failed: {error}"
                ))
            }
        };
        let subtype = unsafe { output_type.GetGUID(&MF_MT_SUBTYPE) }
            .map_err(|error| format!("independent decoder output subtype failed: {error}"))?;
        if subtype == MFVideoFormat_NV12 {
            unsafe { decoder.SetOutputType(0, &output_type, 0) }
                .map_err(|error| format!("independent decoder rejected NV12 output: {error}"))?;
            return Ok(());
        }
    }
    Err("independent decoder exposes no NV12 output type".to_string())
}

#[cfg(target_os = "windows")]
fn drain_h264_self_test_decoder_outputs(
    decoder: &windows::Win32::Media::MediaFoundation::IMFTransform,
    width: u32,
    height: u32,
    deadline: Instant,
) -> Result<u32, String> {
    use std::mem::ManuallyDrop;
    use windows::Win32::Media::MediaFoundation::*;

    let output_info = unsafe { decoder.GetOutputStreamInfo(0) }
        .map_err(|error| format!("independent decoder output stream info failed: {error}"))?;
    let provides_samples = output_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
    let fallback_size = width
        .saturating_mul(height)
        .saturating_mul(3)
        .saturating_div(2);
    let output_size = output_info.cbSize.max(fallback_size).max(1);
    let mut decoded_frames = 0u32;
    for _ in 0..64 {
        if Instant::now() >= deadline {
            return Err("independent decoder output drain exceeded self-test deadline".to_string());
        }
        let requested_sample = if provides_samples {
            None
        } else {
            let output_buffer = unsafe { MFCreateMemoryBuffer(output_size) }
                .map_err(|error| format!("independent decoder output buffer failed: {error}"))?;
            let output_sample = unsafe { MFCreateSample() }
                .map_err(|error| format!("independent decoder output sample failed: {error}"))?;
            unsafe { output_sample.AddBuffer(&output_buffer) }.map_err(|error| {
                format!("independent decoder output buffer attach failed: {error}")
            })?;
            Some(output_sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(requested_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        let result =
            unsafe { decoder.ProcessOutput(0, std::slice::from_mut(&mut output), &mut status) };
        let produced_sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
        drop(events);
        match result {
            Ok(()) => {
                let sample = produced_sample.ok_or_else(|| {
                    "independent decoder returned success without a sample".to_string()
                })?;
                let buffer = unsafe { sample.ConvertToContiguousBuffer() }.map_err(|error| {
                    format!("independent decoder output coalesce failed: {error}")
                })?;
                let length = unsafe { buffer.GetCurrentLength() }.map_err(|error| {
                    format!("independent decoder output length failed: {error}")
                })?;
                if length != 0 {
                    decoded_frames = decoded_frames.saturating_add(1);
                }
            }
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                return Ok(decoded_frames)
            }
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                select_h264_self_test_decoder_output(decoder)?;
            }
            Err(error) => return Err(format!("independent decoder ProcessOutput failed: {error}")),
        }
    }
    Err("independent decoder exceeded the 64-sample output bound".to_string())
}

#[cfg(target_os = "windows")]
struct WindowsH264Encoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    codec_api: Option<windows::Win32::Media::MediaFoundation::ICodecAPI>,
    async_adapter: Option<WindowsAsyncMftAdapter>,
    output_size: u32,
    output_provides_samples: bool,
    frame_duration_100ns: i64,
    next_keyframe_time_100ns: i64,
    bitrate_bps: u32,
    input_width: u32,
    input_height: u32,
    fps: u8,
    diagnostics: H264EncoderDiagnostics,
    _dxgi_device_manager: Option<MfDxgiDeviceManager>,
    uses_gpu_surfaces: bool,
    gpu_surface_release_errors: GpuSurfaceReleaseErrorSink,
}

#[cfg(target_os = "windows")]
struct WindowsH264EncoderCandidate {
    activation: windows::Win32::Media::MediaFoundation::IMFActivate,
    name: String,
    hardware: bool,
    adapter_luid: Option<String>,
    adapter_luid_value: Option<windows::Win32::Foundation::LUID>,
    hardware_url: Option<String>,
    driver_version: Option<String>,
}

const H264_ENCODER_CANDIDATE_REPORT_LIMIT: usize = 16;

#[cfg(target_os = "windows")]
fn encoder_candidate_report(
    candidate: &WindowsH264EncoderCandidate,
    width: u32,
    height: u32,
    fps: u8,
    gpu_surface_input: bool,
) -> H264EncoderCandidateReport {
    H264EncoderCandidateReport {
        name: candidate.name.clone(),
        hardware: candidate.hardware,
        adapter_luid: candidate.adapter_luid.clone(),
        hardware_url: candidate.hardware_url.clone(),
        driver_version: candidate.driver_version.clone(),
        input_width: width,
        input_height: height,
        fps,
        gpu_surface_input,
        failure_stage: Some("activation".to_string()),
        ..Default::default()
    }
}

#[cfg(target_os = "windows")]
fn retain_encoder_candidate_report(
    reports: &mut Vec<H264EncoderCandidateReport>,
    total_count: &mut u32,
    report: H264EncoderCandidateReport,
) {
    *total_count = total_count.saturating_add(1);
    match serde_json::to_string(&report) {
        Ok(report_json) if report.admitted => {
            log::info!("screen-share H.264 encoder candidate admitted: {report_json}")
        }
        Ok(report_json) => {
            log::warn!("screen-share H.264 encoder candidate rejected: {report_json}")
        }
        Err(error) => {
            log::warn!("screen-share H.264 encoder candidate report serialization failed: {error}")
        }
    }
    if reports.len() < H264_ENCODER_CANDIDATE_REPORT_LIMIT {
        reports.push(report);
    } else if report.admitted {
        // Preserve the selected candidate even on machines exposing an
        // unexpectedly large number of encoder registrations.
        reports[H264_ENCODER_CANDIDATE_REPORT_LIMIT - 1] = report;
    }
}

fn bounded_encoder_failure_summary(failures: &[String]) -> Option<String> {
    if failures.is_empty() {
        return None;
    }
    let mut summary = failures
        .iter()
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if failures.len() > 4 {
        summary.push_str(&format!("; and {} more", failures.len() - 4));
    }
    if summary.len() > 1_024 {
        summary.truncate(1_024);
    }
    Some(summary)
}

#[cfg(target_os = "windows")]
fn media_foundation_attribute_string(
    attributes: &windows::Win32::Media::MediaFoundation::IMFAttributes,
    key: &windows::core::GUID,
) -> Option<String> {
    let length = unsafe { attributes.GetStringLength(key) }.ok()? as usize;
    let mut buffer = vec![0u16; length.saturating_add(1)];
    unsafe { attributes.GetString(key, &mut buffer, None) }.ok()?;
    Some(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(target_os = "windows")]
fn media_foundation_attribute_display(
    attributes: &windows::Win32::Media::MediaFoundation::IMFAttributes,
    key: &windows::core::GUID,
) -> Option<String> {
    media_foundation_attribute_string(attributes, key).or_else(|| {
        unsafe { attributes.GetUINT64(key) }
            .ok()
            .map(|value| format!("0x{value:016X}"))
    })
}

#[cfg(target_os = "windows")]
fn media_foundation_attribute_luid(
    attributes: &windows::Win32::Media::MediaFoundation::IMFAttributes,
    key: &windows::core::GUID,
) -> Option<windows::Win32::Foundation::LUID> {
    use std::mem::{size_of, MaybeUninit};

    use windows::Win32::Foundation::LUID;

    let expected_size = u32::try_from(size_of::<LUID>()).ok()?;
    if unsafe { attributes.GetBlobSize(key) }.ok()? != expected_size {
        return None;
    }
    let mut luid = MaybeUninit::<LUID>::zeroed();
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(luid.as_mut_ptr().cast::<u8>(), expected_size as usize)
    };
    let mut actual_size = 0;
    unsafe { attributes.GetBlob(key, bytes, Some(&mut actual_size)) }.ok()?;
    if actual_size != expected_size {
        return None;
    }
    Some(unsafe { luid.assume_init() })
}

#[cfg(target_os = "windows")]
fn format_adapter_luid(luid: &windows::Win32::Foundation::LUID) -> String {
    format!("0x{:08X}:{:08X}", luid.HighPart as u32, luid.LowPart)
}

#[cfg(target_os = "windows")]
fn adapter_luids_match(
    left: &windows::Win32::Foundation::LUID,
    right: &windows::Win32::Foundation::LUID,
) -> bool {
    left.HighPart == right.HighPart && left.LowPart == right.LowPart
}

#[cfg(target_os = "windows")]
fn enumerate_h264_encoder_candidates(
    hardware: bool,
) -> Result<Vec<WindowsH264EncoderCandidate>, String> {
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::CoTaskMemFree;

    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let flags = if hardware {
        MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER
    } else {
        MFT_ENUM_FLAG_SYNCMFT
            | MFT_ENUM_FLAG_ASYNCMFT
            | MFT_ENUM_FLAG_LOCALMFT
            | MFT_ENUM_FLAG_SORTANDFILTER
    };
    let mut raw_activations: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut activation_count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&input),
            Some(&output),
            &mut raw_activations,
            &mut activation_count,
        )
    }
    .map_err(|error| {
        format!(
            "{} H.264 MFT enumeration failed: {error}",
            if hardware { "hardware" } else { "software" }
        )
    })?;

    if raw_activations.is_null() || activation_count == 0 {
        if !raw_activations.is_null() {
            unsafe { CoTaskMemFree(Some(raw_activations.cast())) };
        }
        return Ok(Vec::new());
    }

    let mut candidates = Vec::with_capacity(activation_count as usize);
    let activations =
        unsafe { std::slice::from_raw_parts_mut(raw_activations, activation_count as usize) };
    for (index, slot) in activations.iter_mut().enumerate() {
        let Some(activation) = slot.take() else {
            continue;
        };
        let name = media_foundation_attribute_string(&activation, &MFT_FRIENDLY_NAME_Attribute)
            .unwrap_or_else(|| {
                format!(
                    "{} H.264 MFT #{}",
                    if hardware { "Hardware" } else { "Software" },
                    index + 1
                )
            });
        let adapter_luid_value =
            media_foundation_attribute_luid(&activation, &MFT_ENUM_ADAPTER_LUID);
        let adapter_luid = adapter_luid_value
            .as_ref()
            .map(format_adapter_luid)
            .or_else(|| media_foundation_attribute_display(&activation, &MFT_ENUM_ADAPTER_LUID));
        let hardware_url =
            media_foundation_attribute_display(&activation, &MFT_ENUM_HARDWARE_URL_Attribute);
        let driver_version =
            media_foundation_attribute_display(&activation, &MFT_GFX_DRIVER_VERSION_ID_Attribute);
        candidates.push(WindowsH264EncoderCandidate {
            activation,
            name,
            hardware,
            adapter_luid,
            adapter_luid_value,
            hardware_url,
            driver_version,
        });
    }
    unsafe { CoTaskMemFree(Some(raw_activations.cast())) };
    Ok(candidates)
}

#[cfg(target_os = "windows")]
fn win_result<T>(result: windows::core::Result<T>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn variant_as_u32(value: &windows::core::VARIANT) -> Option<u32> {
    u32::try_from(value).ok().or_else(|| {
        i32::try_from(value)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
    })
}

#[cfg(target_os = "windows")]
fn negotiate_codec_u32(
    api: &windows::Win32::Media::MediaFoundation::ICodecAPI,
    property: &windows::core::GUID,
    desired: u32,
) -> H264EncoderCapabilitySnapshot {
    negotiate_codec_u32_labeled(api, property, desired, desired.to_string())
}

#[cfg(target_os = "windows")]
fn negotiate_codec_u32_labeled(
    api: &windows::Win32::Media::MediaFoundation::ICodecAPI,
    property: &windows::core::GUID,
    desired: u32,
    requested_value: String,
) -> H264EncoderCapabilitySnapshot {
    use windows::core::VARIANT;

    let mut snapshot = H264EncoderCapabilitySnapshot {
        requested_value: Some(requested_value),
        ..Default::default()
    };
    if let Err(error) = unsafe { api.IsSupported(property) } {
        snapshot.hresult = Some(format_hresult(error.code()));
        snapshot.detail = Some(format!("not supported: {error}"));
        return snapshot;
    }
    snapshot.supported = true;
    let modifiable = unsafe { api.IsModifiable(property) };
    if modifiable != windows::core::HRESULT(0) {
        snapshot.hresult = Some(format_hresult(modifiable));
        snapshot.detail = Some(format!(
            "IsModifiable did not return S_OK ({})",
            format_hresult(modifiable)
        ));
        return snapshot;
    }
    snapshot.modifiable = true;
    let desired_variant = VARIANT::from(desired);
    if let Err(error) = unsafe { api.SetValue(property, &desired_variant) } {
        snapshot.hresult = Some(format_hresult(error.code()));
        snapshot.detail = Some(format!("SetValue failed: {error}"));
        return snapshot;
    }
    snapshot.set_succeeded = true;
    let readback = match unsafe { api.GetValue(property) } {
        Ok(readback) => readback,
        Err(error) => {
            snapshot.hresult = Some(format_hresult(error.code()));
            snapshot.detail = Some(format!("GetValue failed after SetValue: {error}"));
            return snapshot;
        }
    };
    snapshot.readback_succeeded = true;
    match variant_as_u32(&readback) {
        Some(actual) => {
            snapshot.final_value = Some(actual.to_string());
            snapshot.value_matches = actual == desired;
            if actual != desired {
                snapshot.detail = Some(format!(
                    "readback mismatch: requested {desired}, received {actual}"
                ));
            }
        }
        None => snapshot.detail = Some("GetValue returned a non-integer VARIANT".to_string()),
    }
    snapshot
}

#[cfg(target_os = "windows")]
fn format_hresult(hresult: windows::core::HRESULT) -> String {
    format!("0x{:08X}", hresult.0 as u32)
}

#[cfg(target_os = "windows")]
fn negotiate_codec_bool(
    api: &windows::Win32::Media::MediaFoundation::ICodecAPI,
    property: &windows::core::GUID,
    desired: bool,
) -> H264EncoderCapabilitySnapshot {
    use windows::core::VARIANT;

    let mut snapshot = H264EncoderCapabilitySnapshot {
        requested_value: Some(desired.to_string()),
        ..Default::default()
    };
    if let Err(error) = unsafe { api.IsSupported(property) } {
        snapshot.hresult = Some(format_hresult(error.code()));
        snapshot.detail = Some(format!("not supported: {error}"));
        return snapshot;
    }
    snapshot.supported = true;
    let modifiable = unsafe { api.IsModifiable(property) };
    if modifiable != windows::core::HRESULT(0) {
        snapshot.hresult = Some(format_hresult(modifiable));
        snapshot.detail = Some(format!(
            "IsModifiable did not return S_OK ({})",
            format_hresult(modifiable)
        ));
        return snapshot;
    }
    snapshot.modifiable = true;
    let desired_variant = VARIANT::from(desired);
    if let Err(error) = unsafe { api.SetValue(property, &desired_variant) } {
        snapshot.hresult = Some(format_hresult(error.code()));
        snapshot.detail = Some(format!("SetValue failed: {error}"));
        return snapshot;
    }
    snapshot.set_succeeded = true;
    let readback = match unsafe { api.GetValue(property) } {
        Ok(readback) => readback,
        Err(error) => {
            snapshot.hresult = Some(format_hresult(error.code()));
            snapshot.detail = Some(format!("GetValue failed after SetValue: {error}"));
            return snapshot;
        }
    };
    snapshot.readback_succeeded = true;
    match bool::try_from(&readback) {
        Ok(actual) => {
            snapshot.final_value = Some(actual.to_string());
            snapshot.value_matches = actual == desired;
            if actual != desired {
                snapshot.detail = Some(format!(
                    "readback mismatch: requested {desired}, received {actual}"
                ));
            }
        }
        Err(_) => snapshot.detail = Some("GetValue returned a non-boolean VARIANT".to_string()),
    }
    snapshot
}

fn capability_degradation_reason(
    label: &str,
    capability: &H264EncoderCapabilitySnapshot,
) -> Option<String> {
    if capability.value_matches {
        return None;
    }
    Some(format!(
        "{label}: {}",
        capability
            .detail
            .as_deref()
            .unwrap_or("requested value was not confirmed")
    ))
}

fn b_frame_configuration_confirmed(
    capability: &H264EncoderCapabilitySnapshot,
    baseline_profile_confirmed: bool,
    observed_b_slice_count: u32,
) -> bool {
    observed_b_slice_count == 0 && (capability.value_matches || baseline_profile_confirmed)
}

#[cfg(target_os = "windows")]
fn negotiate_encoder_capabilities(
    codec_api: Option<&windows::Win32::Media::MediaFoundation::ICodecAPI>,
    bitrate_bps: u32,
) -> H264EncoderCapabilitiesSnapshot {
    use windows::Win32::Media::MediaFoundation::*;

    let Some(api) = codec_api else {
        let unavailable = H264EncoderCapabilitySnapshot {
            detail: Some("ICodecAPI is unavailable".to_string()),
            ..Default::default()
        };
        return H264EncoderCapabilitiesSnapshot {
            low_latency: unavailable.clone(),
            rate_control: unavailable.clone(),
            rate_control_attempts: vec![unavailable.clone()],
            buffer_size: unavailable.clone(),
            max_bitrate: unavailable.clone(),
            reference_frames: unavailable.clone(),
            cabac: unavailable.clone(),
            b_frames_disabled: unavailable.clone(),
            dynamic_bitrate: unavailable.clone(),
            force_keyframe: unavailable,
            degradation_reasons: vec!["encoder does not expose ICodecAPI".to_string()],
        };
    };

    let mut low_latency = negotiate_codec_bool(api, &CODECAPI_AVLowLatencyMode, true);
    if !low_latency.value_matches {
        low_latency = negotiate_codec_bool(api, &CODECAPI_AVEncCommonLowLatency, true);
    }
    let mut rate_control_attempts = Vec::new();
    for (label, mode) in [
        (
            "LowDelayVBR(4)",
            eAVEncCommonRateControlMode_LowDelayVBR.0 as u32,
        ),
        ("CBR(0)", eAVEncCommonRateControlMode_CBR.0 as u32),
        (
            "PeakConstrainedVBR(1)",
            eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32,
        ),
    ] {
        let attempt = negotiate_codec_u32_labeled(
            api,
            &CODECAPI_AVEncCommonRateControlMode,
            mode,
            label.to_string(),
        );
        let selected = attempt.value_matches;
        rate_control_attempts.push(attempt);
        if selected {
            break;
        }
    }
    let rate_control = rate_control_attempts
        .iter()
        .find(|attempt| attempt.value_matches)
        .or_else(|| rate_control_attempts.last())
        .cloned()
        .unwrap_or_default();
    let buffer_size = negotiate_codec_u32(
        api,
        &CODECAPI_AVEncCommonBufferSize,
        bitrate_bps.saturating_div(2).max(1),
    );
    let max_bitrate = negotiate_codec_u32(
        api,
        &CODECAPI_AVEncCommonMaxBitRate,
        bitrate_bps.saturating_add(bitrate_bps.saturating_div(5)),
    );
    let reference_frames = negotiate_codec_u32(api, &CODECAPI_AVEncVideoMaxNumRefFrame, 1);
    // The production type remains Baseline until High/CABAC has its own
    // compatibility gate. Explicitly request CAVLC and record the result.
    let cabac = negotiate_codec_bool(api, &CODECAPI_AVEncH264CABACEnable, false);
    let b_frames_disabled = negotiate_codec_u32(api, &CODECAPI_AVEncMPVDefaultBPictureCount, 0);
    let dynamic_bitrate = negotiate_codec_u32(api, &CODECAPI_AVEncCommonMeanBitRate, bitrate_bps);
    // A false command is side-effect free, but still proves that the command
    // property accepts SetValue and supplies the requested GetValue evidence.
    let force_keyframe = negotiate_codec_u32(api, &CODECAPI_AVEncVideoForceKeyFrame, 0);

    let mut degradation_reasons = Vec::new();
    for (label, capability) in [
        ("low_latency", &low_latency),
        ("rate_control", &rate_control),
        ("buffer_size", &buffer_size),
        ("max_bitrate", &max_bitrate),
        ("reference_frames", &reference_frames),
        ("cabac_disabled", &cabac),
        ("b_frames_disabled", &b_frames_disabled),
        ("dynamic_bitrate", &dynamic_bitrate),
        ("force_keyframe", &force_keyframe),
    ] {
        if let Some(reason) = capability_degradation_reason(label, capability) {
            degradation_reasons.push(reason);
        }
    }
    H264EncoderCapabilitiesSnapshot {
        low_latency,
        rate_control,
        rate_control_attempts,
        buffer_size,
        max_bitrate,
        reference_frames,
        cabac,
        b_frames_disabled,
        dynamic_bitrate,
        force_keyframe,
        degradation_reasons,
    }
}

#[cfg(target_os = "windows")]
impl WindowsH264Encoder {
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
        hardware_allowed: bool,
        hardware_runtime_failure: Option<String>,
    ) -> Result<Self, String> {
        use windows::Win32::Media::MediaFoundation::IMFTransform;
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

        let (software_width, software_height, software_fps) =
            software_encoder_limits(width as usize, height as usize, fps);
        let software_width = software_width as u32;
        let software_height = software_height as u32;

        let mut hardware_failures = Vec::new();
        let mut candidate_reports = Vec::new();
        let mut candidate_report_total_count = 0u32;
        if !hardware_allowed {
            hardware_failures.push(hardware_runtime_failure.unwrap_or_else(|| {
                "hardware encoder disabled after a runtime failure".to_string()
            }));
        } else {
            match enumerate_h264_encoder_candidates(true) {
                Ok(candidates) if candidates.is_empty() => {
                    hardware_failures.push("no hardware H.264 MFT was enumerated".to_string());
                }
                Ok(candidates) => {
                    for candidate in candidates {
                        let mut report =
                            encoder_candidate_report(&candidate, width, height, fps, false);
                        let transform = match unsafe {
                            candidate.activation.ActivateObject::<IMFTransform>()
                        } {
                            Ok(transform) => transform,
                            Err(error) => {
                                report.failure_reason = Some(error.to_string());
                                retain_encoder_candidate_report(
                                    &mut candidate_reports,
                                    &mut candidate_report_total_count,
                                    report,
                                );
                                hardware_failures
                                    .push(format!("{} activation failed: {error}", candidate.name));
                                continue;
                            }
                        };
                        match Self::from_transform(
                            transform,
                            candidate.name.clone(),
                            candidate.hardware,
                            candidate.adapter_luid.clone(),
                            candidate.hardware_url.clone(),
                            candidate.driver_version.clone(),
                            None,
                            false,
                            None,
                            false,
                            None,
                            width,
                            height,
                            fps,
                            bitrate_bps,
                            &mut report,
                        ) {
                            Ok(mut encoder) => {
                                retain_encoder_candidate_report(
                                    &mut candidate_reports,
                                    &mut candidate_report_total_count,
                                    report,
                                );
                                encoder.diagnostics.candidate_report_total_count =
                                    candidate_report_total_count;
                                encoder.diagnostics.candidate_reports = candidate_reports;
                                return Ok(encoder);
                            }
                            Err(error) => {
                                report.failure_reason = Some(error.clone());
                                retain_encoder_candidate_report(
                                    &mut candidate_reports,
                                    &mut candidate_report_total_count,
                                    report,
                                );
                                hardware_failures.push(format!("{}: {error}", candidate.name))
                            }
                        }
                    }
                }
                Err(error) => hardware_failures.push(error),
            }
        }
        let hardware_fallback_reason = bounded_encoder_failure_summary(&hardware_failures)
            .unwrap_or_else(|| {
                "hardware encoder selection did not produce a candidate".to_string()
            });

        let mut all_failures = hardware_failures;
        match enumerate_h264_encoder_candidates(false) {
            Ok(candidates) => {
                for candidate in candidates {
                    let mut report = encoder_candidate_report(
                        &candidate,
                        software_width,
                        software_height,
                        software_fps,
                        false,
                    );
                    let transform =
                        match unsafe { candidate.activation.ActivateObject::<IMFTransform>() } {
                            Ok(transform) => transform,
                            Err(error) => {
                                report.failure_reason = Some(error.to_string());
                                retain_encoder_candidate_report(
                                    &mut candidate_reports,
                                    &mut candidate_report_total_count,
                                    report,
                                );
                                all_failures
                                    .push(format!("{} activation failed: {error}", candidate.name));
                                continue;
                            }
                        };
                    match Self::from_transform(
                        transform,
                        candidate.name.clone(),
                        candidate.hardware,
                        candidate.adapter_luid.clone(),
                        candidate.hardware_url.clone(),
                        candidate.driver_version.clone(),
                        Some(hardware_fallback_reason.clone()),
                        software_width != width || software_height != height || software_fps != fps,
                        None,
                        false,
                        None,
                        software_width,
                        software_height,
                        software_fps,
                        bitrate_bps,
                        &mut report,
                    ) {
                        Ok(mut encoder) => {
                            retain_encoder_candidate_report(
                                &mut candidate_reports,
                                &mut candidate_report_total_count,
                                report,
                            );
                            encoder.diagnostics.candidate_report_total_count =
                                candidate_report_total_count;
                            encoder.diagnostics.candidate_reports = candidate_reports;
                            return Ok(encoder);
                        }
                        Err(error) => {
                            report.failure_reason = Some(error.clone());
                            retain_encoder_candidate_report(
                                &mut candidate_reports,
                                &mut candidate_report_total_count,
                                report,
                            );
                            all_failures.push(format!("{}: {error}", candidate.name));
                        }
                    }
                }
            }
            Err(error) => all_failures.push(error),
        }

        // Keep the known inbox Microsoft software encoder as the final
        // compatibility path even if MFT enumeration is unavailable or filtered.
        let mut inbox_report = H264EncoderCandidateReport {
            name: "Microsoft H.264 Video Encoder MFT".to_string(),
            input_width: software_width,
            input_height: software_height,
            fps: software_fps,
            failure_stage: Some("activation".to_string()),
            ..Default::default()
        };
        let transform: IMFTransform = unsafe {
            CoCreateInstance(
                &windows::Win32::Media::MediaFoundation::CLSID_MSH264EncoderMFT,
                None,
                CLSCTX_INPROC_SERVER,
            )
        }
        .map_err(|error| {
            inbox_report.failure_reason = Some(error.to_string());
            retain_encoder_candidate_report(
                &mut candidate_reports,
                &mut candidate_report_total_count,
                inbox_report.clone(),
            );
            all_failures.push(format!(
                "Microsoft software encoder activation failed: {error}"
            ));
            format!(
                "No usable H.264 encoder: {}",
                bounded_encoder_failure_summary(&all_failures).unwrap_or_default()
            )
        })?;
        match Self::from_transform(
            transform,
            "Microsoft H.264 Video Encoder MFT".to_string(),
            false,
            None,
            None,
            None,
            Some(hardware_fallback_reason),
            software_width != width || software_height != height || software_fps != fps,
            None,
            false,
            None,
            software_width,
            software_height,
            software_fps,
            bitrate_bps,
            &mut inbox_report,
        ) {
            Ok(mut encoder) => {
                retain_encoder_candidate_report(
                    &mut candidate_reports,
                    &mut candidate_report_total_count,
                    inbox_report,
                );
                encoder.diagnostics.candidate_report_total_count = candidate_report_total_count;
                encoder.diagnostics.candidate_reports = candidate_reports;
                Ok(encoder)
            }
            Err(error) => {
                inbox_report.failure_reason = Some(error.clone());
                retain_encoder_candidate_report(
                    &mut candidate_reports,
                    &mut candidate_report_total_count,
                    inbox_report,
                );
                all_failures.push(format!("Microsoft software encoder: {error}"));
                Err(format!(
                    "No usable H.264 encoder: {}",
                    bounded_encoder_failure_summary(&all_failures).unwrap_or_default()
                ))
            }
        }
    }

    fn new_for_gpu_surface(
        surface: &GpuNv12Surface,
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
    ) -> Result<Self, String> {
        use windows::core::Interface;
        use windows::Win32::Graphics::Dxgi::IDXGIDevice;
        use windows::Win32::Media::MediaFoundation::IMFTransform;

        let device = unsafe { surface.texture().GetDevice() }
            .map_err(|error| format!("H.264 GPU input device lookup failed: {error}"))?;
        let dxgi_device: IDXGIDevice = device
            .cast()
            .map_err(|error| format!("H.264 GPU input device is not an IDXGIDevice: {error}"))?;
        let adapter = unsafe { dxgi_device.GetAdapter() }
            .map_err(|error| format!("H.264 GPU input adapter lookup failed: {error}"))?;
        let adapter_descriptor = unsafe { adapter.GetDesc() }
            .map_err(|error| format!("H.264 GPU input adapter descriptor failed: {error}"))?;
        let device_luid = adapter_descriptor.AdapterLuid;
        let device_luid_display = format_adapter_luid(&device_luid);
        let description_end = adapter_descriptor
            .Description
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(adapter_descriptor.Description.len());
        let input_adapter = H264DxgiAdapterIdentity {
            description: Some(String::from_utf16_lossy(
                &adapter_descriptor.Description[..description_end],
            )),
            vendor_id: Some(format!("0x{:04X}", adapter_descriptor.VendorId)),
            device_id: Some(format!("0x{:04X}", adapter_descriptor.DeviceId)),
            luid: Some(device_luid_display.clone()),
            // DXGI_DESC exposes no PNP or driver-version identity. Do not infer
            // either from the MFT activation metadata.
            driver_version: None,
            pnp_device_id: None,
        };
        let candidates = enumerate_h264_encoder_candidates(true)?;
        if candidates.is_empty() {
            return Err("No hardware H.264 MFT was enumerated for GPU-surface input".to_string());
        }
        let mut failures = Vec::new();
        let mut candidate_reports = Vec::new();
        let mut candidate_report_total_count = 0u32;
        for candidate in candidates {
            let mut report = encoder_candidate_report(&candidate, width, height, fps, true);
            report.input_adapter = Some(input_adapter.clone());
            report.activation_adapter_luid = candidate
                .adapter_luid_value
                .as_ref()
                .map(format_adapter_luid);
            report.failure_stage = Some("adapter_match".to_string());
            // Some drivers accept a foreign DXGI manager and then stop emitting
            // async events after ProcessInput. A missing LUID cannot be probed
            // safely, so require the activation's blob to match the input GPU.
            let Some(candidate_luid) = candidate.adapter_luid_value.as_ref() else {
                let error = format!(
                    "hardware MFT did not expose a valid MFT_ENUM_ADAPTER_LUID blob; refusing direct DXGI input from adapter {device_luid_display}"
                );
                report.failure_reason = Some(error.clone());
                retain_encoder_candidate_report(
                    &mut candidate_reports,
                    &mut candidate_report_total_count,
                    report,
                );
                failures.push(format!("{}: {error}", candidate.name));
                continue;
            };
            if !adapter_luids_match(candidate_luid, &device_luid) {
                report.luid_match = Some(false);
                let error = format!(
                    "hardware MFT adapter {} does not match DXGI input adapter {device_luid_display}",
                    format_adapter_luid(candidate_luid)
                );
                report.failure_reason = Some(error.clone());
                retain_encoder_candidate_report(
                    &mut candidate_reports,
                    &mut candidate_report_total_count,
                    report,
                );
                failures.push(format!("{}: {error}", candidate.name));
                continue;
            }
            report.luid_match = Some(true);
            report.failure_stage = Some("activation".to_string());
            let transform = match unsafe { candidate.activation.ActivateObject::<IMFTransform>() } {
                Ok(transform) => transform,
                Err(error) => {
                    report.failure_reason = Some(error.to_string());
                    retain_encoder_candidate_report(
                        &mut candidate_reports,
                        &mut candidate_report_total_count,
                        report,
                    );
                    failures.push(format!("{} activation failed: {error}", candidate.name));
                    continue;
                }
            };
            let manager = match create_mf_dxgi_device_manager(&device) {
                Ok(manager) => manager,
                Err(error) => {
                    report.failure_stage = Some("dxgi_device_manager".to_string());
                    report.failure_reason = Some(error.to_string());
                    retain_encoder_candidate_report(
                        &mut candidate_reports,
                        &mut candidate_report_total_count,
                        report,
                    );
                    failures.push(format!("{} device manager: {error}", candidate.name));
                    continue;
                }
            };
            match Self::from_transform(
                transform,
                candidate.name.clone(),
                true,
                candidate.adapter_luid.clone(),
                candidate.hardware_url.clone(),
                candidate.driver_version.clone(),
                None,
                false,
                Some(manager),
                true,
                Some(surface),
                width,
                height,
                fps,
                bitrate_bps,
                &mut report,
            ) {
                Ok(mut encoder) => {
                    retain_encoder_candidate_report(
                        &mut candidate_reports,
                        &mut candidate_report_total_count,
                        report,
                    );
                    encoder.diagnostics.candidate_report_total_count = candidate_report_total_count;
                    encoder.diagnostics.candidate_reports = candidate_reports;
                    return Ok(encoder);
                }
                Err(error) => {
                    report.failure_reason = Some(error.clone());
                    retain_encoder_candidate_report(
                        &mut candidate_reports,
                        &mut candidate_report_total_count,
                        report,
                    );
                    failures.push(format!("{}: {error}", candidate.name));
                }
            }
        }
        Err(format!(
            "No hardware H.264 encoder accepted the WGC DXGI surface: {}",
            bounded_encoder_failure_summary(&failures).unwrap_or_default()
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn from_transform(
        transform: windows::Win32::Media::MediaFoundation::IMFTransform,
        encoder_name: String,
        encoder_hardware: bool,
        adapter_luid: Option<String>,
        hardware_url: Option<String>,
        driver_version: Option<String>,
        fallback_reason: Option<String>,
        software_fallback_limited: bool,
        dxgi_device_manager: Option<MfDxgiDeviceManager>,
        uses_gpu_surfaces: bool,
        gpu_startup_surface: Option<&GpuNv12Surface>,
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
        candidate_report: &mut H264EncoderCandidateReport,
    ) -> Result<Self, String> {
        use windows::core::{Interface, VARIANT};
        use windows::Win32::Media::MediaFoundation::*;

        candidate_report.activation_succeeded = true;
        candidate_report.failure_stage = Some("async_adapter".to_string());
        if let Ok(attributes) = unsafe { transform.GetAttributes() } {
            let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        }
        // Async transforms are not legal to drive with the synchronous
        // ProcessInput/ProcessOutput loop. Detect them before type negotiation,
        // unlock only those transforms, and retain their event generator.
        let async_adapter = WindowsAsyncMftAdapter::for_transform(&transform)?;
        if let Some(manager) = dxgi_device_manager.as_ref() {
            candidate_report.failure_stage = Some("dxgi_device_manager".to_string());
            unsafe {
                win_result(transform.ProcessMessage(
                    MFT_MESSAGE_SET_D3D_MANAGER,
                    manager.manager.as_raw() as usize,
                ))?;
            }
        }
        candidate_report.failure_stage = Some("media_type_negotiation".to_string());
        let frame_size = (u64::from(width) << 32) | u64::from(height);
        let frame_rate = (u64::from(fps.max(1)) << 32) | 1;
        let output = unsafe { MFCreateMediaType() }
            .map_err(|error| format!("H.264 output media type creation failed: {error}"))?;
        unsafe {
            win_result(output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            win_result(output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))?;
            win_result(output.SetUINT64(&MF_MT_FRAME_SIZE, frame_size))?;
            win_result(output.SetUINT64(&MF_MT_FRAME_RATE, frame_rate))?;
            win_result(output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps))?;
            win_result(
                output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
            )?;
            win_result(output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32))?;
            win_result(output.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, u32::from(fps.max(1)) * 2))?;
            win_result(output.SetUINT32(&MF_MT_REALTIME_CONTENT, 1))?;
            win_result(output.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32))?;
            win_result(output.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32))?;
            win_result(output.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT601.0 as u32))?;
            win_result(
                output.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32),
            )?;
            win_result(transform.SetOutputType(0, &output, 0))?;
        }
        let output_profile_baseline_confirmed = unsafe { transform.GetOutputCurrentType(0) }
            .ok()
            .and_then(|current| unsafe { current.GetUINT32(&MF_MT_MPEG2_PROFILE) }.ok())
            .is_some_and(|profile| profile == eAVEncH264VProfile_Base.0 as u32);
        let input = unsafe { MFCreateMediaType() }
            .map_err(|error| format!("H.264 input media type creation failed: {error}"))?;
        unsafe {
            win_result(input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            win_result(input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))?;
            win_result(input.SetUINT64(&MF_MT_FRAME_SIZE, frame_size))?;
            win_result(input.SetUINT64(&MF_MT_FRAME_RATE, frame_rate))?;
            win_result(
                input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
            )?;
            win_result(input.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32))?;
            win_result(input.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32))?;
            win_result(input.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT601.0 as u32))?;
            win_result(
                input.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32),
            )?;
            win_result(transform.SetInputType(0, &input, 0))?;
        }
        let codec_api = transform.cast::<ICodecAPI>().ok();
        if let Some(api) = codec_api.as_ref() {
            let gop_size = VARIANT::from(u32::from(fps.max(1)) * 2);
            match unsafe { api.IsSupported(&CODECAPI_AVEncMPVGOPSize) } {
                Ok(()) => {
                    if let Err(error) =
                        unsafe { api.SetValue(&CODECAPI_AVEncMPVGOPSize, &gop_size) }
                    {
                        log::warn!(
                            "H.264 encoder '{encoder_name}' optional GOP size setting failed: {error}"
                        );
                    }
                }
                Err(error) => log::warn!(
                    "H.264 encoder '{encoder_name}' does not support optional GOP size control: {error}"
                ),
            }
        }
        // Every optional property is negotiated and reported independently.
        // Baseline profile remains the hard muxing guard when B-frame control
        // is missing or an implementation rejects the optional SetValue.
        let capabilities = negotiate_encoder_capabilities(codec_api.as_ref(), bitrate_bps);
        candidate_report.capabilities = capabilities.clone();
        for reason in &capabilities.degradation_reasons {
            log::warn!("H.264 encoder '{encoder_name}' optional capability degraded: {reason}");
        }
        candidate_report.failure_stage = Some("stream_configuration".to_string());
        let output_info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|error| format!("H.264 output stream info failed: {error}"))?;
        let output_provides_samples =
            output_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        unsafe {
            // 不在这里发 MFT_MESSAGE_COMMAND_FLUSH：刚激活并完成类型协商的 MFT
            // 没有任何可丢弃的缓冲数据，而 Microsoft 的软件 H.264 MFT 会对尚未
            // 开始流传输的 FLUSH 返回 E_FAIL，把本可用的候选整个否掉。
            win_result(transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
            win_result(transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
        }
        candidate_report.configuration_succeeded = true;
        candidate_report.failure_stage = Some("startup_self_test".to_string());
        let async_mode = async_adapter.is_some();
        let mut encoder = Self {
            transform,
            codec_api,
            async_adapter,
            output_size: output_info.cbSize.max(width.saturating_mul(height)),
            output_provides_samples,
            frame_duration_100ns: 10_000_000 / i64::from(fps.max(1)),
            next_keyframe_time_100ns: H264_KEYFRAME_INTERVAL_100NS,
            bitrate_bps,
            input_width: width,
            input_height: height,
            fps,
            diagnostics: H264EncoderDiagnostics {
                name: encoder_name,
                hardware: encoder_hardware,
                async_mode,
                adapter_luid,
                hardware_url,
                driver_version,
                fallback_reason,
                input_width: width,
                input_height: height,
                fps,
                software_fallback_limited,
                self_test: H264EncoderSelfTestSnapshot::default(),
                capabilities,
                candidate_report_total_count: 0,
                candidate_reports: Vec::new(),
            },
            _dxgi_device_manager: dxgi_device_manager,
            uses_gpu_surfaces,
            gpu_surface_release_errors: GpuSurfaceReleaseErrorSink::default(),
        };
        let mut self_test = match (uses_gpu_surfaces, gpu_startup_surface) {
            (true, Some(surface)) => encoder.run_gpu_surface_startup_self_test(surface),
            (true, None) => H264EncoderSelfTestSnapshot {
                attempted: true,
                gpu_surface_input: true,
                failure_reason: Some(
                    "GPU-surface encoder candidate was not tested with a DXGI surface".to_string(),
                ),
                ..Default::default()
            },
            (false, _) => encoder.run_startup_self_test(),
        };
        self_test.baseline_profile_confirmed = output_profile_baseline_confirmed;
        if self_test.passed
            && !b_frame_configuration_confirmed(
                &encoder.diagnostics.capabilities.b_frames_disabled,
                output_profile_baseline_confirmed,
                self_test.b_slice_count,
            )
        {
            self_test.passed = false;
            self_test.failure_reason = Some(
                "B=0 was neither confirmed by CodecAPI nor guaranteed by the negotiated Baseline profile"
                    .to_string(),
            );
            candidate_report.failure_stage = Some("required_b_frame_guard".to_string());
        }
        encoder.diagnostics.self_test = self_test.clone();
        candidate_report.self_test = self_test.clone();
        if !self_test.passed {
            return Err(format!(
                "startup self-test failed after {} ms: {}",
                self_test.duration_ms,
                self_test
                    .failure_reason
                    .as_deref()
                    .unwrap_or("unspecified failure")
            ));
        }
        candidate_report.admitted = true;
        candidate_report.failure_stage = None;
        candidate_report.failure_reason = None;
        Ok(encoder)
    }

    fn run_gpu_surface_startup_self_test(
        &mut self,
        surface: &GpuNv12Surface,
    ) -> H264EncoderSelfTestSnapshot {
        let started = Instant::now();
        let deadline = started + H264_ENCODER_SELF_TEST_TIMEOUT;
        let width = self.input_width as usize;
        let height = self.input_height as usize;
        let frame_bytes = match width
            .checked_mul(height)
            .and_then(|luma| luma.checked_add(luma / 2))
        {
            Some(frame_bytes) if frame_bytes != 0 => frame_bytes,
            _ => {
                return H264EncoderSelfTestSnapshot {
                    attempted: true,
                    gpu_surface_input: true,
                    dynamic_pattern_input: true,
                    duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    failure_reason: Some(
                        "DXGI-surface encoder self-test NV12 dimensions overflow".to_string(),
                    ),
                    ..Default::default()
                };
            }
        };
        let mut nv12 = vec![0u8; frame_bytes];
        let mut encoded = Vec::new();
        let _ = self.request_next_keyframe();
        let minimum_observation_frames = h264_self_test_minimum_observation_frames(self.fps);
        let maximum_frames = usize::from(self.fps.max(1)).saturating_mul(2).clamp(4, 60);
        for frame_index in 0..maximum_frames {
            if Instant::now() >= deadline {
                break;
            }
            fill_h264_self_test_pattern(&mut nv12, width, height, frame_index as u32);
            let sample_time = i64::try_from(frame_index)
                .unwrap_or(i64::MAX)
                .saturating_mul(self.frame_duration_100ns);
            match self.encode_dynamic_dxgi_self_test_frame(
                surface,
                &nv12,
                sample_time,
                deadline.saturating_duration_since(Instant::now()),
            ) {
                Ok(mut outputs) => encoded.append(&mut outputs),
                Err(error) => {
                    let mut snapshot = inspect_encoded_h264_self_test_outputs(&encoded);
                    snapshot.gpu_surface_input = true;
                    snapshot.dynamic_pattern_input = true;
                    snapshot.duration_ms =
                        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                    snapshot.failure_reason = Some(format!(
                        "DXGI-surface encoder self-test failed during ProcessInput/ProcessOutput: {error}"
                    ));
                    return snapshot;
                }
            }
            let evidence = inspect_encoded_h264_self_test_outputs(&encoded);
            if evidence.failure_reason.is_none()
                && frame_index + 1 >= minimum_observation_frames
                && encoded.len() >= minimum_observation_frames
            {
                break;
            }
        }

        let mut snapshot = inspect_encoded_h264_self_test_outputs(&encoded);
        snapshot.gpu_surface_input = true;
        snapshot.dynamic_pattern_input = true;
        if snapshot.failure_reason.is_none() && encoded.len() < minimum_observation_frames {
            snapshot.failure_reason = Some(format!(
                "DXGI-surface encoder self-test produced only {} access units; at least {minimum_observation_frames} are required to cover the 0.5 second B-frame/reordering window",
                encoded.len()
            ));
        }
        if snapshot.failure_reason.is_none() && Instant::now() < deadline {
            match decode_h264_self_test_access_units(
                self.input_width,
                self.input_height,
                self.fps,
                &encoded,
                deadline,
            ) {
                Ok(frame_count) if frame_count != 0 => snapshot.decoder_frame_count = frame_count,
                Ok(_) => {
                    snapshot.failure_reason =
                        Some("independent H.264 decoder produced no DXGI-input frame".to_string())
                }
                Err(error) => snapshot.failure_reason = Some(error),
            }
        }
        if snapshot.failure_reason.is_none() && Instant::now() >= deadline {
            snapshot.failure_reason = Some(format!(
                "DXGI-surface encoder self-test exceeded the {} ms deadline",
                H264_ENCODER_SELF_TEST_TIMEOUT.as_millis()
            ));
        }
        if snapshot.failure_reason.is_none() {
            if let Err(error) = self.flush() {
                snapshot.failure_reason = Some(format!(
                    "DXGI-surface encoder self-test passed but live-pipeline reset failed: {error}"
                ));
            }
        }
        snapshot.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        snapshot.passed = snapshot.failure_reason.is_none() && snapshot.decoder_frame_count != 0;
        snapshot
    }

    fn encode_dynamic_dxgi_self_test_frame(
        &mut self,
        production_surface: &GpuNv12Surface,
        nv12: &[u8],
        sample_time_100ns: i64,
        async_event_timeout: Duration,
    ) -> Result<Vec<EncodedH264Output>, String> {
        use windows::core::Interface;
        use windows::Win32::Foundation::FALSE;
        use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_SUBRESOURCE_DATA};
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
        use windows::Win32::Media::MediaFoundation::MFCreateDXGISurfaceBuffer;

        let texture = production_surface.texture();
        let mut descriptor = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut descriptor) };
        if descriptor.Width != self.input_width
            || descriptor.Height != self.input_height
            || descriptor.Format != DXGI_FORMAT_NV12
        {
            return Err(format!(
                "production DXGI surface descriptor does not match encoder input ({}x{} {:?})",
                descriptor.Width, descriptor.Height, descriptor.Format
            ));
        }
        let expected_len = usize::try_from(descriptor.Width)
            .ok()
            .and_then(|width| {
                usize::try_from(descriptor.Height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|luma| luma.checked_add(luma / 2))
            .ok_or_else(|| "DXGI self-test surface dimensions overflow".to_string())?;
        if nv12.len() != expected_len {
            return Err(format!(
                "DXGI self-test pattern length mismatch: expected {expected_len}, received {}",
                nv12.len()
            ));
        }
        let device = unsafe { texture.GetDevice() }
            .map_err(|error| format!("DXGI self-test device lookup failed: {error}"))?;
        let initial_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: nv12.as_ptr().cast(),
            SysMemPitch: descriptor.Width,
            SysMemSlicePitch: nv12.len().min(u32::MAX as usize) as u32,
        };
        let mut dynamic_texture = None;
        unsafe {
            device.CreateTexture2D(&descriptor, Some(&initial_data), Some(&mut dynamic_texture))
        }
        .map_err(|error| format!("dynamic DXGI NV12 self-test texture creation failed: {error}"))?;
        let dynamic_texture = dynamic_texture.ok_or_else(|| {
            "dynamic DXGI NV12 self-test texture creation returned no texture".to_string()
        })?;
        // NVIDIA's encoder MFT can accept an initialized DXGI texture yet fail
        // to observe its upload unless the producing device is flushed first.
        // This is startup-only, so the synchronization does not affect the live
        // zero-copy frame path.
        let context = unsafe { device.GetImmediateContext() }
            .map_err(|error| format!("dynamic DXGI self-test context lookup failed: {error}"))?;
        unsafe { context.Flush() };
        let input_buffer =
            unsafe { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &dynamic_texture, 0, FALSE) }
                .map_err(|error| format!("dynamic DXGI self-test surface wrap failed: {error}"))?;
        let buffer_length = unsafe { input_buffer.GetMaxLength() }
            .map_err(|error| format!("dynamic DXGI self-test buffer length failed: {error}"))?;
        unsafe { input_buffer.SetCurrentLength(buffer_length) }.map_err(|error| {
            format!("dynamic DXGI self-test current buffer length failed: {error}")
        })?;
        self.encode_buffer_with_async_timeout(input_buffer, sample_time_100ns, async_event_timeout)
    }

    fn run_startup_self_test(&mut self) -> H264EncoderSelfTestSnapshot {
        let started = Instant::now();
        let deadline = started + H264_ENCODER_SELF_TEST_TIMEOUT;
        let width = self.input_width as usize;
        let height = self.input_height as usize;
        let frame_bytes = match width
            .checked_mul(height)
            .and_then(|luma| luma.checked_add(luma / 2))
        {
            Some(frame_bytes) if frame_bytes != 0 => frame_bytes,
            _ => {
                return H264EncoderSelfTestSnapshot {
                    attempted: true,
                    dynamic_pattern_input: true,
                    duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    failure_reason: Some("encoder self-test NV12 dimensions overflow".to_string()),
                    ..Default::default()
                };
            }
        };
        let mut nv12 = vec![0u8; frame_bytes];
        let mut encoded = Vec::new();
        let _ = self.request_next_keyframe();
        let minimum_observation_frames = h264_self_test_minimum_observation_frames(self.fps);
        let maximum_frames = usize::from(self.fps.max(1)).saturating_mul(2).clamp(4, 60);
        for frame_index in 0..maximum_frames {
            if Instant::now() >= deadline {
                break;
            }
            fill_h264_self_test_pattern(&mut nv12, width, height, frame_index as u32);
            let sample_time = i64::try_from(frame_index)
                .unwrap_or(i64::MAX)
                .saturating_mul(self.frame_duration_100ns);
            match self.encode(&nv12, sample_time) {
                Ok(mut outputs) => encoded.append(&mut outputs),
                Err(error) => {
                    let mut snapshot = inspect_encoded_h264_self_test_outputs(&encoded);
                    snapshot.dynamic_pattern_input = true;
                    snapshot.duration_ms =
                        started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                    snapshot.failure_reason =
                        Some(format!("encoder self-test encode failed: {error}"));
                    return snapshot;
                }
            }
            let evidence = inspect_encoded_h264_self_test_outputs(&encoded);
            if evidence.failure_reason.is_none()
                && frame_index + 1 >= minimum_observation_frames
                && encoded.len() >= minimum_observation_frames
            {
                break;
            }
        }

        let mut snapshot = inspect_encoded_h264_self_test_outputs(&encoded);
        snapshot.dynamic_pattern_input = true;
        if snapshot.failure_reason.is_none() && encoded.len() < minimum_observation_frames {
            snapshot.failure_reason = Some(format!(
                "encoder self-test produced only {} access units; at least {minimum_observation_frames} are required to cover the 0.5 second B-frame/reordering window",
                encoded.len()
            ));
        }
        if snapshot.failure_reason.is_none() && Instant::now() < deadline {
            match decode_h264_self_test_access_units(
                self.input_width,
                self.input_height,
                self.fps,
                &encoded,
                deadline,
            ) {
                Ok(frame_count) if frame_count != 0 => snapshot.decoder_frame_count = frame_count,
                Ok(_) => {
                    snapshot.failure_reason =
                        Some("independent H.264 decoder produced no frame".to_string())
                }
                Err(error) => snapshot.failure_reason = Some(error),
            }
        }
        if snapshot.failure_reason.is_none() && Instant::now() >= deadline {
            snapshot.failure_reason = Some(format!(
                "encoder self-test exceeded the {} ms deadline",
                H264_ENCODER_SELF_TEST_TIMEOUT.as_millis()
            ));
        }
        if snapshot.failure_reason.is_none() {
            if let Err(error) = self.flush() {
                snapshot.failure_reason = Some(format!(
                    "encoder self-test passed but live-pipeline reset failed: {error}"
                ));
            }
        }
        snapshot.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        snapshot.passed = snapshot.failure_reason.is_none() && snapshot.decoder_frame_count != 0;
        snapshot
    }

    fn encode(
        &mut self,
        nv12: &[u8],
        sample_time_100ns: i64,
    ) -> Result<Vec<EncodedH264Output>, String> {
        use windows::Win32::Media::MediaFoundation::*;

        if sample_time_100ns >= self.next_keyframe_time_100ns {
            let _ = self.request_next_keyframe();
            self.next_keyframe_time_100ns =
                sample_time_100ns.saturating_add(H264_KEYFRAME_INTERVAL_100NS);
        }

        let input_buffer = unsafe { MFCreateMemoryBuffer(nv12.len() as u32) }
            .map_err(|error| format!("H.264 input buffer creation failed: {error}"))?;
        let mut input_ptr = std::ptr::null_mut();
        unsafe {
            input_buffer
                .Lock(&mut input_ptr, None, None)
                .map_err(|error| format!("H.264 input buffer lock failed: {error}"))?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), input_ptr, nv12.len());
            input_buffer
                .Unlock()
                .map_err(|error| format!("H.264 input buffer unlock failed: {error}"))?;
            input_buffer
                .SetCurrentLength(nv12.len() as u32)
                .map_err(|error| format!("H.264 input buffer length failed: {error}"))?;
        }
        self.encode_buffer(input_buffer, sample_time_100ns)
    }

    fn encode_surface(
        &mut self,
        surface: GpuNv12Surface,
        sample_time_100ns: i64,
    ) -> Result<Vec<EncodedH264Output>, String> {
        use windows::core::Interface;
        use windows::Win32::Media::MediaFoundation::{IMFSample, MFCreateTrackedSample};

        if !self.uses_gpu_surfaces {
            let _ = surface.release_after_encoder_done();
            return Err("selected H.264 encoder was not configured for DXGI surfaces".to_string());
        }
        if let Some(error) = self.gpu_surface_release_errors.take() {
            let _ = surface.release_after_encoder_done();
            return Err(format!(
                "a previous IMFTrackedSample failed to recycle its GPU surface: {error}"
            ));
        }
        if sample_time_100ns >= self.next_keyframe_time_100ns {
            let _ = self.request_next_keyframe();
            self.next_keyframe_time_100ns =
                sample_time_100ns.saturating_add(H264_KEYFRAME_INTERVAL_100NS);
        }
        let input_buffer = match surface.create_mf_surface_buffer() {
            Ok(buffer) => buffer,
            Err(error) => {
                let _ = surface.release_after_encoder_done();
                return Err(format!("H.264 DXGI surface wrap failed: {error}"));
            }
        };
        let tracked_sample = match unsafe { MFCreateTrackedSample() } {
            Ok(sample) => sample,
            Err(error) => {
                let _ = surface.release_after_encoder_done();
                return Err(format!(
                    "H.264 tracked input sample creation failed: {error}"
                ));
            }
        };
        let input_sample: IMFSample = match tracked_sample.cast() {
            Ok(sample) => sample,
            Err(error) => {
                let _ = surface.release_after_encoder_done();
                return Err(format!("H.264 tracked input sample cast failed: {error}"));
            }
        };
        if let Err(error) = unsafe { input_sample.AddBuffer(&input_buffer) } {
            let _ = surface.release_after_encoder_done();
            return Err(format!("H.264 tracked input sample buffer failed: {error}"));
        }

        // The callback owns the pool lease after SetAllocator succeeds. The MFT
        // may retain a sample beyond ProcessOutput (for example as a reference
        // frame), so output PTS must never be used as a recycle notification.
        let holder = Arc::new(GpuSurfaceLeaseHolder::new(
            surface,
            self.gpu_surface_release_errors.clone(),
        ));
        let callback: IMFAsyncCallback = GpuSurfaceReleaseCallback {
            holder: Arc::clone(&holder),
        }
        .into();
        if let Err(error) = unsafe { tracked_sample.SetAllocator(&callback, None) } {
            holder.release("IMFTrackedSample::SetAllocator failure");
            return Err(format!(
                "H.264 tracked input sample allocator callback failed: {error}"
            ));
        }
        drop(callback);
        drop(holder);
        drop(tracked_sample);

        let result = self.encode_sample(input_sample, sample_time_100ns);
        if let Some(error) = self.gpu_surface_release_errors.take() {
            return Err(format!(
                "IMFTrackedSample failed to recycle its GPU surface: {error}"
            ));
        }
        result
    }

    fn encode_buffer(
        &mut self,
        input_buffer: windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
        sample_time_100ns: i64,
    ) -> Result<Vec<EncodedH264Output>, String> {
        self.encode_buffer_with_async_timeout(
            input_buffer,
            sample_time_100ns,
            Duration::from_millis(250),
        )
    }

    fn encode_buffer_with_async_timeout(
        &mut self,
        input_buffer: windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
        sample_time_100ns: i64,
        async_event_timeout: Duration,
    ) -> Result<Vec<EncodedH264Output>, String> {
        use windows::Win32::Media::MediaFoundation::*;
        let input_sample = unsafe { MFCreateSample() }
            .map_err(|error| format!("H.264 input sample creation failed: {error}"))?;
        unsafe { win_result(input_sample.AddBuffer(&input_buffer))? };
        self.encode_sample_with_async_timeout(input_sample, sample_time_100ns, async_event_timeout)
    }

    fn encode_sample(
        &mut self,
        input_sample: windows::Win32::Media::MediaFoundation::IMFSample,
        sample_time_100ns: i64,
    ) -> Result<Vec<EncodedH264Output>, String> {
        self.encode_sample_with_async_timeout(
            input_sample,
            sample_time_100ns,
            Duration::from_millis(250),
        )
    }

    fn encode_sample_with_async_timeout(
        &mut self,
        input_sample: windows::Win32::Media::MediaFoundation::IMFSample,
        sample_time_100ns: i64,
        async_event_timeout: Duration,
    ) -> Result<Vec<EncodedH264Output>, String> {
        if let Some(adapter) = self.async_adapter.as_mut() {
            adapter.wait_for_input_credit(async_event_timeout)?;
        }
        unsafe {
            win_result(input_sample.SetSampleTime(sample_time_100ns))?;
            win_result(input_sample.SetSampleDuration(self.frame_duration_100ns))?;
            win_result(self.transform.ProcessInput(0, &input_sample, 0))?;
        }
        let mut outputs = Vec::new();
        if let Some(adapter) = self.async_adapter.as_mut() {
            adapter.wait_for_output_or_next_input(async_event_timeout)?;
            while self
                .async_adapter
                .as_mut()
                .is_some_and(WindowsAsyncMftAdapter::take_output_credit)
            {
                match self.process_one_output(sample_time_100ns)? {
                    H264OutputOutcome::Produced(output) => outputs.push(output),
                    // 格式变更消耗了这次事件，等下一个 HaveOutput 再取。
                    H264OutputOutcome::Renegotiated => {}
                    H264OutputOutcome::NeedMoreInput => {
                        return Err(
                            "async H.264 MFT signaled HaveOutput but ProcessOutput requested more input"
                                .to_string(),
                        )
                    }
                }
                if let Some(adapter) = self.async_adapter.as_mut() {
                    adapter.poll_available()?;
                }
            }
        } else {
            loop {
                match self.process_one_output(sample_time_100ns)? {
                    H264OutputOutcome::Produced(output) => outputs.push(output),
                    // 同步 MFT 在协商后已就地重试，这里不会出现；保留分支只为穷尽。
                    H264OutputOutcome::Renegotiated => {}
                    H264OutputOutcome::NeedMoreInput => break,
                }
            }
        }
        Ok(outputs)
    }

    /// `MF_E_TRANSFORM_STREAM_CHANGE` 是 Media Foundation 的正常协议事件，不是
    /// 失败：MFT 要求重新协商输出类型后才继续产出。Intel Quick Sync 等硬件编码器
    /// 在起始若干帧后经常触发它，早期实现把它当硬错误，导致这些编码器被整体否掉
    /// 并一路回退到 MJPEG。
    ///
    /// 协商后必须**立即重试** `ProcessOutput`（MSDN 对该事件的规定处理），并且
    /// 重试前必须已经刷新输出流信息。这两点缺一不可，实测可证：
    /// 只重试不刷新会得到 `E_UNEXPECTED`（用旧的缓冲大小/样本归属调用）；
    /// 只刷新不重试则会等事件超时——格式变更已经消耗掉本次 `METransformHaveOutput`，
    /// 异步 MFT 不会再为同一帧补发事件。
    ///
    /// 重试若返回"需要更多输入"，说明该 MFT 要求先补一帧再出码流，这不是协议
    /// 违规，按 `Renegotiated` 返回让调用方继续等下一个事件即可。
    fn process_one_output(
        &mut self,
        fallback_sample_time_100ns: i64,
    ) -> Result<H264OutputOutcome, String> {
        match self.try_process_one_output(fallback_sample_time_100ns) {
            Ok(Some(output)) => Ok(H264OutputOutcome::Produced(output)),
            Ok(None) => Ok(H264OutputOutcome::NeedMoreInput),
            Err(H264ProcessOutputError::Failed(message)) => Err(message),
            Err(H264ProcessOutputError::StreamChange) => {
                self.renegotiate_output_type()?;
                match self.try_process_one_output(fallback_sample_time_100ns) {
                    Ok(Some(output)) => Ok(H264OutputOutcome::Produced(output)),
                    Ok(None) => Ok(H264OutputOutcome::Renegotiated),
                    Err(H264ProcessOutputError::Failed(message)) => Err(message),
                    Err(H264ProcessOutputError::StreamChange) => Err(
                        "H.264 encoder requested another output type change immediately after renegotiation"
                            .to_string(),
                    ),
                }
            }
        }
    }

    /// 按 MFT 协议重新协商输出类型。两个必须做的动作：
    /// 1. 重新读取 `GetOutputStreamInfo`——新类型可能改变输出缓冲大小，也可能改变
    ///    由谁分配样本；沿用旧值会让随后的 `ProcessOutput` 收到无效参数。
    /// 2. 重新校验 §2.2 的时间线前提：当前 muxer 假设 DTS = PTS，因此只有仍是
    ///    Baseline profile、或 B=0 属性已确认可写时才允许继续使用该编码器。
    fn renegotiate_output_type(&mut self) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::{
            eAVEncH264VProfile_Base, MFVideoFormat_H264, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
            MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE,
        };

        // 上限只是防御一个不断返回类型的异常 MFT，不是协议要求。
        for index in 0..32u32 {
            let candidate = unsafe { self.transform.GetOutputAvailableType(0, index) }
                .map_err(|error| {
                    format!(
                        "H.264 encoder requested an output type change but exposed no usable type: {error}"
                    )
                })?;
            let is_h264 = unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) }
                .is_ok_and(|subtype| subtype == MFVideoFormat_H264);
            if !is_h264 {
                continue;
            }
            unsafe { win_result(self.transform.SetOutputType(0, &candidate, 0))? };
            let baseline_profile = unsafe { self.transform.GetOutputCurrentType(0) }
                .ok()
                .and_then(|current| unsafe { current.GetUINT32(&MF_MT_MPEG2_PROFILE) }.ok())
                .is_some_and(|profile| profile == eAVEncH264VProfile_Base.0 as u32);
            // 复用 §2.2 的唯一判定实现：自检已确认无 B-slice，因此这里传 0。
            if !b_frame_configuration_confirmed(
                &self.diagnostics.capabilities.b_frames_disabled,
                baseline_profile,
                0,
            ) {
                return Err(
                    "H.264 encoder renegotiated to an output type without a confirmed B-frame guarantee"
                        .to_string(),
                );
            }
            // 新输出类型可能改变缓冲大小与样本归属，必须重新读取后再取输出。
            let output_info =
                unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(|error| {
                    format!("H.264 output stream info after renegotiation failed: {error}")
                })?;
            self.output_size = output_info
                .cbSize
                .max(self.input_width.saturating_mul(self.input_height));
            self.output_provides_samples =
                output_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
            return Ok(());
        }
        Err(
            "H.264 encoder exposed no H.264 output type after requesting a stream change"
                .to_string(),
        )
    }

    fn try_process_one_output(
        &self,
        fallback_sample_time_100ns: i64,
    ) -> Result<Option<EncodedH264Output>, H264ProcessOutputError> {
        use std::mem::ManuallyDrop;
        use windows::Win32::Media::MediaFoundation::*;

        let requested_sample = if self.output_provides_samples {
            None
        } else {
            let output_buffer = unsafe { MFCreateMemoryBuffer(self.output_size) }
                .map_err(|error| format!("H.264 output buffer creation failed: {error}"))?;
            let output_sample = unsafe { MFCreateSample() }
                .map_err(|error| format!("H.264 output sample creation failed: {error}"))?;
            unsafe { output_sample.AddBuffer(&output_buffer) }
                .map_err(|error| format!("H.264 output sample buffer failed: {error}"))?;
            Some(output_sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(requested_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        let result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
        };
        let produced_sample = unsafe { ManuallyDrop::take(&mut output.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output.pEvents) };
        drop(events);
        match result {
            Ok(()) => {
                let sample = produced_sample
                    .ok_or_else(|| "H.264 encoder returned no output sample".to_string())?;
                let reported_sample_time = unsafe { sample.GetSampleTime() }.ok();
                let reported_sample_duration = unsafe { sample.GetSampleDuration() }.ok();
                let sample_time = reported_sample_time.unwrap_or(fallback_sample_time_100ns);
                let sample_duration = reported_sample_duration.unwrap_or(self.frame_duration_100ns);
                let buffer = unsafe { sample.ConvertToContiguousBuffer() }
                    .map_err(|error| format!("H.264 output coalesce failed: {error}"))?;
                let mut pointer = std::ptr::null_mut();
                let mut length = 0;
                unsafe { buffer.Lock(&mut pointer, None, Some(&mut length)) }
                    .map_err(|error| format!("H.264 output buffer lock failed: {error}"))?;
                let bytes =
                    unsafe { std::slice::from_raw_parts(pointer, length as usize) }.to_vec();
                unsafe { buffer.Unlock() }
                    .map_err(|error| format!("H.264 output buffer unlock failed: {error}"))?;
                Ok((!bytes.is_empty()).then_some(EncodedH264Output {
                    bytes,
                    sample_time_100ns: sample_time,
                    sample_duration_100ns: sample_duration,
                    sample_time_from_encoder: reported_sample_time.is_some(),
                    sample_duration_from_encoder: reported_sample_duration.is_some(),
                }))
            }
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                Err(H264ProcessOutputError::StreamChange)
            }
            Err(error) => Err(H264ProcessOutputError::Failed(format!(
                "H.264 ProcessOutput failed: {error}"
            ))),
        }
    }

    fn flush(&mut self) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
        };

        unsafe {
            win_result(self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0))?;
            win_result(
                self.transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
            )?;
        }
        if let Some(adapter) = self.async_adapter.as_mut() {
            adapter.reset_after_flush();
        }
        self.next_keyframe_time_100ns = H264_KEYFRAME_INTERVAL_100NS;
        Ok(())
    }

    fn request_next_keyframe(&self) -> Result<bool, String> {
        use windows::core::VARIANT;
        use windows::Win32::Media::MediaFoundation::CODECAPI_AVEncVideoForceKeyFrame;

        if !self.diagnostics.capabilities.force_keyframe.supported {
            return Ok(false);
        }
        let Some(api) = self.codec_api.as_ref() else {
            return Ok(false);
        };
        let force_keyframe = VARIANT::from(1u32);
        unsafe { api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &force_keyframe) }
            .map_err(|error| format!("Failed to request an H.264 keyframe: {error}"))?;
        Ok(true)
    }

    fn update_bitrate(&mut self, bitrate_bps: u32) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::CODECAPI_AVEncCommonMeanBitRate;

        if !self.diagnostics.capabilities.dynamic_bitrate.supported
            || !self.diagnostics.capabilities.dynamic_bitrate.modifiable
        {
            return Err("selected H.264 encoder did not negotiate dynamic bitrate updates".into());
        }
        let api = self
            .codec_api
            .as_ref()
            .ok_or_else(|| "selected H.264 encoder does not expose ICodecAPI".to_string())?;
        let result = negotiate_codec_u32(api, &CODECAPI_AVEncCommonMeanBitRate, bitrate_bps);
        self.diagnostics.capabilities.dynamic_bitrate = result.clone();
        if !result.value_matches {
            return Err(result
                .detail
                .unwrap_or_else(|| "dynamic bitrate readback did not match the request".into()));
        }
        self.bitrate_bps = bitrate_bps;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsH264Encoder {
    fn drop(&mut self) {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
        };
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_pipeline_metrics_expose_active_fallback_and_backpressure() {
        let state = H264MediaState::new();
        state.record_gpu_preprocess(Duration::from_micros(42));
        state.record_gpu_backpressure_drop();
        state.set_gpu_pipeline_active();
        let active = state.metrics();
        assert!(active.gpu_pipeline_active);
        assert_eq!(active.gpu_preprocess.sample_count, 1);
        assert_eq!(active.gpu_preprocess.p50, 42);
        assert_eq!(active.gpu_backpressure_dropped_frames, 1);
        assert_eq!(active.dropped_input_frames, 1);

        state.record_gpu_fallback("code=VideoProcessorBltFailed".to_string());
        let fallback = state.metrics();
        assert!(!fallback.gpu_pipeline_active);
        assert_eq!(fallback.gpu_fallback_count, 1);
        assert_eq!(
            fallback.gpu_fallback_reason.as_deref(),
            Some("code=VideoProcessorBltFailed")
        );
    }

    fn install_test_stream(state: &H264MediaState) -> u64 {
        state.install_stream(
            "avc1.42C01F".to_string(),
            1280,
            720,
            15,
            3_000_000,
            vec![1, 2, 3],
        )
    }

    fn test_segment(generation: u64, sequence: u64, keyframe: bool) -> H264MediaSegment {
        H264MediaSegment {
            generation,
            sequence,
            keyframe,
            timestamp_us: sequence.saturating_mul(1_000),
            duration_us: 1_000,
            capture_sequence: sequence,
            captured_at_unix_ms: sequence,
            visible_input_sequence: None,
            input_applied_at_server_unix_ms: None,
            access_unit_avcc: Arc::new(Bytes::from(format!("au-{sequence}"))),
            bytes: Arc::new(Bytes::from(format!("segment-{sequence}"))),
        }
    }

    #[test]
    fn annex_b_parser_extracts_parameter_sets_and_keyframe() {
        let bytes = [
            0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f, 0xaa, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 0, 1,
            0x65, 0x88, 0x84,
        ];
        let parsed = parse_annex_b_access_unit(&bytes).unwrap();
        assert!(parsed.keyframe);
        assert_eq!(
            parsed.sps.as_deref(),
            Some(&[0x67, 0x42, 0xc0, 0x1f, 0xaa][..])
        );
        assert_eq!(parsed.pps.as_deref(), Some(&[0x68, 0xce, 0x06, 0xe2][..]));
        assert_eq!(&parsed.avcc[..4], &5u32.to_be_bytes());
        assert_eq!(
            codec_from_sps(parsed.sps.as_ref().unwrap()).unwrap(),
            "avc1.42C01F"
        );
    }

    #[test]
    fn nv12_conversion_crops_odd_dimensions_and_preserves_plane_layout() {
        let pixels = vec![255u8; 3 * 3 * 4];
        let mut output = Vec::new();
        let dimensions = bgra_to_nv12(&pixels, 3, 3, 12, &mut output).unwrap();
        assert_eq!(dimensions, (2, 2));
        assert_eq!(output.len(), 6);
        assert!(output[..4].iter().all(|value| *value >= 230));
        assert_eq!(&output[4..], &[128, 128]);
    }

    fn assert_nv12_matches_scalar(pixels: &[u8], width: usize, height: usize, stride: usize) {
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        let expected_dimensions =
            bgra_to_nv12_scalar(pixels, width, height, stride, &mut expected).unwrap();
        let actual_dimensions = bgra_to_nv12(pixels, width, height, stride, &mut actual).unwrap();
        assert_eq!(actual_dimensions, expected_dimensions);
        assert_eq!(actual, expected);

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        if std::is_x86_feature_detected!("ssse3") {
            let mut direct_simd = Vec::new();
            prepare_nv12_output(
                &mut direct_simd,
                expected_dimensions.0,
                expected_dimensions.1,
            );
            // SAFETY: this branch performs the required runtime feature check;
            // the scalar conversion above already validated the frame layout.
            unsafe {
                bgra_to_nv12_ssse3_into(
                    pixels,
                    expected_dimensions.0,
                    expected_dimensions.1,
                    stride,
                    &mut direct_simd,
                );
            }
            assert_eq!(direct_simd, expected);
        }
    }

    #[test]
    fn nv12_simd_matches_scalar_for_constructed_pixels_and_stride() {
        let width = 8;
        let height = 4;
        let stride = width * 4 + 7;
        let colors = [
            [0, 0, 0, 0],
            [255, 255, 255, 255],
            [255, 0, 0, 17],
            [0, 255, 0, 34],
            [0, 0, 255, 51],
            [13, 127, 241, 68],
            [241, 127, 13, 85],
            [64, 192, 33, 102],
        ];
        let mut pixels = vec![0xa5; height * stride];
        for y in 0..height {
            for x in 0..width {
                let offset = y * stride + x * 4;
                pixels[offset..offset + 4].copy_from_slice(&colors[(x + y * 3) % colors.len()]);
            }
        }
        assert_nv12_matches_scalar(&pixels, width, height, stride);
    }

    #[test]
    fn nv12_simd_matches_scalar_for_cropping_boundaries_and_random_pixels() {
        let cases = [
            (2usize, 2usize, 0usize),
            (3, 3, 1),
            (4, 2, 7),
            (6, 4, 3),
            (17, 9, 16),
            (64, 34, 0),
            (65, 35, 31),
        ];
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for (width, height, padding) in cases {
            let stride = width * 4 + padding;
            let mut pixels = vec![0u8; height * stride];
            for byte in &mut pixels {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                *byte = (state >> 32) as u8;
            }
            assert_nv12_matches_scalar(&pixels, width, height, stride);
        }
    }

    #[test]
    fn nv12_conversion_rejects_invalid_boundaries() {
        let mut output = vec![1, 2, 3];
        assert!(bgra_to_nv12(&[0; 8], 1, 2, 4, &mut output).is_err());
        assert!(bgra_to_nv12(&[0; 8], 2, 1, 8, &mut output).is_err());
        assert!(bgra_to_nv12(&[0; 16], 2, 2, 7, &mut output).is_err());
        assert!(bgra_to_nv12(&[0; 15], 2, 2, 8, &mut output).is_err());
    }

    #[test]
    fn software_encoder_limits_preserve_aspect_and_cap_fps() {
        assert_eq!(software_encoder_limits(3840, 2160, 60), (1920, 1080, 30));
        assert_eq!(software_encoder_limits(2560, 1080, 30), (1920, 810, 30));
        assert_eq!(software_encoder_limits(2160, 3840, 60), (606, 1080, 30));
        assert_eq!(software_encoder_limits(1280, 720, 60), (1280, 720, 30));
        assert_eq!(software_encoder_limits(1280, 720, 15), (1280, 720, 15));
        // 1080p 是屏幕共享最常见的桌面尺寸，且内容以文字为主：不得无条件降采样，
        // 否则正文不可读。降级必须由实测编码耗时驱动，而不是写死在上限里。
        assert_eq!(software_encoder_limits(1920, 1080, 30), (1920, 1080, 30));
        assert_eq!(software_encoder_limits(1024, 768, 30), (1024, 768, 30));
    }

    #[test]
    fn software_scaler_resamples_real_bgra_pixels() {
        let mut source = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            for x in 0..4 {
                let offset = (y * 4 + x) * 4;
                source[offset..offset + 4].copy_from_slice(&[
                    x as u8,
                    y as u8,
                    (x + y * 4) as u8,
                    255,
                ]);
            }
        }
        let mut scaled = Vec::new();
        scale_bgra_nearest(&source, 4, 4, 16, 2, 2, &mut scaled).unwrap();
        assert_eq!(scaled.len(), 16);
        assert_eq!(&scaled[0..4], &[0, 0, 0, 255]);
        assert_eq!(&scaled[4..8], &[2, 0, 2, 255]);
        assert_eq!(&scaled[8..12], &[0, 2, 8, 255]);
        assert_eq!(&scaled[12..16], &[2, 2, 10, 255]);
    }

    #[test]
    fn fmp4_init_and_media_segments_have_required_boxes() {
        let sps = [0x67, 0x42, 0xc0, 0x1f, 0xaa];
        let pps = [0x68, 0xce, 0x06, 0xe2];
        let init = build_init_segment(1920, 1080, 15, &sps, &pps).unwrap();
        assert!(init.windows(4).any(|window| window == b"ftyp"));
        assert!(init.windows(4).any(|window| window == b"moov"));
        assert!(init.windows(4).any(|window| window == b"mvex"));
        assert!(init.windows(4).any(|window| window == b"avcC"));
        let decoder_configuration = extract_avcc_decoder_configuration(&init).unwrap();
        assert_eq!(decoder_configuration[0], 1);
        assert_eq!(&decoder_configuration[1..4], &sps[1..4]);
        assert!(decoder_configuration
            .windows(sps.len())
            .any(|item| item == sps));
        assert!(decoder_configuration
            .windows(pps.len())
            .any(|item| item == pps));

        let media =
            build_media_segment(7, 180_000, 6_000, true, &[0, 0, 0, 2, 0x65, 0x88]).unwrap();
        assert_eq!(&media[4..8], b"moof");
        assert!(media.windows(4).any(|window| window == b"tfdt"));
        assert!(media.windows(4).any(|window| window == b"trun"));
        assert!(media.windows(4).any(|window| window == b"mdat"));
        let tfdt = media
            .windows(4)
            .position(|window| window == b"tfdt")
            .unwrap();
        assert_eq!(
            u32::from_be_bytes(media[tfdt + 8..tfdt + 12].try_into().unwrap()),
            180_000
        );
    }

    #[test]
    fn media_state_keeps_latest_keyframe_gop() {
        let state = H264MediaState::new();
        let generation = install_test_stream(&state);
        state.publish_segment(H264MediaSegment {
            generation,
            sequence: 1,
            keyframe: true,
            timestamp_us: 1_000,
            duration_us: 1_000,
            capture_sequence: 1,
            captured_at_unix_ms: 1,
            visible_input_sequence: None,
            input_applied_at_server_unix_ms: None,
            access_unit_avcc: Arc::new(Bytes::from_static(b"key-1-au")),
            bytes: Arc::new(Bytes::from_static(b"key-1")),
        });
        state.publish_segment(H264MediaSegment {
            generation,
            sequence: 2,
            keyframe: false,
            timestamp_us: 2_000,
            duration_us: 1_000,
            capture_sequence: 2,
            captured_at_unix_ms: 2,
            visible_input_sequence: None,
            input_applied_at_server_unix_ms: None,
            access_unit_avcc: Arc::new(Bytes::from_static(b"delta-au")),
            bytes: Arc::new(Bytes::from_static(b"delta")),
        });
        state.publish_segment(H264MediaSegment {
            generation,
            sequence: 3,
            keyframe: true,
            timestamp_us: 3_000,
            duration_us: 1_000,
            capture_sequence: 3,
            captured_at_unix_ms: 3,
            visible_input_sequence: None,
            input_applied_at_server_unix_ms: None,
            access_unit_avcc: Arc::new(Bytes::from_static(b"key-2-au")),
            bytes: Arc::new(Bytes::from_static(b"key-2")),
        });
        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.segments.len(), 1);
        assert_eq!(snapshot.segments[0].sequence, 3);
        assert!(snapshot.segments[0].keyframe);
        assert_eq!(snapshot.segments[0].timestamp_us, 3_000);
        assert_eq!(snapshot.segments[0].duration_us, 1_000);
        assert_eq!(snapshot.segments[0].capture_sequence, 3);
        assert_eq!(snapshot.segments[0].captured_at_unix_ms, 3);
    }

    #[test]
    fn media_state_does_not_expose_delta_before_idr() {
        let state = H264MediaState::new();
        let generation = install_test_stream(&state);

        state.publish_segment(test_segment(generation, 1, false));
        assert!(state.snapshot().is_none());
        assert_eq!(state.descriptor().unwrap().generation, generation);

        state.publish_segment(test_segment(generation, 2, true));
        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.segments.len(), 1);
        assert_eq!(snapshot.segments[0].sequence, 2);
    }

    #[test]
    fn media_state_never_truncates_a_cached_dependency_chain() {
        let state = H264MediaState::new();
        let generation = install_test_stream(&state);
        state.publish_segment(test_segment(generation, 1, true));
        for sequence in 2..=H264_GOP_CACHE_LIMIT as u64 {
            state.publish_segment(test_segment(generation, sequence, false));
        }
        let full = state.snapshot().unwrap();
        assert_eq!(full.segments.len(), H264_GOP_CACHE_LIMIT);
        assert_eq!(full.segments.first().unwrap().sequence, 1);
        assert_eq!(full.segments.last().unwrap().sequence, 180);

        let mut events = state.subscribe();
        state.publish_segment(test_segment(generation, 181, false));
        assert!(state.snapshot().is_none());
        assert!(matches!(
            events.try_recv().unwrap().as_ref(),
            H264MediaEvent::Segment(segment) if segment.sequence == 181
        ));

        state.publish_segment(test_segment(generation, 182, false));
        assert!(state.snapshot().is_none());
        state.publish_segment(test_segment(generation, 183, true));
        let recovered = state.snapshot().unwrap();
        assert_eq!(recovered.segments.len(), 1);
        assert_eq!(recovered.segments[0].sequence, 183);
    }

    #[test]
    fn media_state_invalidates_cache_on_sequence_gap() {
        let state = H264MediaState::new();
        let generation = install_test_stream(&state);
        state.publish_segment(test_segment(generation, 10, true));
        state.publish_segment(test_segment(generation, 12, false));
        assert!(state.snapshot().is_none());

        state.publish_segment(test_segment(generation, 13, false));
        assert!(state.snapshot().is_none());
        state.publish_segment(test_segment(generation, 20, true));
        assert_eq!(state.snapshot().unwrap().segments[0].sequence, 20);
    }

    #[test]
    fn media_state_rejects_stale_generation_requests_and_segments() {
        let state = H264MediaState::new();
        let first_generation = install_test_stream(&state);
        let second_generation = install_test_stream(&state);
        assert!(second_generation > first_generation);
        assert_eq!(
            state.request_keyframe(first_generation),
            H264KeyframeRequestResult::StaleGeneration
        );
        assert_eq!(state.metrics().idr_request_stale_count, 1);

        state.publish_segment(test_segment(first_generation, 1, true));
        assert!(state.snapshot().is_none());
        state.publish_segment(test_segment(second_generation, 1, true));
        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.descriptor.generation, second_generation);
        assert!(snapshot
            .segments
            .iter()
            .all(|segment| segment.generation == second_generation));
    }

    #[test]
    fn keyframe_gate_coalesces_and_rate_limits_requests() {
        let state = H264MediaState::new();
        let generation = install_test_stream(&state);
        let start = Instant::now();

        assert_eq!(
            state.request_keyframe_at(generation, start),
            H264KeyframeRequestResult::Scheduled
        );
        assert_eq!(
            state.request_keyframe_at(generation, start + Duration::from_millis(50)),
            H264KeyframeRequestResult::Coalesced
        );
        assert!(!state.take_keyframe_request_at(
            generation,
            start + H264_KEYFRAME_REQUEST_MERGE_WINDOW - Duration::from_millis(1)
        ));
        assert!(
            state.take_keyframe_request_at(generation, start + H264_KEYFRAME_REQUEST_MERGE_WINDOW)
        );
        assert!(
            !state.take_keyframe_request_at(generation, start + H264_KEYFRAME_REQUEST_MERGE_WINDOW)
        );

        assert_eq!(
            state.request_keyframe_at(generation, start + Duration::from_millis(250)),
            H264KeyframeRequestResult::Scheduled
        );
        assert_eq!(
            state.request_keyframe_at(generation, start + Duration::from_millis(300)),
            H264KeyframeRequestResult::Coalesced
        );
        assert!(!state.take_keyframe_request_at(generation, start + Duration::from_millis(699)));
        assert!(state.take_keyframe_request_at(generation, start + Duration::from_millis(700)));

        let metrics = state.metrics();
        assert_eq!(metrics.idr_request_scheduled_count, 2);
        assert_eq!(metrics.idr_request_coalesced_count, 2);
        assert_eq!(metrics.idr_request_rate_limited_count, 1);
        assert_eq!(metrics.idr_request_dispatch_count, 2);
    }

    #[test]
    fn target_bitrate_stays_within_lan_limits() {
        assert_eq!(target_bitrate_bps(320, 240, 5, 10), 1_200_000);
        assert_eq!(target_bitrate_bps(7680, 4320, 30, 100), 12_000_000);
        assert!((4_000_000..=7_000_000).contains(&target_bitrate_bps(1920, 1080, 15, 70)));
    }

    #[test]
    fn bounded_distribution_keeps_recent_samples_and_total_count() {
        let samples = H264BoundedSamples::default();
        for value in 0..(H264_METRIC_SAMPLE_LIMIT as u64 + 10) {
            samples.record(value);
        }
        let snapshot = samples.snapshot();
        assert_eq!(snapshot.sample_count, H264_METRIC_SAMPLE_LIMIT as u64 + 10);
        assert_eq!(snapshot.max, H264_METRIC_SAMPLE_LIMIT as u64 + 9);
        assert!(snapshot.p50 >= 10);
        assert!(snapshot.p50 <= snapshot.p95);
        assert!(snapshot.p95 <= snapshot.p99);
    }

    #[test]
    fn capability_degradation_distinguishes_confirmed_and_failed_features() {
        let confirmed = H264EncoderCapabilitySnapshot {
            supported: true,
            modifiable: true,
            set_succeeded: true,
            readback_succeeded: true,
            value_matches: true,
            detail: None,
            ..Default::default()
        };
        assert!(capability_degradation_reason("low_latency", &confirmed).is_none());

        let rejected = H264EncoderCapabilitySnapshot {
            supported: true,
            detail: Some("SetValue failed".to_string()),
            ..Default::default()
        };
        assert_eq!(
            capability_degradation_reason("dynamic_bitrate", &rejected).as_deref(),
            Some("dynamic_bitrate: SetValue failed")
        );

        assert!(b_frame_configuration_confirmed(&confirmed, false, 0));
        assert!(b_frame_configuration_confirmed(&rejected, true, 0));
        assert!(!b_frame_configuration_confirmed(&rejected, false, 0));
        assert!(!b_frame_configuration_confirmed(&confirmed, true, 1));
    }

    #[test]
    fn encoder_fallback_summary_is_bounded_and_classifies_empty_input() {
        assert_eq!(bounded_encoder_failure_summary(&[]), None);
        let failures = (0..8)
            .map(|index| format!("hardware candidate {index} failed"))
            .collect::<Vec<_>>();
        let summary = bounded_encoder_failure_summary(&failures).unwrap();
        assert!(summary.contains("hardware candidate 0 failed"));
        assert!(summary.contains("and 4 more"));
        assert!(summary.len() <= 1_024);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn encoder_candidate_reports_are_bounded_and_preserve_the_admitted_candidate() {
        let mut reports = Vec::new();
        let mut total_count = 0;
        for index in 0..(H264_ENCODER_CANDIDATE_REPORT_LIMIT + 2) {
            retain_encoder_candidate_report(
                &mut reports,
                &mut total_count,
                H264EncoderCandidateReport {
                    name: format!("candidate-{index}"),
                    admitted: index == H264_ENCODER_CANDIDATE_REPORT_LIMIT + 1,
                    ..Default::default()
                },
            );
        }
        assert_eq!(
            total_count as usize,
            H264_ENCODER_CANDIDATE_REPORT_LIMIT + 2
        );
        assert_eq!(reports.len(), H264_ENCODER_CANDIDATE_REPORT_LIMIT);
        assert!(reports.last().is_some_and(|report| report.admitted));
        assert_eq!(
            reports.last().map(|report| report.name.as_str()),
            Some("candidate-17")
        );
    }

    #[test]
    fn async_mft_state_requires_and_bounds_event_credits() {
        let mut state = H264AsyncMftState::default();
        assert!(!state.take_input_credit());
        state.observe(H264AsyncMftEvent::NeedInput).unwrap();
        state.observe(H264AsyncMftEvent::HaveOutput).unwrap();
        assert!(state.take_input_credit());
        assert!(state.take_output_credit());
        assert!(!state.take_output_credit());

        for _ in 0..H264_ASYNC_EVENT_CREDIT_LIMIT {
            state.observe(H264AsyncMftEvent::NeedInput).unwrap();
        }
        assert!(state.observe(H264AsyncMftEvent::NeedInput).is_err());
        assert!(state.observe(H264AsyncMftEvent::DrainComplete).is_err());
        state.begin_drain();
        state.observe(H264AsyncMftEvent::DrainComplete).unwrap();
        assert!(state.drain_complete);
        state.reset_after_flush();
        assert_eq!(state, H264AsyncMftState::default());
    }

    #[test]
    fn h264_self_test_inspection_rejects_b_slices_and_bad_timeline() {
        // first_mb_in_slice=0 (1), slice_type=1/B (010) => 1010_0000.
        let parameter_and_idr = [
            0, 0, 0, 1, 0x67, 0x42, 0, 0x1f, 0, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 0, 1, 0x65,
            0x80,
        ];
        let b_slice = [0, 0, 0, 1, 0x41, 0xa0];
        assert_eq!(h264_slice_type(&b_slice[4..]), Some(1));
        let snapshot = inspect_h264_self_test_access_units(&[
            H264SelfTestAccessUnit {
                bytes: &parameter_and_idr,
                sample_time_100ns: 10,
                sample_duration_100ns: 10,
            },
            H264SelfTestAccessUnit {
                bytes: &b_slice,
                sample_time_100ns: 10,
                sample_duration_100ns: 0,
            },
        ]);
        assert!(snapshot.found_sps);
        assert!(snapshot.found_pps);
        assert!(snapshot.found_idr);
        assert_eq!(snapshot.b_slice_count, 1);
        assert!(!snapshot.timeline_monotonic);
        assert!(snapshot.failure_reason.unwrap().contains("timestamps"));
    }

    #[test]
    fn h264_self_test_inspection_accepts_idr_evidence_without_reordering() {
        let access_unit = [
            0, 0, 1, 0x67, 0x42, 0, 0x1f, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 1, 0x65, 0x80,
        ];
        let snapshot = inspect_h264_self_test_access_units(&[H264SelfTestAccessUnit {
            bytes: &access_unit,
            sample_time_100ns: 0,
            sample_duration_100ns: 333_333,
        }]);
        assert_eq!(snapshot.failure_reason, None);
        assert!(snapshot.timeline_monotonic);
        assert!(snapshot.timestamps_from_encoder);
        assert!(snapshot.durations_from_encoder);
        assert_eq!(snapshot.b_slice_count, 0);
    }

    #[test]
    fn h264_self_test_observation_window_covers_at_least_half_a_second() {
        assert_eq!(h264_self_test_minimum_observation_frames(1), 4);
        assert_eq!(h264_self_test_minimum_observation_frames(30), 15);
        assert_eq!(h264_self_test_minimum_observation_frames(60), 30);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn h264_self_test_rejects_synthesized_output_timing() {
        let access_unit = vec![
            0, 0, 1, 0x67, 0x42, 0, 0x1f, 0, 0, 1, 0x68, 0xce, 0x06, 0xe2, 0, 0, 1, 0x65, 0x80,
        ];
        let snapshot = inspect_encoded_h264_self_test_outputs(&[EncodedH264Output {
            bytes: access_unit,
            sample_time_100ns: 0,
            sample_duration_100ns: 333_333,
            sample_time_from_encoder: false,
            sample_duration_from_encoder: false,
        }]);
        assert!(!snapshot.timestamps_from_encoder);
        assert!(!snapshot.durations_from_encoder);
        assert!(snapshot
            .failure_reason
            .as_deref()
            .is_some_and(|error| error.contains("omitted a sample timestamp")));
    }

    #[test]
    fn h264_self_test_pattern_is_dynamic_and_non_black() {
        let mut first = vec![0u8; 16 * 16 * 3 / 2];
        let mut second = first.clone();
        fill_h264_self_test_pattern(&mut first, 16, 16, 0);
        fill_h264_self_test_pattern(&mut second, 16, 16, 1);
        assert_ne!(first, second);
        assert!(first[..16 * 16].iter().any(|value| *value > 16));
        assert!(first[16 * 16..].iter().any(|value| *value != 128));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn media_foundation_adapter_luid_blob_round_trips_and_matches_exactly() {
        use std::mem::size_of;

        use windows::Win32::Foundation::LUID;
        use windows::Win32::Media::MediaFoundation::{MFCreateAttributes, MFT_ENUM_ADAPTER_LUID};

        let mut attributes = None;
        unsafe { MFCreateAttributes(&mut attributes, 1) }.unwrap();
        let attributes = attributes.expect("MFCreateAttributes returned no attribute store");
        let expected = LUID {
            LowPart: 0x89AB_CDEF,
            HighPart: 0x1234_5678,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts((&expected as *const LUID).cast::<u8>(), size_of::<LUID>())
        };
        unsafe { attributes.SetBlob(&MFT_ENUM_ADAPTER_LUID, bytes) }.unwrap();

        let parsed = media_foundation_attribute_luid(&attributes, &MFT_ENUM_ADAPTER_LUID)
            .expect("valid MFT adapter LUID blob should parse");
        assert!(adapter_luids_match(&parsed, &expected));
        assert_eq!(format_adapter_luid(&parsed), "0x12345678:89ABCDEF");

        unsafe { attributes.SetBlob(&MFT_ENUM_ADAPTER_LUID, &[1, 2, 3, 4]) }.unwrap();
        assert!(media_foundation_attribute_luid(&attributes, &MFT_ENUM_ADAPTER_LUID).is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires Windows Media Foundation H.264 encoder and decoder components"]
    fn windows_media_foundation_encoder_passes_startup_self_test() {
        let _ = env_logger::builder().is_test(true).try_init();
        use windows::core::HRESULT;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

        const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
        let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = if com_result.is_ok() {
            true
        } else if com_result == RPC_E_CHANGED_MODE {
            false
        } else {
            panic!("CoInitializeEx failed: {com_result:?}");
        };
        let _runtime = MediaFoundationRuntime::startup(com_initialized).unwrap();
        let encoder = WindowsH264Encoder::new(320, 240, 15, 1_200_000, true, None).unwrap();
        eprintln!(
            "screen-share system-memory MF integration encoder: name={:?}, hardware={}, async={}, adapter_luid={:?}, driver_version={:?}, candidate_reports={:#?}",
            encoder.diagnostics.name,
            encoder.diagnostics.hardware,
            encoder.diagnostics.async_mode,
            encoder.diagnostics.adapter_luid,
            encoder.diagnostics.driver_version,
            encoder.diagnostics.candidate_reports,
        );
        assert!(encoder.diagnostics.self_test.passed);
        assert!(encoder.diagnostics.self_test.decoder_frame_count > 0);
        assert!(!encoder.diagnostics.self_test.gpu_surface_input);
        assert!(encoder.diagnostics.self_test.dynamic_pattern_input);
        assert!(!encoder
            .diagnostics
            .capabilities
            .rate_control_attempts
            .is_empty());
        assert!(encoder
            .diagnostics
            .capabilities
            .rate_control
            .requested_value
            .is_some());
        assert!(b_frame_configuration_confirmed(
            &encoder.diagnostics.capabilities.b_frames_disabled,
            encoder.diagnostics.self_test.baseline_profile_confirmed,
            encoder.diagnostics.self_test.b_slice_count,
        ));
        assert!(encoder.diagnostics.candidate_report_total_count > 0);
        assert!(encoder
            .diagnostics
            .candidate_reports
            .iter()
            .any(|report| report.admitted));
        if encoder.diagnostics.hardware {
            assert!(encoder.diagnostics.async_mode);
        }
    }

    /// 软件 H.264 MFT 是降级链的最后一级，硬件候选被拒时全靠它出图。上面那个
    /// 门禁在有硬件编码器的机器上会直接采纳硬件候选，永远走不到这一级，因此
    /// 单独锁定：曾经在 `NOTIFY_BEGIN_STREAMING` 之前发 `COMMAND_FLUSH`，
    /// Microsoft 的软件编码器对此返回 E_FAIL，使这一级也失败并一路掉到 MJPEG。
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires Windows Media Foundation software H.264 encoder and decoder components"]
    fn windows_software_h264_encoder_passes_startup_self_test() {
        let _ = env_logger::builder().is_test(true).try_init();
        use windows::core::HRESULT;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

        const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
        let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = if com_result.is_ok() {
            true
        } else if com_result == RPC_E_CHANGED_MODE {
            false
        } else {
            panic!("CoInitializeEx failed: {com_result:?}");
        };
        let _runtime = MediaFoundationRuntime::startup(com_initialized).unwrap();
        // hardware_allowed=false 强制走 enumerate_h264_encoder_candidates(false)。
        let encoder = WindowsH264Encoder::new(320, 240, 15, 1_200_000, false, None)
            .expect("software H.264 encoder must remain usable as the last fallback");
        eprintln!(
            "screen-share software MF encoder: name={:?}, hardware={}, self_test={:#?}",
            encoder.diagnostics.name, encoder.diagnostics.hardware, encoder.diagnostics.self_test,
        );
        assert!(!encoder.diagnostics.hardware);
        assert!(encoder.diagnostics.self_test.passed);
        assert!(encoder.diagnostics.self_test.found_sps);
        assert!(encoder.diagnostics.self_test.found_pps);
        assert!(encoder.diagnostics.self_test.found_idr);
        assert!(encoder.diagnostics.self_test.decoder_frame_count > 0);
        assert!(encoder.diagnostics.self_test.timeline_monotonic);
        assert!(b_frame_configuration_confirmed(
            &encoder.diagnostics.capabilities.b_frames_disabled,
            encoder.diagnostics.self_test.baseline_profile_confirmed,
            encoder.diagnostics.self_test.b_slice_count,
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires a D3D11 video processor plus Windows hardware H.264 encoder and decoder components"]
    fn windows_gpu_preprocess_and_mf_dxgi_encoder_passes_integration_self_test() {
        let _ = env_logger::builder().is_test(true).try_init();
        use crate::screenshare_gpu::{GpuPreprocessConfig, GpuVideoPreprocessor, SurfacePhase};
        use windows::core::{Interface, HRESULT};
        use windows::Win32::Foundation::HMODULE;
        use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
        use windows::Win32::Graphics::Direct3D11::{
            D3D11CreateDevice, ID3D11Multithread, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
            D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        };
        use windows::Win32::Graphics::Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
        };
        use windows::Win32::Graphics::Dxgi::IDXGIDevice;
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

        const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
        const WIDTH: u32 = 1280;
        const HEIGHT: u32 = 720;
        const FPS: u8 = 15;

        let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = if com_result.is_ok() {
            true
        } else if com_result == RPC_E_CHANGED_MODE {
            false
        } else {
            panic!("CoInitializeEx failed: {com_result:?}");
        };
        let _runtime = MediaFoundationRuntime::startup(com_initialized).unwrap();

        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .unwrap();
        let device = device.expect("D3D11CreateDevice returned no device");
        let context = context.expect("D3D11CreateDevice returned no immediate context");
        let dxgi_device: IDXGIDevice = device.cast().expect("D3D11 device is not a DXGI device");
        let adapter = unsafe { dxgi_device.GetAdapter() }.expect("DXGI device has no adapter");
        let adapter_descriptor = unsafe { adapter.GetDesc() }.expect("DXGI adapter has no desc");
        let adapter_name = String::from_utf16_lossy(
            &adapter_descriptor.Description[..adapter_descriptor
                .Description
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(adapter_descriptor.Description.len())],
        );
        eprintln!("screen-share GPU integration adapter: {adapter_name}");
        let multithread: ID3D11Multithread = context
            .cast()
            .expect("D3D11 video context does not expose ID3D11Multithread");
        unsafe {
            let _ = multithread.SetMultithreadProtected(true);
        }

        let mut bgra = vec![0u8; WIDTH as usize * HEIGHT as usize * 4];
        for y in 0..HEIGHT as usize {
            for x in 0..WIDTH as usize {
                let offset = (y * WIDTH as usize + x) * 4;
                bgra[offset] = ((x * 3 + y) & 0xff) as u8;
                bgra[offset + 1] = ((x + y * 2) & 0xff) as u8;
                bgra[offset + 2] = ((x * 2 + y * 3) & 0xff) as u8;
                bgra[offset + 3] = 0xff;
            }
        }
        let descriptor = D3D11_TEXTURE2D_DESC {
            Width: WIDTH,
            Height: HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let initial_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: bgra.as_ptr().cast(),
            SysMemPitch: WIDTH * 4,
            SysMemSlicePitch: bgra.len() as u32,
        };
        let mut source = None;
        unsafe { device.CreateTexture2D(&descriptor, Some(&initial_data), Some(&mut source)) }
            .unwrap();
        let source: ID3D11Texture2D = source.expect("CreateTexture2D returned no BGRA texture");

        let mut preprocessor = GpuVideoPreprocessor::new(
            device,
            context,
            GpuPreprocessConfig {
                input_width: WIDTH,
                input_height: HEIGHT,
                output_width: WIDTH,
                output_height: HEIGHT,
                frame_rate_numerator: u32::from(FPS),
                frame_rate_denominator: 1,
                generation: 1,
            },
        )
        .unwrap();
        let capabilities = preprocessor.capabilities();
        assert!(capabilities.bgra_input);
        assert!(capabilities.nv12_output);

        let surface = preprocessor.preprocess(&source).unwrap();
        assert_eq!(surface.width(), WIDTH);
        assert_eq!(surface.height(), HEIGHT);
        surface.create_mf_surface_buffer().unwrap();

        let mut encoder =
            WindowsH264Encoder::new_for_gpu_surface(&surface, WIDTH, HEIGHT, FPS, 4_000_000)
                .unwrap();
        assert!(encoder.diagnostics.hardware);
        assert!(encoder.diagnostics.self_test.passed);
        assert!(encoder.diagnostics.self_test.gpu_surface_input);
        assert!(encoder.diagnostics.self_test.decoder_frame_count > 0);

        let _ = encoder.encode_surface(surface, 0).unwrap();
        encoder.flush().unwrap();
        drop(encoder);

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = preprocessor.pool_snapshot().unwrap();
            if snapshot.iter().all(|slot| slot.phase == SurfacePhase::Free) {
                eprintln!(
                    "screen-share H.264 GPU surface pool recycle assertion: {{\"attempted\":true,\"all_slots_free\":true,\"pool_size\":{}}}",
                    snapshot.len()
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "hardware encoder retained a GPU surface after flush/drop: {snapshot:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
