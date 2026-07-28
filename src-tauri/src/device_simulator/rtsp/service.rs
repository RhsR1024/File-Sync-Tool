use super::rtp::{tcp_interleaved_frame, RtpPacketizer};
use super::scheduler::{ScheduledAccessUnit, SharedFrameScheduler};
use super::state::{
    build_rtsp_response, declared_body_length, parse_rtsp_request, RtspDecision, RtspSession,
    MAX_RTSP_REQUEST_BYTES,
};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch};

use crate::device_simulator::media::{Codec, SharedMediaPack};

static NEXT_RTP_CLIENT_ID: AtomicU32 = AtomicU32::new(1);
const LEGACY_RTP_SSRC: u32 = 0x0c8c_750a;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RtspServerStats {
    pub active_clients: u32,
    pub bytes_sent: u64,
    pub disconnected_clients: u64,
}

#[derive(Debug, Default)]
struct RtspServerMetrics {
    active_clients: AtomicU32,
    bytes_sent: AtomicU64,
    disconnected_clients: AtomicU64,
}

impl RtspServerMetrics {
    fn snapshot(&self) -> RtspServerStats {
        RtspServerStats {
            active_clients: self.active_clients.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            disconnected_clients: self.disconnected_clients.load(Ordering::Relaxed),
        }
    }
}

struct ActiveRtspClient {
    metrics: Arc<RtspServerMetrics>,
}

impl ActiveRtspClient {
    fn new(metrics: Arc<RtspServerMetrics>) -> Self {
        metrics.active_clients.fetch_add(1, Ordering::Relaxed);
        Self { metrics }
    }
}

impl Drop for ActiveRtspClient {
    fn drop(&mut self) {
        self.metrics.active_clients.fetch_sub(1, Ordering::Relaxed);
        self.metrics
            .disconnected_clients
            .fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClientRtpState {
    ssrc: u32,
    next_sequence: u16,
    timestamp_offset: u32,
}

fn next_client_rtp_state() -> ClientRtpState {
    let id = NEXT_RTP_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
    ClientRtpState {
        // IPCRtspLib.py advertises c8c750a and uses 0c8c750a in RTP.
        ssrc: LEGACY_RTP_SSRC,
        next_sequence: (id as u16).wrapping_mul(7_919),
        timestamp_offset: id.wrapping_mul(0x9e37_79b9),
    }
}

#[derive(Debug, Clone)]
pub struct RtspStreamSource {
    pub stream_id: String,
    pub sdp: Arc<[u8]>,
    pub scheduler: SharedFrameScheduler,
    pub codec: Codec,
    pub payload_type: u8,
    pub max_rtp_payload_bytes: usize,
    pub metadata_only: bool,
}

impl RtspStreamSource {
    pub fn from_media(
        stream_id: impl Into<String>,
        sdp: impl Into<Arc<[u8]>>,
        media: Arc<SharedMediaPack>,
        scheduler_queue_capacity: usize,
        max_rtp_payload_bytes: usize,
    ) -> Result<Self, RtspServiceError> {
        let stream_id = stream_id.into();
        let sdp = sdp.into();
        validate_sdp_media_contract(
            &sdp,
            media.manifest().codec,
            media.manifest().payload_type,
            media.manifest().clock_rate,
        )?;
        let scheduler =
            SharedFrameScheduler::from_media(Arc::clone(&media), scheduler_queue_capacity)
                .map_err(|error| service_error(error.code, error.message))?;
        Self::from_scheduler(
            stream_id,
            sdp,
            scheduler,
            media.manifest().codec,
            media.manifest().payload_type,
            max_rtp_payload_bytes,
        )
    }

    pub fn from_scheduler(
        stream_id: impl Into<String>,
        sdp: impl Into<Arc<[u8]>>,
        scheduler: SharedFrameScheduler,
        codec: Codec,
        payload_type: u8,
        max_rtp_payload_bytes: usize,
    ) -> Result<Self, RtspServiceError> {
        let stream_id = stream_id.into();
        let sdp = sdp.into();
        validate_sdp_media_contract(&sdp, codec, payload_type, scheduler.clock_rate())?;
        Ok(Self {
            stream_id,
            sdp,
            scheduler,
            codec,
            payload_type,
            max_rtp_payload_bytes,
            metadata_only: false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RtspEndpointConfig {
    pub bind_addr: SocketAddr,
    pub routes: BTreeMap<String, RtspStreamSource>,
    pub client_write_queue: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspServiceError {
    pub code: &'static str,
    pub message: String,
}

fn service_error(code: &'static str, message: impl Into<String>) -> RtspServiceError {
    RtspServiceError {
        code,
        message: message.into(),
    }
}

impl RtspEndpointConfig {
    /// Resolve a request URI to a stream.
    ///
    /// The legacy IPCRtsp server never inspected the request path: every URI on
    /// a listener was answered with that listener's stream. Exact matches keep
    /// the reviewed per-path behaviour (notably the metadata-only track), and
    /// anything else falls back to the listener's video route so NVR channels
    /// `c2..cN`, trailing slashes, `trackID` suffixes and query strings play
    /// instead of failing with 404.
    fn resolve_route(&self, path: &str) -> Option<&RtspStreamSource> {
        self.routes
            .get(path)
            .or_else(|| self.routes.values().find(|source| !source.metadata_only))
    }

    pub fn validate(&self) -> Result<(), RtspServiceError> {
        if self.bind_addr.ip().is_unspecified() || self.bind_addr.port() == 0 {
            return Err(service_error(
                "device_simulator.rtsp.bind_invalid",
                "RTSP must bind an explicit virtual IP and non-zero port",
            ));
        }
        if self.routes.is_empty() || self.client_write_queue == 0 || self.client_write_queue > 4096
        {
            return Err(service_error(
                "device_simulator.rtsp.configuration_invalid",
                "RTSP routes and bounded client queue are required",
            ));
        }
        for (path, source) in &self.routes {
            if !valid_route(path)
                || !valid_stream_id(&source.stream_id)
                || source.sdp.is_empty()
                || source.sdp.len() > 64 * 1024
                || !(96..=127).contains(&source.payload_type)
                || !(256..=65_000).contains(&source.max_rtp_payload_bytes)
            {
                return Err(service_error(
                    "device_simulator.rtsp.route_invalid",
                    format!("RTSP route '{path}' is invalid"),
                ));
            }
            validate_sdp_media_contract(
                &source.sdp,
                source.codec,
                source.payload_type,
                source.scheduler.clock_rate(),
            )?;
        }
        Ok(())
    }
}

fn valid_route(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 512
        && !path.contains("..")
        && !path.contains(['\r', '\n', '\0', '?', '#'])
}

fn valid_stream_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn validate_sdp_media_contract(
    sdp: &[u8],
    codec: Codec,
    payload_type: u8,
    clock_rate: u32,
) -> Result<(), RtspServiceError> {
    let sdp = std::str::from_utf8(sdp).map_err(|_| {
        service_error(
            "device_simulator.rtsp.sdp_invalid",
            "SDP must be valid UTF-8 text",
        )
    })?;
    let codec_name = match codec {
        Codec::H264 => "H264",
        Codec::H265 => "H265",
    };
    let media_token = format!("RTP/AVP {payload_type}");
    let mapping_token = format!("a=rtpmap:{payload_type} {codec_name}/{clock_rate}");
    if !sdp.contains(&media_token) || !sdp.contains(&mapping_token) {
        return Err(service_error(
            "device_simulator.rtsp.sdp_media_mismatch",
            "SDP codec, clock, or payload type does not match the verified media manifest",
        ));
    }
    Ok(())
}

struct AbortTaskOnDrop<T> {
    handle: Option<tokio::task::JoinHandle<T>>,
}

impl<T> AbortTaskOnDrop<T> {
    fn new(handle: tokio::task::JoinHandle<T>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    fn abort(&self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }

    async fn join(&mut self) -> Result<T, tokio::task::JoinError> {
        let result = self.handle.as_mut().expect("task handle is present").await;
        self.handle = None;
        result
    }
}

impl<T> Drop for AbortTaskOnDrop<T> {
    fn drop(&mut self) {
        self.abort();
    }
}

pub struct RtspServerHandle {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    join: AbortTaskOnDrop<Result<(), RtspServiceError>>,
    producer_tasks: Vec<AbortTaskOnDrop<()>>,
    metrics: Arc<RtspServerMetrics>,
}

impl RtspServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stats(&self) -> RtspServerStats {
        self.metrics.snapshot()
    }

    pub async fn stop(mut self, timeout: Duration) -> Result<(), RtspServiceError> {
        let _ = self.shutdown.send(true);
        tokio::time::timeout(timeout, async {
            let mut first_error = match self.join.join().await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(source) => Some(service_error(
                    "device_simulator.rtsp.task_panicked",
                    format!("RTSP service task failed: {source}"),
                )),
            };
            for producer_task in &mut self.producer_tasks {
                if let Err(source) = producer_task.join().await {
                    let error = service_error(
                        "device_simulator.rtsp.task_panicked",
                        format!("RTSP scheduler task failed: {source}"),
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            match first_error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        })
        .await
        .map_err(|_| {
            service_error(
                "device_simulator.rtsp.stop_timeout",
                "RTSP service did not stop within the finite timeout",
            )
        })?
    }
}

pub async fn start_rtsp_server(
    config: RtspEndpointConfig,
) -> Result<RtspServerHandle, RtspServiceError> {
    config.validate()?;
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .map_err(|source| {
            service_error(
                "device_simulator.rtsp.bind_failed",
                format!(
                    "could not bind RTSP endpoint {}: {source}",
                    config.bind_addr
                ),
            )
        })?;
    let local_addr = listener.local_addr().map_err(|source| {
        service_error(
            "device_simulator.rtsp.bind_failed",
            format!("could not read RTSP endpoint address: {source}"),
        )
    })?;
    let (shutdown, shutdown_rx) = watch::channel(false);
    let mut schedulers = Vec::<SharedFrameScheduler>::new();
    for source in config
        .routes
        .values()
        .filter(|source| !source.metadata_only && source.scheduler.owns_indexed_producer())
    {
        if !schedulers
            .iter()
            .any(|scheduler| scheduler.shares_channel_with(&source.scheduler))
        {
            schedulers.push(source.scheduler.clone());
        }
    }
    let producer_tasks = schedulers
        .into_iter()
        .map(|scheduler| AbortTaskOnDrop::new(scheduler.spawn(shutdown.subscribe())))
        .collect();
    let metrics = Arc::new(RtspServerMetrics::default());
    let join = tokio::spawn(serve(
        listener,
        Arc::new(config),
        shutdown_rx,
        Arc::clone(&metrics),
    ));
    Ok(RtspServerHandle {
        local_addr,
        shutdown,
        join: AbortTaskOnDrop::new(join),
        producer_tasks,
        metrics,
    })
}

async fn serve(
    listener: TcpListener,
    config: Arc<RtspEndpointConfig>,
    mut shutdown: watch::Receiver<bool>,
    metrics: Arc<RtspServerMetrics>,
) -> Result<(), RtspServiceError> {
    let mut clients = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.map_err(|source| service_error(
                    "device_simulator.rtsp.accept_failed",
                    format!("RTSP accept failed: {source}"),
                ))?;
                let config = Arc::clone(&config);
                let metrics = Arc::clone(&metrics);
                clients.spawn(async move {
                    let _active = ActiveRtspClient::new(Arc::clone(&metrics));
                    let _ = serve_client(stream, config, metrics).await;
                });
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
        }
    }
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    Ok(())
}

async fn serve_client(
    stream: TcpStream,
    config: Arc<RtspEndpointConfig>,
    metrics: Arc<RtspServerMetrics>,
) -> Result<(), RtspServiceError> {
    let (mut reader, mut writer) = stream.into_split();
    let (outgoing, mut outbound) = mpsc::channel::<Vec<u8>>(config.client_write_queue);
    let writer_metrics = Arc::clone(&metrics);
    let mut writer_task = AbortTaskOnDrop::new(tokio::spawn(async move {
        while let Some(bytes) = outbound.recv().await {
            writer.write_all(&bytes).await?;
            writer_metrics
                .bytes_sent
                .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        writer.shutdown().await
    }));
    let session_id = uuid::Uuid::new_v4().simple().to_string();
    let mut session = RtspSession::new(session_id.clone());
    let mut source: Option<RtspStreamSource> = None;
    let mut channels = (0u8, 1u8);
    let client_rtp_state = next_client_rtp_state();
    let mut stream_task: Option<AbortTaskOnDrop<()>> = None;

    loop {
        let bytes = match read_request(&mut reader).await? {
            Some(bytes) => bytes,
            None => break,
        };
        let request = match parse_rtsp_request(&bytes) {
            Ok(request) => request,
            // A malformed request is answered and skipped. Legacy ignored what
            // it could not parse and kept serving, so one odd probe from a
            // platform must not drop an established video session.
            Err(_) => {
                send_response(
                    &outgoing,
                    build_rtsp_response(0, 400, "Bad Request", &[], &[]),
                )
                .await?;
                continue;
            }
        };
        let selected = request_path(&request.uri)
            .and_then(|path| config.resolve_route(path))
            .cloned();
        let route_switch_rejected = selected
            .as_ref()
            .zip(source.as_ref())
            .is_some_and(|(selected, active)| selected.stream_id != active.stream_id)
            && matches!(
                request.method,
                super::state::RtspMethod::Setup
                    | super::state::RtspMethod::Play
                    | super::state::RtspMethod::FastPlay
                    | super::state::RtspMethod::GetParameter
                    | super::state::RtspMethod::Teardown
            );
        let decision = if route_switch_rejected {
            RtspDecision::Error {
                status: 459,
                reason: "Aggregate Operation Not Allowed",
            }
        } else if selected.is_none() && !matches!(request.method, super::state::RtspMethod::Options)
        {
            RtspDecision::Error {
                status: 404,
                reason: "Not Found",
            }
        } else {
            session.handle(&request)
        };
        match decision {
            RtspDecision::Options => {
                send_response(
                    &outgoing,
                    build_rtsp_response(
                        request.cseq,
                        200,
                        "OK",
                        &[(
                            "Public",
                            "OPTIONS,DESCRIBE,SETUP,PLAY,FASTPLAY,PAUSE,TEARDOWN,ANNOUNCE,SET_PARAMETER,GET_PARAMETER",
                        )],
                        &[],
                    ),
                )
                .await?
            }
            RtspDecision::Describe => {
                let selected = selected.expect("selected route checked");
                source = Some(selected.clone());
                let content_base = content_base_for_request(config.bind_addr, &request.uri);
                send_response(
                    &outgoing,
                    build_rtsp_response(
                        request.cseq,
                        200,
                        "OK",
                        &[("Content-Base", &content_base), ("Content-Type", "application/sdp")],
                        &selected.sdp,
                    ),
                )
                .await?;
            }
            RtspDecision::SetupTcpInterleaved {
                rtp_channel,
                rtcp_channel,
            } => {
                let metadata_only = selected.as_ref().is_some_and(|source| source.metadata_only);
                if !metadata_only {
                    channels = (rtp_channel, rtcp_channel);
                    source = selected;
                }
                let transport = format!(
                    "RTP/AVP/TCP;unicast;interleaved={rtp_channel}-{rtcp_channel};ssrc={:x};mode=\"PLAY\"",
                    client_rtp_state.ssrc
                );
                send_response(
                    &outgoing,
                    build_rtsp_response(
                        request.cseq,
                        200,
                        "OK",
                        &[("Transport", &transport), ("Session", &session_id)],
                        &[],
                    ),
                )
                .await?;
            }
            RtspDecision::Play => {
                // A PLAY that never ran SETUP still streams, on the legacy
                // default interleaved channel pair, exactly as IPCRtsp did.
                let selected = source.clone().or(selected).ok_or_else(|| {
                    service_error(
                        "device_simulator.rtsp.route_missing",
                        "RTSP stream is missing",
                    )
                })?;
                source = Some(selected.clone());
                send_response(
                    &outgoing,
                    build_rtsp_response(request.cseq, 200, "OK", &[("Session", &session_id)], &[]),
                )
                .await?;
                if stream_task.is_none() && !selected.metadata_only {
                    stream_task = Some(AbortTaskOnDrop::new(tokio::spawn(stream_frames(
                        selected,
                        channels.0,
                        outgoing.clone(),
                        client_rtp_state,
                    ))));
                }
            }
            RtspDecision::FastPlay => {
                let selected = selected.expect("selected route checked");
                source = Some(selected.clone());
                let content_base = content_base_for_request(config.bind_addr, &request.uri);
                send_response(
                    &outgoing,
                    build_rtsp_response(
                        request.cseq,
                        200,
                        "OK",
                        &[("Content-Base", &content_base), ("Content-Type", "application/sdp")],
                        &selected.sdp,
                    ),
                )
                .await?;
                if stream_task.is_none() && !selected.metadata_only {
                    stream_task = Some(AbortTaskOnDrop::new(tokio::spawn(stream_frames(
                        selected,
                        channels.0,
                        outgoing.clone(),
                        client_rtp_state,
                    ))));
                }
            }
            RtspDecision::KeepAlive => {
                send_response(
                    &outgoing,
                    build_rtsp_response(request.cseq, 200, "OK", &[("Session", &session_id)], &[]),
                )
                .await?;
            }
            RtspDecision::Teardown => {
                send_response(
                    &outgoing,
                    build_rtsp_response(request.cseq, 200, "OK", &[], &[]),
                )
                .await?;
                break;
            }
            RtspDecision::Error { status, reason } => {
                send_response(
                    &outgoing,
                    build_rtsp_response(request.cseq, status, reason, &[], &[]),
                )
                .await?;
            }
        }
    }
    if let Some(mut task) = stream_task {
        task.abort();
        let _ = task.join().await;
    }
    drop(outgoing);
    writer_task
        .join()
        .await
        .map_err(|source| {
            service_error(
                "device_simulator.rtsp.writer_panicked",
                format!("RTSP client writer failed: {source}"),
            )
        })?
        .map_err(|source| {
            service_error(
                "device_simulator.rtsp.write_failed",
                format!("RTSP client write failed: {source}"),
            )
        })
}

async fn stream_frames(
    source: RtspStreamSource,
    channel: u8,
    outgoing: mpsc::Sender<Vec<u8>>,
    client_state: ClientRtpState,
) {
    let mut receiver = source.scheduler.subscribe();
    let mut packetizer = RtpPacketizer {
        payload_type: source.payload_type,
        ssrc: client_state.ssrc,
        next_sequence: client_state.next_sequence,
        max_payload_bytes: source.max_rtp_payload_bytes,
    };
    let mut waiting_for_keyframe = true;
    loop {
        match receiver.recv().await {
            Ok(ScheduledAccessUnit {
                timestamp,
                access_unit,
                ..
            }) => {
                if waiting_for_keyframe && !access_unit.keyframe {
                    continue;
                }
                waiting_for_keyframe = false;
                let nals = access_unit
                    .nals
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>();
                let Ok(packets) = packetizer.packetize_access_unit(
                    source.codec,
                    &nals,
                    timestamp.wrapping_add(client_state.timestamp_offset),
                ) else {
                    break;
                };
                let mut batch = Vec::new();
                for packet in packets {
                    let Ok(frame) = tcp_interleaved_frame(channel, &packet.bytes) else {
                        return;
                    };
                    batch.extend_from_slice(&frame);
                }
                if outgoing.try_send(batch).is_err() {
                    // A complete access unit is queued atomically. Slow or
                    // disconnected clients cannot block or receive a partial frame.
                    return;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                waiting_for_keyframe = true;
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn send_response(
    outgoing: &mpsc::Sender<Vec<u8>>,
    response: Vec<u8>,
) -> Result<(), RtspServiceError> {
    outgoing.send(response).await.map_err(|_| disconnected())
}

fn disconnected() -> RtspServiceError {
    service_error(
        "device_simulator.rtsp.client_disconnected",
        "RTSP client disconnected",
    )
}

async fn read_request<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Vec<u8>>, RtspServiceError> {
    let mut bytes = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    loop {
        let read = reader.read(&mut byte).await.map_err(|source| {
            service_error(
                "device_simulator.rtsp.read_failed",
                format!("RTSP client read failed: {source}"),
            )
        })?;
        if read == 0 {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err(service_error(
                    "device_simulator.rtsp.request_truncated",
                    "RTSP client disconnected during request headers",
                ))
            };
        }
        bytes.push(byte[0]);
        if bytes.len() == 1 && bytes[0] == b'$' {
            let mut interleaved_header = [0_u8; 3];
            reader
                .read_exact(&mut interleaved_header)
                .await
                .map_err(|source| {
                    service_error(
                        "device_simulator.rtsp.interleaved_truncated",
                        format!("client interleaved frame header is truncated: {source}"),
                    )
                })?;
            let length =
                u16::from_be_bytes([interleaved_header[1], interleaved_header[2]]) as usize;
            let mut payload = vec![0_u8; length];
            reader.read_exact(&mut payload).await.map_err(|source| {
                service_error(
                    "device_simulator.rtsp.interleaved_truncated",
                    format!("client interleaved frame payload is truncated: {source}"),
                )
            })?;
            bytes.clear();
            continue;
        }
        if bytes.len() > MAX_RTSP_REQUEST_BYTES {
            return Err(service_error(
                "device_simulator.rtsp.request_size_invalid",
                "RTSP request exceeds 64 KiB",
            ));
        }
        if bytes.ends_with(b"\r\n\r\n") {
            // GET_PARAMETER and SET_PARAMETER may carry a body. Draining it
            // here keeps the next request aligned with the socket; leaving it
            // behind would make the following parse fail on body text.
            let body_length = declared_body_length(&bytes);
            if body_length > 0 {
                let mut body = vec![0_u8; body_length];
                reader.read_exact(&mut body).await.map_err(|source| {
                    service_error(
                        "device_simulator.rtsp.request_truncated",
                        format!("RTSP request body is truncated: {source}"),
                    )
                })?;
                bytes.extend_from_slice(&body);
            }
            return Ok(Some(bytes));
        }
    }
}

fn request_path(uri: &str) -> Option<&str> {
    if uri == "*" {
        return Some("*");
    }
    // Query strings and fragments are addressing decoration, not part of the
    // stream path; strip them so a decorated URI resolves like the bare one.
    let uri = uri.split(['?', '#']).next().unwrap_or(uri);
    if uri.starts_with('/') {
        return valid_route(uri).then_some(uri);
    }
    let authority_and_path = uri.strip_prefix("rtsp://")?;
    let Some(slash) = authority_and_path.find('/') else {
        // live555 may use an authority-only Content-Base as the aggregate PLAY
        // target. Treat it as the listener root so legacy path-agnostic routing
        // can reuse the stream selected by DESCRIBE/SETUP instead of returning 404.
        return Some("/");
    };
    let path = &authority_and_path[slash..];
    valid_route(path).then_some(path)
}

fn content_base_for_request(bind_addr: SocketAddr, request_uri: &str) -> String {
    let path = request_path(request_uri)
        .filter(|path| !matches!(*path, "*" | "/"))
        .unwrap_or("/");
    format!("rtsp://{bind_addr}{path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::rtsp::scheduler::{SharedAccessUnit, SharedNal};
    use std::net::IpAddr;

    fn source() -> RtspStreamSource {
        let frames = vec![Arc::new(SharedAccessUnit {
            nals: vec![SharedNal::from_bytes([0x65, 1, 2].as_slice())].into(),
            keyframe: true,
        })]
        .into();
        RtspStreamSource {
            stream_id: "main".into(),
            sdp: Arc::from(
                b"v=0\r\nm=video 0 RTP/AVP 105\r\na=rtpmap:105 H264/90000\r\n".as_slice(),
            ),
            scheduler: SharedFrameScheduler::new(frames, 90_000, 25, 8).unwrap(),
            codec: Codec::H264,
            payload_type: 105,
            max_rtp_payload_bytes: 1_200,
            metadata_only: false,
        }
    }

    /// `read_request` already drains a declared body, so the response comes
    /// back with its SDP appended and framed.
    async fn read_response(client: &mut TcpStream) -> Vec<u8> {
        read_request(client)
            .await
            .unwrap()
            .expect("RTSP response headers")
    }

    async fn read_interleaved_rtp(client: &mut TcpStream) -> (u8, u16, u32, u32) {
        let mut interleaved = [0_u8; 4];
        client.read_exact(&mut interleaved).await.unwrap();
        assert_eq!(interleaved[0], b'$');
        let length = u16::from_be_bytes([interleaved[2], interleaved[3]]) as usize;
        let mut rtp = vec![0_u8; length];
        client.read_exact(&mut rtp).await.unwrap();
        let sequence = u16::from_be_bytes([rtp[2], rtp[3]]);
        let timestamp = u32::from_be_bytes([rtp[4], rtp[5], rtp[6], rtp[7]]);
        let ssrc = u32::from_be_bytes([rtp[8], rtp[9], rtp[10], rtp[11]]);
        (interleaved[1], sequence, timestamp, ssrc)
    }

    async fn connect_and_play(address: SocketAddr) -> (TcpStream, u16, u32, u32) {
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"DESCRIBE rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 1\r\n\r\n")
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));
        client
            .write_all(
                b"SETUP rtsp://127.0.0.1/media/video1/video RTSP/1.0\r\nCSeq: 2\r\nTransport: RTP/AVP/TCP;unicast;interleaved=2-3\r\n\r\n",
            )
            .await
            .unwrap();
        let setup = read_response(&mut client).await;
        let setup = String::from_utf8_lossy(&setup);
        assert!(setup.contains("200 OK"));
        assert!(setup.contains("ssrc=c8c750a"));
        client
            .write_all(b"PLAY rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 3\r\n\r\n")
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));

        let (channel, sequence, timestamp, ssrc) = read_interleaved_rtp(&mut client).await;
        assert_eq!(channel, 2);
        (client, sequence, timestamp, ssrc)
    }

    #[test]
    fn rejects_wildcard_bind_and_unapproved_route_shapes() {
        let config = RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([0, 0, 0, 0]), 554),
            routes: BTreeMap::from([("/media/video1".into(), source())]),
            client_write_queue: 8,
        };
        assert_eq!(
            config.validate().unwrap_err().code,
            "device_simulator.rtsp.bind_invalid"
        );
        assert_eq!(
            request_path("rtsp://192.0.2.2/media/video1"),
            Some("/media/video1")
        );
        assert_eq!(request_path("rtsp://192.0.2.2:554"), Some("/"));
        assert_eq!(request_path("/../secret"), None);
    }

    #[tokio::test]
    async fn vlc_aggregate_play_without_path_reuses_the_selected_video_stream() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let shared_source = source();
        let server = start_rtsp_server(RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([
                ("/media/video1".into(), shared_source.clone()),
                ("/media/video1/video".into(), shared_source),
            ]),
            client_write_queue: 16,
        })
        .await
        .unwrap();
        let mut client = TcpStream::connect(server.local_addr()).await.unwrap();

        client
            .write_all(
                format!(
                    "DESCRIBE rtsp://127.0.0.1:{port}/media/video1 RTSP/1.0\r\nCSeq: 1\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let describe = String::from_utf8_lossy(&read_response(&mut client).await).into_owned();
        assert!(describe.contains("RTSP/1.0 200 OK"));
        assert!(describe.contains(&format!(
            "Content-Base: rtsp://127.0.0.1:{port}/media/video1"
        )));

        client
            .write_all(
                format!(
                    "SETUP rtsp://127.0.0.1:{port}/media/video1/video RTSP/1.0\r\nCSeq: 2\r\nTransport: RTP/AVP/TCP;unicast;interleaved=0-1\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));

        // live555 uses the aggregate Content-Base for PLAY and some versions
        // omit its trailing slash. The listener must still start RTP.
        client
            .write_all(
                format!("PLAY rtsp://127.0.0.1:{port} RTSP/1.0\r\nCSeq: 3\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));
        let (channel, _, _, _) =
            tokio::time::timeout(Duration::from_secs(1), read_interleaved_rtp(&mut client))
                .await
                .expect("VLC-compatible aggregate PLAY starts interleaved RTP");
        assert_eq!(channel, 0);

        drop(client);
        server.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn ignores_bounded_client_interleaved_frames_between_rtsp_requests() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        client
            .write_all(b"$\x01\x00\x04rtcpOPTIONS * RTSP/1.0\r\nCSeq: 9\r\n\r\n")
            .await
            .unwrap();
        let request = read_request(&mut server).await.unwrap().unwrap();
        assert!(request.starts_with(b"OPTIONS * RTSP/1.0"));
    }

    #[tokio::test]
    async fn server_owns_one_producer_per_cloned_route_and_stops_it() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let shared_source = source();
        let scheduler_probe = shared_source.scheduler.clone();
        let server = start_rtsp_server(RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([
                ("/media/video1".into(), shared_source.clone()),
                ("/media/video1/video".into(), shared_source),
            ]),
            client_write_queue: 16,
        })
        .await
        .unwrap();

        let (mut client, _, first_timestamp, _) = tokio::time::timeout(
            Duration::from_secs(1),
            connect_and_play(server.local_addr()),
        )
        .await
        .expect("server-owned scheduler producer sends an interleaved RTP frame");
        let mut previous_timestamp = first_timestamp;
        for _ in 0..3 {
            let (_, _, timestamp, _) =
                tokio::time::timeout(Duration::from_secs(1), read_interleaved_rtp(&mut client))
                    .await
                    .expect("server-owned scheduler producer keeps sending RTP frames");
            assert_eq!(timestamp.wrapping_sub(previous_timestamp), 3_600);
            previous_timestamp = timestamp;
        }
        drop(client);

        server.stop(Duration::from_secs(2)).await.unwrap();
        let mut after_stop = scheduler_probe.subscribe();
        assert!(
            tokio::time::timeout(Duration::from_millis(150), after_stop.recv())
                .await
                .is_err(),
            "scheduler producer must stop before RtspServerHandle::stop returns"
        );
        TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    }

    #[tokio::test]
    async fn legacy_fastplay_returns_sdp_and_starts_interleaved_rtp_without_setup() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let server = start_rtsp_server(RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([("/media/video1".into(), source())]),
            client_write_queue: 16,
        })
        .await
        .unwrap();
        let mut client = TcpStream::connect(server.local_addr()).await.unwrap();
        client
            .write_all(b"FASTPLAY rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 1\r\n\r\n")
            .await
            .unwrap();

        let response = String::from_utf8_lossy(&read_response(&mut client).await).into_owned();
        assert!(response.contains("RTSP/1.0 200 OK"));
        assert!(response.contains("Content-Type: application/sdp"));
        assert!(response.contains("m=video 0 RTP/AVP 105"));
        let (channel, _, _, _) =
            tokio::time::timeout(Duration::from_secs(1), read_interleaved_rtp(&mut client))
                .await
                .expect("legacy FASTPLAY starts the RTP stream without SETUP");
        assert_eq!(channel, 0);

        drop(client);
        server.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn legacy_path_agnostic_routing_keeps_unknown_urls_and_verbs_playing() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let server = start_rtsp_server(RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([("/unicast/c1/s0/live".into(), source())]),
            client_write_queue: 16,
        })
        .await
        .unwrap();
        let mut client = TcpStream::connect(server.local_addr()).await.unwrap();

        // An NVR channel the plan never registered, carrying a query string.
        client
            .write_all(
                b"DESCRIBE rtsp://127.0.0.1/unicast/c7/s0/live?tcp RTSP/1.0\r\nCSeq: 1\r\n\r\n",
            )
            .await
            .unwrap();
        let describe = String::from_utf8_lossy(&read_response(&mut client).await).into_owned();
        assert!(describe.contains("RTSP/1.0 200 OK"));
        assert!(describe.contains("m=video 0 RTP/AVP 105"));

        // An unhandled verb with a body falls back to the SDP without
        // desynchronising the next request.
        client
            .write_all(
                b"SET_PARAMETER rtsp://127.0.0.1/unicast/c7/s0/live RTSP/1.0\r\nCSeq: 2\r\nContent-Length: 5\r\n\r\nhello",
            )
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));

        // PLAY without a preceding SETUP still streams, on channel 0.
        client
            .write_all(b"PLAY rtsp://127.0.0.1/unicast/c7/s0/live RTSP/1.0\r\nCSeq: 3\r\n\r\n")
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));
        let (channel, _, _, _) =
            tokio::time::timeout(Duration::from_secs(1), read_interleaved_rtp(&mut client))
                .await
                .expect("an unregistered channel still streams the listener's media");
        assert_eq!(channel, 0);

        drop(client);
        server.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn stop_joins_remaining_producers_after_a_task_failure() {
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let cleanup_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleanup_finished_task = Arc::clone(&cleanup_finished);
        let failed_producer = tokio::spawn(std::future::pending::<()>());
        failed_producer.abort();
        let cleanup_producer = tokio::spawn(async move {
            shutdown_rx.changed().await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            cleanup_finished_task.store(true, Ordering::Release);
        });
        let server = RtspServerHandle {
            local_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 554),
            shutdown,
            join: AbortTaskOnDrop::new(tokio::spawn(async { Ok::<(), RtspServiceError>(()) })),
            producer_tasks: vec![
                AbortTaskOnDrop::new(failed_producer),
                AbortTaskOnDrop::new(cleanup_producer),
            ],
            metrics: Arc::new(RtspServerMetrics::default()),
        };

        let error = server.stop(Duration::from_secs(1)).await.unwrap_err();
        assert_eq!(error.code, "device_simulator.rtsp.task_panicked");
        assert!(
            cleanup_finished.load(Ordering::Acquire),
            "stop must join every producer even after an earlier join fails"
        );
    }

    #[tokio::test]
    async fn server_handles_options_and_releases_port_after_bounded_stop() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let config = RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([("/media/video1".into(), source())]),
            client_write_queue: 8,
        };
        let server = start_rtsp_server(config).await.unwrap();
        let mut client = TcpStream::connect(server.local_addr()).await.unwrap();
        client
            .write_all(b"OPTIONS * RTSP/1.0\r\nCSeq: 1\r\n\r\n")
            .await
            .unwrap();
        let mut response = [0u8; 512];
        let read = client.read(&mut response).await.unwrap();
        assert!(String::from_utf8_lossy(&response[..read]).contains("RTSP/1.0 200 OK"));
        let stats = server.stats();
        assert_eq!(stats.active_clients, 1);
        assert!(stats.bytes_sent >= read as u64);
        drop(client);
        server.stop(Duration::from_secs(2)).await.unwrap();
        TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    }

    #[tokio::test]
    async fn clients_keep_independent_sequences_with_legacy_ssrc_and_bounded_cleanup() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let shared_source = source();
        let config = RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([
                ("/media/video1".into(), shared_source.clone()),
                ("/media/video1/video".into(), shared_source),
            ]),
            client_write_queue: 16,
        };
        let server = start_rtsp_server(config).await.unwrap();

        let (first, first_sequence, first_timestamp, first_ssrc) =
            connect_and_play(server.local_addr()).await;
        let (second, second_sequence, second_timestamp, second_ssrc) =
            connect_and_play(server.local_addr()).await;
        assert_eq!(first_ssrc, LEGACY_RTP_SSRC);
        assert_eq!(second_ssrc, LEGACY_RTP_SSRC);
        assert_ne!(first_sequence, second_sequence);
        assert_ne!(first_timestamp, second_timestamp);
        drop(first);
        drop(second);

        let (mut reconnected, _, _, reconnect_ssrc) = connect_and_play(server.local_addr()).await;
        assert_eq!(reconnect_ssrc, LEGACY_RTP_SSRC);
        server.stop(Duration::from_secs(2)).await.unwrap();
        let mut trailing = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(1),
            reconnected.read_to_end(&mut trailing),
        )
        .await
        .expect("active client closes after server cancellation")
        .expect("read connection shutdown");
        TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    }

    #[tokio::test]
    async fn metadata_setup_is_accepted_without_replacing_the_video_track() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let shared_source = source();
        let metadata_source = RtspStreamSource {
            metadata_only: true,
            ..shared_source.clone()
        };
        let server = start_rtsp_server(RtspEndpointConfig {
            bind_addr: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            routes: BTreeMap::from([
                ("/media/video1".into(), shared_source.clone()),
                ("/media/video1/video".into(), shared_source),
                ("/media/video1/metadata".into(), metadata_source),
            ]),
            client_write_queue: 16,
        })
        .await
        .unwrap();

        let mut client = TcpStream::connect(server.local_addr()).await.unwrap();
        for request in [
            "DESCRIBE rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 1\r\n\r\n",
            "SETUP rtsp://127.0.0.1/media/video1/video RTSP/1.0\r\nCSeq: 2\r\nTransport: RTP/AVP/TCP;unicast;interleaved=0-1\r\n\r\n",
            "SETUP rtsp://127.0.0.1/media/video1/metadata RTSP/1.0\r\nCSeq: 3\r\nTransport: RTP/AVP/TCP;unicast;interleaved=2-3\r\n\r\n",
            "PLAY rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 4\r\n\r\n",
        ] {
            client.write_all(request.as_bytes()).await.unwrap();
            assert!(String::from_utf8_lossy(&read_response(&mut client).await).contains("200 OK"));
        }
        let mut interleaved = [0_u8; 4];
        client.read_exact(&mut interleaved).await.unwrap();
        assert_eq!(&interleaved[..2], b"$\0");

        drop(client);
        server.stop(Duration::from_secs(2)).await.unwrap();
    }
}
