//! Receive-only WebRTC transport for the LAN screen-share viewer.
//!
//! HTTP authentication, body limits and the global viewer/IP lease are owned by
//! `screenshare.rs`. This module only manages peer/media state and packetizes the
//! shared H.264 access units; it never creates an encoder of its own.

use axum::http::StatusCode;
use bytes::Bytes;
use rtcp::payload_feedbacks::full_intra_request::FullIntraRequest;
use rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use rtcp::transport_feedbacks::transport_layer_cc::TransportLayerCc;
use rtcp::transport_feedbacks::transport_layer_nack::TransportLayerNack;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, watch};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264};
use webrtc::api::APIBuilder;
use webrtc::interceptor::registry::Registry;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpHeaderExtensionCapability, RTPCodecType,
};
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::TrackLocal;
use webrtc::util::{Marshal, MarshalSize};

use crate::screenshare_media::{
    H264KeyframeRequestResult, H264MediaEvent, H264MediaSegment, H264MediaState,
    H264StreamDescriptor,
};

// Match the current media-viewer soft ceiling so the M4 prototype can execute
// the same 30-viewer matrix as MSE instead of failing before the comparison.
const MAX_WEBRTC_PEERS: u32 = 40;
const MAX_SIGNALING_BYTES: usize = 256 * 1024;
const ICE_GATHER_TIMEOUT: Duration = Duration::from_secs(5);
const PEER_INITIAL_CONNECTION_TIMEOUT: Duration = Duration::from_secs(15);
const PEER_DISCONNECTED_GRACE: Duration = Duration::from_secs(5);
const PEER_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const MEDIA_SEND_TIMEOUT: Duration = Duration::from_secs(1);
const METRIC_SAMPLE_LIMIT: usize = 256;
const METRIC_MEASUREMENT_SCOPE: &str = "cumulative_count_with_rolling_distribution";
const ACCESS_UNIT_CACHE_LIMIT: usize = 192;
const H264_RTP_FMTP: &str =
    "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f";
/// WebRTC's experimental Absolute Capture Time RTP header extension. Chromium
/// advertises this URI when it can map a remote capture clock into
/// requestVideoFrameCallback metadata. The eight-byte mandatory field is an
/// unsigned 64-bit UQ32.32 NTP timestamp. This is not RFC 8872 (that RFC covers
/// RTP multi-stream multiplexing).
const ABSOLUTE_CAPTURE_TIME_URI: &str =
    "http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AbsoluteCaptureTimeExtension {
    capture_timestamp_ntp: u64,
}

impl AbsoluteCaptureTimeExtension {
    fn from_system_time(captured_at: SystemTime) -> Self {
        Self {
            capture_timestamp_ntp: webrtc::rtp::extension::abs_send_time_extension::unix2ntp(
                captured_at,
            ),
        }
    }
}

impl MarshalSize for AbsoluteCaptureTimeExtension {
    fn marshal_size(&self) -> usize {
        std::mem::size_of::<u64>()
    }
}

impl Marshal for AbsoluteCaptureTimeExtension {
    fn marshal_to(&self, buffer: &mut [u8]) -> webrtc::util::Result<usize> {
        if buffer.len() < self.marshal_size() {
            return Err(webrtc::util::Error::ErrBufferShort);
        }
        buffer[..8].copy_from_slice(&self.capture_timestamp_ntp.to_be_bytes());
        Ok(8)
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct WebRtcDistributionSnapshot {
    pub sample_count: u64,
    pub retained_sample_count: u32,
    pub retained_sample_capacity: u32,
    pub measurement_scope: &'static str,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct WebRtcMetricsSnapshot {
    pub active_peers: u32,
    pub peer_limit: u32,
    pub offers_accepted: u64,
    pub offers_rejected: u64,
    pub peers_connected: u64,
    pub peers_disconnected: u64,
    pub peers_failed: u64,
    pub initial_connection_timeouts: u64,
    pub disconnected_timeouts: u64,
    /// Encoded H.264 access-unit bytes handed to the RTP packetizer.
    pub media_payload_bytes_sent: u64,
    pub media_samples_sent: u64,
    pub media_send_timeouts: u64,
    pub media_send_errors: u64,
    pub media_source_lag_disconnects: u64,
    /// True when the server MediaEngine is configured for Absolute Capture Time.
    pub absolute_capture_time_extension_registered: bool,
    /// Accepted offers whose final answer negotiated the experimental URI.
    pub absolute_capture_time_offers_negotiated: u64,
    /// Access units successfully handed to a track with the extension negotiated.
    pub absolute_capture_time_samples_sent: u64,
    pub rtcp_packets_received: u64,
    pub nack_packets_received: u64,
    pub transport_cc_packets_received: u64,
    pub transport_cc_status_count: u64,
    pub transport_cc_received_delta_count: u64,
    pub pli_packets_received: u64,
    pub pli_keyframe_scheduled: u64,
    pub pli_keyframe_coalesced: u64,
    pub pli_keyframe_stale: u64,
    pub keyframe_recoveries: u64,
    /// Milliseconds from the first outstanding PLI/FIR to the next IDR sent.
    pub keyframe_recovery_ms: WebRtcDistributionSnapshot,
    /// Milliseconds from source capture wall-clock metadata to RTP packetization.
    pub capture_to_packetizer_ms: WebRtcDistributionSnapshot,
    pub latest_capture_sequence: u64,
    pub latest_source_timestamp_us: u64,
}

#[derive(Default)]
struct BoundedSamples {
    values: Mutex<VecDeque<u64>>,
    total_count: AtomicU64,
}

impl BoundedSamples {
    fn record(&self, value: u64) {
        self.total_count.fetch_add(1, Ordering::Relaxed);
        let Ok(mut values) = self.values.lock() else {
            return;
        };
        if values.len() >= METRIC_SAMPLE_LIMIT {
            values.pop_front();
        }
        values.push_back(value);
    }

    fn snapshot(&self) -> WebRtcDistributionSnapshot {
        let mut values = self
            .values
            .lock()
            .map(|values| values.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        if values.is_empty() {
            return WebRtcDistributionSnapshot {
                sample_count: self.total_count.load(Ordering::Relaxed),
                retained_sample_capacity: METRIC_SAMPLE_LIMIT as u32,
                measurement_scope: METRIC_MEASUREMENT_SCOPE,
                ..Default::default()
            };
        }
        values.sort_unstable();
        WebRtcDistributionSnapshot {
            sample_count: self.total_count.load(Ordering::Relaxed),
            retained_sample_count: values.len().min(u32::MAX as usize) as u32,
            retained_sample_capacity: METRIC_SAMPLE_LIMIT as u32,
            measurement_scope: METRIC_MEASUREMENT_SCOPE,
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
struct WebRtcMetrics {
    offers_accepted: AtomicU64,
    offers_rejected: AtomicU64,
    peers_connected: AtomicU64,
    peers_disconnected: AtomicU64,
    peers_failed: AtomicU64,
    initial_connection_timeouts: AtomicU64,
    disconnected_timeouts: AtomicU64,
    media_payload_bytes_sent: AtomicU64,
    media_samples_sent: AtomicU64,
    media_send_timeouts: AtomicU64,
    media_send_errors: AtomicU64,
    media_source_lag_disconnects: AtomicU64,
    absolute_capture_time_offers_negotiated: AtomicU64,
    absolute_capture_time_samples_sent: AtomicU64,
    rtcp_packets_received: AtomicU64,
    nack_packets_received: AtomicU64,
    transport_cc_packets_received: AtomicU64,
    transport_cc_status_count: AtomicU64,
    transport_cc_received_delta_count: AtomicU64,
    pli_packets_received: AtomicU64,
    pli_keyframe_scheduled: AtomicU64,
    pli_keyframe_coalesced: AtomicU64,
    pli_keyframe_stale: AtomicU64,
    keyframe_recoveries: AtomicU64,
    keyframe_recovery_ms: BoundedSamples,
    capture_to_packetizer_ms: BoundedSamples,
    latest_capture_sequence: AtomicU64,
    latest_source_timestamp_us: AtomicU64,
}

struct PeerRecord {
    connection: Arc<RTCPeerConnection>,
    shutdown: watch::Sender<bool>,
    /// Owned by the peer for its full lifetime. The main screen-share router
    /// stores its global viewer/IP guard here so all media transports share one
    /// aggregate viewer ceiling and release path.
    viewer_lease: Option<Box<dyn Send + Sync>>,
}

#[derive(Clone)]
struct AccessUnitCacheEntry {
    generation: u64,
    sequence: u64,
    bytes: Arc<Bytes>,
}

pub struct WebRtcTransportState {
    media: Arc<H264MediaState>,
    peers: Mutex<HashMap<u64, PeerRecord>>,
    reserved_peer_slots: AtomicU32,
    next_peer_id: AtomicU64,
    metrics: WebRtcMetrics,
    access_unit_cache: Mutex<VecDeque<AccessUnitCacheEntry>>,
}

impl WebRtcTransportState {
    pub fn new(media: Arc<H264MediaState>) -> Arc<Self> {
        Arc::new(Self {
            media,
            peers: Mutex::new(HashMap::new()),
            reserved_peer_slots: AtomicU32::new(0),
            next_peer_id: AtomicU64::new(1),
            metrics: WebRtcMetrics::default(),
            access_unit_cache: Mutex::new(VecDeque::new()),
        })
    }

    pub fn metrics_snapshot(&self) -> WebRtcMetricsSnapshot {
        WebRtcMetricsSnapshot {
            active_peers: self.reserved_peer_slots.load(Ordering::Relaxed),
            peer_limit: MAX_WEBRTC_PEERS,
            offers_accepted: self.metrics.offers_accepted.load(Ordering::Relaxed),
            offers_rejected: self.metrics.offers_rejected.load(Ordering::Relaxed),
            peers_connected: self.metrics.peers_connected.load(Ordering::Relaxed),
            peers_disconnected: self.metrics.peers_disconnected.load(Ordering::Relaxed),
            peers_failed: self.metrics.peers_failed.load(Ordering::Relaxed),
            initial_connection_timeouts: self
                .metrics
                .initial_connection_timeouts
                .load(Ordering::Relaxed),
            disconnected_timeouts: self.metrics.disconnected_timeouts.load(Ordering::Relaxed),
            media_payload_bytes_sent: self
                .metrics
                .media_payload_bytes_sent
                .load(Ordering::Relaxed),
            media_samples_sent: self.metrics.media_samples_sent.load(Ordering::Relaxed),
            media_send_timeouts: self.metrics.media_send_timeouts.load(Ordering::Relaxed),
            media_send_errors: self.metrics.media_send_errors.load(Ordering::Relaxed),
            media_source_lag_disconnects: self
                .metrics
                .media_source_lag_disconnects
                .load(Ordering::Relaxed),
            absolute_capture_time_extension_registered: true,
            absolute_capture_time_offers_negotiated: self
                .metrics
                .absolute_capture_time_offers_negotiated
                .load(Ordering::Relaxed),
            absolute_capture_time_samples_sent: self
                .metrics
                .absolute_capture_time_samples_sent
                .load(Ordering::Relaxed),
            rtcp_packets_received: self.metrics.rtcp_packets_received.load(Ordering::Relaxed),
            nack_packets_received: self.metrics.nack_packets_received.load(Ordering::Relaxed),
            transport_cc_packets_received: self
                .metrics
                .transport_cc_packets_received
                .load(Ordering::Relaxed),
            transport_cc_status_count: self
                .metrics
                .transport_cc_status_count
                .load(Ordering::Relaxed),
            transport_cc_received_delta_count: self
                .metrics
                .transport_cc_received_delta_count
                .load(Ordering::Relaxed),
            pli_packets_received: self.metrics.pli_packets_received.load(Ordering::Relaxed),
            pli_keyframe_scheduled: self.metrics.pli_keyframe_scheduled.load(Ordering::Relaxed),
            pli_keyframe_coalesced: self.metrics.pli_keyframe_coalesced.load(Ordering::Relaxed),
            pli_keyframe_stale: self.metrics.pli_keyframe_stale.load(Ordering::Relaxed),
            keyframe_recoveries: self.metrics.keyframe_recoveries.load(Ordering::Relaxed),
            keyframe_recovery_ms: self.metrics.keyframe_recovery_ms.snapshot(),
            capture_to_packetizer_ms: self.metrics.capture_to_packetizer_ms.snapshot(),
            latest_capture_sequence: self.metrics.latest_capture_sequence.load(Ordering::Relaxed),
            latest_source_timestamp_us: self
                .metrics
                .latest_source_timestamp_us
                .load(Ordering::Relaxed),
        }
    }

    pub async fn answer_offer_with_lease(
        self: &Arc<Self>,
        offer: RTCSessionDescription,
        viewer_lease: Option<Box<dyn Send + Sync>>,
    ) -> Result<RTCSessionDescription, WebRtcOfferError> {
        if offer.sdp_type != RTCSdpType::Offer {
            self.metrics.offers_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(WebRtcOfferError::bad_request(
                "WebRTC signaling requires an SDP offer",
            ));
        }
        if offer.sdp.len() > MAX_SIGNALING_BYTES {
            self.metrics.offers_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(WebRtcOfferError::bad_request("SDP offer is too large"));
        }
        if self.media.descriptor().is_none() {
            self.metrics.offers_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(WebRtcOfferError::unavailable("H.264 encoder is not ready"));
        }
        if !self.try_reserve_peer_slot() {
            self.metrics.offers_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(WebRtcOfferError::too_many_peers());
        }

        match self.answer_reserved_offer(offer, viewer_lease).await {
            Ok(answer) => {
                self.metrics.offers_accepted.fetch_add(1, Ordering::Relaxed);
                Ok(answer)
            }
            Err(error) => {
                self.release_peer_slot();
                self.metrics.offers_rejected.fetch_add(1, Ordering::Relaxed);
                Err(error)
            }
        }
    }

    pub async fn shutdown_all(self: &Arc<Self>) {
        let peer_ids = self
            .peers
            .lock()
            .map(|peers| peers.keys().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut shutdowns = tokio::task::JoinSet::new();
        for peer_id in peer_ids {
            let state = Arc::clone(self);
            shutdowns.spawn(async move { terminate_peer(&state, peer_id, false).await });
        }
        while shutdowns.join_next().await.is_some() {
            // Each close has its own deadline; joining concurrently keeps the
            // session-wide shutdown bounded even at the 40-peer ceiling.
        }
    }

    async fn answer_reserved_offer(
        self: &Arc<Self>,
        offer: RTCSessionDescription,
        viewer_lease: Option<Box<dyn Send + Sync>>,
    ) -> Result<RTCSessionDescription, WebRtcOfferError> {
        let api = build_api().map_err(WebRtcOfferError::internal)?;
        let connection = Arc::new(
            api.new_peer_connection(RTCConfiguration {
                ice_servers: Vec::new(),
                ..Default::default()
            })
            .await
            .map_err(|error| WebRtcOfferError::internal(error.to_string()))?,
        );

        let result = self
            .configure_peer_connection(Arc::clone(&connection), offer, viewer_lease)
            .await;
        if result.is_err() {
            let _ = connection.close().await;
        }
        result
    }

    async fn configure_peer_connection(
        self: &Arc<Self>,
        connection: Arc<RTCPeerConnection>,
        offer: RTCSessionDescription,
        viewer_lease: Option<Box<dyn Send + Sync>>,
    ) -> Result<RTCSessionDescription, WebRtcOfferError> {
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_owned(),
                clock_rate: 90_000,
                sdp_fmtp_line: H264_RTP_FMTP.to_owned(),
                ..Default::default()
            },
            "screen".to_owned(),
            "file-sync-tool".to_owned(),
        ));
        let sender = connection
            .add_track(Arc::clone(&track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|error| WebRtcOfferError::bad_request(error.to_string()))?;

        connection
            .set_remote_description(offer)
            .await
            .map_err(|error| WebRtcOfferError::bad_request(error.to_string()))?;
        let answer = connection
            .create_answer(None)
            .await
            .map_err(|error| WebRtcOfferError::bad_request(error.to_string()))?;
        let mut gather_complete = connection.gathering_complete_promise().await;
        connection
            .set_local_description(answer)
            .await
            .map_err(|error| WebRtcOfferError::internal(error.to_string()))?;
        if tokio::time::timeout(ICE_GATHER_TIMEOUT, gather_complete.recv())
            .await
            .is_err()
        {
            return Err(WebRtcOfferError::unavailable(
                "timed out gathering LAN ICE candidates",
            ));
        }
        let local_description = connection
            .local_description()
            .await
            .ok_or_else(|| WebRtcOfferError::internal("missing local SDP answer"))?;
        let absolute_capture_time_negotiated =
            sdp_negotiates_extension(&local_description.sdp, ABSOLUTE_CAPTURE_TIME_URI);

        // Subscribe before reading the descriptor so reset/segment events cannot
        // be missed between those two operations.
        let media_events = self.media.subscribe();
        let descriptor = self
            .media
            .descriptor()
            .ok_or_else(|| WebRtcOfferError::unavailable("H.264 encoder stopped"))?;
        let peer_id = self.next_peer_id.fetch_add(1, Ordering::Relaxed).max(1);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (connected_tx, connected_rx) = watch::channel(false);

        install_connection_state_handler(
            &connection,
            Arc::downgrade(self),
            peer_id,
            connected_tx,
            shutdown_tx.clone(),
        );

        self.peers
            .lock()
            .map_err(|_| WebRtcOfferError::internal("WebRTC peer registry is poisoned"))?
            .insert(
                peer_id,
                PeerRecord {
                    connection,
                    shutdown: shutdown_tx,
                    viewer_lease,
                },
            );
        if absolute_capture_time_negotiated {
            self.metrics
                .absolute_capture_time_offers_negotiated
                .fetch_add(1, Ordering::Relaxed);
        }

        let current_generation = Arc::new(AtomicU64::new(descriptor.generation));
        let pending_pli = Arc::new(Mutex::new(None::<Instant>));
        tokio::spawn(run_media_peer(
            Arc::clone(self),
            peer_id,
            track,
            descriptor,
            media_events,
            connected_rx,
            shutdown_rx.clone(),
            Arc::clone(&current_generation),
            Arc::clone(&pending_pli),
            absolute_capture_time_negotiated,
        ));
        tokio::spawn(run_rtcp_peer(
            Arc::clone(self),
            peer_id,
            sender,
            shutdown_rx,
            current_generation,
            pending_pli,
        ));

        Ok(local_description)
    }

    fn try_reserve_peer_slot(&self) -> bool {
        let mut current = self.reserved_peer_slots.load(Ordering::Relaxed);
        loop {
            if current >= MAX_WEBRTC_PEERS {
                return false;
            }
            match self.reserved_peer_slots.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    fn release_peer_slot(&self) {
        let _ =
            self.reserved_peer_slots
                .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |value| {
                    value.checked_sub(1)
                });
    }

    fn access_unit(
        &self,
        descriptor: &H264StreamDescriptor,
        segment: &H264MediaSegment,
    ) -> Result<Arc<Bytes>, String> {
        let mut cache = self
            .access_unit_cache
            .lock()
            .map_err(|_| "WebRTC access-unit cache is poisoned".to_owned())?;
        if let Some(entry) = cache.iter().find(|entry| {
            entry.generation == segment.generation && entry.sequence == segment.sequence
        }) {
            return Ok(Arc::clone(&entry.bytes));
        }

        let parameter_sets = if segment.keyframe {
            avcc_parameter_sets(&descriptor.init_segment).unwrap_or_default()
        } else {
            Vec::new()
        };
        let bytes = Arc::new(Bytes::from(avcc_to_annex_b(
            segment.access_unit_avcc.as_ref(),
            segment.keyframe.then_some(parameter_sets.as_slice()),
        )?));
        if cache.len() >= ACCESS_UNIT_CACHE_LIMIT {
            cache.pop_front();
        }
        cache.push_back(AccessUnitCacheEntry {
            generation: segment.generation,
            sequence: segment.sequence,
            bytes: Arc::clone(&bytes),
        });
        Ok(bytes)
    }
}

fn build_api() -> Result<webrtc::api::API, String> {
    let mut media_engine = MediaEngine::default();
    media_engine
        .register_default_codecs()
        .map_err(|error| error.to_string())?;
    media_engine
        .register_header_extension(
            RTCRtpHeaderExtensionCapability {
                uri: ABSOLUTE_CAPTURE_TIME_URI.to_owned(),
            },
            RTPCodecType::Video,
            None,
        )
        .map_err(|error| error.to_string())?;
    // The default registry configures NACK/reports but only installs the TWCC
    // receiver interceptor. This endpoint sends RTP, so it also needs the TWCC
    // sender interceptor to populate transport-wide sequence extensions and let
    // Chromium return actionable transport-cc feedback.
    let mut registry = register_default_interceptors(Registry::new(), &mut media_engine)
        .map_err(|error| error.to_string())?;
    registry.add(Box::new(
        webrtc::interceptor::twcc::sender::Sender::builder(),
    ));
    Ok(APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build())
}

fn sdp_negotiates_extension(sdp: &str, expected_uri: &str) -> bool {
    sdp.lines().any(|line| {
        let Some(extension) = line.trim_end_matches('\r').strip_prefix("a=extmap:") else {
            return false;
        };
        extension.split_ascii_whitespace().nth(1) == Some(expected_uri)
    })
}

fn install_connection_state_handler(
    connection: &Arc<RTCPeerConnection>,
    state: Weak<WebRtcTransportState>,
    peer_id: u64,
    connected: watch::Sender<bool>,
    shutdown: watch::Sender<bool>,
) {
    // The callback retains only a weak transport state, not the peer
    // connection itself, so removing the peer record breaks the lifecycle.
    connection.on_peer_connection_state_change(Box::new(move |connection_state| {
        let state = state.clone();
        let connected = connected.clone();
        let shutdown = shutdown.clone();
        Box::pin(async move {
            match connection_state {
                RTCPeerConnectionState::Connected => {
                    let _ = connected.send(true);
                    if let Some(state) = state.upgrade() {
                        state
                            .metrics
                            .peers_connected
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
                RTCPeerConnectionState::Disconnected => {
                    let _ = connected.send(false);
                    if let Some(state) = state.upgrade() {
                        state
                            .metrics
                            .peers_disconnected
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
                RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                    let _ = shutdown.send(true);
                    if let Some(state) = state.upgrade() {
                        if connection_state == RTCPeerConnectionState::Failed {
                            state.metrics.peers_failed.fetch_add(1, Ordering::Relaxed);
                        }
                        terminate_peer(&state, peer_id, false).await;
                    }
                }
                _ => {}
            }
        })
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaGateDecision {
    Ignore,
    Send,
    RequestKeyframe,
}

#[derive(Debug, Clone, Copy)]
struct MediaGate {
    generation: u64,
    next_sequence: Option<u64>,
}

#[derive(Debug, Default)]
struct RtpSourceTimeline {
    last_generation: Option<u64>,
    last_timestamp_us: u64,
    last_duration_us: u64,
    last_captured_at_unix_ms: u64,
}

impl RtpSourceTimeline {
    fn gap_before(&mut self, segment: &H264MediaSegment) -> Option<Duration> {
        let elapsed_us = self.last_generation.and_then(|generation| {
            if generation == segment.generation && segment.timestamp_us > self.last_timestamp_us {
                Some(segment.timestamp_us - self.last_timestamp_us)
            } else if segment.captured_at_unix_ms > self.last_captured_at_unix_ms {
                Some(
                    (segment.captured_at_unix_ms - self.last_captured_at_unix_ms)
                        .saturating_mul(1_000),
                )
            } else {
                None
            }
        });
        let previous_duration_us = self.last_duration_us;
        let gap_us = elapsed_us
            .unwrap_or_default()
            .saturating_sub(previous_duration_us);
        self.last_generation = Some(segment.generation);
        self.last_timestamp_us = segment.timestamp_us;
        self.last_duration_us = segment.duration_us.max(1);
        self.last_captured_at_unix_ms = segment.captured_at_unix_ms;
        let discontinuity_threshold_us = previous_duration_us.saturating_div(2).max(1_000);
        (gap_us >= discontinuity_threshold_us)
            .then(|| Duration::from_micros(gap_us.min(60_000_000)))
    }
}

impl MediaGate {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            next_sequence: None,
        }
    }

    fn reset(&mut self, generation: u64) {
        self.generation = generation;
        self.next_sequence = None;
    }

    fn observe(&mut self, segment: &H264MediaSegment) -> MediaGateDecision {
        if segment.generation != self.generation {
            return MediaGateDecision::Ignore;
        }
        match self.next_sequence {
            None if !segment.keyframe => MediaGateDecision::Ignore,
            None => {
                self.next_sequence = Some(segment.sequence.saturating_add(1));
                MediaGateDecision::Send
            }
            Some(expected) if segment.sequence == expected => {
                self.next_sequence = Some(segment.sequence.saturating_add(1));
                MediaGateDecision::Send
            }
            Some(_) if segment.keyframe => {
                self.next_sequence = Some(segment.sequence.saturating_add(1));
                MediaGateDecision::Send
            }
            Some(_) => {
                self.next_sequence = None;
                MediaGateDecision::RequestKeyframe
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_media_peer(
    state: Arc<WebRtcTransportState>,
    peer_id: u64,
    track: Arc<TrackLocalStaticSample>,
    mut descriptor: Arc<H264StreamDescriptor>,
    mut events: broadcast::Receiver<Arc<H264MediaEvent>>,
    mut connected: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    current_generation: Arc<AtomicU64>,
    pending_pli: Arc<Mutex<Option<Instant>>>,
    absolute_capture_time_negotiated: bool,
) {
    let mut gate = MediaGate::new(descriptor.generation);
    let mut needs_keyframe_request = true;
    let mut media_receiver_synchronized = false;
    let mut ever_connected = false;
    let mut rtp_timeline = RtpSourceTimeline::default();

    loop {
        while !*connected.borrow() {
            let timeout_kind = if ever_connected {
                PeerConnectionWait::Reconnect
            } else {
                PeerConnectionWait::Initial
            };
            let wait = tokio::time::sleep(timeout_kind.timeout());
            tokio::pin!(wait);
            tokio::select! {
                changed = connected.changed() => {
                    if changed.is_err() {
                        terminate_peer(&state, peer_id, false).await;
                        return;
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        terminate_peer(&state, peer_id, false).await;
                        return;
                    }
                }
                _ = &mut wait => {
                    match timeout_kind {
                        PeerConnectionWait::Initial => state
                            .metrics
                            .initial_connection_timeouts
                            .fetch_add(1, Ordering::Relaxed),
                        PeerConnectionWait::Reconnect => state
                            .metrics
                            .disconnected_timeouts
                            .fetch_add(1, Ordering::Relaxed),
                    };
                    terminate_peer(&state, peer_id, true).await;
                    return;
                }
            }
        }
        ever_connected = true;
        if !media_receiver_synchronized {
            // SDP/ICE establishment may take longer than the bounded broadcast
            // ring. Discard everything queued before the peer became connected,
            // then recover from a newly requested IDR. Treating this expected
            // pre-connect backlog as a live-client Lagged error would make slow
            // ICE gathering fail immediately after it finally succeeds.
            loop {
                match events.try_recv() {
                    Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        terminate_peer(&state, peer_id, false).await;
                        return;
                    }
                }
            }
            let Some(current_descriptor) = state.media.descriptor() else {
                terminate_peer(&state, peer_id, true).await;
                return;
            };
            descriptor = current_descriptor;
            current_generation.store(descriptor.generation, Ordering::Relaxed);
            gate.reset(descriptor.generation);
            media_receiver_synchronized = true;
        }
        if needs_keyframe_request {
            let _ = state.media.request_keyframe(descriptor.generation);
            needs_keyframe_request = false;
        }

        let event = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    terminate_peer(&state, peer_id, false).await;
                    return;
                }
                continue;
            }
            changed = connected.changed() => {
                if changed.is_err() {
                    terminate_peer(&state, peer_id, false).await;
                    return;
                }
                if !*connected.borrow() {
                    gate.reset(descriptor.generation);
                    needs_keyframe_request = true;
                    media_receiver_synchronized = false;
                }
                continue;
            }
            event = events.recv() => event,
        };

        match event {
            Ok(event) => match event.as_ref() {
                H264MediaEvent::Reset(next_descriptor) => {
                    descriptor = Arc::clone(next_descriptor);
                    current_generation.store(descriptor.generation, Ordering::Relaxed);
                    gate.reset(descriptor.generation);
                    let _ = state.media.request_keyframe(descriptor.generation);
                }
                H264MediaEvent::Segment(segment) => match gate.observe(segment) {
                    MediaGateDecision::Ignore => {}
                    MediaGateDecision::RequestKeyframe => {
                        let _ = state.media.request_keyframe(descriptor.generation);
                    }
                    MediaGateDecision::Send => {
                        if let Some(gap) = rtp_timeline.gap_before(segment) {
                            // TrackLocalStaticSample ignores Sample.timestamp. An empty
                            // sample advances only its RTP clock, preserving source time
                            // across frames intentionally dropped while waiting for IDR.
                            let gap_sample = Sample {
                                duration: gap,
                                ..Default::default()
                            };
                            match tokio::time::timeout(
                                MEDIA_SEND_TIMEOUT,
                                track.write_sample(&gap_sample),
                            )
                            .await
                            {
                                Ok(Ok(())) => {}
                                Ok(Err(_)) => {
                                    state
                                        .metrics
                                        .media_send_errors
                                        .fetch_add(1, Ordering::Relaxed);
                                    terminate_peer(&state, peer_id, true).await;
                                    return;
                                }
                                Err(_) => {
                                    state
                                        .metrics
                                        .media_send_timeouts
                                        .fetch_add(1, Ordering::Relaxed);
                                    terminate_peer(&state, peer_id, true).await;
                                    return;
                                }
                            }
                        }
                        let access_unit = match state.access_unit(&descriptor, segment) {
                            Ok(access_unit) => access_unit,
                            Err(_) => {
                                state
                                    .metrics
                                    .media_send_errors
                                    .fetch_add(1, Ordering::Relaxed);
                                terminate_peer(&state, peer_id, true).await;
                                return;
                            }
                        };
                        let captured_at = UNIX_EPOCH
                            .checked_add(Duration::from_millis(segment.captured_at_unix_ms))
                            .unwrap_or(UNIX_EPOCH);
                        let sample = Sample {
                            data: (*access_unit).clone(),
                            timestamp: captured_at,
                            duration: Duration::from_micros(segment.duration_us.max(1)),
                            ..Default::default()
                        };
                        let write_sample = async {
                            if absolute_capture_time_negotiated {
                                let extensions =
                                    [webrtc::rtp::extension::HeaderExtension::Custom {
                                        uri: ABSOLUTE_CAPTURE_TIME_URI.into(),
                                        extension: Box::new(
                                            AbsoluteCaptureTimeExtension::from_system_time(
                                                captured_at,
                                            ),
                                        ),
                                    }];
                                track
                                    .write_sample_with_extensions(&sample, &extensions)
                                    .await
                            } else {
                                track.write_sample(&sample).await
                            }
                        };
                        match tokio::time::timeout(MEDIA_SEND_TIMEOUT, write_sample).await {
                            Ok(Ok(())) => {
                                state
                                    .metrics
                                    .media_payload_bytes_sent
                                    .fetch_add(access_unit.len() as u64, Ordering::Relaxed);
                                state
                                    .metrics
                                    .media_samples_sent
                                    .fetch_add(1, Ordering::Relaxed);
                                if absolute_capture_time_negotiated {
                                    state
                                        .metrics
                                        .absolute_capture_time_samples_sent
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                                state
                                    .metrics
                                    .latest_capture_sequence
                                    .store(segment.capture_sequence, Ordering::Relaxed);
                                state
                                    .metrics
                                    .latest_source_timestamp_us
                                    .store(segment.timestamp_us, Ordering::Relaxed);
                                if let Ok(age) = SystemTime::now().duration_since(captured_at) {
                                    state
                                        .metrics
                                        .capture_to_packetizer_ms
                                        .record(age.as_millis().min(u128::from(u64::MAX)) as u64);
                                }
                                if segment.keyframe {
                                    let recovery_started = pending_pli
                                        .lock()
                                        .ok()
                                        .and_then(|mut pending| pending.take());
                                    if let Some(recovery_started) = recovery_started {
                                        state
                                            .metrics
                                            .keyframe_recoveries
                                            .fetch_add(1, Ordering::Relaxed);
                                        state.metrics.keyframe_recovery_ms.record(
                                            recovery_started
                                                .elapsed()
                                                .as_millis()
                                                .min(u128::from(u64::MAX))
                                                as u64,
                                        );
                                    }
                                }
                            }
                            Ok(Err(_)) => {
                                state
                                    .metrics
                                    .media_send_errors
                                    .fetch_add(1, Ordering::Relaxed);
                                terminate_peer(&state, peer_id, true).await;
                                return;
                            }
                            Err(_) => {
                                state
                                    .metrics
                                    .media_send_timeouts
                                    .fetch_add(1, Ordering::Relaxed);
                                terminate_peer(&state, peer_id, true).await;
                                return;
                            }
                        }
                    }
                },
                H264MediaEvent::Unavailable { .. } => {
                    terminate_peer(&state, peer_id, true).await;
                    return;
                }
            },
            Err(broadcast::error::RecvError::Lagged(_)) => {
                state
                    .metrics
                    .media_source_lag_disconnects
                    .fetch_add(1, Ordering::Relaxed);
                terminate_peer(&state, peer_id, true).await;
                return;
            }
            Err(broadcast::error::RecvError::Closed) => {
                terminate_peer(&state, peer_id, false).await;
                return;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerConnectionWait {
    Initial,
    Reconnect,
}

impl PeerConnectionWait {
    fn timeout(self) -> Duration {
        match self {
            Self::Initial => PEER_INITIAL_CONNECTION_TIMEOUT,
            Self::Reconnect => PEER_DISCONNECTED_GRACE,
        }
    }
}

async fn run_rtcp_peer(
    state: Arc<WebRtcTransportState>,
    peer_id: u64,
    sender: Arc<webrtc::rtp_transceiver::rtp_sender::RTCRtpSender>,
    mut shutdown: watch::Receiver<bool>,
    current_generation: Arc<AtomicU64>,
    pending_pli: Arc<Mutex<Option<Instant>>>,
) {
    loop {
        let packets = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
                continue;
            }
            result = sender.read_rtcp() => match result {
                Ok((packets, _)) => packets,
                Err(_) => {
                    terminate_peer(&state, peer_id, false).await;
                    return;
                }
            },
        };
        state
            .metrics
            .rtcp_packets_received
            .fetch_add(packets.len() as u64, Ordering::Relaxed);
        for packet in packets {
            if packet
                .as_any()
                .downcast_ref::<TransportLayerNack>()
                .is_some()
            {
                // Reading through RTCRtpSender is what allows the default NACK
                // interceptor to service retransmissions before packets arrive here.
                state
                    .metrics
                    .nack_packets_received
                    .fetch_add(1, Ordering::Relaxed);
            }
            if let Some(feedback) = packet.as_any().downcast_ref::<TransportLayerCc>() {
                state
                    .metrics
                    .transport_cc_packets_received
                    .fetch_add(1, Ordering::Relaxed);
                state
                    .metrics
                    .transport_cc_status_count
                    .fetch_add(u64::from(feedback.packet_status_count), Ordering::Relaxed);
                state
                    .metrics
                    .transport_cc_received_delta_count
                    .fetch_add(feedback.recv_deltas.len() as u64, Ordering::Relaxed);
            }
            let requests_keyframe = packet
                .as_any()
                .downcast_ref::<PictureLossIndication>()
                .is_some()
                || packet.as_any().downcast_ref::<FullIntraRequest>().is_some();
            if requests_keyframe {
                state
                    .metrics
                    .pli_packets_received
                    .fetch_add(1, Ordering::Relaxed);
                if let Ok(mut pending) = pending_pli.lock() {
                    if pending.is_none() {
                        *pending = Some(Instant::now());
                    }
                }
                match state
                    .media
                    .request_keyframe(current_generation.load(Ordering::Relaxed))
                {
                    H264KeyframeRequestResult::Scheduled => {
                        state
                            .metrics
                            .pli_keyframe_scheduled
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    H264KeyframeRequestResult::Coalesced => {
                        state
                            .metrics
                            .pli_keyframe_coalesced
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    H264KeyframeRequestResult::StaleGeneration => {
                        state
                            .metrics
                            .pli_keyframe_stale
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

async fn terminate_peer(state: &Arc<WebRtcTransportState>, peer_id: u64, failed: bool) {
    let record = state
        .peers
        .lock()
        .ok()
        .and_then(|mut peers| peers.remove(&peer_id));
    let Some(record) = record else {
        return;
    };
    if failed {
        state.metrics.peers_failed.fetch_add(1, Ordering::Relaxed);
    }
    state.release_peer_slot();
    let PeerRecord {
        connection,
        shutdown,
        viewer_lease,
    } = record;
    let _ = shutdown.send(true);
    // Global viewer/IP accounting must not depend on a third-party close
    // future completing. Release the lease before the bounded close.
    drop(viewer_lease);
    if tokio::time::timeout(PEER_CLOSE_TIMEOUT, connection.close())
        .await
        .is_err()
    {
        log::warn!("WebRTC peer {peer_id} close exceeded its deadline");
    }
}

#[derive(Debug)]
pub struct WebRtcOfferError {
    status: StatusCode,
    message: String,
}

impl WebRtcOfferError {
    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl WebRtcOfferError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }

    fn too_many_peers() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "WebRTC peer limit reached".to_owned(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

fn avcc_parameter_sets(init_segment: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let marker = init_segment
        .windows(4)
        .position(|window| window == b"avcC")
        .ok_or_else(|| "H.264 init segment has no avcC box".to_owned())?;
    let data = init_segment
        .get(marker + 4..)
        .ok_or_else(|| "truncated avcC box".to_owned())?;
    if data.len() < 7 || data[0] != 1 {
        return Err("invalid avcC record".to_owned());
    }
    if data[4] & 0x03 != 0x03 {
        return Err("unsupported avcC NAL length size".to_owned());
    }
    let mut offset = 6usize;
    let sps_count = usize::from(data[5] & 0x1f);
    let mut parameter_sets = Vec::with_capacity(sps_count.saturating_add(1));
    for _ in 0..sps_count {
        parameter_sets.push(read_avcc_item(data, &mut offset)?);
    }
    let pps_count = usize::from(
        *data
            .get(offset)
            .ok_or_else(|| "truncated avcC PPS count".to_owned())?,
    );
    offset += 1;
    for _ in 0..pps_count {
        parameter_sets.push(read_avcc_item(data, &mut offset)?);
    }
    Ok(parameter_sets)
}

fn read_avcc_item(data: &[u8], offset: &mut usize) -> Result<Vec<u8>, String> {
    let length_end = offset.saturating_add(2);
    let length_bytes = data
        .get(*offset..length_end)
        .ok_or_else(|| "truncated avcC parameter-set length".to_owned())?;
    let length = usize::from(u16::from_be_bytes(
        length_bytes
            .try_into()
            .map_err(|_| "invalid avcC parameter-set length".to_owned())?,
    ));
    *offset = length_end;
    let item_end = offset.saturating_add(length);
    let item = data
        .get(*offset..item_end)
        .ok_or_else(|| "truncated avcC parameter set".to_owned())?;
    *offset = item_end;
    Ok(item.to_vec())
}

fn avcc_to_annex_b(
    avcc: &[u8],
    keyframe_parameter_sets: Option<&[Vec<u8>]>,
) -> Result<Vec<u8>, String> {
    let mut offset = 0usize;
    let mut nal_units = Vec::new();
    let mut has_sps = false;
    let mut has_pps = false;
    while offset < avcc.len() {
        let length_end = offset.saturating_add(4);
        let length_bytes = avcc
            .get(offset..length_end)
            .ok_or_else(|| "truncated AVCC NAL length".to_owned())?;
        let length = usize::try_from(u32::from_be_bytes(
            length_bytes
                .try_into()
                .map_err(|_| "invalid AVCC NAL length".to_owned())?,
        ))
        .map_err(|_| "AVCC NAL is too large".to_owned())?;
        offset = length_end;
        if length == 0 {
            return Err("AVCC access unit contains an empty NAL".to_owned());
        }
        let nal_end = offset.saturating_add(length);
        let nal = avcc
            .get(offset..nal_end)
            .ok_or_else(|| "truncated AVCC NAL".to_owned())?;
        match nal[0] & 0x1f {
            7 => has_sps = true,
            8 => has_pps = true,
            _ => {}
        }
        nal_units.push(nal);
        offset = nal_end;
    }

    let parameter_bytes = keyframe_parameter_sets
        .map(|sets| {
            sets.iter()
                .filter(|set| match set.first().map(|byte| byte & 0x1f) {
                    Some(7) => !has_sps,
                    Some(8) => !has_pps,
                    _ => false,
                })
                .map(|set| set.len().saturating_add(4))
                .sum::<usize>()
        })
        .unwrap_or_default();
    let nal_bytes = nal_units
        .iter()
        .map(|nal| nal.len().saturating_add(4))
        .sum::<usize>();
    let mut annex_b = Vec::with_capacity(parameter_bytes.saturating_add(nal_bytes));
    if let Some(parameter_sets) = keyframe_parameter_sets {
        for parameter_set in parameter_sets {
            let include = match parameter_set.first().map(|byte| byte & 0x1f) {
                Some(7) => !has_sps,
                Some(8) => !has_pps,
                _ => false,
            };
            if include {
                annex_b.extend_from_slice(&[0, 0, 0, 1]);
                annex_b.extend_from_slice(parameter_set);
            }
        }
    }
    for nal in nal_units {
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(nal);
    }
    Ok(annex_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(generation: u64, sequence: u64, keyframe: bool) -> H264MediaSegment {
        H264MediaSegment {
            generation,
            sequence,
            keyframe,
            timestamp_us: sequence * 33_333,
            duration_us: 33_333,
            capture_sequence: sequence,
            captured_at_unix_ms: 1,
            visible_input_sequence: None,
            input_applied_at_server_unix_ms: None,
            access_unit_avcc: Arc::new(Bytes::new()),
            bytes: Arc::new(Bytes::new()),
        }
    }

    #[test]
    fn media_gate_waits_for_idr_and_recovers_only_on_an_idr() {
        let mut gate = MediaGate::new(4);
        assert_eq!(
            gate.observe(&segment(4, 10, false)),
            MediaGateDecision::Ignore
        );
        assert_eq!(gate.observe(&segment(4, 11, true)), MediaGateDecision::Send);
        assert_eq!(
            gate.observe(&segment(4, 12, false)),
            MediaGateDecision::Send
        );
        assert_eq!(
            gate.observe(&segment(4, 14, false)),
            MediaGateDecision::RequestKeyframe
        );
        assert_eq!(
            gate.observe(&segment(4, 15, false)),
            MediaGateDecision::Ignore
        );
        assert_eq!(gate.observe(&segment(4, 16, true)), MediaGateDecision::Send);
    }

    #[test]
    fn media_gate_rejects_stale_generations_after_reset() {
        let mut gate = MediaGate::new(1);
        gate.reset(2);
        assert_eq!(
            gate.observe(&segment(1, 1, true)),
            MediaGateDecision::Ignore
        );
        assert_eq!(gate.observe(&segment(2, 1, true)), MediaGateDecision::Send);
    }

    #[test]
    fn converts_a_complete_avcc_access_unit_and_injects_parameter_sets() {
        let idr = [0x65, 0xaa, 0xbb];
        let mut avcc = Vec::new();
        avcc.extend_from_slice(&(idr.len() as u32).to_be_bytes());
        avcc.extend_from_slice(&idr);
        let parameter_sets = vec![vec![0x67, 0x42], vec![0x68, 0xce]];
        assert_eq!(
            avcc_to_annex_b(&avcc, Some(&parameter_sets)).unwrap(),
            vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68, 0xce, 0, 0, 0, 1, 0x65, 0xaa, 0xbb,]
        );
    }

    #[test]
    fn parses_sps_and_pps_from_an_avcc_record_inside_init_segment() {
        let mut init = b"prefix-avcC".to_vec();
        init.extend_from_slice(&[
            1, 0x42, 0xc0, 0x1f, 0xff, 0xe1, 0, 3, 0x67, 0x42, 0xc0, 1, 0, 2, 0x68, 0xce,
        ]);
        assert_eq!(
            avcc_parameter_sets(&init).unwrap(),
            vec![vec![0x67, 0x42, 0xc0], vec![0x68, 0xce]]
        );
    }

    #[test]
    fn bounded_metric_distribution_keeps_count_and_percentiles() {
        let samples = BoundedSamples::default();
        for value in 1..=100 {
            samples.record(value);
        }
        assert_eq!(
            samples.snapshot(),
            WebRtcDistributionSnapshot {
                sample_count: 100,
                retained_sample_count: 100,
                retained_sample_capacity: METRIC_SAMPLE_LIMIT as u32,
                measurement_scope: METRIC_MEASUREMENT_SCOPE,
                p50: 50,
                p95: 95,
                p99: 99,
                max: 100,
            }
        );
    }

    #[test]
    fn peer_limit_supports_the_thirty_viewer_comparison_matrix() {
        let state = WebRtcTransportState::new(Arc::new(H264MediaState::new()));
        assert!(state.metrics_snapshot().peer_limit >= 30);
        for _ in 0..MAX_WEBRTC_PEERS {
            assert!(state.try_reserve_peer_slot());
        }
        assert!(!state.try_reserve_peer_slot());
        state.release_peer_slot();
        assert!(state.try_reserve_peer_slot());
    }

    #[test]
    fn peer_connection_waits_are_bounded_and_reconnect_is_shorter() {
        assert_eq!(
            PeerConnectionWait::Initial.timeout(),
            PEER_INITIAL_CONNECTION_TIMEOUT
        );
        assert_eq!(
            PeerConnectionWait::Reconnect.timeout(),
            PEER_DISCONNECTED_GRACE
        );
        assert!(PEER_INITIAL_CONNECTION_TIMEOUT > PEER_DISCONNECTED_GRACE);
        assert!(PEER_DISCONNECTED_GRACE <= Duration::from_secs(5));
    }

    #[test]
    fn h264_rtp_capability_requires_non_interleaved_packetization() {
        assert!(H264_RTP_FMTP.contains("packetization-mode=1"));
        assert!(!H264_RTP_FMTP.contains("packetization-mode=0"));
    }

    #[test]
    fn absolute_capture_time_uses_the_eight_byte_uq32_32_ntp_field() {
        let extension = AbsoluteCaptureTimeExtension::from_system_time(UNIX_EPOCH);
        let encoded = extension.marshal().unwrap();
        assert_eq!(encoded.len(), 8);
        assert_eq!(encoded.as_ref(), &0x83aa_7e80_0000_0000_u64.to_be_bytes());

        let mut short = [0_u8; 7];
        assert_eq!(
            extension.marshal_to(&mut short),
            Err(webrtc::util::Error::ErrBufferShort)
        );

        let fractional = AbsoluteCaptureTimeExtension::from_system_time(
            UNIX_EPOCH + Duration::from_millis(1_500),
        );
        assert_eq!(fractional.capture_timestamp_ntp, 0x83aa_7e81_8000_0000_u64);
    }

    #[test]
    fn absolute_capture_time_is_counted_only_when_the_answer_negotiates_its_extmap() {
        let answer = format!(
            "v=0\r\na=extmap:4 urn:ietf:params:rtp-hdrext:sdes:mid\r\na=extmap:9/recvonly {ABSOLUTE_CAPTURE_TIME_URI}\r\n"
        );
        assert!(sdp_negotiates_extension(&answer, ABSOLUTE_CAPTURE_TIME_URI));
        assert!(!sdp_negotiates_extension(
            "v=0\r\na=x-note:http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time\r\n",
            ABSOLUTE_CAPTURE_TIME_URI
        ));
    }

    #[tokio::test]
    async fn absolute_capture_time_survives_real_media_engine_offer_answer_negotiation() {
        let offerer = build_api()
            .unwrap()
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        let answerer = build_api()
            .unwrap()
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        offerer
            .add_transceiver_from_kind(RTPCodecType::Video, None)
            .await
            .unwrap();
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_owned(),
                clock_rate: 90_000,
                sdp_fmtp_line: H264_RTP_FMTP.to_owned(),
                ..Default::default()
            },
            "screen".to_owned(),
            "offer-answer-test".to_owned(),
        ));
        answerer
            .add_track(track as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .unwrap();

        let offer = offerer.create_offer(None).await.unwrap();
        assert!(sdp_negotiates_extension(
            &offer.sdp,
            ABSOLUTE_CAPTURE_TIME_URI
        ));
        answerer.set_remote_description(offer).await.unwrap();
        let answer = answerer.create_answer(None).await.unwrap();
        assert!(sdp_negotiates_extension(
            &answer.sdp,
            ABSOLUTE_CAPTURE_TIME_URI
        ));

        offerer.close().await.unwrap();
        answerer.close().await.unwrap();
    }

    #[test]
    fn rtp_source_timeline_advances_over_dropped_frames_and_generation_resets() {
        let mut timeline = RtpSourceTimeline::default();
        let mut first = segment(1, 1, true);
        first.timestamp_us = 100_000;
        first.duration_us = 33_333;
        first.captured_at_unix_ms = 1_000;
        assert_eq!(timeline.gap_before(&first), None);

        let mut contiguous = segment(1, 2, false);
        contiguous.timestamp_us = 133_333;
        contiguous.duration_us = 33_333;
        contiguous.captured_at_unix_ms = 1_033;
        assert_eq!(timeline.gap_before(&contiguous), None);

        let mut recovered = segment(1, 5, true);
        recovered.timestamp_us = 233_332;
        recovered.duration_us = 33_333;
        recovered.captured_at_unix_ms = 1_133;
        assert_eq!(
            timeline.gap_before(&recovered),
            Some(Duration::from_micros(66_666))
        );

        let mut reset = segment(2, 1, true);
        reset.timestamp_us = 0;
        reset.duration_us = 33_333;
        reset.captured_at_unix_ms = 2_133;
        assert_eq!(
            timeline.gap_before(&reset),
            Some(Duration::from_micros(966_667))
        );
    }

    #[test]
    fn empty_h264_sample_advances_the_dependency_packetizer_clock_without_emitting_rtp() {
        use webrtc::rtp::codecs::h264::H264Payloader;
        use webrtc::rtp::packetizer::{new_packetizer, Packetizer};
        use webrtc::rtp::sequence::new_random_sequencer;

        let mut packetizer = new_packetizer(
            1_200,
            96,
            1,
            Box::new(H264Payloader::default()),
            Box::new(new_random_sequencer()),
            90_000,
        );
        let payload = Bytes::from_static(&[0x65, 1, 2, 3]);
        let first = packetizer.packetize(&payload, 3_000).unwrap();
        assert_eq!(first.len(), 1);
        let skipped = packetizer.packetize(&Bytes::new(), 9_000).unwrap();
        assert!(skipped.is_empty());
        let next = packetizer.packetize(&payload, 3_000).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(
            next[0]
                .header
                .timestamp
                .wrapping_sub(first[0].header.timestamp),
            12_000
        );
    }
}
