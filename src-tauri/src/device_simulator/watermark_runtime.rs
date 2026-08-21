use crate::device_simulator::media::{
    Codec, ParameterSetKind, SharedMediaPack, MAX_RECOMMENDED_BITRATE_BPS,
};
use crate::device_simulator::rtsp::scheduler::{
    ScheduledAccessUnit, SharedAccessUnit, SharedFramePublisher, SharedFrameScheduler, SharedNal,
};
use crate::device_simulator::runtime_assets::{RuntimeAssetLayout, RuntimeMediaKind};
use crate::device_simulator::telemetry::ProtocolDiagnosticSink;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLIENT_QUEUE_CAPACITY: usize = 128;
const PIPELINE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PREWARM_FRAME_LIMIT: usize = 300;
const DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(5);
const ALL_IDR_BITRATE_MULTIPLIER: u64 = 4;
const MAIN_ALL_IDR_MINIMUM_BITRATE_BPS: u64 = 18_000_000;
const SECONDARY_ALL_IDR_MINIMUM_BITRATE_BPS: u64 = 2_500_000;
const ALL_IDR_PEAK_BITRATE_NUMERATOR: u64 = 3;
const ALL_IDR_PEAK_BITRATE_DENOMINATOR: u64 = 2;
#[cfg(target_os = "windows")]
const H264_ACCESS_UNIT_DELIMITER: [u8; 2] = [0x09, 0xf0];

#[derive(Debug, Clone)]
pub struct WatermarkStreamSource {
    pub scheduler: SharedFrameScheduler,
    pub sps: Arc<[u8]>,
    pub pps: Arc<[u8]>,
    pub payload_type: u8,
    pub clock_rate: u32,
    pub frame_rate_numerator: u32,
    pub frame_rate_denominator: u32,
    pub bitrate_bps: u64,
    pub maximum_bitrate_bps: u64,
    pub buffer_size_bits: u64,
    pub encoder_backend: Arc<str>,
    pub encoder_name: Arc<str>,
    pub hardware: bool,
    pub rate_control_mode: Arc<str>,
    pub all_idr: bool,
}

#[derive(Debug)]
struct PipelineReady {
    sps: Arc<[u8]>,
    pps: Arc<[u8]>,
    bitrate_bps: u32,
    maximum_bitrate_bps: u32,
    buffer_size_bits: u32,
    encoder_backend: Arc<str>,
    encoder_name: Arc<str>,
    hardware: bool,
    rate_control_mode: Arc<str>,
    all_idr: bool,
}

pub struct WatermarkMediaHub {
    streams: BTreeMap<RuntimeMediaKind, WatermarkStreamSource>,
    shutdown: Arc<AtomicBool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl WatermarkMediaHub {
    pub async fn start(
        assets: Arc<RuntimeAssetLayout>,
        diagnostics: Option<ProtocolDiagnosticSink>,
    ) -> Result<Self, String> {
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut hub = Self {
            streams: BTreeMap::new(),
            shutdown,
            tasks: Vec::new(),
        };

        // Start and retain each MFT before opening the next one. This proves that
        // all three encoder sessions can coexist instead of passing a one-at-a-time
        // capability probe that would fail when the simulator becomes active.
        for kind in [
            RuntimeMediaKind::Main,
            RuntimeMediaKind::Sub,
            RuntimeMediaKind::Third,
        ] {
            let media = assets.media(kind);
            match start_pipeline(kind, media, Arc::clone(&hub.shutdown), diagnostics.clone()).await
            {
                Ok((source, task)) => {
                    log::info!(
                        "device simulator time watermark {:?} stream ready: backend={}, encoder='{}', implementation={}, rate_control={}, target_average={}bps, target_maximum={}bps, buffer={}bits, all_idr={}",
                        kind,
                        source.encoder_backend,
                        source.encoder_name,
                        if source.hardware { "hardware" } else { "software" },
                        source.rate_control_mode,
                        source.bitrate_bps,
                        source.maximum_bitrate_bps,
                        source.buffer_size_bits,
                        source.all_idr
                    );
                    hub.streams.insert(kind, source);
                    hub.tasks.push(task);
                }
                Err(error) => {
                    hub.stop(Duration::from_secs(10)).await;
                    return Err(format!(
                        "{:?} stream watermark initialization failed: {error}. Disable the time watermark and retry if this computer has no usable H.264 encoder",
                        kind
                    ));
                }
            }
        }
        Ok(hub)
    }

    pub fn stream(&self, kind: RuntimeMediaKind) -> &WatermarkStreamSource {
        self.streams
            .get(&kind)
            .expect("all watermark media kinds are initialized")
    }

    pub async fn stop(mut self, timeout: Duration) {
        self.shutdown.store(true, Ordering::Release);
        for task in self.tasks.drain(..) {
            match tokio::time::timeout(timeout, task).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    log::warn!("device simulator time watermark task panicked: {error}");
                }
                Err(_) => {
                    log::warn!("device simulator time watermark task did not stop in time");
                }
            }
        }
    }
}

async fn start_pipeline(
    kind: RuntimeMediaKind,
    media: Arc<SharedMediaPack>,
    shutdown: Arc<AtomicBool>,
    diagnostics: Option<ProtocolDiagnosticSink>,
) -> Result<(WatermarkStreamSource, tokio::task::JoinHandle<()>), String> {
    if media.manifest().codec != Codec::H264 {
        return Err(format!(
            "time watermark supports H.264 media only, but {:?} uses {:?}",
            kind,
            media.manifest().codec
        ));
    }
    let frame_duration_ticks = constant_frame_duration(&media)?;
    let (scheduler, publisher) = SharedFrameScheduler::external(
        media.manifest().clock_rate,
        frame_duration_ticks,
        CLIENT_QUEUE_CAPACITY,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let pipeline_media = Arc::clone(&media);
    let task = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            match initialize_pipeline(kind, &pipeline_media) {
                Ok((mut transcoder, mut state, ready)) => {
                    if ready_tx.send(Ok(ready)).is_err() {
                        return;
                    }
                    if let Err(error) = run_active_pipeline(
                        kind,
                        &pipeline_media,
                        &publisher,
                        &shutdown,
                        &mut transcoder,
                        &mut state,
                        diagnostics.as_ref(),
                    ) {
                        log::error!(
                            "device simulator time watermark {:?} pipeline stopped: {}",
                            kind,
                            error
                        );
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (kind, pipeline_media, publisher, shutdown, diagnostics);
            let _ = ready_tx.send(Err(
                "time watermark requires Windows Media Foundation".to_string()
            ));
        }
    });
    let ready = match tokio::time::timeout(PIPELINE_READY_TIMEOUT, ready_rx).await {
        Ok(Ok(Ok(ready))) => ready,
        Ok(Ok(Err(error))) => return Err(error),
        Ok(Err(_)) => return Err("watermark pipeline stopped during prewarm".into()),
        Err(_) => return Err("watermark pipeline prewarm timed out after 30 seconds".into()),
    };
    Ok((
        WatermarkStreamSource {
            scheduler,
            sps: ready.sps,
            pps: ready.pps,
            payload_type: media.manifest().payload_type,
            clock_rate: media.manifest().clock_rate,
            frame_rate_numerator: media.manifest().frame_rate_numerator,
            frame_rate_denominator: media.manifest().frame_rate_denominator,
            bitrate_bps: u64::from(ready.bitrate_bps),
            maximum_bitrate_bps: u64::from(ready.maximum_bitrate_bps),
            buffer_size_bits: u64::from(ready.buffer_size_bits),
            encoder_backend: ready.encoder_backend,
            encoder_name: ready.encoder_name,
            hardware: ready.hardware,
            rate_control_mode: ready.rate_control_mode,
            all_idr: ready.all_idr,
        },
        task,
    ))
}

fn constant_frame_duration(media: &SharedMediaPack) -> Result<u32, String> {
    let manifest = media.manifest();
    let duration = media
        .frames()
        .first()
        .map(|frame| frame.duration_ticks)
        .ok_or_else(|| "watermark media has no frames".to_string())?;
    if media
        .frames()
        .iter()
        .any(|frame| frame.duration_ticks != duration)
    {
        return Err("watermark media requires a constant frame duration".into());
    }
    if manifest.frame_rate_numerator == 0 || manifest.frame_rate_denominator == 0 {
        return Err("watermark media frame rate must be non-zero".into());
    }
    let rtp_duration_product = u64::from(duration)
        .checked_mul(u64::from(manifest.frame_rate_numerator))
        .ok_or_else(|| "watermark media clock validation overflow".to_string())?;
    let declared_duration_product = u64::from(manifest.clock_rate)
        .checked_mul(u64::from(manifest.frame_rate_denominator))
        .ok_or_else(|| "watermark media clock validation overflow".to_string())?;
    if rtp_duration_product != declared_duration_product {
        return Err(format!(
            "watermark media RTP duration {duration}/{} does not match frame rate {}/{}",
            manifest.clock_rate, manifest.frame_rate_numerator, manifest.frame_rate_denominator
        ));
    }
    Ok(duration)
}

fn watermark_encoder_rate_control(
    kind: RuntimeMediaKind,
    recommended_bitrate_bps: u64,
) -> crate::device_simulator::media::mf_h264::H264EncoderRateControl {
    let minimum_bitrate_bps = match kind {
        RuntimeMediaKind::Main => MAIN_ALL_IDR_MINIMUM_BITRATE_BPS,
        RuntimeMediaKind::Sub | RuntimeMediaKind::Third => SECONDARY_ALL_IDR_MINIMUM_BITRATE_BPS,
    };
    let average_bitrate_bps = recommended_bitrate_bps
        .saturating_mul(ALL_IDR_BITRATE_MULTIPLIER)
        .max(minimum_bitrate_bps)
        .min(MAX_RECOMMENDED_BITRATE_BPS)
        .min(u64::from(u32::MAX));
    let maximum_bitrate_bps = average_bitrate_bps
        .saturating_mul(ALL_IDR_PEAK_BITRATE_NUMERATOR)
        .saturating_div(ALL_IDR_PEAK_BITRATE_DENOMINATOR)
        .min(MAX_RECOMMENDED_BITRATE_BPS)
        .min(u64::from(u32::MAX));
    crate::device_simulator::media::mf_h264::H264EncoderRateControl {
        average_bitrate_bps: average_bitrate_bps as u32,
        maximum_bitrate_bps: maximum_bitrate_bps as u32,
        // One second of buffering lets complex IDR frames use the negotiated
        // peak budget without changing capture cadence or RTP timestamps.
        buffer_size_bits: average_bitrate_bps.max(1) as u32,
    }
}

#[cfg(target_os = "windows")]
struct PipelineState {
    source_frame_index: usize,
    input_time_100ns: i64,
    output_frame_index: usize,
    first_output_time_100ns: Option<i64>,
    last_output_time_100ns: Option<i64>,
    output_time_base_100ns: Option<i64>,
    output_wall_base: Option<Instant>,
    sps: Vec<u8>,
    pps: Vec<u8>,
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct WatermarkTimingDiagnostics {
    window_started_at: Instant,
    inputs: u64,
    outputs: u64,
    published: u64,
    keyframes: u64,
    zero_output_inputs: u64,
    multi_output_inputs: u64,
    input_gap_count: u64,
    output_gap_count: u64,
    short_output_step_count: u64,
    rtp_gap_count: u64,
    short_rtp_step_count: u64,
    max_input_gap: Duration,
    max_output_gap_100ns: i64,
    max_rtp_step: u32,
    total_transcode_time: Duration,
    max_transcode_time: Duration,
    previous_input_at: Option<Instant>,
    first_output_time_100ns: Option<i64>,
    last_output_time_100ns: Option<i64>,
    previous_output_time_100ns: Option<i64>,
    first_rtp_timestamp: Option<u32>,
    last_rtp_timestamp: Option<u32>,
    previous_rtp_timestamp: Option<u32>,
    first_publish_at: Option<Instant>,
    last_publish_at: Option<Instant>,
}

#[cfg(target_os = "windows")]
impl WatermarkTimingDiagnostics {
    fn new(now: Instant) -> Self {
        Self {
            window_started_at: now,
            inputs: 0,
            outputs: 0,
            published: 0,
            keyframes: 0,
            zero_output_inputs: 0,
            multi_output_inputs: 0,
            input_gap_count: 0,
            output_gap_count: 0,
            short_output_step_count: 0,
            rtp_gap_count: 0,
            short_rtp_step_count: 0,
            max_input_gap: Duration::ZERO,
            max_output_gap_100ns: 0,
            max_rtp_step: 0,
            total_transcode_time: Duration::ZERO,
            max_transcode_time: Duration::ZERO,
            previous_input_at: None,
            first_output_time_100ns: None,
            last_output_time_100ns: None,
            previous_output_time_100ns: None,
            first_rtp_timestamp: None,
            last_rtp_timestamp: None,
            previous_rtp_timestamp: None,
            first_publish_at: None,
            last_publish_at: None,
        }
    }

    fn record_input(
        &mut self,
        captured_at: Instant,
        transcode_time: Duration,
        output_count: usize,
        expected_period: Duration,
    ) {
        self.inputs = self.inputs.saturating_add(1);
        self.outputs = self.outputs.saturating_add(output_count as u64);
        self.zero_output_inputs += u64::from(output_count == 0);
        self.multi_output_inputs += u64::from(output_count > 1);
        self.total_transcode_time = self.total_transcode_time.saturating_add(transcode_time);
        self.max_transcode_time = self.max_transcode_time.max(transcode_time);
        if let Some(previous) = self.previous_input_at {
            let gap = captured_at.saturating_duration_since(previous);
            self.max_input_gap = self.max_input_gap.max(gap);
            if gap > expected_period.saturating_add(expected_period / 2) {
                self.input_gap_count = self.input_gap_count.saturating_add(1);
            }
        }
        self.previous_input_at = Some(captured_at);
    }

    fn record_publish(
        &mut self,
        output_time_100ns: i64,
        rtp_timestamp: u32,
        published_at: Instant,
        keyframe: bool,
        expected_output_step_100ns: i64,
        expected_rtp_step: u32,
    ) {
        self.published = self.published.saturating_add(1);
        self.keyframes += u64::from(keyframe);
        self.first_output_time_100ns
            .get_or_insert(output_time_100ns);
        self.last_output_time_100ns = Some(output_time_100ns);
        self.first_rtp_timestamp.get_or_insert(rtp_timestamp);
        self.last_rtp_timestamp = Some(rtp_timestamp);
        self.first_publish_at.get_or_insert(published_at);
        self.last_publish_at = Some(published_at);

        if let Some(previous) = self.previous_output_time_100ns {
            let step = output_time_100ns.saturating_sub(previous);
            self.max_output_gap_100ns = self.max_output_gap_100ns.max(step);
            if step
                > expected_output_step_100ns
                    .saturating_add(expected_output_step_100ns.saturating_div(2))
            {
                self.output_gap_count = self.output_gap_count.saturating_add(1);
            } else if step.saturating_add(expected_output_step_100ns.saturating_div(2))
                < expected_output_step_100ns
            {
                self.short_output_step_count = self.short_output_step_count.saturating_add(1);
            }
        }
        if let Some(previous) = self.previous_rtp_timestamp {
            let step = rtp_timestamp.wrapping_sub(previous);
            self.max_rtp_step = self.max_rtp_step.max(step);
            if step > expected_rtp_step.saturating_add(expected_rtp_step / 2) {
                self.rtp_gap_count = self.rtp_gap_count.saturating_add(1);
            } else if step.saturating_add(expected_rtp_step / 2) < expected_rtp_step {
                self.short_rtp_step_count = self.short_rtp_step_count.saturating_add(1);
            }
        }
        self.previous_output_time_100ns = Some(output_time_100ns);
        self.previous_rtp_timestamp = Some(rtp_timestamp);
    }

    fn due(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.window_started_at) >= DIAGNOSTIC_INTERVAL
    }

    fn has_anomaly(&self) -> bool {
        self.zero_output_inputs > 0
            || self.multi_output_inputs > 0
            || self.output_gap_count > 0
            || self.short_output_step_count > 0
            || self.rtp_gap_count > 0
            || self.short_rtp_step_count > 0
            || (self.published > 0 && self.keyframes != self.published)
    }

    fn message(
        &self,
        kind: RuntimeMediaKind,
        subscribers: usize,
        clock_rate: u32,
        frame_duration_ticks: u32,
        reason: &str,
        now: Instant,
    ) -> String {
        let window = now.saturating_duration_since(self.window_started_at);
        let window_secs = window.as_secs_f64().max(f64::EPSILON);
        let output_media_ms = self
            .first_output_time_100ns
            .zip(self.last_output_time_100ns)
            .map_or(0.0, |(first, last)| {
                last.saturating_sub(first) as f64 / 10_000.0
            });
        let rtp_media_ms = self
            .first_rtp_timestamp
            .zip(self.last_rtp_timestamp)
            .map_or(0.0, |(first, last)| {
                f64::from(last.wrapping_sub(first)) * 1_000.0 / f64::from(clock_rate.max(1))
            });
        let publish_wall_ms = self
            .first_publish_at
            .zip(self.last_publish_at)
            .map_or(0.0, |(first, last)| {
                last.saturating_duration_since(first).as_secs_f64() * 1_000.0
            });
        let rtp_wall_ratio = if publish_wall_ms > 0.0 {
            rtp_media_ms / publish_wall_ms
        } else {
            0.0
        };
        let expected_fps = f64::from(clock_rate) / f64::from(frame_duration_ticks.max(1));
        let average_transcode_ms = if self.inputs == 0 {
            0.0
        } else {
            self.total_transcode_time.as_secs_f64() * 1_000.0 / self.inputs as f64
        };
        format!(
            "WM_DIAG mode=watermark kind={kind:?} reason={reason} window_ms={} subscribers={subscribers} expected_fps={expected_fps:.3} inputs={} input_fps={:.2} outputs={} output_fps={:.2} published={} publish_fps={:.2} keyframes={} zero_output_inputs={} multi_output_inputs={} input_gap_count={} output_gap_count={} short_output_step_count={} rtp_gap_count={} short_rtp_step_count={} max_input_gap_ms={:.3} max_output_gap_ms={:.3} max_rtp_step={} output_media_ms={:.3} rtp_media_ms={:.3} publish_wall_ms={:.3} rtp_wall_ratio={:.4} avg_transcode_ms={:.3} max_transcode_ms={:.3}",
            window.as_millis(),
            self.inputs,
            self.inputs as f64 / window_secs,
            self.outputs,
            self.outputs as f64 / window_secs,
            self.published,
            self.published as f64 / window_secs,
            self.keyframes,
            self.zero_output_inputs,
            self.multi_output_inputs,
            self.input_gap_count,
            self.output_gap_count,
            self.short_output_step_count,
            self.rtp_gap_count,
            self.short_rtp_step_count,
            self.max_input_gap.as_secs_f64() * 1_000.0,
            self.max_output_gap_100ns.max(0) as f64 / 10_000.0,
            self.max_rtp_step,
            output_media_ms,
            rtp_media_ms,
            publish_wall_ms,
            rtp_wall_ratio,
            average_transcode_ms,
            self.max_transcode_time.as_secs_f64() * 1_000.0,
        )
    }
}

#[cfg(target_os = "windows")]
#[allow(clippy::too_many_arguments)]
fn flush_watermark_diagnostics(
    diagnostics: Option<&ProtocolDiagnosticSink>,
    window: &mut WatermarkTimingDiagnostics,
    kind: RuntimeMediaKind,
    subscribers: usize,
    clock_rate: u32,
    frame_duration_ticks: u32,
    reason: &str,
    now: Instant,
) {
    if window.has_anomaly() {
        if let Some(diagnostics) = diagnostics {
            diagnostics.info(
                "watermark_timing",
                window.message(
                    kind,
                    subscribers,
                    clock_rate,
                    frame_duration_ticks,
                    reason,
                    now,
                ),
            );
        }
    }
    let previous_input_at = window.previous_input_at;
    let previous_output_time_100ns = window.previous_output_time_100ns;
    let previous_rtp_timestamp = window.previous_rtp_timestamp;
    *window = WatermarkTimingDiagnostics::new(now);
    window.previous_input_at = previous_input_at;
    window.previous_output_time_100ns = previous_output_time_100ns;
    window.previous_rtp_timestamp = previous_rtp_timestamp;
}

#[cfg(target_os = "windows")]
fn initialize_pipeline(
    kind: RuntimeMediaKind,
    media: &SharedMediaPack,
) -> Result<
    (
        crate::device_simulator::media::mf_h264::H264WatermarkTranscoder,
        PipelineState,
        PipelineReady,
    ),
    String,
> {
    use crate::device_simulator::media::mf_h264::{h264_sps_dimensions, H264WatermarkTranscoder};

    let input_sps = media
        .parameter_set(ParameterSetKind::Sps)
        .ok_or_else(|| "input H.264 media has no SPS".to_string())?;
    let input_pps = media
        .parameter_set(ParameterSetKind::Pps)
        .ok_or_else(|| "input H.264 media has no PPS".to_string())?;
    let (width, height) = h264_sps_dimensions(&input_sps)
        .map_err(|error| format!("input SPS dimensions are invalid: {error}"))?;
    // Every watermarked frame is an IDR for compatibility with the target
    // recorder. Intra-only H.264 cannot reuse neighbouring frames, so the
    // source stream's inter-frame bitrate is not enough to retain its detail.
    let rate_control =
        watermark_encoder_rate_control(kind, media.manifest().recommended_bitrate_bps);
    let mut transcoder = H264WatermarkTranscoder::new(
        width,
        height,
        media.manifest().frame_rate_numerator,
        media.manifest().frame_rate_denominator,
        rate_control,
        &input_sps,
        &input_pps,
    )?;
    let descriptor = transcoder.descriptor().clone();
    if (descriptor.width, descriptor.height) != (width, height) {
        return Err("watermark transcoder dimensions do not match the input stream".into());
    }
    let frame_duration_100ns = frame_duration_100ns(media)?;
    let mut state = PipelineState {
        source_frame_index: 0,
        input_time_100ns: 0,
        output_frame_index: 0,
        first_output_time_100ns: None,
        last_output_time_100ns: None,
        output_time_base_100ns: None,
        output_wall_base: None,
        sps: Vec::new(),
        pps: Vec::new(),
    };
    let mut has_keyframe = false;
    let attempts = media
        .frames()
        .len()
        .saturating_add(32)
        .min(PREWARM_FRAME_LIMIT);
    for _ in 0..attempts {
        let input = source_annex_b_access_unit(media, state.source_frame_index)?;
        let outputs = transcoder.transcode(&input, state.input_time_100ns, |nv12| {
            render_current_time(nv12, width, height)?;
            Ok(())
        })?;
        state.source_frame_index = (state.source_frame_index + 1) % media.frames().len();
        state.input_time_100ns = state.input_time_100ns.saturating_add(frame_duration_100ns);
        for output in outputs {
            observe_output_time(
                &mut state.first_output_time_100ns,
                &mut state.last_output_time_100ns,
                output.sample_time_100ns,
            )?;
            update_parameter_sets(&output.nals, &mut state.sps, &mut state.pps);
            has_keyframe |= output.keyframe;
        }
        if has_keyframe && !state.sps.is_empty() && !state.pps.is_empty() {
            let (actual_width, actual_height) = h264_sps_dimensions(&state.sps)
                .map_err(|error| format!("encoder produced an invalid SPS: {error}"))?;
            if (actual_width, actual_height) != (width, height) {
                return Err(format!(
                    "encoder changed {:?} dimensions from {}x{} to {}x{}",
                    kind, width, height, actual_width, actual_height
                ));
            }
            let ready = PipelineReady {
                sps: Arc::from(state.sps.clone()),
                pps: Arc::from(state.pps.clone()),
                bitrate_bps: descriptor.average_bitrate_bps,
                maximum_bitrate_bps: descriptor.maximum_bitrate_bps,
                buffer_size_bits: descriptor.buffer_size_bits,
                encoder_backend: Arc::from(descriptor.backend),
                encoder_name: Arc::from(descriptor.encoder_name),
                hardware: descriptor.hardware,
                rate_control_mode: Arc::from(descriptor.rate_control_mode),
                all_idr: descriptor.all_idr,
            };
            return Ok((transcoder, state, ready));
        }
    }
    Err(format!(
        "encoder did not produce SPS, PPS, and IDR within {attempts} {:?} input frames",
        kind
    ))
}

#[cfg(target_os = "windows")]
fn run_active_pipeline(
    kind: RuntimeMediaKind,
    media: &SharedMediaPack,
    publisher: &SharedFramePublisher,
    shutdown: &AtomicBool,
    transcoder: &mut crate::device_simulator::media::mf_h264::H264WatermarkTranscoder,
    state: &mut PipelineState,
    diagnostics: Option<&ProtocolDiagnosticSink>,
) -> Result<(), String> {
    let width_height = crate::device_simulator::media::mf_h264::h264_sps_dimensions(&state.sps)?;
    let frame_duration_ticks = constant_frame_duration(media)?;
    let frame_duration_100ns = frame_duration_100ns(media)?;
    let frame_period = Duration::from_secs_f64(
        f64::from(frame_duration_ticks) / f64::from(media.manifest().clock_rate),
    );
    let mut subscribers = 0usize;
    let mut next_frame_at = Instant::now();
    let mut active_input_wall_base = None::<Instant>;
    let mut active_input_time_base_100ns = state.input_time_100ns;
    let mut diagnostics_window = WatermarkTimingDiagnostics::new(Instant::now());
    while !shutdown.load(Ordering::Acquire) {
        let current_subscribers = publisher.receiver_count();
        if current_subscribers == 0 {
            if subscribers != 0 {
                flush_watermark_diagnostics(
                    diagnostics,
                    &mut diagnostics_window,
                    kind,
                    subscribers,
                    media.manifest().clock_rate,
                    frame_duration_ticks,
                    "idle",
                    Instant::now(),
                );
                log::debug!(
                    "device simulator time watermark {:?} pipeline is idle",
                    kind
                );
            }
            subscribers = 0;
            std::thread::sleep(IDLE_POLL_INTERVAL);
            next_frame_at = Instant::now();
            active_input_wall_base = None;
            state.output_time_base_100ns = None;
            state.output_wall_base = None;
            continue;
        }
        if current_subscribers > subscribers {
            transcoder.request_keyframe()?;
            log::debug!(
                "device simulator time watermark {:?} pipeline active: {} subscriber(s)",
                kind,
                current_subscribers
            );
        }
        subscribers = current_subscribers;
        let now = Instant::now();
        if next_frame_at > now {
            std::thread::sleep(next_frame_at - now);
        }
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        // Keep wall-clock stalls visible, but express the encoder sample clock
        // on the declared constant-frame-rate grid. Some legacy recorders use
        // the sample/RTP clock to split and index access units and mis-handle
        // otherwise valid sub-frame timestamp jitter.
        let capture_at = Instant::now();
        let input_wall_base = *active_input_wall_base.get_or_insert_with(|| {
            active_input_time_base_100ns = state.input_time_100ns;
            capture_at
        });
        let elapsed = capture_at.saturating_duration_since(input_wall_base);
        let input_time_100ns =
            media_time_for_elapsed(active_input_time_base_100ns, elapsed, frame_duration_100ns)?
                .max(state.input_time_100ns);
        let input = source_annex_b_access_unit(media, state.source_frame_index)?;
        let transcode_started_at = Instant::now();
        let outputs = transcoder.transcode(&input, input_time_100ns, |nv12| {
            render_current_time(nv12, width_height.0, width_height.1)?;
            Ok(())
        })?;
        let transcode_time = transcode_started_at.elapsed();
        diagnostics_window.record_input(capture_at, transcode_time, outputs.len(), frame_period);
        state.source_frame_index = (state.source_frame_index + 1) % media.frames().len();
        state.input_time_100ns = input_time_100ns.saturating_add(frame_duration_100ns);
        for output in outputs {
            let output_time_100ns = output.sample_time_100ns;
            observe_output_time(
                &mut state.first_output_time_100ns,
                &mut state.last_output_time_100ns,
                output_time_100ns,
            )?;
            pace_output_frame(state, output_time_100ns)?;
            let previous_sps = state.sps.clone();
            let previous_pps = state.pps.clone();
            update_parameter_sets(&output.nals, &mut state.sps, &mut state.pps);
            if state.sps != previous_sps || state.pps != previous_pps {
                return Err("encoder parameter sets changed after SDP publication".into());
            }
            let nals = access_unit_with_parameter_sets(output.nals, &state.sps, &state.pps);
            let timestamp = rtp_timestamp_for_output_time(
                state
                    .first_output_time_100ns
                    .expect("observed output time establishes a media-clock origin"),
                output_time_100ns,
                frame_duration_100ns,
                frame_duration_ticks,
            )?;
            publisher.publish(ScheduledAccessUnit {
                frame_index: state.output_frame_index,
                timestamp,
                access_unit: Arc::new(SharedAccessUnit {
                    nals: nals
                        .into_iter()
                        .map(SharedNal::from_bytes)
                        .collect::<Vec<_>>()
                        .into(),
                    keyframe: output.keyframe,
                }),
            });
            diagnostics_window.record_publish(
                output_time_100ns,
                timestamp,
                Instant::now(),
                output.keyframe,
                frame_duration_100ns,
                frame_duration_ticks,
            );
            state.output_frame_index = state.output_frame_index.wrapping_add(1);
        }
        next_frame_at += frame_period;
        if next_frame_at < Instant::now() {
            next_frame_at = Instant::now();
        }
        let diagnostics_now = Instant::now();
        if diagnostics_window.due(diagnostics_now) {
            flush_watermark_diagnostics(
                diagnostics,
                &mut diagnostics_window,
                kind,
                subscribers,
                media.manifest().clock_rate,
                frame_duration_ticks,
                "periodic",
                diagnostics_now,
            );
        }
    }
    flush_watermark_diagnostics(
        diagnostics,
        &mut diagnostics_window,
        kind,
        subscribers,
        media.manifest().clock_rate,
        frame_duration_ticks,
        "shutdown",
        Instant::now(),
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn observe_output_time(
    first: &mut Option<i64>,
    last: &mut Option<i64>,
    current: i64,
) -> Result<(), String> {
    if last.is_some_and(|previous| current <= previous) {
        return Err(format!(
            "encoder output time is not monotonic: previous={:?}, current={current}",
            *last
        ));
    }
    first.get_or_insert(current);
    *last = Some(current);
    Ok(())
}

#[cfg(target_os = "windows")]
fn media_time_for_elapsed(
    base_time_100ns: i64,
    elapsed: Duration,
    frame_duration_100ns: i64,
) -> Result<i64, String> {
    let frame_duration_100ns = u128::try_from(frame_duration_100ns)
        .ok()
        .filter(|duration| *duration > 0)
        .ok_or_else(|| "watermark frame duration must be positive".to_string())?;
    let elapsed_100ns = elapsed.as_nanos() / 100;
    let frame_slot = elapsed_100ns
        .checked_add(frame_duration_100ns / 2)
        .ok_or_else(|| "watermark input clock duration overflowed".to_string())?
        / frame_duration_100ns;
    let quantized_elapsed_100ns = frame_slot
        .checked_mul(frame_duration_100ns)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| "watermark input clock duration overflowed".to_string())?;
    base_time_100ns
        .checked_add(quantized_elapsed_100ns)
        .ok_or_else(|| "watermark input clock timestamp overflowed".to_string())
}

#[cfg(target_os = "windows")]
fn pace_output_frame(state: &mut PipelineState, output_time_100ns: i64) -> Result<(), String> {
    let base_time_100ns = *state
        .output_time_base_100ns
        .get_or_insert(output_time_100ns);
    let base_wall = *state.output_wall_base.get_or_insert_with(Instant::now);
    let elapsed_100ns = output_time_100ns
        .checked_sub(base_time_100ns)
        .ok_or_else(|| "watermark encoder output clock moved backwards".to_string())?;
    let nanos = u128::try_from(elapsed_100ns)
        .ok()
        .and_then(|value| value.checked_mul(100))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| "watermark output clock duration overflowed".to_string())?;
    let deadline = base_wall
        .checked_add(Duration::from_nanos(nanos))
        .ok_or_else(|| "watermark output clock deadline overflowed".to_string())?;
    let now = Instant::now();
    if deadline > now {
        std::thread::sleep(deadline - now);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn rtp_timestamp_for_output_time(
    first_output_time_100ns: i64,
    output_time_100ns: i64,
    frame_duration_100ns: i64,
    frame_duration_ticks: u32,
) -> Result<u32, String> {
    // Snap the encoder clock to the exact CFR grid advertised in SDP and SPS.
    // A real stall still advances by multiple frame slots, while harmless MFT
    // jitter cannot turn the stream into a variable-rate recording index.
    let elapsed_100ns = output_time_100ns
        .checked_sub(first_output_time_100ns)
        .ok_or_else(|| "watermark encoder output clock moved before its origin".to_string())?;
    let frame_duration_100ns = u128::try_from(frame_duration_100ns)
        .ok()
        .filter(|duration| *duration > 0)
        .ok_or_else(|| "watermark frame duration must be positive".to_string())?;
    let frame_slot = u128::try_from(elapsed_100ns)
        .ok()
        .and_then(|value| value.checked_add(frame_duration_100ns / 2))
        .ok_or_else(|| "watermark RTP timestamp conversion overflowed".to_string())?
        / frame_duration_100ns;
    let ticks = frame_slot
        .checked_mul(u128::from(frame_duration_ticks))
        .ok_or_else(|| "watermark RTP timestamp conversion overflowed".to_string())?;
    Ok(ticks as u32)
}

#[cfg(target_os = "windows")]
fn render_current_time(nv12: &mut [u8], width: u32, height: u32) -> Result<(), String> {
    let text = crate::device_simulator::media::watermark::format_time_watermark(
        chrono::Local::now().naive_local(),
    );
    crate::device_simulator::media::watermark::render_time_watermark_nv12(
        nv12,
        width as usize,
        height as usize,
        &text,
    )?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn frame_duration_100ns(media: &SharedMediaPack) -> Result<i64, String> {
    let duration = 10_000_000u64
        .checked_mul(u64::from(media.manifest().frame_rate_denominator))
        .ok_or_else(|| "watermark frame duration overflow".to_string())?
        / u64::from(media.manifest().frame_rate_numerator.max(1));
    i64::try_from(duration.max(1)).map_err(|_| "watermark frame duration overflow".into())
}

#[cfg(target_os = "windows")]
fn source_annex_b_access_unit(
    media: &SharedMediaPack,
    frame_index: usize,
) -> Result<Vec<u8>, String> {
    let frame = media
        .frames()
        .get(frame_index)
        .ok_or_else(|| "watermark source frame index is invalid".to_string())?;
    let capacity = frame
        .nals
        .iter()
        .try_fold(0usize, |total, nal| total.checked_add(nal.length + 4))
        .ok_or_else(|| "watermark source access unit is too large".to_string())?;
    let mut bytes = Vec::with_capacity(capacity);
    for nal in frame.nals.iter() {
        let payload = media
            .read_nal(nal)
            .map_err(|error| format!("watermark source NAL read failed: {error}"))?;
        bytes.extend_from_slice(&[0, 0, 0, 1]);
        bytes.extend_from_slice(&payload);
    }
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn update_parameter_sets(nals: &[Vec<u8>], sps: &mut Vec<u8>, pps: &mut Vec<u8>) {
    for nal in nals {
        match nal.first().map(|header| header & 0x1f) {
            Some(7) => sps.clone_from(nal),
            Some(8) => pps.clone_from(nal),
            _ => {}
        }
    }
}

#[cfg(target_os = "windows")]
fn access_unit_with_parameter_sets(nals: Vec<Vec<u8>>, sps: &[u8], pps: &[u8]) -> Vec<Vec<u8>> {
    let keyframe = nals
        .iter()
        .any(|nal| nal.first().is_some_and(|header| header & 0x1f == 5));
    let access_unit_delimiter = nals
        .iter()
        .find(|nal| nal.first().is_some_and(|header| header & 0x1f == 9))
        .cloned()
        .unwrap_or_else(|| H264_ACCESS_UNIT_DELIMITER.to_vec());
    let mut result = Vec::with_capacity(nals.len() + 3);
    // The reviewed indexed stream starts every frame with an AUD. RTP marker
    // bits are sufficient for a conforming receiver, but several legacy NVRs
    // also require an in-band delimiter when constructing recording indexes.
    result.push(access_unit_delimiter);
    if keyframe {
        result.push(sps.to_vec());
        result.push(pps.to_vec());
    }
    for nal in nals {
        let nal_type = nal.first().map(|header| header & 0x1f);
        if nal_type == Some(9) || (keyframe && matches!(nal_type, Some(7 | 8))) {
            continue;
        }
        result.push(nal);
    }
    result
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::device_simulator::rtsp::service::{
        start_rtsp_server, RtspEndpointConfig, RtspStreamSource,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use std::net::{Ipv4Addr, TcpListener};
    use std::process::Command;

    #[test]
    fn keyframes_are_published_with_aud_and_parameter_sets() {
        let nals = vec![vec![0x65, 1, 2]];
        assert_eq!(
            access_unit_with_parameter_sets(nals, &[0x67, 9], &[0x68, 8]),
            vec![
                vec![0x09, 0xf0],
                vec![0x67, 9],
                vec![0x68, 8],
                vec![0x65, 1, 2]
            ]
        );
    }

    #[test]
    fn non_keyframes_are_published_with_one_leading_aud() {
        let nals = vec![vec![0x06, 1], vec![0x09, 0x30], vec![0x41, 2]];
        assert_eq!(
            access_unit_with_parameter_sets(nals, &[0x67, 9], &[0x68, 8]),
            vec![vec![0x09, 0x30], vec![0x06, 1], vec![0x41, 2]]
        );
    }

    #[test]
    fn encoder_parameter_sets_are_replaced_with_the_published_copies() {
        let nals = vec![
            vec![0x09, 0xf0],
            vec![0x67, 1],
            vec![0x68, 2],
            vec![0x06, 4],
            vec![0x65, 3],
        ];
        assert_eq!(
            access_unit_with_parameter_sets(nals, &[0x67, 9], &[0x68, 8]),
            vec![
                vec![0x09, 0xf0],
                vec![0x67, 9],
                vec![0x68, 8],
                vec![0x06, 4],
                vec![0x65, 3]
            ]
        );
    }

    #[test]
    fn rtp_timestamps_use_constant_frame_rate_slots_and_preserve_gaps() {
        let origin = 1_000_000;
        assert_eq!(
            rtp_timestamp_for_output_time(origin, origin, 400_000, 3_600).unwrap(),
            0
        );
        assert_eq!(
            rtp_timestamp_for_output_time(origin, 1_399_000, 400_000, 3_600).unwrap(),
            3_600
        );
        assert_eq!(
            rtp_timestamp_for_output_time(origin, 1_660_000, 400_000, 3_600).unwrap(),
            7_200
        );
        assert_eq!(
            rtp_timestamp_for_output_time(origin, 2_419_740, 400_000, 3_600).unwrap(),
            14_400
        );
    }

    #[test]
    fn watermark_diagnostics_expose_encoder_and_rtp_gaps_as_text() {
        let started_at = Instant::now();
        let mut diagnostics = WatermarkTimingDiagnostics::new(started_at);
        diagnostics.record_input(
            started_at,
            Duration::from_millis(5),
            1,
            Duration::from_millis(40),
        );
        diagnostics.record_publish(1_000_000, 0, started_at, true, 400_000, 3_600);
        diagnostics.record_input(
            started_at + Duration::from_millis(120),
            Duration::from_millis(9),
            1,
            Duration::from_millis(40),
        );
        diagnostics.record_publish(
            2_200_000,
            10_800,
            started_at + Duration::from_millis(120),
            false,
            400_000,
            3_600,
        );

        assert_eq!(diagnostics.input_gap_count, 1);
        assert_eq!(diagnostics.output_gap_count, 1);
        assert_eq!(diagnostics.rtp_gap_count, 1);
        assert!(diagnostics.has_anomaly());
        let message = diagnostics.message(
            RuntimeMediaKind::Main,
            1,
            90_000,
            3_600,
            "test",
            started_at + Duration::from_secs(5),
        );
        assert!(message.contains("WM_DIAG mode=watermark kind=Main"));
        assert!(message.contains("input_gap_count=1"));
        assert!(message.contains("rtp_gap_count=1"));
        assert!(message.contains("max_rtp_step=10800"));
    }

    #[test]
    fn all_idr_watermark_rate_control_has_per_stream_quality_headroom_and_a_hard_cap() {
        let legacy_main = watermark_encoder_rate_control(RuntimeMediaKind::Main, 3_400_000);
        assert_eq!(legacy_main.average_bitrate_bps, 18_000_000);
        assert_eq!(legacy_main.maximum_bitrate_bps, 27_000_000);
        assert_eq!(legacy_main.buffer_size_bits, 18_000_000);

        let improved_main = watermark_encoder_rate_control(RuntimeMediaKind::Main, 6_000_000);
        assert_eq!(improved_main.average_bitrate_bps, 24_000_000);
        assert_eq!(improved_main.maximum_bitrate_bps, 36_000_000);

        for kind in [RuntimeMediaKind::Sub, RuntimeMediaKind::Third] {
            let legacy = watermark_encoder_rate_control(kind, 500_000);
            assert_eq!(legacy.average_bitrate_bps, 2_500_000);
            assert_eq!(legacy.maximum_bitrate_bps, 3_750_000);
            let improved = watermark_encoder_rate_control(kind, 1_000_000);
            assert_eq!(improved.average_bitrate_bps, 4_000_000);
            assert_eq!(improved.maximum_bitrate_bps, 6_000_000);
        }

        let capped = watermark_encoder_rate_control(RuntimeMediaKind::Main, 100_000_000);
        assert_eq!(capped.average_bitrate_bps, 100_000_000);
        assert_eq!(capped.maximum_bitrate_bps, 100_000_000);
        assert_eq!(capped.buffer_size_bits, 100_000_000);
    }

    #[test]
    fn input_media_time_uses_constant_frame_rate_slots() {
        assert_eq!(
            media_time_for_elapsed(1_000_000, Duration::from_millis(39), 400_000).unwrap(),
            1_400_000
        );
        assert_eq!(
            media_time_for_elapsed(1_000_000, Duration::from_millis(61), 400_000).unwrap(),
            1_800_000
        );
    }

    #[test]
    fn rejects_non_monotonic_encoder_output_time() {
        let mut first = None;
        let mut last = None;
        observe_output_time(&mut first, &mut last, 400_000).unwrap();
        assert_eq!(first, Some(400_000));
        assert!(observe_output_time(&mut first, &mut last, 400_000).is_err());
        assert!(observe_output_time(&mut first, &mut last, 399_999).is_err());
    }

    #[tokio::test]
    async fn approved_main_stream_preserves_recording_clock_and_stops() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let pack = std::path::Path::new(&root)
            .join("media-h264-live")
            .join(version);
        let manifest = if pack.join("media/themes/classic/main/media.json").is_file() {
            "media/themes/classic/main/media.json"
        } else {
            "media/main/media.json"
        };
        let media = crate::device_simulator::media::load_media_pack(&pack, manifest).unwrap();
        let frame_rate_numerator = media.manifest().frame_rate_numerator;
        let frame_rate_denominator = media.manifest().frame_rate_denominator;
        let frame_duration_ticks = constant_frame_duration(&media).unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (source, task) =
            start_pipeline(RuntimeMediaKind::Main, media, Arc::clone(&shutdown), None)
                .await
                .unwrap();
        let mut receiver = source.scheduler.subscribe();
        let started_at = Instant::now();
        let frames = tokio::time::timeout(Duration::from_secs(5), async {
            let mut frames = Vec::with_capacity(26);
            while frames.len() < 26 {
                let frame = receiver.recv().await.unwrap();
                frames.push((Instant::now(), frame));
            }
            frames
        })
        .await
        .expect("watermark pipeline did not preserve its live frame cadence");
        let elapsed = started_at.elapsed();
        drop(receiver);
        shutdown.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("watermark pipeline did not stop")
            .unwrap();
        assert!(
            elapsed >= Duration::from_millis(750),
            "26 frames arrived too quickly for a 25 fps recording clock: {elapsed:?}"
        );
        let first = &frames.first().unwrap();
        let last = &frames.last().unwrap();
        let arrival_span = last.0.saturating_duration_since(first.0);
        let rtp_span = Duration::from_secs_f64(
            f64::from(last.1.timestamp.wrapping_sub(first.1.timestamp))
                / f64::from(source.clock_rate),
        );
        let clock_drift = if rtp_span >= arrival_span {
            rtp_span - arrival_span
        } else {
            arrival_span - rtp_span
        };
        assert!(
            clock_drift <= Duration::from_millis(150),
            "watermark RTP time {rtp_span:?} diverged from its live capture clock {arrival_span:?}"
        );
        assert!(frames.windows(2).all(|pair| {
            pair[1].1.timestamp.wrapping_sub(pair[0].1.timestamp) % frame_duration_ticks == 0
        }));
        assert!(frames.iter().all(|(_, frame)| {
            frame
                .access_unit
                .nals
                .first()
                .is_some_and(|nal| nal.as_ref()[0] & 0x1f == 9)
        }));
        let frame = &frames[0].1;
        assert!(frame.access_unit.keyframe);
        let sps = frame
            .access_unit
            .nals
            .iter()
            .find(|nal| nal.as_ref()[0] & 0x1f == 7)
            .expect("first encoded keyframe carries its active SPS");
        assert_eq!(
            crate::device_simulator::media::mf_h264::normalize_h264_sps_frame_rate(
                sps.as_ref(),
                frame_rate_numerator,
                frame_rate_denominator,
            )
            .unwrap(),
            sps.as_ref()
        );
    }

    #[tokio::test]
    async fn approved_three_stream_encoders_preserve_profile_without_level_downgrade() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let pack = std::path::Path::new(&root)
            .join("media-h264-live")
            .join(version);
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut tasks = Vec::new();
        let mut observed = Vec::new();
        for (kind, name) in [
            (RuntimeMediaKind::Main, "main"),
            (RuntimeMediaKind::Sub, "sub"),
            (RuntimeMediaKind::Third, "third"),
        ] {
            let themed_manifest = format!("media/themes/classic/{name}/media.json");
            let legacy_manifest = format!("media/{name}/media.json");
            let manifest = if pack.join(&themed_manifest).is_file() {
                themed_manifest
            } else {
                legacy_manifest
            };
            let media = crate::device_simulator::media::load_media_pack(&pack, &manifest).unwrap();
            let input_sps = media.parameter_set(ParameterSetKind::Sps).unwrap();
            let frame_duration_ticks = constant_frame_duration(&media).unwrap();
            let expected_rate_control =
                watermark_encoder_rate_control(kind, media.manifest().recommended_bitrate_bps);
            let started = start_pipeline(kind, media, Arc::clone(&shutdown), None).await;
            let (source, task) = match started {
                Ok(started) => started,
                Err(error) => {
                    shutdown.store(true, Ordering::Release);
                    for task in tasks {
                        let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
                    }
                    panic!("{name} High Profile watermark encoder failed: {error}");
                }
            };
            assert_eq!(
                source.bitrate_bps,
                u64::from(expected_rate_control.average_bitrate_bps)
            );
            assert_eq!(
                source.maximum_bitrate_bps,
                u64::from(expected_rate_control.maximum_bitrate_bps)
            );
            assert_eq!(
                source.buffer_size_bits,
                u64::from(expected_rate_control.buffer_size_bits)
            );
            assert_eq!(source.encoder_backend.as_ref(), "media-foundation");
            assert!(source.all_idr);

            let mut receiver = source.scheduler.subscribe();
            let capture_started_at = Instant::now();
            let mut captured = Vec::with_capacity(11);
            while captured.len() < 11 {
                captured.push(
                    tokio::time::timeout(Duration::from_secs(3), receiver.recv())
                        .await
                        .expect("watermark stream stopped producing at its declared cadence")
                        .unwrap(),
                );
            }
            assert!(
                capture_started_at.elapsed() >= Duration::from_millis(300),
                "{name} emitted 11 watermarked frames too quickly"
            );
            assert!(captured.iter().all(|frame| frame.access_unit.keyframe));
            assert!(captured.windows(2).all(|pair| {
                pair[1].timestamp.wrapping_sub(pair[0].timestamp) % frame_duration_ticks == 0
            }));
            drop(receiver);
            observed.push((
                name,
                (input_sps[1], input_sps[3]),
                (source.sps[1], source.sps[3]),
            ));
            tasks.push(task);
        }
        shutdown.store(true, Ordering::Release);
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(10), task)
                .await
                .expect("watermark encoder did not stop")
                .unwrap();
        }
        for (name, expected, actual) in observed {
            assert_eq!(
                actual.0, expected.0,
                "{name} watermark encoder changed the source H.264 profile"
            );
            assert!(
                actual.1 >= expected.1,
                "{name} watermark encoder downgraded the source H.264 level from {} to {}",
                expected.1,
                actual.1
            );
        }
    }

    struct FfmpegRecordingProbe {
        capture_elapsed: Duration,
        encoded_bytes: u64,
        keyframe_count: usize,
        stream: serde_json::Value,
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_and_probe_rtsp_source(
        label: &str,
        scheduler: SharedFrameScheduler,
        sps: &[u8],
        pps: &[u8],
        payload_type: u8,
        clock_rate: u32,
        frame_count: u32,
        ffmpeg: &str,
        ffprobe: &str,
    ) -> FfmpegRecordingProbe {
        let probe_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = probe_listener.local_addr().unwrap().port();
        drop(probe_listener);
        let route = "/media/video1";
        let control_route = "/media/video1/video";
        let sdp = format!(
            "v=0\r\no=- 1001 1 IN IP4 127.0.0.1\r\ns={label} recording clock\r\nt=0 0\r\nm=video 0 RTP/AVP {payload_type}\r\nc=IN IP4 127.0.0.1\r\na=rtpmap:{payload_type} H264/{clock_rate}\r\na=fmtp:{payload_type} packetization-mode=1; sprop-parameter-sets={},{}\r\na=control:rtsp://127.0.0.1:{port}{control_route}\r\n",
            BASE64_STANDARD.encode(sps),
            BASE64_STANDARD.encode(pps),
        );
        let stream = RtspStreamSource::from_scheduler(
            format!("{label}-main"),
            sdp.into_bytes(),
            scheduler,
            Codec::H264,
            payload_type,
            1_200,
        )
        .unwrap();
        let server = start_rtsp_server(RtspEndpointConfig {
            bind_addr: (Ipv4Addr::LOCALHOST, port).into(),
            routes: BTreeMap::from([
                (route.into(), stream.clone()),
                (control_route.into(), stream),
            ]),
            client_write_queue: 32,
        })
        .await
        .unwrap();

        let output_directory = tempfile::tempdir().unwrap();
        let raw_h264 = output_directory
            .path()
            .join(format!("{label}-recording.h264"));
        let url = format!("rtsp://127.0.0.1:{port}{route}");
        let ffmpeg_result = tokio::task::spawn_blocking({
            let ffmpeg = ffmpeg.to_owned();
            let raw_h264 = raw_h264.clone();
            move || {
                let started_at = Instant::now();
                let output = Command::new(ffmpeg)
                    .args([
                        "-hide_banner",
                        "-loglevel",
                        "error",
                        "-rtsp_transport",
                        "tcp",
                        "-i",
                        &url,
                        "-an",
                        "-frames:v",
                        &frame_count.to_string(),
                        "-c:v",
                        "copy",
                        "-f",
                        "h264",
                        "-y",
                    ])
                    .arg(&raw_h264)
                    .output();
                (started_at.elapsed(), output)
            }
        })
        .await
        .unwrap();

        server.stop(Duration::from_secs(2)).await.unwrap();
        let (capture_elapsed, ffmpeg_output) = ffmpeg_result;
        let ffmpeg_output = ffmpeg_output.unwrap();
        assert!(
            ffmpeg_output.status.success(),
            "FFmpeg recording failed: {}",
            String::from_utf8_lossy(&ffmpeg_output.stderr)
        );
        assert!(
            capture_elapsed >= Duration::from_millis(1_500),
            "two seconds of video were emitted too quickly: {capture_elapsed:?}"
        );
        let ffprobe_output = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-count_frames",
                "-show_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=profile,level,avg_frame_rate,r_frame_rate,nb_read_frames:frame=key_frame",
                "-of",
                "json",
            ])
            .arg(&raw_h264)
            .output()
            .unwrap();
        assert!(
            ffprobe_output.status.success(),
            "FFprobe failed: {}",
            String::from_utf8_lossy(&ffprobe_output.stderr)
        );
        let probe: serde_json::Value = serde_json::from_slice(&ffprobe_output.stdout).unwrap();
        let keyframe_count = probe["frames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|frame| frame["key_frame"].as_i64() == Some(1))
            .count();
        FfmpegRecordingProbe {
            capture_elapsed,
            encoded_bytes: std::fs::metadata(&raw_h264).unwrap().len(),
            keyframe_count,
            stream: probe["streams"][0].clone(),
        }
    }

    #[tokio::test]
    async fn approved_main_stream_records_at_declared_frame_rate_with_ffmpeg() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let ffmpeg = std::env::var("FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".into());
        let ffprobe = std::env::var("FFPROBE_BIN").unwrap_or_else(|_| "ffprobe".into());
        if Command::new(&ffmpeg).arg("-version").output().is_err()
            || Command::new(&ffprobe).arg("-version").output().is_err()
        {
            return;
        }

        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let pack = std::path::Path::new(&root)
            .join("media-h264-live")
            .join(version);
        let manifest = if pack.join("media/themes/classic/main/media.json").is_file() {
            "media/themes/classic/main/media.json"
        } else {
            "media/main/media.json"
        };
        let media = crate::device_simulator::media::load_media_pack(&pack, manifest).unwrap();
        let frame_rate_numerator = media.manifest().frame_rate_numerator;
        let frame_rate_denominator = media.manifest().frame_rate_denominator;
        let payload_type = media.manifest().payload_type;
        let clock_rate = media.manifest().clock_rate;
        let source_bitrate_bps = media.manifest().recommended_bitrate_bps;
        let frame_count = frame_rate_numerator
            .saturating_mul(2)
            .checked_div(frame_rate_denominator)
            .unwrap_or(0)
            .max(2)
            + 1;
        let original_sps = media.parameter_set(ParameterSetKind::Sps).unwrap();
        let original_pps = media.parameter_set(ParameterSetKind::Pps).unwrap();
        let indexed_scheduler = SharedFrameScheduler::from_media(Arc::clone(&media), 128).unwrap();

        let shutdown = Arc::new(AtomicBool::new(false));
        let (source, pipeline_task) =
            start_pipeline(RuntimeMediaKind::Main, media, Arc::clone(&shutdown), None)
                .await
                .unwrap();
        assert_eq!(
            source.bitrate_bps,
            u64::from(
                watermark_encoder_rate_control(RuntimeMediaKind::Main, source_bitrate_bps)
                    .average_bitrate_bps
            ),
            "the all-IDR watermark stream must expose its increased encoder bitrate"
        );
        assert!(source.all_idr);
        let watermark_probe = record_and_probe_rtsp_source(
            "watermark",
            source.scheduler.clone(),
            &source.sps,
            &source.pps,
            payload_type,
            clock_rate,
            frame_count,
            &ffmpeg,
            &ffprobe,
        )
        .await;
        shutdown.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(10), pipeline_task)
            .await
            .expect("watermark pipeline did not stop after FFmpeg recording")
            .unwrap();

        let indexed_probe = record_and_probe_rtsp_source(
            "indexed",
            indexed_scheduler,
            &original_sps,
            &original_pps,
            payload_type,
            clock_rate,
            frame_count,
            &ffmpeg,
            &ffprobe,
        )
        .await;

        eprintln!(
            "watermark recording probe: {}, keyframes={}, bytes={}; indexed recording probe: {}, keyframes={}, bytes={}",
            watermark_probe.stream,
            watermark_probe.keyframe_count,
            watermark_probe.encoded_bytes,
            indexed_probe.stream,
            indexed_probe.keyframe_count,
            indexed_probe.encoded_bytes,
        );
        assert!(
            watermark_probe.capture_elapsed >= Duration::from_millis(1_500),
            "watermark video was emitted too quickly: {:?}",
            watermark_probe.capture_elapsed
        );
        let expected_rate = format!("{frame_rate_numerator}/{frame_rate_denominator}");
        assert_eq!(
            watermark_probe.stream["avg_frame_rate"].as_str(),
            Some(expected_rate.as_str())
        );
        assert_eq!(
            indexed_probe.stream["avg_frame_rate"].as_str(),
            Some(expected_rate.as_str())
        );
        assert_eq!(
            watermark_probe.stream["profile"], indexed_probe.stream["profile"],
            "watermark encoding must preserve the source H.264 profile for recorder compatibility"
        );
        let watermark_level = watermark_probe.stream["level"]
            .as_i64()
            .expect("watermark ffprobe output has an H.264 level");
        let indexed_level = indexed_probe.stream["level"]
            .as_i64()
            .expect("indexed ffprobe output has an H.264 level");
        assert!(
            watermark_level >= indexed_level,
            "watermark encoding must not downgrade the source H.264 level: {indexed_level} -> {watermark_level}"
        );
        let expected_frame_count = frame_count.to_string();
        assert_eq!(
            watermark_probe.stream["nb_read_frames"].as_str(),
            Some(expected_frame_count.as_str())
        );
        assert_eq!(
            watermark_probe.keyframe_count, frame_count as usize,
            "every watermarked frame must be independently decodable by legacy recorders"
        );
        assert!(
            watermark_probe.encoded_bytes >= indexed_probe.encoded_bytes.saturating_mul(2),
            "the all-IDR watermark stream must retain meaningful bitrate headroom: watermark={} bytes, indexed={} bytes",
            watermark_probe.encoded_bytes,
            indexed_probe.encoded_bytes,
        );
        assert!(
            indexed_probe.keyframe_count < frame_count as usize,
            "the indexed control stream must retain its inter-frame GOP"
        );
    }
}
