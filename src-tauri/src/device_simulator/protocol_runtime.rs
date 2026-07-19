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
use crate::device_simulator::media::ParameterSetKind;
use crate::device_simulator::profiles::schema::DeviceProfileV1;
use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use crate::device_simulator::rtsp::routes::{
    plan_rtsp_routes, RtspPorts as PlannedRtspPorts, RtspStreamKind,
};
use crate::device_simulator::rtsp::service::{
    start_rtsp_server, RtspEndpointConfig, RtspServerHandle, RtspStreamSource,
};
use crate::device_simulator::runtime_assets::{RuntimeAssetLayout, RuntimeMediaKind};
use crate::device_simulator::telemetry::ProtocolFailureMetrics;
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
    device_metrics: BTreeMap<String, Arc<DeviceProtocolMetrics>>,
}

impl ProtocolRuntime {
    pub async fn start(config: ProtocolRuntimeConfig) -> Result<Self, ProtocolRuntimeError> {
        validate_runtime_config(&config)?;
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
            device_metrics: BTreeMap::new(),
        };

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

            if config.enable_discovery {
                match start_discovery_task(
                    Arc::clone(&config.assets),
                    config.request.device_http_port,
                    device.clone(),
                    profile.clone(),
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

            let rtsp_ports = PlannedRtspPorts {
                main: config.request.rtsp_ports.main,
                sub: config.request.rtsp_ports.sub,
                third: config.request.rtsp_ports.third,
            };
            let plans = plan_rtsp_routes(
                device.device_kind,
                device.ip,
                rtsp_ports,
                device.channel_count,
            )
            .map_err(|source| runtime_error(source.code, source.message))?;
            for plan in plans {
                let media_kind = match plan.stream {
                    RtspStreamKind::Main => RuntimeMediaKind::Main,
                    RtspStreamKind::Sub => RuntimeMediaKind::Sub,
                    RtspStreamKind::Third => RuntimeMediaKind::Third,
                };
                let media = config.assets.media(media_kind);
                let mut routes = BTreeMap::new();
                for route in plan
                    .routes
                    .iter()
                    .filter(|route| route.evidence.runtime_activation_allowed())
                {
                    let sdp = build_reviewed_static_sdp(device.ip, &route.path, media.as_ref())?;
                    let source = RtspStreamSource::from_media(
                        format!("{}:{:?}", device.device_id, plan.stream),
                        Arc::<[u8]>::from(sdp),
                        Arc::clone(&media),
                        128,
                        1_200,
                    )
                    .map_err(|source| runtime_error(source.code, source.message))?;
                    routes.insert(route.path.clone(), source);
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
                    bitrate_bps: media.actual_bitrate_bps(),
                    handle,
                });
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

async fn start_http_task(
    assets: Arc<RuntimeAssetLayout>,
    picture_cache: SharedImageCache,
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
                            if clients.len() >= HTTP_CLIENT_TASK_LIMIT {
                                drop(stream);
                                continue;
                            }
                            let listener = Arc::clone(&listener);
                            let assets = Arc::clone(&assets);
                            let picture_cache = Arc::clone(&picture_cache);
                            let device = device.clone();
                            let profile = profile.clone();
                            let device_metrics = Arc::clone(&device_metrics);
                            clients.spawn(async move {
                                let _active = ActiveHttpConnection::new(device_metrics);
                                if let Err(source) = serve_http_connection(
                                    listener,
                                    assets,
                                    picture_cache,
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
    let response =
        if request.method == HttpMethod::Get && request.path == "/LAPI/V1.0/System/Picture" {
            picture_response(&request, &picture_cache.read())
        } else {
            match resolve_http_template(profile_id, &request) {
                Some(selection) => {
                    let bytes = assets
                        .read_profile_or_core(profile_id, &selection.path)
                        .map_err(|source| runtime_error(source.code, source.message))?;
                    let body = render_legacy_template(
                        &bytes, &request, peer, platform, http_port, &device, &profile,
                    )?;
                    HttpResponse {
                        status: if matches!(
                            profile_id,
                            FirstReleaseProfileId::NvrCommon | FirstReleaseProfileId::NvrVehicle
                        ) && request.method == HttpMethod::Get
                            && request.path.contains("/Channels/0/Smart/Capabilities")
                        {
                            599
                        } else {
                            200
                        },
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
    http_port: u16,
    device: DeviceIdentityPreviewDto,
    profile: DeviceProfileV1,
) -> Result<(ServiceTask, String), ProtocolRuntimeError> {
    let profile_id = parse_profile_id(&device.profile_id)?;
    let template_path = match profile_id {
        FirstReleaseProfileId::NvrCommon | FirstReleaseProfileId::NvrVehicle => {
            "xml/Common/search-aibox.xml"
        }
        FirstReleaseProfileId::IpcFaceAccess => "xml/Common/search-acs.xml",
        FirstReleaseProfileId::IpcCustom
        | FirstReleaseProfileId::IpcSmart
        | FirstReleaseProfileId::IpcStructured => "xml/Common/search.xml",
    };
    let template = assets
        .read_from_pack("protocol-core", template_path)
        .map_err(|source| runtime_error(source.code, source.message))?;
    let metrics = Arc::new(ProtocolFailureMetrics::default());
    let listener = Arc::new(
        DiscoveryListener::bind(DiscoveryBindPlan::legacy_ws_discovery(device.ip), metrics)
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
                            let mut response = match render_discovery_template(
                                &template,
                                &probe.message_id,
                                http_port,
                                &device,
                                &profile,
                            ) {
                                Ok(response) => response,
                                Err(source) => {
                                    log::warn!("device simulator discovery render failed: {source}");
                                    continue;
                                }
                            };
                            if response.len() > MAX_DISCOVERY_RESPONSE_BYTES {
                                log::warn!("device simulator discovery response exceeded the safety limit");
                                continue;
                            }
                            if let Err(source) = listener
                                .send_response(&probe, &response, now_ms(), PROTOCOL_LOG_INTERVAL_MS)
                                .await
                            {
                                log::warn!("device simulator discovery response failed: {source}");
                            }
                            response.clear();
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpTemplateSelection {
    path: String,
}

fn resolve_http_template(
    profile: FirstReleaseProfileId,
    request: &HttpRequest,
) -> Option<HttpTemplateSelection> {
    let nvr = matches!(
        profile,
        FirstReleaseProfileId::NvrCommon | FirstReleaseProfileId::NvrVehicle
    );
    match request.method {
        HttpMethod::Post if request.path.starts_with("/onvif/") => {
            let operation = soap_operation(request)?;
            if profile == FirstReleaseProfileId::IpcFaceAccess && operation == "GetScopes" {
                return Some(HttpTemplateSelection {
                    path: "xml/Common/GetScopes-ACS.xml".into(),
                });
            }
            let name = if nvr && operation == "GetVideoSources" {
                "wsdlGetVideoSources"
            } else {
                operation.as_str()
            };
            Some(HttpTemplateSelection {
                path: format!("xml/{}/{}.xml", if nvr { "AIBOX" } else { "Common" }, name),
            })
        }
        HttpMethod::Post if request.path.contains("Event/Subscription") => {
            let path = match profile {
                FirstReleaseProfileId::IpcCustom => "xml/Custom/Event-Subscription.xml",
                FirstReleaseProfileId::NvrCommon | FirstReleaseProfileId::NvrVehicle => {
                    "xml/AIBOX/Event-Subscription.xml"
                }
                FirstReleaseProfileId::IpcSmart
                | FirstReleaseProfileId::IpcStructured
                | FirstReleaseProfileId::IpcFaceAccess => "xml/Common/Event-Subscription.xml",
            };
            Some(HttpTemplateSelection { path: path.into() })
        }
        HttpMethod::Post => body_operation(request).map(|operation| HttpTemplateSelection {
            path: format!(
                "xml/{}/{}.xml",
                if nvr { "AIBOX" } else { "Common" },
                operation
            ),
        }),
        HttpMethod::Get => {
            if profile == FirstReleaseProfileId::IpcFaceAccess
                && request.path == "/LAPI/V1.0/Smart/FaceRecognition/Feature/Version"
            {
                return Some(HttpTemplateSelection {
                    path: "xml/Common/Smart-FaceRecognition-Feature-Version.xml".into(),
                });
            }
            if profile == FirstReleaseProfileId::IpcFaceAccess
                && request.path.contains("/Door/System/ChannelDetailInfos")
            {
                return Some(HttpTemplateSelection {
                    path: "xml/Common/Door-System-ChannelDetailInfos.xml".into(),
                });
            }
            if profile == FirstReleaseProfileId::IpcCustom
                && request.path.contains("Event/Subscription/Capabilities")
            {
                return Some(HttpTemplateSelection {
                    path: "xml/Custom/Subscription-Capabilities.xml".into(),
                });
            }
            if profile == FirstReleaseProfileId::IpcSmart
                && request.path.contains("Smart/Capabilities")
            {
                return Some(HttpTemplateSelection {
                    path: "xml/Smart/Smart-Capabilities.xml".into(),
                });
            }
            let name = last_two_path_segments(&request.path)?;
            if profile == FirstReleaseProfileId::NvrVehicle
                && (request.path.contains("Event/Subscription/Capabilities")
                    || request
                        .path
                        .contains("Smart/Management/AlarmRelatedDataCapabilities")
                    || request.path.contains("Smart/Management/Capabilities"))
            {
                return Some(HttpTemplateSelection {
                    path: format!("xml/Vehicle/{name}.xml"),
                });
            }
            if nvr && request.path.contains("Channels/0/Smart/Capabilities") {
                return Some(HttpTemplateSelection {
                    path: "xml/AIBOX/Smart-Capabilities-0.xml".into(),
                });
            }
            Some(HttpTemplateSelection {
                path: format!("xml/{}/{}.xml", if nvr { "AIBOX" } else { "Common" }, name),
            })
        }
        HttpMethod::Put => {
            let base = if nvr { "AIBOX" } else { "Common" };
            let name = if request.path.contains("KeepAlive") {
                last_two_path_segments(&request.path)?
            } else if nvr && request.path.contains("Event/Subscription") {
                "Event-Subscription-1".into()
            } else if nvr && request.path.contains("System/Time") {
                "System-Time".into()
            } else {
                "PUT".into()
            };
            Some(HttpTemplateSelection {
                path: format!("xml/{base}/{name}.xml"),
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
    let subscription_lifetime_seconds = match device.device_kind {
        crate::device_simulator::assets::catalog::DeviceKind::Ipc => 3_600,
        crate::device_simulator::assets::catalog::DeviceKind::Nvr => 60,
    };
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
    if let Some(channels) = device.channel_count {
        text = text.replace(
            "VideoSourceNumber/8",
            &format!("VideoSourceNumber/{channels}"),
        );
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
    if device.device_kind == crate::device_simulator::assets::catalog::DeviceKind::Nvr {
        render_nvr_channel_metadata(&mut text, request, device)?;
    }
    if text.len() > crate::device_simulator::http::MAX_HTTP_RESPONSE_BYTES {
        return Err(runtime_error(
            "device_simulator.protocol.response_size_exceeded",
            "rendered legacy response exceeds the HTTP response limit",
        ));
    }
    Ok(text.into_bytes())
}

fn render_nvr_channel_metadata(
    text: &mut String,
    request: &HttpRequest,
    device: &DeviceIdentityPreviewDto,
) -> Result<(), ProtocolRuntimeError> {
    let channel_count = device.channel_count.unwrap_or(0);
    if request.path.contains("System/ChannelDetailInfos") {
        let mut value: serde_json::Value = serde_json::from_str(text).map_err(|source| {
            runtime_error(
                "device_simulator.protocol.channel_details_template_invalid",
                format!("NVR channel details template is invalid: {source}"),
            )
        })?;
        let data = value
            .pointer_mut("/Response/Data")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| {
                runtime_error(
                    "device_simulator.protocol.channel_details_template_invalid",
                    "NVR channel details template has no Response.Data object",
                )
            })?;
        let base = data
            .get("DetailInfos")
            .and_then(serde_json::Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .ok_or_else(|| {
                runtime_error(
                    "device_simulator.protocol.channel_details_template_invalid",
                    "NVR channel details template has no base channel",
                )
            })?;
        let channels = (1..=channel_count)
            .map(|channel_id| {
                let mut channel = base.clone();
                if let Some(map) = channel.as_object_mut() {
                    map.insert("ID".into(), serde_json::Value::from(channel_id));
                    map.insert(
                        "Name".into(),
                        serde_json::Value::String(format!("{}_V_{channel_id}", device.ip)),
                    );
                }
                channel
            })
            .collect::<Vec<_>>();
        data.insert("Nums".into(), serde_json::Value::from(channel_count));
        data.insert("DetailInfos".into(), serde_json::Value::Array(channels));
        *text = serde_json::to_string(&value).map_err(|source| {
            runtime_error(
                "device_simulator.protocol.channel_details_render_failed",
                format!("NVR channel details could not be rendered: {source}"),
            )
        })?;
    }
    if request
        .body
        .windows(b"GetVideoSources".len())
        .any(|part| part == b"GetVideoSources")
    {
        resize_onvif_sources(text, "trt:VideoSources", channel_count, false)?;
    }
    if request
        .body
        .windows(b"GetAudioSources".len())
        .any(|part| part == b"GetAudioSources")
    {
        resize_onvif_sources(text, "trt:AudioSources", channel_count, true)?;
    }
    Ok(())
}

fn resize_onvif_sources(
    document: &mut String,
    element: &str,
    channel_count: u16,
    includes_device_source: bool,
) -> Result<(), ProtocolRuntimeError> {
    let blocks = xml_element_blocks(document, element);
    if blocks.is_empty() {
        return Err(runtime_error(
            "device_simulator.protocol.onvif_sources_template_invalid",
            format!("ONVIF response has no {element} elements"),
        ));
    }
    let desired = usize::from(channel_count) + usize::from(includes_device_source);
    let templates = blocks
        .iter()
        .map(|(start, end)| document[*start..*end].to_owned())
        .collect::<Vec<_>>();
    let mut rendered = String::new();
    for index in 0..desired {
        if let Some(existing) = templates.get(index) {
            rendered.push_str(existing);
            continue;
        }
        let template_index = usize::from(includes_device_source).min(templates.len() - 1);
        let channel_number = if includes_device_source {
            index
        } else {
            index + 1
        };
        rendered.push_str(&replace_xml_token(
            &templates[template_index],
            &format!("{channel_number:03}00"),
        )?);
    }
    let start = blocks[0].0;
    let end = blocks[blocks.len() - 1].1;
    document.replace_range(start..end, &rendered);
    Ok(())
}

fn xml_element_blocks(document: &str, element: &str) -> Vec<(usize, usize)> {
    let open = format!("<{element}");
    let close = format!("</{element}>");
    let mut blocks = Vec::new();
    let mut offset = 0;
    while let Some(relative_start) = document[offset..].find(&open) {
        let start = offset + relative_start;
        let Some(relative_end) = document[start..].find(&close) else {
            break;
        };
        let end = start + relative_end + close.len();
        blocks.push((start, end));
        offset = end;
    }
    blocks
}

fn replace_xml_token(block: &str, token: &str) -> Result<String, ProtocolRuntimeError> {
    let marker = "token=\"";
    let start = block
        .find(marker)
        .map(|index| index + marker.len())
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.protocol.onvif_sources_template_invalid",
                "ONVIF source element has no token attribute",
            )
        })?;
    let end = block[start..]
        .find('"')
        .map(|index| start + index)
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.protocol.onvif_sources_template_invalid",
                "ONVIF source token attribute is not closed",
            )
        })?;
    let mut rendered = block.to_owned();
    rendered.replace_range(start..end, token);
    Ok(rendered)
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
    let body = format!(
        "v=0\r\no=- 0 0 IN IP4 {device_ip}\r\ns=File Sync Tool reviewed-static H264\r\nc=IN IP4 {device_ip}\r\nt=0 0\r\nm=video 0 RTP/AVP {payload_type}\r\na=rtpmap:{payload_type} H264/{clock_rate}\r\na=fmtp:{payload_type} packetization-mode=1; sprop-parameter-sets={},{}\r\na=control:{route}\r\n",
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
        "ipc-custom" => Ok(FirstReleaseProfileId::IpcCustom),
        "ipc-smart" => Ok(FirstReleaseProfileId::IpcSmart),
        "ipc-structured" => Ok(FirstReleaseProfileId::IpcStructured),
        "ipc-face-access" => Ok(FirstReleaseProfileId::IpcFaceAccess),
        "nvr-common" => Ok(FirstReleaseProfileId::NvrCommon),
        "nvr-vehicle" => Ok(FirstReleaseProfileId::NvrVehicle),
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
        DeviceGroupDraft, RtspPorts, StreamRuntimeConfig, StreamTransport, TargetPlatformConfig,
        TargetPlatformServer,
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

    fn profile(id: FirstReleaseProfileId) -> DeviceProfileV1 {
        DeviceProfileV1 {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: id.as_str().into(),
            device_kind: id.device_kind(),
            legacy_device_type: id.legacy_device_type().into(),
            identity: ProfileIdentityFacts {
                model: if id.device_kind() == DeviceKind::Nvr {
                    "NVR302-09E2-IQ"
                } else {
                    "IPC3615SB-ADF28KM-I0"
                }
                .into(),
                firmware_version: "STATIC-REVIEW-1".into(),
                nickname: "STATIC".into(),
                device_type_enum: (id.device_kind() == DeviceKind::Nvr) as u16,
            },
            supported_platforms: vec![TargetPlatform::Ums],
            handlers: ProfileHandlerBindings {
                identity: "legacy.identity.v1".into(),
                discovery: if id.device_kind() == DeviceKind::Nvr {
                    "ws_discovery.nvr.v1"
                } else {
                    "ws_discovery.ipc.v1"
                }
                .into(),
                http: "http.profile.v1".into(),
                rtsp: "rtsp.tcp_interleaved.v1".into(),
                alarms: vec!["alarm.smart.v1".into()],
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
            device_kind: if profile_id.starts_with("nvr") {
                DeviceKind::Nvr
            } else {
                DeviceKind::Ipc
            },
            ip: "192.0.2.10".parse().unwrap(),
            mac: "48ea6324780c".into(),
            serial_number: "SIM123".into(),
            hardware_id: "210235SIM0001".into(),
            channel_count: profile_id.starts_with("nvr").then_some(8),
            streams: vec![crate::device_simulator::api::DeviceStreamAddress {
                device_id: "group-0001".into(),
                channel_id: None,
                stream: DeviceSimulatorStreamKind::Main,
                url: "rtsp://192.0.2.10:554/media/video1".into(),
            }],
        }
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
    fn routes_source_confirmed_common_and_aibox_requests_without_guessing_file_paths() {
        let ipc = request(
            HttpMethod::Post,
            "/onvif/device_service",
            Some("\"http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation\""),
            b"<GetDeviceInformation/>",
        );
        assert_eq!(
            resolve_http_template(FirstReleaseProfileId::IpcSmart, &ipc)
                .unwrap()
                .path,
            "xml/Common/GetDeviceInformation.xml"
        );
        let nvr = request(HttpMethod::Get, "/LAPI/V1.0/System/DeviceInfo", None, b"");
        assert_eq!(
            resolve_http_template(FirstReleaseProfileId::NvrCommon, &nvr)
                .unwrap()
                .path,
            "xml/AIBOX/System-DeviceInfo.xml"
        );
        let vehicle = request(
            HttpMethod::Get,
            "/LAPI/V1.0/Smart/Management/Capabilities",
            None,
            b"",
        );
        assert_eq!(
            resolve_http_template(FirstReleaseProfileId::NvrVehicle, &vehicle)
                .unwrap()
                .path,
            "xml/Vehicle/Management-Capabilities.xml"
        );
        let face_scopes = request(
            HttpMethod::Post,
            "/onvif/device_service",
            Some("\"http://www.onvif.org/ver10/device/wsdl/GetScopes\""),
            b"<GetScopes/>",
        );
        assert_eq!(
            resolve_http_template(FirstReleaseProfileId::IpcFaceAccess, &face_scopes)
                .unwrap()
                .path,
            "xml/Common/GetScopes-ACS.xml"
        );
        let feature = request(
            HttpMethod::Get,
            "/LAPI/V1.0/Smart/FaceRecognition/Feature/Version",
            None,
            b"",
        );
        assert_eq!(
            resolve_http_template(FirstReleaseProfileId::IpcFaceAccess, &feature)
                .unwrap()
                .path,
            "xml/Common/Smart-FaceRecognition-Feature-Version.xml"
        );
        let door = request(
            HttpMethod::Get,
            "/LAPI/V1.0/Channels/1/Door/System/ChannelDetailInfos",
            None,
            b"",
        );
        assert_eq!(
            resolve_http_template(FirstReleaseProfileId::IpcFaceAccess, &door)
                .unwrap()
                .path,
            "xml/Common/Door-System-ChannelDetailInfos.xml"
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
    fn renders_ums_device_type_and_profile_specific_subscription_lifetime() {
        let template = br#"{
            "Response":{"Data":{"DeviceType":0,"DeviceModel":"IPC244S-IR9-PF80-DT"}},
            "CurrentTime":1618228378,
            "TerminationTime":1618228438
        }"#;
        let request = request(HttpMethod::Get, "/LAPI/V1.0/System/DeviceInfo", None, b"");
        let mut face_profile = profile(FirstReleaseProfileId::IpcFaceAccess);
        face_profile.identity.model = "ET-S51H@B".into();
        face_profile.identity.device_type_enum = 10;
        let ipc_body = render_legacy_template(
            template,
            &request,
            "192.0.2.20:50000".parse().unwrap(),
            TargetPlatform::Ums,
            80,
            &device("ipc-face-access"),
            &face_profile,
        )
        .unwrap();
        let ipc: serde_json::Value = serde_json::from_slice(&ipc_body).unwrap();
        assert_eq!(ipc["Response"]["Data"]["DeviceType"].as_u64(), Some(10));
        assert_eq!(
            ipc["Response"]["Data"]["DeviceModel"].as_str(),
            Some("ET-S51H@B")
        );
        assert_eq!(
            ipc["TerminationTime"].as_i64().unwrap() - ipc["CurrentTime"].as_i64().unwrap(),
            3_600
        );

        let nvr_body = render_legacy_template(
            template,
            &request,
            "192.0.2.20:50000".parse().unwrap(),
            TargetPlatform::Ums,
            80,
            &device("nvr-common"),
            &profile(FirstReleaseProfileId::NvrCommon),
        )
        .unwrap();
        let nvr: serde_json::Value = serde_json::from_slice(&nvr_body).unwrap();
        assert_eq!(
            nvr["TerminationTime"].as_i64().unwrap() - nvr["CurrentTime"].as_i64().unwrap(),
            60
        );
    }

    #[test]
    fn expands_nvr_channel_details_and_onvif_sources_to_the_configured_count() {
        let mut nvr = device("nvr-common");
        nvr.channel_count = Some(4);
        let channel_request = request(
            HttpMethod::Get,
            "/LAPI/V1.0/Channels/System/ChannelDetailInfos",
            None,
            b"",
        );
        let mut channel_json =
            r#"{"Response":{"Data":{"Nums":1,"DetailInfos":[{"ID":1,"Name":"base","Status":1}]}}}"#
                .to_owned();
        render_nvr_channel_metadata(&mut channel_json, &channel_request, &nvr).unwrap();
        let rendered: serde_json::Value = serde_json::from_str(&channel_json).unwrap();
        let channels = rendered["Response"]["Data"]["DetailInfos"]
            .as_array()
            .unwrap();
        assert_eq!(rendered["Response"]["Data"]["Nums"].as_u64(), Some(4));
        assert_eq!(channels.len(), 4);
        assert_eq!(channels[0]["ID"].as_u64(), Some(1));
        assert_eq!(channels[3]["ID"].as_u64(), Some(4));
        assert_eq!(channels[3]["Name"].as_str(), Some("192.0.2.10_V_4"));

        let video_request = request(
            HttpMethod::Post,
            "/onvif/media_service",
            None,
            b"<trt:GetVideoSources/>",
        );
        let mut video = "<r><trt:VideoSources token=\"00100\"><x>1</x></trt:VideoSources><trt:VideoSources token=\"00200\"><x>2</x></trt:VideoSources></r>".to_owned();
        render_nvr_channel_metadata(&mut video, &video_request, &nvr).unwrap();
        assert_eq!(xml_element_blocks(&video, "trt:VideoSources").len(), 4);
        for token in ["00100", "00200", "00300", "00400"] {
            assert!(video.contains(&format!("token=\"{token}\"")));
        }

        let audio_request = request(
            HttpMethod::Post,
            "/onvif/media_service",
            None,
            b"<trt:GetAudioSources/>",
        );
        let mut audio = "<r><trt:AudioSources token=\"00001\"></trt:AudioSources><trt:AudioSources token=\"00100\"></trt:AudioSources><trt:AudioSources token=\"00200\"></trt:AudioSources></r>".to_owned();
        render_nvr_channel_metadata(&mut audio, &audio_request, &nvr).unwrap();
        assert_eq!(xml_element_blocks(&audio, "trt:AudioSources").len(), 5);
        assert!(audio.contains("token=\"00400\""));
    }

    #[tokio::test]
    async fn approved_release_serves_reviewed_http_and_rtsp_fixtures_when_configured() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let root = PathBuf::from(root);
        let pins = [
            "protocol-core",
            "media-h264-live",
            "ipc-custom",
            "ipc-smart",
            "nvr-common",
            "nvr-vehicle",
        ]
        .into_iter()
        .map(|id| PinnedPackDirectory {
            id: id.into(),
            version: version.clone(),
            directory: root.join(id).join(&version),
        })
        .collect::<Vec<_>>();
        let assets = Arc::new(RuntimeAssetLayout::load(&pins, &["ipc-smart".into()]).unwrap());
        let ports = reserve_tcp_ports(4);
        let request = SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Ums,
                servers: vec![TargetPlatformServer {
                    id: "receiver".into(),
                    host: "127.0.0.1".into(),
                    port: 18080,
                }],
                alarm_receiver_url: None,
            },
            interface_id: "loopback-fixture".into(),
            start_ip: Ipv4Addr::LOCALHOST,
            subnet_prefix: 8,
            device_http_port: ports[0],
            rtsp_ports: RtspPorts {
                main: ports[1],
                sub: ports[2],
                third: ports[3],
            },
            groups: vec![DeviceGroupDraft {
                id: "smart".into(),
                profile_id: "ipc-smart".into(),
                count: 1,
                nvr_channel_count: None,
            }],
            stream: StreamRuntimeConfig {
                transport: StreamTransport::TcpInterleaved,
                enabled_streams: vec![
                    DeviceSimulatorStreamKind::Main,
                    DeviceSimulatorStreamKind::Sub,
                    DeviceSimulatorStreamKind::Third,
                ],
                audio_enabled: false,
            },
        };
        let preview = crate::device_simulator::api::preview_devices(&request).unwrap();
        let runtime = ProtocolRuntime::start(ProtocolRuntimeConfig {
            request: request.clone(),
            preview,
            assets,
            picture_cache: Arc::new(parking_lot::RwLock::new(ImageCache::default())),
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
        let mut rtsp_response = [0_u8; 512];
        let read = rtsp.read(&mut rtsp_response).await.unwrap();
        assert!(String::from_utf8_lossy(&rtsp_response[..read]).contains("RTSP/1.0 200 OK"));
        let stats = runtime.stats();
        assert_eq!(stats.active_rtsp_clients, 1);
        assert!(stats.bytes_sent >= read as u64);
        assert!(stats.outbound_bitrate_kbps > 0);

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
            &device("ipc-smart"),
            &profile(FirstReleaseProfileId::IpcSmart),
        )
        .unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        assert!(rendered.contains("192.0.2.10:81"));
        assert!(rendered.contains("48:ea:63:24:78:0c"));
        assert!(rendered.contains("210235SIM0001"));
        assert!(rendered.contains("IPC3615SB-ADF28KM-I0"));
        assert!(rendered.contains("rtsp://192.0.2.10:554/media/video1"));
        assert!(!rendered.contains("206.2.18.165"));
    }

    #[test]
    fn discovery_message_id_is_xml_escaped_before_insertion() {
        let rendered = render_discovery_template(
            b"<wsa:RelatesTo>uuid:old</wsa:RelatesTo><XAddr>http://206.2.18.165:80</XAddr>",
            "uuid:a&b",
            81,
            &device("ipc-smart"),
            &profile(FirstReleaseProfileId::IpcSmart),
        )
        .unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        assert!(rendered.contains("uuid:a&amp;b"));
        assert!(rendered.contains("http://192.0.2.10:81"));
    }
}
