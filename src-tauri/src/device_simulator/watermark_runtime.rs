use crate::device_simulator::media::{Codec, ParameterSetKind, SharedMediaPack};
use crate::device_simulator::rtsp::scheduler::{
    ScheduledAccessUnit, SharedAccessUnit, SharedFramePublisher, SharedFrameScheduler, SharedNal,
};
use crate::device_simulator::runtime_assets::{RuntimeAssetLayout, RuntimeMediaKind};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CLIENT_QUEUE_CAPACITY: usize = 128;
const PIPELINE_READY_TIMEOUT: Duration = Duration::from_secs(30);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PREWARM_FRAME_LIMIT: usize = 300;

#[derive(Debug, Clone)]
pub struct WatermarkStreamSource {
    pub scheduler: SharedFrameScheduler,
    pub sps: Arc<[u8]>,
    pub pps: Arc<[u8]>,
    pub payload_type: u8,
    pub clock_rate: u32,
    pub bitrate_bps: u64,
    pub encoder_name: Arc<str>,
    pub hardware: bool,
}

#[derive(Debug)]
struct PipelineReady {
    sps: Arc<[u8]>,
    pps: Arc<[u8]>,
    encoder_name: Arc<str>,
    hardware: bool,
}

pub struct WatermarkMediaHub {
    streams: BTreeMap<RuntimeMediaKind, WatermarkStreamSource>,
    shutdown: Arc<AtomicBool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl WatermarkMediaHub {
    pub async fn start(assets: Arc<RuntimeAssetLayout>) -> Result<Self, String> {
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
            match start_pipeline(kind, media, Arc::clone(&hub.shutdown)).await {
                Ok((source, task)) => {
                    log::info!(
                        "device simulator time watermark {:?} stream ready: encoder='{}', path={}, {} bps",
                        kind,
                        source.encoder_name,
                        if source.hardware { "hardware" } else { "software" },
                        source.bitrate_bps
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
            let _ = (kind, pipeline_media, publisher, shutdown);
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
            bitrate_bps: media.manifest().recommended_bitrate_bps,
            encoder_name: ready.encoder_name,
            hardware: ready.hardware,
        },
        task,
    ))
}

fn constant_frame_duration(media: &SharedMediaPack) -> Result<u32, String> {
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
    Ok(duration)
}

#[cfg(target_os = "windows")]
struct PipelineState {
    source_frame_index: usize,
    input_time_100ns: i64,
    rtp_timestamp: u32,
    output_frame_index: usize,
    last_output_time_100ns: Option<i64>,
    sps: Vec<u8>,
    pps: Vec<u8>,
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
    let (width, height) = h264_sps_dimensions(input_sps)
        .map_err(|error| format!("input SPS dimensions are invalid: {error}"))?;
    let bitrate = u32::try_from(media.manifest().recommended_bitrate_bps)
        .map_err(|_| "recommended watermark bitrate exceeds the encoder limit".to_string())?;
    let mut transcoder = H264WatermarkTranscoder::new(
        width,
        height,
        media.manifest().frame_rate_numerator,
        media.manifest().frame_rate_denominator,
        bitrate,
    )?;
    let descriptor = transcoder.descriptor().clone();
    if (descriptor.width, descriptor.height) != (width, height) {
        return Err("watermark transcoder dimensions do not match the input stream".into());
    }
    let frame_duration_100ns = frame_duration_100ns(media)?;
    let mut state = PipelineState {
        source_frame_index: 0,
        input_time_100ns: 0,
        rtp_timestamp: 0,
        output_frame_index: 0,
        last_output_time_100ns: None,
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
            observe_output_time(&mut state.last_output_time_100ns, output.sample_time_100ns)?;
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
                encoder_name: Arc::from(descriptor.encoder_name),
                hardware: descriptor.hardware,
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
) -> Result<(), String> {
    let width_height = crate::device_simulator::media::mf_h264::h264_sps_dimensions(&state.sps)?;
    let frame_duration_ticks = constant_frame_duration(media)?;
    let frame_duration_100ns = frame_duration_100ns(media)?;
    let frame_period = Duration::from_secs_f64(
        f64::from(frame_duration_ticks) / f64::from(media.manifest().clock_rate),
    );
    let mut subscribers = 0usize;
    let mut next_frame_at = Instant::now();
    while !shutdown.load(Ordering::Acquire) {
        let current_subscribers = publisher.receiver_count();
        if current_subscribers == 0 {
            if subscribers != 0 {
                log::info!(
                    "device simulator time watermark {:?} pipeline is idle",
                    kind
                );
            }
            subscribers = 0;
            std::thread::sleep(IDLE_POLL_INTERVAL);
            next_frame_at = Instant::now();
            continue;
        }
        if current_subscribers > subscribers {
            transcoder.request_keyframe()?;
            log::info!(
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
        let input = source_annex_b_access_unit(media, state.source_frame_index)?;
        let outputs = transcoder.transcode(&input, state.input_time_100ns, |nv12| {
            render_current_time(nv12, width_height.0, width_height.1)?;
            Ok(())
        })?;
        state.source_frame_index = (state.source_frame_index + 1) % media.frames().len();
        state.input_time_100ns = state.input_time_100ns.saturating_add(frame_duration_100ns);
        for output in outputs {
            observe_output_time(&mut state.last_output_time_100ns, output.sample_time_100ns)?;
            let previous_sps = state.sps.clone();
            let previous_pps = state.pps.clone();
            update_parameter_sets(&output.nals, &mut state.sps, &mut state.pps);
            if state.sps != previous_sps || state.pps != previous_pps {
                return Err("encoder parameter sets changed after SDP publication".into());
            }
            let nals = access_unit_with_parameter_sets(output.nals, &state.sps, &state.pps);
            publisher.publish(ScheduledAccessUnit {
                frame_index: state.output_frame_index,
                timestamp: state.rtp_timestamp,
                access_unit: Arc::new(SharedAccessUnit {
                    nals: nals
                        .into_iter()
                        .map(SharedNal::from_bytes)
                        .collect::<Vec<_>>()
                        .into(),
                    keyframe: output.keyframe,
                }),
            });
            state.output_frame_index = state.output_frame_index.wrapping_add(1);
            state.rtp_timestamp = state.rtp_timestamp.wrapping_add(frame_duration_ticks);
        }
        next_frame_at += frame_period;
        if next_frame_at < Instant::now() {
            next_frame_at = Instant::now();
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn observe_output_time(last: &mut Option<i64>, current: i64) -> Result<(), String> {
    if last.is_some_and(|previous| current <= previous) {
        return Err(format!(
            "encoder output time is not monotonic: previous={:?}, current={current}",
            *last
        ));
    }
    *last = Some(current);
    Ok(())
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
            .nal_bytes(nal)
            .ok_or_else(|| "watermark source NAL is outside the media buffer".to_string())?;
        bytes.extend_from_slice(&[0, 0, 0, 1]);
        bytes.extend_from_slice(payload);
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
    if !keyframe {
        return nals;
    }
    let has_sps = nals
        .iter()
        .any(|nal| nal.first().is_some_and(|header| header & 0x1f == 7));
    let has_pps = nals
        .iter()
        .any(|nal| nal.first().is_some_and(|header| header & 0x1f == 8));
    let mut result = Vec::with_capacity(nals.len() + usize::from(!has_sps) + usize::from(!has_pps));
    if !has_sps {
        result.push(sps.to_vec());
    }
    if !has_pps {
        result.push(pps.to_vec());
    }
    result.extend(nals);
    result
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn keyframes_are_published_with_parameter_sets() {
        let nals = vec![vec![0x65, 1, 2]];
        assert_eq!(
            access_unit_with_parameter_sets(nals, &[0x67, 9], &[0x68, 8]),
            vec![vec![0x67, 9], vec![0x68, 8], vec![0x65, 1, 2]]
        );
    }

    #[test]
    fn existing_parameter_sets_are_not_duplicated() {
        let nals = vec![vec![0x67, 1], vec![0x68, 2], vec![0x65, 3]];
        assert_eq!(
            access_unit_with_parameter_sets(nals.clone(), &[0x67, 9], &[0x68, 8]),
            nals
        );
    }

    #[tokio::test]
    async fn approved_main_stream_resumes_on_subscribe_and_stops() {
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
        let shutdown = Arc::new(AtomicBool::new(false));
        let (source, task) = start_pipeline(RuntimeMediaKind::Main, media, Arc::clone(&shutdown))
            .await
            .unwrap();
        let mut receiver = source.scheduler.subscribe();
        let received = tokio::time::timeout(Duration::from_secs(5), receiver.recv()).await;
        drop(receiver);
        shutdown.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("watermark pipeline did not stop")
            .unwrap();
        let frame = received
            .expect("watermark pipeline did not resume")
            .unwrap();
        assert!(frame.access_unit.keyframe);
        assert!(frame
            .access_unit
            .nals
            .iter()
            .any(|nal| nal.as_ref()[0] & 0x1f == 7));
    }
}
