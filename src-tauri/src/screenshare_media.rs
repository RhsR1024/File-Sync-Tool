use bytes::Bytes;
use mse_fmp4::avc::AvcDecoderConfigurationRecord;
use mse_fmp4::fmp4::{
    AvcConfigurationBox, AvcSampleEntry, FileTypeBox, InitializationSegment, MovieBox,
    MovieExtendsBox, MovieHeaderBox, SampleEntry, TrackBox, TrackExtendsBox,
};
use mse_fmp4::io::WriteTo;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

const H264_EVENT_CAPACITY: usize = 96;
const H264_INPUT_CAPACITY: usize = 2;
const H264_GOP_CACHE_LIMIT: usize = 180;
const H264_TIMESCALE: u32 = 90_000;
const H264_KEYFRAME_INTERVAL_100NS: i64 = 20_000_000;

#[derive(Debug, Clone)]
pub struct H264StreamDescriptor {
    pub generation: u64,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u8,
    pub bitrate_bps: u32,
    pub init_segment: Arc<Bytes>,
}

#[derive(Debug, Clone)]
pub struct H264MediaSegment {
    pub generation: u64,
    pub sequence: u64,
    pub keyframe: bool,
    pub bytes: Arc<Bytes>,
}

#[derive(Debug, Clone)]
pub enum H264MediaEvent {
    Reset(Arc<H264StreamDescriptor>),
    Segment(Arc<H264MediaSegment>),
    Unavailable { generation: u64, error: String },
}

#[derive(Debug, Clone)]
pub struct H264StreamSnapshot {
    pub descriptor: Arc<H264StreamDescriptor>,
    pub segments: Vec<Arc<H264MediaSegment>>,
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
    pub error: Option<String>,
}

#[derive(Default)]
struct H264MediaInner {
    generation: u64,
    descriptor: Option<Arc<H264StreamDescriptor>>,
    segments: VecDeque<Arc<H264MediaSegment>>,
    error: Option<String>,
}

pub struct H264MediaState {
    inner: Mutex<H264MediaInner>,
    events: broadcast::Sender<Arc<H264MediaEvent>>,
    encoded_frames: AtomicU64,
    encoded_bytes: AtomicU64,
    keyframes: AtomicU64,
    dropped_input_frames: AtomicU64,
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
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<H264MediaEvent>> {
        self.events.subscribe()
    }

    pub fn snapshot(&self) -> Option<H264StreamSnapshot> {
        let inner = self.inner.lock().ok()?;
        Some(H264StreamSnapshot {
            descriptor: inner.descriptor.clone()?,
            segments: inner.segments.iter().cloned().collect(),
        })
    }

    pub fn is_ready(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.descriptor.is_some())
            .unwrap_or(false)
    }

    pub fn error(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|inner| inner.error.clone())
    }

    pub fn metrics(&self) -> H264MediaMetricsSnapshot {
        let inner = self.inner.lock().ok();
        let descriptor = inner.as_ref().and_then(|inner| inner.descriptor.as_ref());
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
            error: inner.and_then(|inner| inner.error.clone()),
        }
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
        let descriptor = {
            let mut inner = self.inner.lock().unwrap();
            inner.generation = inner.generation.saturating_add(1).max(1);
            inner.error = None;
            inner.segments.clear();
            let descriptor = Arc::new(H264StreamDescriptor {
                generation: inner.generation,
                codec,
                width,
                height,
                fps,
                bitrate_bps,
                init_segment: Arc::new(Bytes::from(init_segment)),
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
            }
            if inner.segments.is_empty() && !segment.keyframe {
                return;
            }
            inner.segments.push_back(segment.clone());
            while inner.segments.len() > H264_GOP_CACHE_LIMIT {
                if inner.segments.front().is_some_and(|item| item.keyframe) {
                    if inner.segments.len() <= 1 {
                        break;
                    }
                    inner.segments.remove(1);
                } else {
                    inner.segments.pop_front();
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

struct H264InputFrame {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
    stride: usize,
    captured_at_100ns: i64,
}

pub struct H264EncoderWorker {
    sender: SyncSender<H264InputFrame>,
    state: Arc<H264MediaState>,
    origin: Instant,
}

impl H264EncoderWorker {
    pub fn spawn(state: Arc<H264MediaState>, fps: u8, quality: u8) -> Result<Self, String> {
        let (sender, receiver) = sync_channel(H264_INPUT_CAPACITY);
        let thread_state = state.clone();
        std::thread::Builder::new()
            .name("screen-h264-encoder".into())
            .spawn(move || run_encoder_worker(receiver, thread_state, fps, quality))
            .map_err(|error| format!("Failed to spawn H.264 encoder thread: {error}"))?;
        Ok(Self {
            sender,
            state,
            origin: Instant::now(),
        })
    }

    pub fn try_submit(&self, bgra: &[u8], width: usize, height: usize, stride: usize) -> bool {
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
            pixels: bgra[..required].to_vec(),
            width,
            height,
            stride,
            captured_at_100ns,
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
}

fn run_encoder_worker(
    receiver: Receiver<H264InputFrame>,
    state: Arc<H264MediaState>,
    fps: u8,
    quality: u8,
) {
    #[cfg(target_os = "windows")]
    run_windows_encoder_worker(receiver, state, fps, quality);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = receiver;
        let _ = fps;
        let _ = quality;
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

fn bgra_to_nv12(
    bgra: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    output: &mut Vec<u8>,
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
    let y_size = encoded_width * encoded_height;
    output.clear();
    output.resize(y_size + y_size / 2, 0);
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
    Ok((encoded_width, encoded_height))
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
    let mut retry_after = Instant::now();
    let mut last_dimensions = (0usize, 0usize);
    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;
    let mut active_parameter_sets: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut generation = 0u64;
    let mut sequence = 0u64;

    while let Ok(mut frame) = receiver.recv() {
        // Encoding an old queued frame only increases glass-to-glass latency.
        // Drain the tiny bounded queue and keep the newest available capture.
        while let Ok(newer) = receiver.try_recv() {
            frame = newer;
            state.record_dropped_input();
        }
        let converted = bgra_to_nv12(
            &frame.pixels,
            frame.width,
            frame.height,
            frame.stride,
            &mut nv12,
        );
        let (width, height) = match converted {
            Ok(dimensions) => dimensions,
            Err(error) => {
                state.mark_unavailable(error);
                continue;
            }
        };
        if encoder.is_none() || last_dimensions != (width, height) {
            if Instant::now() < retry_after {
                continue;
            }
            let bitrate = target_bitrate_bps(width as u32, height as u32, fps, quality);
            match WindowsH264Encoder::new(width as u32, height as u32, fps, bitrate) {
                Ok(created) => {
                    encoder = Some(created);
                    last_dimensions = (width, height);
                    sps = None;
                    pps = None;
                    active_parameter_sets = None;
                    sequence = 0;
                }
                Err(error) => {
                    state.mark_unavailable(error);
                    retry_after = Instant::now() + Duration::from_secs(5);
                    continue;
                }
            }
        }
        let Some(active_encoder) = encoder.as_mut() else {
            continue;
        };
        let outputs = match active_encoder.encode(&nv12, frame.captured_at_100ns) {
            Ok(outputs) => outputs,
            Err(error) => {
                state.mark_unavailable(error);
                encoder = None;
                retry_after = Instant::now() + Duration::from_secs(2);
                continue;
            }
        };
        for output in outputs {
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
                    width as u32,
                    height as u32,
                    fps,
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
                    width as u32,
                    height as u32,
                    fps,
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
            state.publish_segment(H264MediaSegment {
                generation,
                sequence,
                keyframe: parsed.keyframe,
                bytes: Arc::new(Bytes::from(fragment)),
            });
        }
    }
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
}

#[cfg(target_os = "windows")]
struct WindowsH264Encoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    codec_api: Option<windows::Win32::Media::MediaFoundation::ICodecAPI>,
    output_size: u32,
    frame_duration_100ns: i64,
    next_keyframe_time_100ns: i64,
    bitrate_bps: u32,
}

#[cfg(target_os = "windows")]
fn win_result<T>(result: windows::core::Result<T>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
impl WindowsH264Encoder {
    fn new(width: u32, height: u32, fps: u8, bitrate_bps: u32) -> Result<Self, String> {
        use windows::core::{Interface, VARIANT};
        use windows::Win32::Media::MediaFoundation::*;
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| format!("H.264 encoder activation failed: {error}"))?;
        if let Ok(attributes) = unsafe { transform.GetAttributes() } {
            let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        }
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
            win_result(transform.SetOutputType(0, &output, 0))?;
        }
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
            win_result(input.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1))?;
            win_result(input.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1))?;
            win_result(transform.SetInputType(0, &input, 0))?;
        }
        let mut codec_api = transform.cast::<ICodecAPI>().ok();
        let supports_forced_keyframes = if let Some(api) = codec_api.as_ref() {
            let gop_size = VARIANT::from(u32::from(fps.max(1)) * 2);
            if unsafe { api.IsSupported(&CODECAPI_AVEncMPVGOPSize) }.is_ok() {
                let _ = unsafe { api.SetValue(&CODECAPI_AVEncMPVGOPSize, &gop_size) };
            }
            unsafe { api.IsSupported(&CODECAPI_AVEncVideoForceKeyFrame) }.is_ok()
        } else {
            false
        };
        if !supports_forced_keyframes {
            codec_api = None;
        }
        let output_info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|error| format!("H.264 output stream info failed: {error}"))?;
        unsafe {
            win_result(transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
            win_result(transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
        }
        Ok(Self {
            transform,
            codec_api,
            output_size: output_info.cbSize.max(width.saturating_mul(height)),
            frame_duration_100ns: 10_000_000 / i64::from(fps.max(1)),
            next_keyframe_time_100ns: H264_KEYFRAME_INTERVAL_100NS,
            bitrate_bps,
        })
    }

    fn encode(
        &mut self,
        nv12: &[u8],
        sample_time_100ns: i64,
    ) -> Result<Vec<EncodedH264Output>, String> {
        use std::mem::ManuallyDrop;
        use windows::core::VARIANT;
        use windows::Win32::Media::MediaFoundation::*;

        if sample_time_100ns >= self.next_keyframe_time_100ns {
            if let Some(api) = self.codec_api.as_ref() {
                let force_keyframe = VARIANT::from(1u32);
                let _ = unsafe { api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &force_keyframe) };
            }
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
        let input_sample = unsafe { MFCreateSample() }
            .map_err(|error| format!("H.264 input sample creation failed: {error}"))?;
        unsafe {
            win_result(input_sample.AddBuffer(&input_buffer))?;
            win_result(input_sample.SetSampleTime(sample_time_100ns))?;
            win_result(input_sample.SetSampleDuration(self.frame_duration_100ns))?;
            win_result(self.transform.ProcessInput(0, &input_sample, 0))?;
        }
        let mut outputs = Vec::new();
        loop {
            let output_buffer = unsafe { MFCreateMemoryBuffer(self.output_size) }
                .map_err(|error| format!("H.264 output buffer creation failed: {error}"))?;
            let output_sample = unsafe { MFCreateSample() }
                .map_err(|error| format!("H.264 output sample creation failed: {error}"))?;
            unsafe { output_sample.AddBuffer(&output_buffer) }
                .map_err(|error| format!("H.264 output sample buffer failed: {error}"))?;
            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(Some(output_sample)),
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
                    let sample_time =
                        unsafe { sample.GetSampleTime() }.unwrap_or(sample_time_100ns);
                    let sample_duration =
                        unsafe { sample.GetSampleDuration() }.unwrap_or(self.frame_duration_100ns);
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
                    if !bytes.is_empty() {
                        outputs.push(EncodedH264Output {
                            bytes,
                            sample_time_100ns: sample_time,
                            sample_duration_100ns: sample_duration,
                        });
                    }
                }
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                Err(error) => {
                    return Err(format!("H.264 ProcessOutput failed: {error}"));
                }
            }
        }
        Ok(outputs)
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

    #[test]
    fn fmp4_init_and_media_segments_have_required_boxes() {
        let sps = [0x67, 0x42, 0xc0, 0x1f, 0xaa];
        let pps = [0x68, 0xce, 0x06, 0xe2];
        let init = build_init_segment(1920, 1080, 15, &sps, &pps).unwrap();
        assert!(init.windows(4).any(|window| window == b"ftyp"));
        assert!(init.windows(4).any(|window| window == b"moov"));
        assert!(init.windows(4).any(|window| window == b"mvex"));
        assert!(init.windows(4).any(|window| window == b"avcC"));

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
        let generation = state.install_stream(
            "avc1.42C01F".to_string(),
            1280,
            720,
            15,
            3_000_000,
            vec![1, 2, 3],
        );
        state.publish_segment(H264MediaSegment {
            generation,
            sequence: 1,
            keyframe: true,
            bytes: Arc::new(Bytes::from_static(b"key-1")),
        });
        state.publish_segment(H264MediaSegment {
            generation,
            sequence: 2,
            keyframe: false,
            bytes: Arc::new(Bytes::from_static(b"delta")),
        });
        state.publish_segment(H264MediaSegment {
            generation,
            sequence: 3,
            keyframe: true,
            bytes: Arc::new(Bytes::from_static(b"key-2")),
        });
        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.segments.len(), 1);
        assert_eq!(snapshot.segments[0].sequence, 3);
        assert!(snapshot.segments[0].keyframe);
    }

    #[test]
    fn target_bitrate_stays_within_lan_limits() {
        assert_eq!(target_bitrate_bps(320, 240, 5, 10), 1_200_000);
        assert_eq!(target_bitrate_bps(7680, 4320, 30, 100), 12_000_000);
        assert!((4_000_000..=7_000_000).contains(&target_bitrate_bps(1920, 1080, 15, 70)));
    }
}
