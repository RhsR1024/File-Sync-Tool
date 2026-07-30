use crate::device_simulator::access_control::PlatformAccessPolicy;
use crate::device_simulator::alarm_runtime::{LearnedAlarmEndpoint, SharedLearnedAlarmEndpoint};
use crate::device_simulator::alarms::{ImageCache, SharedImageCache};
use crate::device_simulator::api::{
    DeviceIdentityPreviewDto, DevicePreview, DeviceSimulatorStreamKind, SimulatorStartRequest,
};
use crate::device_simulator::discovery::{
    DiscoveryBindPlan, DiscoveryListener, MAX_DISCOVERY_RESPONSE_BYTES,
};
use crate::device_simulator::http::{
    DeviceHttpListener, HttpBindPlan, HttpMethod, HttpRequest, HttpResponse,
};
use crate::device_simulator::media::{Codec, ParameterSetKind};
use crate::device_simulator::profiles::schema::DeviceProfileV1;
use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use crate::device_simulator::rtsp::routes::{
    plan_rtsp_routes, RtspPorts as PlannedRtspPorts, RtspRouteRole, RtspStreamKind,
};
use crate::device_simulator::rtsp::service::{
    start_rtsp_server, RtspEndpointConfig, RtspServerHandle, RtspStreamSource,
};
use crate::device_simulator::runtime_assets::{RuntimeAssetLayout, RuntimeMediaKind};
use crate::device_simulator::telemetry::ProtocolFailureMetrics;
use crate::device_simulator::watermark_runtime::WatermarkMediaHub;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{SecondsFormat, Utc};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const PROTOCOL_LOG_INTERVAL_MS: u64 = 10_000;
const HTTP_CLIENT_TASK_LIMIT: usize = 128;

#[derive(Debug, Clone)]
pub struct ProtocolRuntimeConfig {
    pub request: SimulatorStartRequest,
    pub preview: DevicePreview,
    pub assets: Arc<RuntimeAssetLayout>,
    pub picture_cache: SharedImageCache,
    /// Written when the platform advertises its alarm receiver endpoint during
    /// subscription; the alarm runtime reads it so dispatch follows the
    /// platform. See [`SharedLearnedAlarmEndpoint`].
    pub learned_endpoint: SharedLearnedAlarmEndpoint,
    /// Which callers discovery and HTTP answer. Resolved once by the Worker from
    /// [`crate::device_simulator::api::TargetPlatformConfig`]; RTSP is not gated.
    pub access_policy: PlatformAccessPolicy,
    /// Tests may disable the fixed legacy multicast port while still exercising
    /// HTTP and RTSP on loopback. Production Worker sessions always enable it.
    pub enable_discovery: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolRuntimeSummary {
    pub total_devices: u32,
    pub discovery_listeners: usize,
    pub http_listeners: usize,
    pub rtsp_listeners: usize,
    pub bind_addresses: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtocolRuntimeStats {
    pub active_rtsp_clients: u32,
    pub outbound_bitrate_kbps: u64,
    pub bytes_sent: u64,
    pub disconnected_clients: u64,
    pub active_http_connections_by_device: BTreeMap<String, u32>,
    pub active_rtsp_clients_by_device: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolRuntimeError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for ProtocolRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProtocolRuntimeError {}

struct ServiceTask {
    shutdown: watch::Sender<bool>,
    join: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Default)]
struct DeviceProtocolMetrics {
    active_http_connections: AtomicU32,
}

struct ActiveHttpConnection {
    metrics: Arc<DeviceProtocolMetrics>,
}

impl ActiveHttpConnection {
    fn new(metrics: Arc<DeviceProtocolMetrics>) -> Self {
        metrics
            .active_http_connections
            .fetch_add(1, Ordering::Relaxed);
        Self { metrics }
    }
}

impl Drop for ActiveHttpConnection {
    fn drop(&mut self) {
        self.metrics
            .active_http_connections
            .fetch_sub(1, Ordering::Relaxed);
    }
}

struct RtspEndpoint {
    device_id: String,
    bitrate_bps: u64,
    handle: RtspServerHandle,
}

impl ServiceTask {
    async fn stop(self) -> Result<(), ProtocolRuntimeError> {
        let _ = self.shutdown.send(true);
        tokio::time::timeout(SERVICE_STOP_TIMEOUT, self.join)
            .await
            .map_err(|_| {
                runtime_error(
                    "device_simulator.protocol.stop_timeout",
                    "protocol service did not stop within the finite timeout",
                )
            })?
            .map_err(|source| {
                runtime_error(
                    "device_simulator.protocol.task_panicked",
                    format!("protocol service task failed: {source}"),
                )
            })?;
        Ok(())
    }
}

pub struct ProtocolRuntime {
    summary: ProtocolRuntimeSummary,
    discovery: Vec<ServiceTask>,
    http: Vec<ServiceTask>,
    rtsp: Vec<RtspEndpoint>,
    watermark_media: Option<WatermarkMediaHub>,
    device_metrics: BTreeMap<String, Arc<DeviceProtocolMetrics>>,
}

impl ProtocolRuntime {
    pub async fn start(config: ProtocolRuntimeConfig) -> Result<Self, ProtocolRuntimeError> {
        validate_runtime_config(&config)?;
        log::info!(
            "device simulator platform access policy: {}",
            config.access_policy.describe()
        );
        let mut runtime = Self {
            summary: ProtocolRuntimeSummary {
                total_devices: config.preview.total_devices,
                discovery_listeners: 0,
                http_listeners: 0,
                rtsp_listeners: 0,
                bind_addresses: Vec::new(),
            },
            discovery: Vec::new(),
            http: Vec::new(),
            rtsp: Vec::new(),
            watermark_media: None,
            device_metrics: BTreeMap::new(),
        };

        if config.request.stream.time_watermark_enabled {
            runtime.watermark_media = Some(
                WatermarkMediaHub::start(Arc::clone(&config.assets))
                    .await
                    .map_err(|message| {
                        runtime_error("device_simulator.watermark.start_failed", message)
                    })?,
            );
        } else {
            log::info!(
                "device simulator time watermark is disabled; using the original indexed media path"
            );
        }

        for device in &config.preview.devices {
            let device_metrics = Arc::new(DeviceProtocolMetrics::default());
            runtime
                .device_metrics
                .insert(device.device_id.clone(), Arc::clone(&device_metrics));
            let profile_id = parse_profile_id(&device.profile_id)?;
            let profile = config.assets.profile(profile_id).ok_or_else(|| {
                runtime_error(
                    "device_simulator.protocol.profile_missing",
                    format!("runtime profile '{}' is not loaded", device.profile_id),
                )
            })?;

            match start_http_task(
                Arc::clone(&config.assets),
                Arc::clone(&config.picture_cache),
                Arc::clone(&config.learned_endpoint),
                config.access_policy.clone(),
                config.request.platform.kind,
                config.request.device_http_port,
                device.clone(),
                profile.clone(),
                device_metrics,
            )
            .await
            {
                Ok((task, address)) => {
                    runtime.summary.http_listeners += 1;
                    runtime.summary.bind_addresses.push(address);
                    runtime.http.push(task);
                }
                Err(source) => {
                    let _ = runtime.stop().await;
                    return Err(source);
                }
            }

            let rtsp_ports = PlannedRtspPorts {
                main: config.request.rtsp_ports.main,
                sub: config.request.rtsp_ports.sub,
                third: config.request.rtsp_ports.third,
            };
            let plans = plan_rtsp_routes(device.ip, rtsp_ports)
                .map_err(|source| runtime_error(source.code, source.message))?;
            for plan in plans {
                let media_kind = match plan.stream {
                    RtspStreamKind::Main => RuntimeMediaKind::Main,
                    RtspStreamKind::Sub => RuntimeMediaKind::Sub,
                    RtspStreamKind::Third => RuntimeMediaKind::Third,
                };
                let media = config.assets.media(media_kind);
                let watermark_source = runtime
                    .watermark_media
                    .as_ref()
                    .map(|hub| hub.stream(media_kind).clone());
                let mut routes = BTreeMap::new();
                for route in plan
                    .routes
                    .iter()
                    .filter(|route| route.evidence.runtime_activation_allowed())
                {
                    let metadata_only = route.role == RtspRouteRole::MetadataControl;
                    let source = if let Some(watermark) = watermark_source.as_ref() {
                        let sdp = build_reviewed_sdp(
                            device.ip,
                            &route.path,
                            watermark.payload_type,
                            watermark.clock_rate,
                            &watermark.sps,
                            &watermark.pps,
                        )?;
                        RtspStreamSource::from_scheduler(
                            format!("{}:{:?}", device.device_id, plan.stream),
                            Arc::<[u8]>::from(sdp),
                            watermark.scheduler.clone(),
                            Codec::H264,
                            watermark.payload_type,
                            1_200,
                        )
                    } else {
                        let sdp =
                            build_reviewed_static_sdp(device.ip, &route.path, media.as_ref())?;
                        RtspStreamSource::from_media(
                            format!("{}:{:?}", device.device_id, plan.stream),
                            Arc::<[u8]>::from(sdp),
                            Arc::clone(&media),
                            128,
                            1_200,
                        )
                    }
                    .map_err(|source| runtime_error(source.code, source.message))?;
                    routes.insert(
                        route.path.clone(),
                        RtspStreamSource {
                            metadata_only,
                            ..source
                        },
                    );
                }
                let handle = match start_rtsp_server(RtspEndpointConfig {
                    bind_addr: plan.bind_addr,
                    routes,
                    client_write_queue: 256,
                })
                .await
                {
                    Ok(handle) => handle,
                    Err(source) => {
                        let _ = runtime.stop().await;
                        return Err(runtime_error(source.code, source.message));
                    }
                };
                runtime.summary.rtsp_listeners += 1;
                runtime
                    .summary
                    .bind_addresses
                    .push(handle.local_addr().to_string());
                runtime.rtsp.push(RtspEndpoint {
                    device_id: device.device_id.clone(),
                    bitrate_bps: watermark_source
                        .as_ref()
                        .map_or_else(|| media.actual_bitrate_bps(), |source| source.bitrate_bps),
                    handle,
                });
            }
        }

        if config.enable_discovery {
            for devices in legacy_discovery_batches(&config.preview.devices) {
                match start_discovery_task(
                    Arc::clone(&config.assets),
                    config.access_policy.clone(),
                    config.request.device_http_port,
                    devices,
                )
                .await
                {
                    Ok((task, address)) => {
                        runtime.summary.discovery_listeners += 1;
                        runtime.summary.bind_addresses.push(address);
                        runtime.discovery.push(task);
                    }
                    Err(source) => {
                        let _ = runtime.stop().await;
                        return Err(source);
                    }
                }
            }
        }

        runtime.summary.bind_addresses.sort();
        runtime.summary.bind_addresses.dedup();
        Ok(runtime)
    }

    pub fn summary(&self) -> &ProtocolRuntimeSummary {
        &self.summary
    }

    pub fn stats(&self) -> ProtocolRuntimeStats {
        let active_http_connections_by_device = self
            .device_metrics
            .iter()
            .map(|(device_id, metrics)| {
                (
                    device_id.clone(),
                    metrics.active_http_connections.load(Ordering::Relaxed),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut stats = ProtocolRuntimeStats {
            active_http_connections_by_device,
            active_rtsp_clients_by_device: self
                .device_metrics
                .keys()
                .cloned()
                .map(|device_id| (device_id, 0))
                .collect(),
            ..ProtocolRuntimeStats::default()
        };
        let mut outbound_bitrate_bps = 0_u128;
        for endpoint in &self.rtsp {
            let endpoint_stats = endpoint.handle.stats();
            stats.active_rtsp_clients = stats
                .active_rtsp_clients
                .saturating_add(endpoint_stats.active_clients);
            stats.bytes_sent = stats.bytes_sent.saturating_add(endpoint_stats.bytes_sent);
            stats.disconnected_clients = stats
                .disconnected_clients
                .saturating_add(endpoint_stats.disconnected_clients);
            let device_clients = stats
                .active_rtsp_clients_by_device
                .entry(endpoint.device_id.clone())
                .or_default();
            *device_clients = device_clients.saturating_add(endpoint_stats.active_clients);
            outbound_bitrate_bps = outbound_bitrate_bps.saturating_add(
                u128::from(endpoint.bitrate_bps) * u128::from(endpoint_stats.active_clients),
            );
        }
        stats.outbound_bitrate_kbps = (outbound_bitrate_bps / 1_000)
            .try_into()
            .unwrap_or(u64::MAX);
        stats
    }

    pub async fn stop(mut self) -> Result<(), ProtocolRuntimeError> {
        let mut first_error = None;
        while let Some(endpoint) = self.rtsp.pop() {
            if let Err(source) = endpoint.handle.stop(SERVICE_STOP_TIMEOUT).await {
                first_error.get_or_insert_with(|| runtime_error(source.code, source.message));
            }
        }
        if let Some(hub) = self.watermark_media.take() {
            hub.stop(SERVICE_STOP_TIMEOUT).await;
        }
        while let Some(task) = self.http.pop() {
            if let Err(source) = task.stop().await {
                first_error.get_or_insert(source);
            }
        }
        while let Some(task) = self.discovery.pop() {
            if let Err(source) = task.stop().await {
                first_error.get_or_insert(source);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn validate_runtime_config(config: &ProtocolRuntimeConfig) -> Result<(), ProtocolRuntimeError> {
    if config.preview.devices.is_empty()
        || config.preview.total_devices as usize != config.preview.devices.len()
    {
        return Err(runtime_error(
            "device_simulator.protocol.preview_invalid",
            "protocol runtime requires a non-empty, internally consistent device preview",
        ));
    }
    if config.request.groups.is_empty() {
        return Err(runtime_error(
            "device_simulator.protocol.request_invalid",
            "protocol runtime requires at least one configured device group",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn start_http_task(
    assets: Arc<RuntimeAssetLayout>,
    picture_cache: SharedImageCache,
    learned_endpoint: SharedLearnedAlarmEndpoint,
    access_policy: PlatformAccessPolicy,
    platform: TargetPlatform,
    http_port: u16,
    device: DeviceIdentityPreviewDto,
    profile: DeviceProfileV1,
    device_metrics: Arc<DeviceProtocolMetrics>,
) -> Result<(ServiceTask, String), ProtocolRuntimeError> {
    let metrics = Arc::new(ProtocolFailureMetrics::default());
    let listener = Arc::new(
        DeviceHttpListener::bind(
            HttpBindPlan {
                device_ip: device.ip,
                port: http_port,
            },
            metrics,
        )
        .await
        .map_err(|source| runtime_error(source.code, source.message))?,
    );
    let local_addr = listener
        .local_addr()
        .map_err(|source| runtime_error(source.code, source.message))?;
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let join = tokio::spawn(async move {
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer)) => {
                            // Close before a single byte is read, so a platform
                            // outside the allow list cannot even fingerprint the
                            // device from its HTTP responses.
                            if !access_policy.permits_socket(peer) {
                                drop(stream);
                                log::debug!(
                                    "device simulator HTTP connection from {peer} refused by the platform access policy"
                                );
                                continue;
                            }
                            if clients.len() >= HTTP_CLIENT_TASK_LIMIT {
                                drop(stream);
                                continue;
                            }
                            let listener = Arc::clone(&listener);
                            let assets = Arc::clone(&assets);
                            let picture_cache = Arc::clone(&picture_cache);
                            let learned_endpoint = Arc::clone(&learned_endpoint);
                            let device = device.clone();
                            let profile = profile.clone();
                            let device_metrics = Arc::clone(&device_metrics);
                            clients.spawn(async move {
                                let _active = ActiveHttpConnection::new(device_metrics);
                                if let Err(source) = serve_http_connection(
                                    listener,
                                    assets,
                                    picture_cache,
                                    learned_endpoint,
                                    platform,
                                    http_port,
                                    device,
                                    profile,
                                    stream,
                                    peer,
                                ).await {
                                    log::warn!("device simulator HTTP request failed: {source}");
                                }
                            });
                        }
                        Err(source) => {
                            log::warn!("device simulator HTTP accept failed: {source}");
                            break;
                        }
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                Some(_) = clients.join_next(), if !clients.is_empty() => {}
            }
        }
        clients.abort_all();
        while clients.join_next().await.is_some() {}
    });
    Ok((ServiceTask { shutdown, join }, local_addr.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn serve_http_connection(
    listener: Arc<DeviceHttpListener>,
    assets: Arc<RuntimeAssetLayout>,
    picture_cache: SharedImageCache,
    learned_endpoint: SharedLearnedAlarmEndpoint,
    platform: TargetPlatform,
    http_port: u16,
    device: DeviceIdentityPreviewDto,
    profile: DeviceProfileV1,
    mut stream: tokio::net::TcpStream,
    peer: SocketAddr,
) -> Result<(), ProtocolRuntimeError> {
    let (request, _) = listener
        .read_request(&mut stream, now_ms(), PROTOCOL_LOG_INTERVAL_MS)
        .await
        .map_err(|source| runtime_error(source.code, source.message))?;
    let profile_id = parse_profile_id(&device.profile_id)?;
    // Learn the platform's alarm receiver endpoint from subscription requests so
    // alarm dispatch and the rendered subscription Reference follow it, exactly
    // as the legacy tool wrote picconfig/sendport from the subscription Port.
    learn_subscription_endpoint(&request, &learned_endpoint);
    let response =
        if request.method == HttpMethod::Get && request.path == "/LAPI/V1.0/System/Picture" {
            picture_response(&request, &picture_cache.read())
        } else {
            match resolve_http_template(&request) {
                Some(selection) => {
                    let bytes = assets
                        .read_profile_or_core(profile_id, &selection.path)
                        .map_err(|source| runtime_error(source.code, source.message))?;
                    let body = render_legacy_template(
                        &bytes, &request, peer, platform, http_port, &device, &profile,
                    )?;
                    HttpResponse {
                        status: 200,
                        content_type: response_content_type(&request, &body),
                        body,
                    }
                }
                None if request.path.starts_with("/LAPI/") => {
                    let bytes = assets
                        .read_from_pack("protocol-core", "xml/Common/NotSupported-LAPI.xml")
                        .map_err(|source| runtime_error(source.code, source.message))?;
                    HttpResponse {
                        status: 200,
                        content_type: "application/json; charset=utf-8".into(),
                        body: render_legacy_template(
                            &bytes, &request, peer, platform, http_port, &device, &profile,
                        )?,
                    }
                }
                None => HttpResponse {
                    status: 404,
                    content_type: "text/plain; charset=utf-8".into(),
                    body: b"declared simulator route not found".to_vec(),
                },
            }
        };
    listener
        .write_response(&mut stream, &response, now_ms(), PROTOCOL_LOG_INTERVAL_MS)
        .await
        .map_err(|source| runtime_error(source.code, source.message))
}

async fn start_discovery_task(
    assets: Arc<RuntimeAssetLayout>,
    access_policy: PlatformAccessPolicy,
    http_port: u16,
    devices: Vec<DeviceIdentityPreviewDto>,
) -> Result<(ServiceTask, String), ProtocolRuntimeError> {
    let first_device = devices.first().ok_or_else(|| {
        runtime_error(
            "device_simulator.discovery.batch_empty",
            "legacy discovery batch must contain at least one virtual device",
        )
    })?;
    let profile_id = parse_profile_id(&first_device.profile_id)?;
    if devices
        .iter()
        .any(|device| device.profile_id != first_device.profile_id)
    {
        return Err(runtime_error(
            "device_simulator.discovery.batch_profile_mismatch",
            "legacy discovery batch must contain exactly one device profile",
        ));
    }
    let template_path = match profile_id {
        FirstReleaseProfileId::IpcStructured => "xml/Common/search.xml",
    };
    let template = assets
        .read_from_pack("protocol-core", template_path)
        .map_err(|source| runtime_error(source.code, source.message))?;
    let profile = assets.profile(profile_id).cloned().ok_or_else(|| {
        runtime_error(
            "device_simulator.protocol.profile_missing",
            format!(
                "runtime profile '{}' is not loaded",
                first_device.profile_id
            ),
        )
    })?;
    let metrics = Arc::new(ProtocolFailureMetrics::default());
    let listener = Arc::new(
        // Legacy Vsocket_ip.py binds the batch's first virtual address and
        // answers for every device in that same device-type batch.
        DiscoveryListener::bind(
            DiscoveryBindPlan::legacy_ws_discovery(first_device.ip),
            metrics,
        )
        .await
        .map_err(|source| runtime_error(source.code, source.message))?,
    );
    let local_addr = listener
        .local_addr()
        .map_err(|source| runtime_error(source.code, source.message))?;
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                received = listener.receive_probe(now_ms(), PROTOCOL_LOG_INTERVAL_MS) => {
                    match received {
                        Ok((probe, _)) => {
                            // Staying silent is what keeps the devices out of an
                            // unconfigured platform's search results entirely.
                            if !access_policy.permits(*probe.source.ip()) {
                                log::debug!(
                                    "device simulator discovery probe from {} refused by the platform access policy",
                                    probe.source
                                );
                                continue;
                            }
                            for device in &devices {
                                let response = match render_discovery_template(
                                    &template,
                                    &probe.message_id,
                                    http_port,
                                    device,
                                    &profile,
                                ) {
                                    Ok(response) => response,
                                    Err(source) => {
                                        log::warn!(
                                            "device simulator discovery render failed for {}: {source}",
                                            device.ip
                                        );
                                        continue;
                                    }
                                };
                                if response.len() > MAX_DISCOVERY_RESPONSE_BYTES {
                                    log::warn!(
                                        "device simulator discovery response exceeded the safety limit for {}",
                                        device.ip
                                    );
                                    continue;
                                }
                                if let Err(source) = listener
                                    .send_response_from(
                                        device.ip,
                                        &probe,
                                        &response,
                                        now_ms(),
                                        PROTOCOL_LOG_INTERVAL_MS,
                                    )
                                    .await
                                {
                                    // Vsocket_ip.py isolates response failures to
                                    // the affected virtual device and continues.
                                    log::warn!(
                                        "device simulator discovery response failed for {}: {source}",
                                        device.ip
                                    );
                                }
                            }
                        }
                        Err(source) => log::debug!("device simulator discovery probe ignored: {source}"),
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    });
    Ok((ServiceTask { shutdown, join }, local_addr.to_string()))
}

/// Legacy `devip_info` is keyed by device type. The nearest current equivalent
/// is profile ID: one listener is created for each profile, bound to that
/// profile batch's first virtual IP, with devices kept in preview order.
fn legacy_discovery_batches(
    devices: &[DeviceIdentityPreviewDto],
) -> Vec<Vec<DeviceIdentityPreviewDto>> {
    let mut profile_indexes = BTreeMap::<String, usize>::new();
    let mut batches = Vec::<Vec<DeviceIdentityPreviewDto>>::new();
    for device in devices {
        if let Some(index) = profile_indexes.get(&device.profile_id).copied() {
            batches[index].push(device.clone());
        } else {
            profile_indexes.insert(device.profile_id.clone(), batches.len());
            batches.push(vec![device.clone()]);
        }
    }
    batches
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpTemplateSelection {
    path: String,
}

/// Extract the alarm receiver endpoint the platform advertised in a subscription
/// request body and publish it to the shared learned-endpoint handle. Returns
/// the port for immediate response rewriting.
///
/// The request path and method are the only gate: a body carrying `Port` on
/// `POST`/`PUT .../Event/Subscription` is unambiguously a subscription, so no
/// port range may be assumed on top of that. Real UMS deployments allocate the
/// receiver port dynamically (observed: `22815`), and the legacy
/// `55000..55999` assumption silently discarded every such subscription,
/// leaving alarms pointed at the fallback port.
fn learn_subscription_endpoint(
    request: &HttpRequest,
    learned_endpoint: &SharedLearnedAlarmEndpoint,
) -> Option<u16> {
    if !request.path.contains("Event/Subscription")
        || !matches!(request.method, HttpMethod::Post | HttpMethod::Put)
    {
        return None;
    }
    let port = extract_subscription_u16(&request.body, "Port").filter(|port| *port != 0)?;
    *learned_endpoint.write() = Some(LearnedAlarmEndpoint {
        host: extract_subscription_host(&request.body),
        port,
        duration_secs: extract_subscription_u32(&request.body, "Duration")
            .filter(|duration| *duration != 0),
        learned_at_ms: now_ms(),
    });
    Some(port)
}

/// Read the `IPAddress` field out of a LAPI subscription body. Unspecified and
/// broadcast addresses are meaningless as a destination and are ignored so the
/// configured server host stays in effect.
fn extract_subscription_host(body: &[u8]) -> Option<Ipv4Addr> {
    let text = std::str::from_utf8(body).ok()?;
    let key = text.find("\"IPAddress\"")?;
    let after_key = &text[key + "\"IPAddress\"".len()..];
    let colon = after_key.find(':')?;
    let value = after_key[colon + 1..]
        .trim_start_matches([' ', '\t', '"'])
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    let address = value.parse::<Ipv4Addr>().ok()?;
    (!address.is_unspecified() && !address.is_broadcast()).then_some(address)
}

fn extract_subscription_u32(body: &[u8], key: &str) -> Option<u32> {
    extract_subscription_number(body, key)?.parse::<u32>().ok()
}

/// Read a numeric field out of a LAPI subscription request body. The body is
/// tab-indented JSON, so a lightweight scan mirrors the old line split.
fn extract_subscription_u16(body: &[u8], key: &str) -> Option<u16> {
    extract_subscription_number(body, key)?.parse::<u16>().ok()
}

fn extract_subscription_number(body: &[u8], key: &str) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let quoted = format!("\"{key}\"");
    let start = text.find(&quoted)?;
    let after_key = &text[start + quoted.len()..];
    let colon = after_key.find(':')?;
    Some(
        after_key[colon + 1..]
            .trim_start_matches([' ', '\t', '"'])
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>(),
    )
    .filter(|value| !value.is_empty())
}

fn resolve_http_template(request: &HttpRequest) -> Option<HttpTemplateSelection> {
    match request.method {
        HttpMethod::Post if request.path.starts_with("/onvif/") => {
            let operation = soap_operation(request)?;
            Some(HttpTemplateSelection {
                path: format!("xml/Common/{operation}.xml"),
            })
        }
        HttpMethod::Post if request.path.contains("Event/Subscription") => {
            Some(HttpTemplateSelection {
                path: "xml/Common/Event-Subscription.xml".into(),
            })
        }
        HttpMethod::Post => body_operation(request).map(|operation| HttpTemplateSelection {
            path: format!("xml/Common/{operation}.xml"),
        }),
        HttpMethod::Get => {
            let name = last_two_path_segments(&request.path)?;
            Some(HttpTemplateSelection {
                path: format!("xml/Common/{name}.xml"),
            })
        }
        HttpMethod::Put => {
            let name = if request.path.contains("KeepAlive") {
                last_two_path_segments(&request.path)?
            } else {
                "PUT".into()
            };
            Some(HttpTemplateSelection {
                path: format!("xml/Common/{name}.xml"),
            })
        }
    }
}

fn picture_response(request: &HttpRequest, cache: &ImageCache) -> HttpResponse {
    let token = request
        .query
        .as_deref()
        .and_then(|query| query_parameter(query, "Index"))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let Some(image) = token.and_then(|token| cache.get_by_token(token)) else {
        return HttpResponse {
            status: 404,
            content_type: "text/plain; charset=utf-8".into(),
            body: b"approved simulator picture not found".to_vec(),
        };
    };
    HttpResponse {
        status: 200,
        content_type: image.content_type.into(),
        body: image.bytes.to_vec(),
    }
}

fn query_parameter<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    let mut matches = query.split('&').filter_map(|part| part.split_once('='));
    let value = matches.find_map(|(key, value)| (key == name).then_some(value))?;
    if matches.any(|(key, _)| key == name) {
        return None;
    }
    Some(value)
}

fn soap_operation(request: &HttpRequest) -> Option<String> {
    request
        .soap_action()
        .and_then(|value| value.trim_matches(['"', '\'']).rsplit('/').next())
        .filter(|value| safe_operation(value))
        .map(str::to_owned)
        .or_else(|| body_operation(request))
}

fn body_operation(request: &HttpRequest) -> Option<String> {
    let body = std::str::from_utf8(&request.body).ok()?;
    let mut candidates = Vec::new();
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'<'
            || bytes
                .get(index + 1)
                .is_some_and(|byte| matches!(byte, b'/' | b'!' | b'?'))
        {
            index += 1;
            continue;
        }
        let end = body[index + 1..].find([' ', '>', '/'])? + index + 1;
        let raw = &body[index + 1..end];
        let local = raw.rsplit(':').next().unwrap_or(raw);
        if safe_operation(local) {
            candidates.push(local.to_owned());
        }
        index = end + 1;
    }
    candidates
        .into_iter()
        .rev()
        .find(|candidate| !matches!(candidate.as_str(), "Envelope" | "Header" | "Body"))
}

fn safe_operation(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn last_two_path_segments(path: &str) -> Option<String> {
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let last = *segments.last()?;
    let previous = *segments.get(segments.len().checked_sub(2)?)?;
    if !safe_path_component(previous) || !safe_path_component(last) {
        return None;
    }
    Some(format!("{previous}-{last}"))
}

fn safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b','))
}

#[allow(clippy::too_many_arguments)]
fn render_legacy_template(
    bytes: &[u8],
    request: &HttpRequest,
    peer: SocketAddr,
    _platform: TargetPlatform,
    http_port: u16,
    device: &DeviceIdentityPreviewDto,
    profile: &DeviceProfileV1,
) -> Result<Vec<u8>, ProtocolRuntimeError> {
    let mut text = std::str::from_utf8(bytes)
        .map_err(|source| {
            runtime_error(
                "device_simulator.protocol.template_encoding_invalid",
                format!("legacy response template is not UTF-8: {source}"),
            )
        })?
        .to_owned();
    let compact_mac = device.mac.replace(':', "").to_ascii_lowercase();
    let colon_mac = compact_mac
        .as_bytes()
        .chunks(2)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or("00"))
        .collect::<Vec<_>>()
        .join(":");
    let now = Utc::now();
    let unix = now.timestamp().max(0).to_string();
    let utc = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    let gateway = gateway_candidate(device.ip);
    if request
        .body
        .windows(b"media_profile2".len())
        .any(|part| part == b"media_profile2")
    {
        replace_stream_uri(&mut text, device, DeviceSimulatorStreamKind::Sub);
    } else if request
        .body
        .windows(b"media_profile3".len())
        .any(|part| part == b"media_profile3")
    {
        replace_stream_uri(&mut text, device, DeviceSimulatorStreamKind::Third);
    } else {
        replace_stream_uri(&mut text, device, DeviceSimulatorStreamKind::Main);
    }
    let subscription_lifetime_seconds = 3_600;
    for (from, to) in [
        (
            "206.2.18.165:80".to_owned(),
            format!("{}:{http_port}", device.ip),
        ),
        ("48:ea:63:24:78:0c".into(), colon_mac),
        ("48ea6324780c".into(), compact_mac.clone()),
        ("6cf17e06eef6".into(), compact_mac),
        ("206.2.18.165".into(), device.ip.to_string()),
        ("206.2.18.166".into(), peer.ip().to_string()),
        ("206.2.1.1".into(), gateway.to_string()),
        (
            "210235C3NL3203000029_31".into(),
            format!("{}_{}", device.hardware_id, device.ip.octets()[3]),
        ),
        ("210235C1XMA161000144".into(), device.hardware_id.clone()),
        ("IPC244S-IR9-PF80-DT".into(), profile.identity.model.clone()),
        ("ECS-B300-I1@8-B-HD".into(), profile.identity.model.clone()),
        ("ECS-B300-I1@8-HD".into(), profile.identity.model.clone()),
        ("IA5664@PI".into(), profile.identity.model.clone()),
        (
            "IPC_G6102-B5025P22D1907".into(),
            profile.identity.firmware_version.clone(),
        ),
        (
            "NVR-B1227.3.23.211124".into(),
            profile.identity.firmware_version.clone(),
        ),
        (
            "NVR-B1224.2.5.210524".into(),
            profile.identity.firmware_version.clone(),
        ),
        ("1618228377".into(), unix.clone()),
        ("1618228378".into(), unix.clone()),
        (
            "1618228438".into(),
            (now.timestamp() + subscription_lifetime_seconds)
                .max(0)
                .to_string(),
        ),
        (
            "1628218912".into(),
            (now.timestamp() + 60).max(0).to_string(),
        ),
        ("2020-05-29T03:35:51Z".into(), utc),
    ] {
        text = text.replace(&from, &to);
    }
    if request.path.contains("/LAPI/V1.0/System/DeviceInfo") {
        let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|source| {
            runtime_error(
                "device_simulator.protocol.device_info_template_invalid",
                format!("device info response template is invalid: {source}"),
            )
        })?;
        if let Some(data) = value
            .get_mut("Response")
            .and_then(|response| response.get_mut("Data"))
            .and_then(serde_json::Value::as_object_mut)
        {
            if data.contains_key("DeviceType") {
                data.insert(
                    "DeviceType".into(),
                    serde_json::Value::from(profile.identity.device_type_enum),
                );
            }
            if data.contains_key("DeviceTypeV2") {
                data.insert(
                    "DeviceTypeV2".into(),
                    serde_json::Value::String(profile.identity.nickname.clone()),
                );
            }
        }
        text = serde_json::to_string(&value).map_err(|source| {
            runtime_error(
                "device_simulator.protocol.device_info_render_failed",
                format!("device info response could not be rendered: {source}"),
            )
        })?;
    }
    if text.len() > crate::device_simulator::http::MAX_HTTP_RESPONSE_BYTES {
        return Err(runtime_error(
            "device_simulator.protocol.response_size_exceeded",
            "rendered legacy response exceeds the HTTP response limit",
        ));
    }
    Ok(text.into_bytes())
}

fn replace_stream_uri(
    text: &mut String,
    device: &DeviceIdentityPreviewDto,
    stream: DeviceSimulatorStreamKind,
) {
    let Some(address) = device
        .streams
        .iter()
        .find(|address| address.stream == stream)
    else {
        return;
    };
    for placeholder in [
        "rtsp://206.2.18.165/media/video1",
        "rtsp://206.2.18.165:554/unicast/c1/s0/live",
        "rtsp://206.2.18.165:554/unicast/c1/s1/live",
        "rtsp://206.2.18.165:554/unicast/c1/s2/live",
    ] {
        *text = text.replace(placeholder, &address.url);
    }
}

fn response_content_type(request: &HttpRequest, body: &[u8]) -> String {
    if request.path.starts_with("/onvif/") {
        return "application/soap+xml; charset=utf-8".into();
    }
    match body
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
    {
        Some(b'{') | Some(b'[') => "application/json; charset=utf-8".into(),
        Some(b'<') => "application/xml; charset=utf-8".into(),
        _ => "text/plain; charset=utf-8".into(),
    }
}

fn render_discovery_template(
    bytes: &[u8],
    message_id: &str,
    http_port: u16,
    device: &DeviceIdentityPreviewDto,
    profile: &DeviceProfileV1,
) -> Result<Vec<u8>, ProtocolRuntimeError> {
    let request = HttpRequest {
        method: HttpMethod::Post,
        path: "/discovery".into(),
        query: None,
        headers: BTreeMap::new(),
        body: Vec::new(),
    };
    let mut rendered = String::from_utf8(render_legacy_template(
        bytes,
        &request,
        SocketAddr::new(IpAddr::V4(device.ip), 0),
        TargetPlatform::Ums,
        http_port,
        device,
        profile,
    )?)
    .map_err(|source| {
        runtime_error(
            "device_simulator.discovery.response_render_failed",
            format!("rendered discovery response is not UTF-8: {source}"),
        )
    })?;
    replace_xml_element_text(&mut rendered, "wsa:RelatesTo", &xml_escape(message_id));
    if rendered.is_empty() || rendered.len() > MAX_DISCOVERY_RESPONSE_BYTES {
        return Err(runtime_error(
            "device_simulator.discovery.response_size_invalid",
            "rendered discovery response is empty or exceeds the UDP limit",
        ));
    }
    Ok(rendered.into_bytes())
}

fn replace_xml_element_text(document: &mut String, element: &str, value: &str) {
    let start_token = format!("<{element}>");
    let end_token = format!("</{element}>");
    let Some(start) = document
        .find(&start_token)
        .map(|index| index + start_token.len())
    else {
        return;
    };
    let Some(end) = document[start..]
        .find(&end_token)
        .map(|index| start + index)
    else {
        return;
    };
    document.replace_range(start..end, value);
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn build_reviewed_static_sdp(
    device_ip: Ipv4Addr,
    route: &str,
    media: &crate::device_simulator::media::SharedMediaPack,
) -> Result<Vec<u8>, ProtocolRuntimeError> {
    let payload_type = media.manifest().payload_type;
    let clock_rate = media.manifest().clock_rate;
    let sps = media.parameter_set(ParameterSetKind::Sps).ok_or_else(|| {
        runtime_error("device_simulator.rtsp.sps_missing", "media pack has no SPS")
    })?;
    let pps = media.parameter_set(ParameterSetKind::Pps).ok_or_else(|| {
        runtime_error("device_simulator.rtsp.pps_missing", "media pack has no PPS")
    })?;
    build_reviewed_sdp_body(
        device_ip,
        route,
        payload_type,
        clock_rate,
        "64001f",
        sps,
        pps,
    )
}

fn build_reviewed_sdp(
    device_ip: Ipv4Addr,
    route: &str,
    payload_type: u8,
    clock_rate: u32,
    sps: &[u8],
    pps: &[u8],
) -> Result<Vec<u8>, ProtocolRuntimeError> {
    if sps.len() < 4 || sps[0] & 0x1f != 7 || pps.is_empty() || pps[0] & 0x1f != 8 {
        return Err(runtime_error(
            "device_simulator.rtsp.parameter_sets_invalid",
            "H.264 SDP requires valid SPS and PPS NAL units",
        ));
    }
    let profile_level_id = format!("{:02x}{:02x}{:02x}", sps[1], sps[2], sps[3]);
    build_reviewed_sdp_body(
        device_ip,
        route,
        payload_type,
        clock_rate,
        &profile_level_id,
        sps,
        pps,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_reviewed_sdp_body(
    device_ip: Ipv4Addr,
    route: &str,
    payload_type: u8,
    clock_rate: u32,
    profile_level_id: &str,
    sps: &[u8],
    pps: &[u8],
) -> Result<Vec<u8>, ProtocolRuntimeError> {
    // Keep the SDP shape emitted by the legacy IPCRtsp server, including its
    // advertised metadata track. The legacy capture also contains no PT 107
    // metadata RTP; the service accepts that track's SETUP but sends video.
    let control_route = if route.ends_with("/video") {
        route.to_owned()
    } else {
        "/media/video1/video".to_owned()
    };
    let control_url = format!("rtsp://{device_ip}{control_route}");
    let metadata_url = format!("rtsp://{device_ip}/media/video1/metadata");
    let body = format!(
        "v=0\r\no=- 1001 1 IN IP4 {device_ip}\r\ns=VCP IPC Realtime stream\r\nm=video 0 RTP/AVP {payload_type}\r\nc=IN IP4 {device_ip}\r\na=control:{control_url}\r\na=rtpmap:{payload_type} H264/{clock_rate}\r\na=fmtp:{payload_type} profile-level-id={profile_level_id}; packetization-mode=1; sprop-parameter-sets={},{}\r\na=recvonly\r\nm=application 0 RTP/AVP 107\r\nc=IN IP4 {device_ip}\r\na=control:{metadata_url}\r\na=rtpmap:107 vnd.onvif.metadata/90000\r\na=fmtp:107 DecoderTag=h3c-v3 RTCP=0\r\na=recvonly\r\n",
        BASE64_STANDARD.encode(sps),
        BASE64_STANDARD.encode(pps),
    );
    Ok(body.into_bytes())
}

fn gateway_candidate(address: Ipv4Addr) -> Ipv4Addr {
    let [first, second, _, _] = address.octets();
    Ipv4Addr::new(first, second, 1, 1)
}

fn parse_profile_id(value: &str) -> Result<FirstReleaseProfileId, ProtocolRuntimeError> {
    match value {
        "ipc-structured" => Ok(FirstReleaseProfileId::IpcStructured),
        _ => Err(runtime_error(
            "device_simulator.protocol.profile_unknown",
            format!("unknown first-release profile '{value}'"),
        )),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn runtime_error(code: &'static str, message: impl Into<String>) -> ProtocolRuntimeError {
    ProtocolRuntimeError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::alarms::{image_reference_token, ImageAssetRef, ImageExtension};
    use crate::device_simulator::api::{
        DeviceGroupDraft, PlatformAccessMode, RtspPorts, StreamRuntimeConfig, StreamTransport,
        TargetPlatformConfig, TargetPlatformServer,
    };
    use crate::device_simulator::assets::catalog::DeviceKind;
    use crate::device_simulator::profiles::schema::{
        EvidenceStatus, EvidenceTopic, ProfileEvidence, ProfileHandlerBindings,
        ProfileIdentityFacts, PROFILE_SCHEMA_VERSION,
    };
    use crate::device_simulator::runtime_assets::PinnedPackDirectory;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn read_test_rtsp_response(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            assert!(
                response.len() < 64 * 1024,
                "RTSP response headers are bounded"
            );
            response.push(
                tokio::time::timeout(Duration::from_secs(5), stream.read_u8())
                    .await
                    .expect("RTSP response header timed out")
                    .unwrap(),
            );
        }
        let headers = String::from_utf8_lossy(&response);
        let body_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        let header_length = response.len();
        response.resize(header_length + body_length, 0);
        tokio::time::timeout(
            Duration::from_secs(5),
            stream.read_exact(&mut response[header_length..]),
        )
        .await
        .expect("RTSP response body timed out")
        .unwrap();
        response
    }

    fn profile(id: FirstReleaseProfileId) -> DeviceProfileV1 {
        DeviceProfileV1 {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: id.as_str().into(),
            device_kind: id.device_kind(),
            legacy_device_type: id.legacy_device_type().into(),
            identity: ProfileIdentityFacts {
                model: "HIC6881-IR@X38-L-WSGB-VC".into(),
                firmware_version: "STATIC-REVIEW-1".into(),
                nickname: "STATIC".into(),
                device_type_enum: 0,
            },
            supported_platforms: vec![TargetPlatform::Ums],
            handlers: ProfileHandlerBindings {
                identity: "legacy.identity.v1".into(),
                discovery: "ws_discovery.ipc.v1".into(),
                http: "http.profile.v1".into(),
                rtsp: "rtsp.tcp_interleaved.v1".into(),
                alarms: vec!["alarm.structured.v1".into()],
            },
            evidence: [
                EvidenceTopic::Identity,
                EvidenceTopic::Discovery,
                EvidenceTopic::Http,
                EvidenceTopic::Rtsp,
                EvidenceTopic::Alarm,
            ]
            .into_iter()
            .map(|topic| ProfileEvidence {
                topic,
                status: EvidenceStatus::ReviewedStatic,
                sources: vec!["script/evidence.py".into()],
                verified_platforms: vec![],
                intentional_changes: vec![],
            })
            .collect(),
        }
    }

    fn device(profile_id: &str) -> DeviceIdentityPreviewDto {
        DeviceIdentityPreviewDto {
            device_id: "group-0001".into(),
            group_id: "group".into(),
            profile_id: profile_id.into(),
            device_kind: DeviceKind::Ipc,
            ip: "192.0.2.10".parse().unwrap(),
            mac: "48ea6324780c".into(),
            serial_number: "SIM123".into(),
            hardware_id: "210235SIM0001".into(),
            streams: vec![crate::device_simulator::api::DeviceStreamAddress {
                device_id: "group-0001".into(),
                channel_id: None,
                stream: DeviceSimulatorStreamKind::Main,
                url: "rtsp://192.0.2.10:554/media/video1".into(),
            }],
        }
    }

    fn device_at(
        profile_id: &str,
        group_id: &str,
        device_id: &str,
        ip: &str,
    ) -> DeviceIdentityPreviewDto {
        let mut device = device(profile_id);
        device.group_id = group_id.into();
        device.device_id = device_id.into();
        device.ip = ip.parse().unwrap();
        device
    }

    fn request(
        method: HttpMethod,
        path: &str,
        soap_action: Option<&str>,
        body: &[u8],
    ) -> HttpRequest {
        let mut headers = BTreeMap::new();
        if let Some(action) = soap_action {
            headers.insert("soapaction".into(), action.into());
        }
        HttpRequest {
            method,
            path: path.into(),
            query: None,
            headers,
            body: body.to_vec(),
        }
    }

    fn reserve_tcp_ports(count: usize) -> Vec<u16> {
        let probes = (0..count)
            .map(|_| std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
            .collect::<Vec<_>>();
        probes
            .iter()
            .map(|probe| probe.local_addr().unwrap().port())
            .collect()
    }

    #[test]
    fn routes_source_confirmed_common_requests_without_guessing_file_paths() {
        let onvif = request(
            HttpMethod::Post,
            "/onvif/device_service",
            Some("\"http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation\""),
            b"<GetDeviceInformation/>",
        );
        assert_eq!(
            resolve_http_template(&onvif).unwrap().path,
            "xml/Common/GetDeviceInformation.xml"
        );
        let device_info = request(HttpMethod::Get, "/LAPI/V1.0/System/DeviceInfo", None, b"");
        assert_eq!(
            resolve_http_template(&device_info).unwrap().path,
            "xml/Common/System-DeviceInfo.xml"
        );
        let subscription = request(
            HttpMethod::Post,
            "/LAPI/V1.0/System/Event/Subscription",
            None,
            b"{}",
        );
        assert_eq!(
            resolve_http_template(&subscription).unwrap().path,
            "xml/Common/Event-Subscription.xml"
        );
        let keep_alive = request(
            HttpMethod::Put,
            "/LAPI/V1.0/System/Event/Subscription/1/KeepAlive",
            None,
            b"{}",
        );
        assert_eq!(
            resolve_http_template(&keep_alive).unwrap().path,
            "xml/Common/1-KeepAlive.xml"
        );
    }

    #[test]
    fn serves_only_content_addressed_cached_alarm_pictures() {
        let root = TempDir::new().unwrap();
        let user_root = root.path().join("user");
        std::fs::create_dir_all(&user_root).unwrap();
        let bytes = b"reviewed-picture";
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        std::fs::write(user_root.join(format!("{sha256}.jpg")), bytes).unwrap();
        let reference = ImageAssetRef::UserAsset {
            image_id: sha256.clone(),
            extension: ImageExtension::Jpg,
            sha256,
            size: bytes.len() as u64,
        };
        let cache = ImageCache::load_at_start(
            [reference.clone()],
            &root.path().join("packs"),
            &user_root,
            &BTreeMap::new(),
        )
        .unwrap();
        let mut picture = request(HttpMethod::Get, "/LAPI/V1.0/System/Picture", None, b"");
        picture.query = Some(format!(
            "Type=23&Index={}&Size={}",
            image_reference_token(&reference),
            bytes.len()
        ));
        let response = picture_response(&picture, &cache);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "image/jpeg");
        assert_eq!(response.body, bytes);

        picture.query = Some(format!("Type=23&Index={}&Size=1", "0".repeat(64)));
        assert_eq!(picture_response(&picture, &cache).status, 404);
    }

    #[test]
    fn renders_ums_device_type_and_subscription_lifetime() {
        let template = br#"{
            "Response":{"Data":{"DeviceType":0,"DeviceModel":"IPC244S-IR9-PF80-DT"}},
            "CurrentTime":1618228378,
            "TerminationTime":1618228438
        }"#;
        let request = request(HttpMethod::Get, "/LAPI/V1.0/System/DeviceInfo", None, b"");
        let structured_profile = profile(FirstReleaseProfileId::IpcStructured);
        let body = render_legacy_template(
            template,
            &request,
            "192.0.2.20:50000".parse().unwrap(),
            TargetPlatform::Ums,
            80,
            &device("ipc-structured"),
            &structured_profile,
        )
        .unwrap();
        let rendered: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(rendered["Response"]["Data"]["DeviceType"].as_u64(), Some(0));
        assert_eq!(
            rendered["Response"]["Data"]["DeviceModel"].as_str(),
            Some("HIC6881-IR@X38-L-WSGB-VC")
        );
        assert_eq!(
            rendered["TerminationTime"].as_i64().unwrap()
                - rendered["CurrentTime"].as_i64().unwrap(),
            3_600
        );
    }

    #[test]
    fn subscription_learning_accepts_any_advertised_port_and_captures_the_endpoint() {
        let learned: SharedLearnedAlarmEndpoint =
            std::sync::Arc::new(parking_lot::RwLock::new(None));
        // Captured from a real UMS deployment: the receiver port sits far
        // outside the legacy 55000..55999 range, and the platform names the
        // receiver host explicitly.
        let post = request(
            HttpMethod::Post,
            "/LAPI/V1.0/System/Event/Subscription",
            None,
            b"{\"AddressType\":0,\"IPAddress\":\"192.115.1.55\",\"Port\":22815,\"Duration\":600}",
        );
        assert_eq!(learn_subscription_endpoint(&post, &learned), Some(22815));
        let endpoint = learned.read().clone().unwrap();
        assert_eq!(endpoint.port, 22815);
        assert_eq!(endpoint.host, Some(Ipv4Addr::new(192, 115, 1, 55)));
        assert_eq!(endpoint.duration_secs, Some(600));
        assert_eq!(
            endpoint.expires_at_ms(),
            Some(endpoint.learned_at_ms + 600_000)
        );

        // A zero port is not a destination and must not replace what was learned.
        let zero = request(
            HttpMethod::Post,
            "/LAPI/V1.0/System/Event/Subscription",
            None,
            b"{\"Port\":0,\"Duration\":600}",
        );
        assert_eq!(learn_subscription_endpoint(&zero, &learned), None);
        assert_eq!(learned.read().as_ref().unwrap().port, 22815);

        // Non-subscription traffic never touches the learned endpoint.
        let unrelated = request(
            HttpMethod::Post,
            "/LAPI/V1.0/System/DeviceBasicInfo",
            None,
            b"{\"Port\":9000}",
        );
        assert_eq!(learn_subscription_endpoint(&unrelated, &learned), None);
        assert_eq!(learned.read().as_ref().unwrap().port, 22815);

        // An unspecified receiver address leaves the configured host in effect.
        let unspecified = request(
            HttpMethod::Post,
            "/LAPI/V1.0/System/Event/Subscription",
            None,
            b"{\"IPAddress\":\"0.0.0.0\",\"Port\":30000}",
        );
        assert_eq!(
            learn_subscription_endpoint(&unspecified, &learned),
            Some(30000)
        );
        let endpoint = learned.read().clone().unwrap();
        assert_eq!(endpoint.host, None);
        assert_eq!(endpoint.duration_secs, None);
    }

    #[tokio::test]
    async fn approved_release_serves_reviewed_http_and_rtsp_fixtures_when_configured() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let root = PathBuf::from(root);
        let pins = ["protocol-core", "media-h264-live", "ipc-structured"]
            .into_iter()
            .map(|id| PinnedPackDirectory {
                id: id.into(),
                version: version.clone(),
                directory: root.join(id).join(&version),
            })
            .collect::<Vec<_>>();
        let assets = Arc::new(RuntimeAssetLayout::load(&pins, &["ipc-structured".into()]).unwrap());
        let ports = reserve_tcp_ports(4);
        let request = SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Ums,
                servers: vec![TargetPlatformServer {
                    id: "receiver".into(),
                    host: "127.0.0.1".into(),
                    port: 18080,
                }],
                access_mode: PlatformAccessMode::Open,
                alarm_receiver_url: None,
                alarm_receiver_port: Some(55_025),
            },
            interface_id: "loopback-fixture".into(),
            start_ip: Ipv4Addr::LOCALHOST,
            device_ips: vec![],
            subnet_prefix: 8,
            device_http_port: ports[0],
            rtsp_ports: RtspPorts {
                main: ports[1],
                sub: ports[2],
                third: ports[3],
            },
            _legacy_allow_local_player_access: true,
            media_theme_id: crate::device_simulator::api::DEFAULT_MEDIA_THEME_ID.into(),
            groups: vec![DeviceGroupDraft {
                id: "smart".into(),
                profile_id: "ipc-structured".into(),
                count: 1,
            }],
            stream: StreamRuntimeConfig {
                transport: StreamTransport::TcpInterleaved,
                enabled_streams: vec![
                    DeviceSimulatorStreamKind::Main,
                    DeviceSimulatorStreamKind::Sub,
                    DeviceSimulatorStreamKind::Third,
                ],
                audio_enabled: false,
                time_watermark_enabled: true,
            },
        };
        let preview = crate::device_simulator::api::preview_devices(&request).unwrap();
        let runtime = ProtocolRuntime::start(ProtocolRuntimeConfig {
            request: request.clone(),
            preview,
            assets,
            picture_cache: Arc::new(parking_lot::RwLock::new(ImageCache::default())),
            learned_endpoint: Arc::new(parking_lot::RwLock::new(None)),
            access_policy: PlatformAccessPolicy::open(),
            enable_discovery: false,
        })
        .await
        .unwrap();

        let body = b"<GetDeviceInformation/>";
        let mut http = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, ports[0]))
            .await
            .unwrap();
        http.write_all(
            format!(
                "POST /onvif/device_service HTTP/1.1\r\nHost: 127.0.0.1\r\nSOAPAction: \"http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation\"\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        http.write_all(body).await.unwrap();
        let mut http_response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), http.read_to_end(&mut http_response))
            .await
            .unwrap()
            .unwrap();
        let http_response = String::from_utf8_lossy(&http_response);
        assert!(http_response.contains("HTTP/1.1 200 OK"));
        assert!(http_response.contains("IPC3615SB-ADF28KM-I0"));

        let mut rtsp = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, ports[1]))
            .await
            .unwrap();
        rtsp.write_all(b"OPTIONS * RTSP/1.0\r\nCSeq: 1\r\n\r\n")
            .await
            .unwrap();
        let rtsp_response = read_test_rtsp_response(&mut rtsp).await;
        assert!(String::from_utf8_lossy(&rtsp_response).contains("RTSP/1.0 200 OK"));
        let stats = runtime.stats();
        assert_eq!(stats.active_rtsp_clients, 1);
        assert!(stats.bytes_sent >= rtsp_response.len() as u64);
        assert!(stats.outbound_bitrate_kbps > 0);

        rtsp.write_all(b"DESCRIBE rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 2\r\n\r\n")
            .await
            .unwrap();
        let describe = read_test_rtsp_response(&mut rtsp).await;
        let describe = String::from_utf8_lossy(&describe);
        assert!(describe.contains("sprop-parameter-sets="));
        assert!(describe.contains("profile-level-id="));

        rtsp.write_all(b"SETUP rtsp://127.0.0.1/media/video1/video RTSP/1.0\r\nCSeq: 3\r\nTransport: RTP/AVP/TCP;unicast;interleaved=0-1\r\n\r\n")
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&read_test_rtsp_response(&mut rtsp).await)
                .contains("RTSP/1.0 200 OK")
        );
        rtsp.write_all(b"PLAY rtsp://127.0.0.1/media/video1 RTSP/1.0\r\nCSeq: 4\r\n\r\n")
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&read_test_rtsp_response(&mut rtsp).await)
                .contains("RTSP/1.0 200 OK")
        );
        let mut saw_sps = false;
        for _ in 0..8 {
            let mut interleaved = [0u8; 4];
            tokio::time::timeout(Duration::from_secs(5), rtsp.read_exact(&mut interleaved))
                .await
                .unwrap()
                .unwrap();
            assert_eq!((interleaved[0], interleaved[1]), (b'$', 0));
            let packet_length = u16::from_be_bytes([interleaved[2], interleaved[3]]) as usize;
            let mut packet = vec![0u8; packet_length];
            tokio::time::timeout(Duration::from_secs(5), rtsp.read_exact(&mut packet))
                .await
                .unwrap()
                .unwrap();
            assert!(packet.len() > 12);
            saw_sps |= packet[12] & 0x1f == 7;
            if saw_sps {
                break;
            }
        }
        assert!(saw_sps, "first encoded keyframe carries its active SPS");

        drop(rtsp);
        runtime.stop().await.unwrap();
    }

    #[test]
    fn legacy_template_replacement_is_device_scoped_and_stream_aware() {
        let request = request(
            HttpMethod::Post,
            "/onvif/media_service",
            None,
            b"<trt:GetStreamUri><ProfileToken>media_profile1</ProfileToken></trt:GetStreamUri>",
        );
        let rendered = render_legacy_template(
            b"206.2.18.165:80 48:ea:63:24:78:0c 210235C1XMA161000144 IPC244S-IR9-PF80-DT rtsp://206.2.18.165/media/video1",
            &request,
            "198.51.100.2:50000".parse().unwrap(),
            TargetPlatform::Ums,
            81,
            &device("ipc-structured"),
            &profile(FirstReleaseProfileId::IpcStructured),
        )
        .unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        assert!(rendered.contains("192.0.2.10:81"));
        assert!(rendered.contains("48:ea:63:24:78:0c"));
        assert!(rendered.contains("210235SIM0001"));
        assert!(rendered.contains("HIC6881-IR@X38-L-WSGB-VC"));
        assert!(rendered.contains("rtsp://192.0.2.10:554/media/video1"));
        assert!(!rendered.contains("206.2.18.165"));
    }

    #[test]
    fn reviewed_sdp_derives_profile_level_id_from_the_active_encoder_sps() {
        let sdp = build_reviewed_sdp(
            "192.0.2.10".parse().unwrap(),
            "/media/video1",
            96,
            90_000,
            &[0x67, 0x42, 0xc0, 0x1f],
            &[0x68, 0xce, 0x06],
        )
        .unwrap();
        let sdp = String::from_utf8(sdp).unwrap();
        assert!(sdp.contains("profile-level-id=42c01f"));
        assert!(sdp.contains("sprop-parameter-sets=Z0LAHw==,aM4G"));
    }

    #[test]
    fn reviewed_sdp_rejects_invalid_encoder_parameter_sets() {
        let error = build_reviewed_sdp(
            "192.0.2.10".parse().unwrap(),
            "/media/video1",
            96,
            90_000,
            &[0x68, 0x42, 0xc0, 0x1f],
            &[0x68],
        )
        .unwrap_err();
        assert_eq!(error.code, "device_simulator.rtsp.parameter_sets_invalid");
    }

    #[test]
    fn discovery_message_id_is_xml_escaped_before_insertion() {
        let rendered = render_discovery_template(
            b"<wsa:RelatesTo>uuid:old</wsa:RelatesTo><XAddr>http://206.2.18.165:80</XAddr>",
            "uuid:a&b",
            81,
            &device("ipc-structured"),
            &profile(FirstReleaseProfileId::IpcStructured),
        )
        .unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        assert!(rendered.contains("uuid:a&amp;b"));
        assert!(rendered.contains("http://192.0.2.10:81"));
    }

    #[test]
    fn legacy_discovery_batches_once_per_profile_and_preserves_device_order() {
        // Every device now shares the single structured profile, so the whole
        // fleet forms one batch in the order the devices were planned.
        let devices = vec![
            device_at("ipc-structured", "group-a", "group-a-0001", "192.0.2.10"),
            device_at("ipc-structured", "group-b", "group-b-0001", "192.0.2.20"),
            device_at("ipc-structured", "group-a", "group-a-0002", "192.0.2.30"),
        ];

        let batches = legacy_discovery_batches(&devices);
        assert_eq!(batches.len(), 1);
        assert_eq!(
            batches[0]
                .iter()
                .map(|device| device.device_id.as_str())
                .collect::<Vec<_>>(),
            ["group-a-0001", "group-b-0001", "group-a-0002"]
        );
        assert_eq!(batches[0][0].ip, "192.0.2.10".parse::<Ipv4Addr>().unwrap());
    }
}
