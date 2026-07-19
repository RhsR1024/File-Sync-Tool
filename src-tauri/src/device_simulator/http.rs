use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use crate::device_simulator::telemetry::ProtocolFailureMetrics;
use crate::device_simulator::template::CompiledTemplate;
use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const MAX_HTTP_HEADER_BYTES: usize = 32 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_HTTP_REQUEST_BYTES: usize = MAX_HTTP_HEADER_BYTES + MAX_HTTP_BODY_BYTES;
pub const MAX_HTTP_HEADERS: usize = 64;
pub const MAX_HTTP_TARGET_BYTES: usize = 2048;
pub const MAX_HTTP_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpBindPlan {
    pub device_ip: Ipv4Addr,
    pub port: u16,
}

impl HttpBindPlan {
    pub fn validate(self) -> Result<(), HttpError> {
        if self.device_ip.is_unspecified()
            || self.device_ip.is_multicast()
            || self.device_ip == Ipv4Addr::BROADCAST
        {
            return Err(error(
                "device_simulator.http.bind_ip_invalid",
                "HTTP listener must bind an explicit unicast device IPv4 address",
            ));
        }
        if self.port == 0 {
            return Err(error(
                "device_simulator.http.port_invalid",
                "HTTP listener port must be non-zero",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
}

impl HttpMethod {
    fn parse(value: &str) -> Result<Self, HttpError> {
        match value {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            _ => Err(error(
                "device_simulator.http.method_unsupported",
                format!("HTTP method '{value}' is not enabled for the first release"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub query: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get("content-type").map(String::as_str)
    }

    pub fn soap_action(&self) -> Option<&str> {
        self.headers.get("soapaction").map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RouteMatcher {
    pub method: HttpMethod,
    pub path: String,
    pub content_type: Option<String>,
    pub soap_action: Option<String>,
    pub body_contains: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrongHandlerId {
    EventSubscription,
    StreamUri,
    SmartCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTarget {
    Template { template_id: String },
    StrongHandler(StrongHandlerId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSpec {
    pub id: String,
    /// Empty means common to every first-release profile. Non-empty profile
    /// routes take precedence over common routes with the same match shape.
    pub profiles: Vec<FirstReleaseProfileId>,
    pub platforms: Vec<TargetPlatform>,
    pub matcher: RouteMatcher,
    pub target: RouteTarget,
    pub response_content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompiledRoute {
    spec: RouteSpec,
    profile_specific: bool,
}

#[derive(Debug)]
pub struct CompiledHttpRouter {
    local_ip: Ipv4Addr,
    profile: FirstReleaseProfileId,
    platform: TargetPlatform,
    routes: Vec<CompiledRoute>,
    templates: BTreeMap<String, CompiledTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteResolution<'a> {
    Template {
        route_id: &'a str,
        template_id: &'a str,
        content_type: &'a str,
    },
    StrongHandler {
        route_id: &'a str,
        handler: StrongHandlerId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HttpError {}

impl CompiledHttpRouter {
    pub fn compile(
        local_ip: Ipv4Addr,
        profile: FirstReleaseProfileId,
        platform: TargetPlatform,
        routes: Vec<RouteSpec>,
        templates: BTreeMap<String, CompiledTemplate>,
    ) -> Result<Self, HttpError> {
        HttpBindPlan {
            device_ip: local_ip,
            port: 1,
        }
        .validate()?;
        let mut ids = BTreeSet::new();
        let mut compiled = Vec::new();
        for spec in routes {
            validate_route(&spec, &templates)?;
            if !ids.insert(spec.id.clone()) {
                return Err(error(
                    "device_simulator.http.route_id_duplicate",
                    format!("route id '{}' is duplicated", spec.id),
                ));
            }
            if !spec.platforms.contains(&platform)
                || (!spec.profiles.is_empty() && !spec.profiles.contains(&profile))
            {
                continue;
            }
            let profile_specific = !spec.profiles.is_empty();
            if compiled.iter().any(|existing: &CompiledRoute| {
                existing.profile_specific == profile_specific
                    && matchers_overlap(&existing.spec.matcher, &spec.matcher)
            }) {
                return Err(error(
                    "device_simulator.http.route_ambiguous",
                    format!("route '{}' overlaps another active matcher", spec.id),
                ));
            }
            compiled.push(CompiledRoute {
                spec,
                profile_specific,
            });
        }
        compiled.sort_by_key(|route| !route.profile_specific);
        Ok(Self {
            local_ip,
            profile,
            platform,
            routes: compiled,
            templates,
        })
    }

    pub fn local_ip(&self) -> Ipv4Addr {
        self.local_ip
    }

    pub fn profile(&self) -> FirstReleaseProfileId {
        self.profile
    }

    pub fn platform(&self) -> TargetPlatform {
        self.platform
    }

    pub fn resolve<'a>(
        &'a self,
        accepted_local_ip: Ipv4Addr,
        request: &HttpRequest,
    ) -> Result<RouteResolution<'a>, HttpError> {
        if accepted_local_ip != self.local_ip {
            return Err(error(
                "device_simulator.http.listener_identity_mismatch",
                format!(
                    "request accepted on {accepted_local_ip}, but router belongs to {}",
                    self.local_ip
                ),
            ));
        }
        let route = self
            .routes
            .iter()
            .find(|route| matcher_matches(&route.spec.matcher, request))
            .ok_or_else(|| {
                error(
                    "device_simulator.http.route_not_found",
                    format!(
                        "no declared route matches {:?} {}",
                        request.method, request.path
                    ),
                )
            })?;
        Ok(match &route.spec.target {
            RouteTarget::Template { template_id } => RouteResolution::Template {
                route_id: &route.spec.id,
                template_id,
                content_type: &route.spec.response_content_type,
            },
            RouteTarget::StrongHandler(handler) => RouteResolution::StrongHandler {
                route_id: &route.spec.id,
                handler: *handler,
            },
        })
    }

    pub fn render_template_response(
        &self,
        resolution: &RouteResolution<'_>,
        values: &BTreeMap<String, String>,
    ) -> Result<HttpResponse, HttpError> {
        let RouteResolution::Template {
            template_id,
            content_type,
            ..
        } = resolution
        else {
            return Err(error(
                "device_simulator.http.strong_handler_required",
                "route requires a compiled strong handler rather than template rendering",
            ));
        };
        let template = self.templates.get(*template_id).ok_or_else(|| {
            error(
                "device_simulator.http.template_missing",
                format!("compiled template '{template_id}' is missing"),
            )
        })?;
        let body = template.render(values).map_err(|source| {
            error(
                "device_simulator.http.template_render_failed",
                format!("failed to render HTTP template '{template_id}': {source}"),
            )
        })?;
        Ok(HttpResponse {
            status: 200,
            content_type: (*content_type).to_owned(),
            body,
        })
    }
}

pub fn parse_http_request(bytes: &[u8]) -> Result<HttpRequest, HttpError> {
    if bytes.is_empty() || bytes.len() > MAX_HTTP_REQUEST_BYTES {
        return Err(error(
            "device_simulator.http.request_size_invalid",
            "HTTP request is empty or exceeds the supported limit",
        ));
    }
    let header_end = find_header_end(bytes).ok_or_else(|| {
        error(
            "device_simulator.http.headers_incomplete",
            "HTTP request headers are incomplete",
        )
    })?;
    if header_end > MAX_HTTP_HEADER_BYTES {
        return Err(error(
            "device_simulator.http.headers_too_large",
            "HTTP request headers exceed the supported limit",
        ));
    }
    let header_bytes = &bytes[..header_end - 4];
    let header_text = std::str::from_utf8(header_bytes).map_err(|source| {
        error(
            "device_simulator.http.header_encoding_invalid",
            format!("HTTP headers are not UTF-8: {source}"),
        )
    })?;
    if header_text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
    {
        return Err(error(
            "device_simulator.http.header_control_character",
            "HTTP headers contain forbidden control characters",
        ));
    }
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or_else(|| {
        error(
            "device_simulator.http.request_line_invalid",
            "HTTP request line is missing",
        )
    })?;
    let parts = request_line.split(' ').collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err(error(
            "device_simulator.http.request_line_invalid",
            "HTTP request line must use origin-form HTTP/1.1",
        ));
    }
    let method = HttpMethod::parse(parts[0])?;
    let (path, query) = parse_target(parts[1])?;
    let mut headers = BTreeMap::new();
    for line in lines {
        if headers.len() >= MAX_HTTP_HEADERS {
            return Err(error(
                "device_simulator.http.header_count_invalid",
                format!("HTTP request declares more than {MAX_HTTP_HEADERS} headers"),
            ));
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(error(
                "device_simulator.http.header_folding_rejected",
                "obsolete folded HTTP headers are not accepted",
            ));
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            error(
                "device_simulator.http.header_invalid",
                "HTTP header is missing a colon",
            )
        })?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(error(
                "device_simulator.http.header_name_invalid",
                format!("HTTP header name '{name}' is invalid"),
            ));
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim();
        if value.chars().any(char::is_control) {
            return Err(error(
                "device_simulator.http.header_value_invalid",
                format!("HTTP header '{name}' contains control characters"),
            ));
        }
        if headers.insert(name.clone(), value.to_owned()).is_some() {
            return Err(error(
                "device_simulator.http.header_duplicate",
                format!("duplicate HTTP header '{name}' is not accepted"),
            ));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(error(
            "device_simulator.http.transfer_encoding_unsupported",
            "HTTP transfer-encoding is not supported by the bounded first-release parser",
        ));
    }
    let content_length = parse_content_length(&headers)?;
    if content_length > MAX_HTTP_BODY_BYTES {
        return Err(error(
            "device_simulator.http.body_too_large",
            "HTTP request body exceeds the supported limit",
        ));
    }
    let body = &bytes[header_end..];
    if body.len() != content_length {
        return Err(error(
            "device_simulator.http.body_length_invalid",
            format!(
                "HTTP body length {} does not match Content-Length {content_length}",
                body.len()
            ),
        ));
    }
    Ok(HttpRequest {
        method,
        path,
        query,
        headers,
        body: body.to_vec(),
    })
}

pub struct DeviceHttpListener {
    listener: TcpListener,
    local_ip: Ipv4Addr,
    metrics: Arc<ProtocolFailureMetrics>,
}

impl DeviceHttpListener {
    pub async fn bind(
        plan: HttpBindPlan,
        metrics: Arc<ProtocolFailureMetrics>,
    ) -> Result<Self, HttpError> {
        plan.validate()?;
        let address = (plan.device_ip, plan.port);
        let listener = TcpListener::bind(address).await.map_err(|source| {
            error(
                "device_simulator.http.bind_failed",
                format!(
                    "failed to bind HTTP listener to {}:{}: {source}",
                    plan.device_ip, plan.port
                ),
            )
        })?;
        Ok(Self {
            listener,
            local_ip: plan.device_ip,
            metrics,
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, HttpError> {
        self.listener.local_addr().map_err(|source| {
            error(
                "device_simulator.http.local_addr_failed",
                format!("failed to inspect HTTP listener address: {source}"),
            )
        })
    }

    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr), HttpError> {
        self.listener.accept().await.map_err(|source| {
            error(
                "device_simulator.http.accept_failed",
                format!("failed to accept HTTP connection: {source}"),
            )
        })
    }

    pub async fn read_request(
        &self,
        stream: &mut TcpStream,
        now_ms: u64,
        log_interval_ms: u64,
    ) -> Result<(HttpRequest, bool), HttpError> {
        let read = async {
            let mut request = Vec::with_capacity(4096);
            let mut chunk = [0_u8; 4096];
            let mut expected_length = None;
            loop {
                let length = stream.read(&mut chunk).await.map_err(|source| {
                    error(
                        "device_simulator.http.read_failed",
                        format!("failed to read HTTP request: {source}"),
                    )
                })?;
                if length == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..length]);
                if request.len() > MAX_HTTP_REQUEST_BYTES {
                    return Err(error(
                        "device_simulator.http.request_size_invalid",
                        "HTTP request exceeds the supported limit",
                    ));
                }
                if expected_length.is_none() {
                    if let Some(header_end) = find_header_end(&request) {
                        if header_end > MAX_HTTP_HEADER_BYTES {
                            return Err(error(
                                "device_simulator.http.headers_too_large",
                                "HTTP request headers exceed the supported limit",
                            ));
                        }
                        let header_text =
                            std::str::from_utf8(&request[..header_end - 4]).map_err(|_| {
                                error(
                                    "device_simulator.http.header_encoding_invalid",
                                    "HTTP headers are not UTF-8",
                                )
                            })?;
                        let content_length = header_text
                            .split("\r\n")
                            .skip(1)
                            .filter_map(|line| line.split_once(':'))
                            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .map(|(_, value)| value.trim().parse::<usize>())
                            .transpose()
                            .map_err(|_| {
                                error(
                                    "device_simulator.http.content_length_invalid",
                                    "Content-Length is not a valid unsigned integer",
                                )
                            })?
                            .unwrap_or(0);
                        if content_length > MAX_HTTP_BODY_BYTES {
                            return Err(error(
                                "device_simulator.http.body_too_large",
                                "HTTP request body exceeds the supported limit",
                            ));
                        }
                        expected_length = Some(header_end + content_length);
                    }
                }
                if expected_length.is_some_and(|expected| request.len() >= expected) {
                    break;
                }
            }
            parse_http_request(&request)
        };
        let result = tokio::time::timeout(HTTP_READ_TIMEOUT, read)
            .await
            .map_err(|_| {
                error(
                    "device_simulator.http.read_timeout",
                    "HTTP request was not completed within the bounded read timeout",
                )
            })?;
        match result {
            Ok(request) => Ok((request, false)),
            Err(source) => {
                let should_log = self.metrics.record_parse_failure(now_ms, log_interval_ms);
                Err(HttpError {
                    code: source.code,
                    message: format!("{}; rate_limited_log_admitted={should_log}", source.message),
                })
            }
        }
    }

    pub async fn write_response(
        &self,
        stream: &mut TcpStream,
        response: &HttpResponse,
        now_ms: u64,
        log_interval_ms: u64,
    ) -> Result<(), HttpError> {
        let encoded = encode_http_response(response)?;
        if let Err(source) = stream.write_all(&encoded).await {
            let should_log = self.metrics.record_send_failure(now_ms, log_interval_ms);
            return Err(error(
                "device_simulator.http.write_failed",
                format!("failed to write HTTP response: {source}; rate_limited_log_admitted={should_log}"),
            ));
        }
        stream.shutdown().await.map_err(|source| {
            error(
                "device_simulator.http.shutdown_failed",
                format!("failed to close HTTP response stream: {source}"),
            )
        })
    }

    pub fn local_ip(&self) -> Ipv4Addr {
        self.local_ip
    }
}

pub fn encode_http_response(response: &HttpResponse) -> Result<Vec<u8>, HttpError> {
    if response.body.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err(error(
            "device_simulator.http.response_too_large",
            "HTTP response exceeds the supported limit",
        ));
    }
    if !matches!(
        response.status,
        200 | 400 | 404 | 405 | 413 | 415 | 500 | 599
    ) {
        return Err(error(
            "device_simulator.http.status_invalid",
            format!(
                "HTTP status {} is not in the compiled allowlist",
                response.status
            ),
        ));
    }
    if !is_safe_content_type(&response.content_type) {
        return Err(error(
            "device_simulator.http.response_content_type_invalid",
            "HTTP response Content-Type is invalid",
        ));
    }
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        500 => "Internal Server Error",
        599 => "OK",
        _ => unreachable!(),
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );
    let mut output = Vec::with_capacity(header.len() + response.body.len());
    output.extend_from_slice(header.as_bytes());
    output.extend_from_slice(&response.body);
    Ok(output)
}

fn validate_route(
    route: &RouteSpec,
    templates: &BTreeMap<String, CompiledTemplate>,
) -> Result<(), HttpError> {
    if route.id.is_empty()
        || route.id.len() > 96
        || !route.id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(error(
            "device_simulator.http.route_id_invalid",
            format!("route id '{}' is not a safe identifier", route.id),
        ));
    }
    validate_route_path(&route.matcher.path)?;
    if route.platforms.is_empty() {
        return Err(error(
            "device_simulator.http.route_platform_missing",
            format!("route '{}' does not declare a target platform", route.id),
        ));
    }
    if has_duplicates(&route.platforms) || has_duplicates(&route.profiles) {
        return Err(error(
            "device_simulator.http.route_scope_duplicate",
            format!("route '{}' repeats a profile or platform scope", route.id),
        ));
    }
    if route
        .matcher
        .content_type
        .as_deref()
        .is_some_and(|value| !is_safe_content_type(value))
    {
        return Err(error(
            "device_simulator.http.route_content_type_invalid",
            format!("route '{}' has an invalid Content-Type matcher", route.id),
        ));
    }
    if route.matcher.soap_action.as_deref().is_some_and(|value| {
        value.is_empty() || value.len() > 512 || value.chars().any(char::is_control)
    }) {
        return Err(error(
            "device_simulator.http.route_soap_action_invalid",
            format!("route '{}' has an invalid SOAPAction matcher", route.id),
        ));
    }
    if route
        .matcher
        .body_contains
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > 256)
    {
        return Err(error(
            "device_simulator.http.route_body_feature_invalid",
            format!("route '{}' has an invalid bounded body feature", route.id),
        ));
    }
    match &route.target {
        RouteTarget::Template { template_id } => {
            if !is_safe_template_id(template_id) || !templates.contains_key(template_id) {
                return Err(error(
                    "device_simulator.http.route_template_unknown",
                    format!(
                        "route '{}' references unknown template '{template_id}'",
                        route.id
                    ),
                ));
            }
        }
        RouteTarget::StrongHandler(_) => {}
    }
    if !is_safe_content_type(&route.response_content_type) {
        return Err(error(
            "device_simulator.http.response_content_type_invalid",
            format!("route '{}' has an invalid response Content-Type", route.id),
        ));
    }
    Ok(())
}

fn matcher_matches(matcher: &RouteMatcher, request: &HttpRequest) -> bool {
    if matcher.method != request.method || matcher.path != request.path {
        return false;
    }
    if let Some(expected) = &matcher.content_type {
        let actual = request
            .content_type()
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        if !actual.is_some_and(|actual| actual.eq_ignore_ascii_case(expected)) {
            return false;
        }
    }
    if let Some(expected) = &matcher.soap_action {
        let actual = request.soap_action().map(|value| value.trim_matches('"'));
        if actual != Some(expected.as_str()) {
            return false;
        }
    }
    if let Some(feature) = &matcher.body_contains {
        if !request
            .body
            .windows(feature.len())
            .any(|window| window == feature)
        {
            return false;
        }
    }
    true
}

fn matchers_overlap(left: &RouteMatcher, right: &RouteMatcher) -> bool {
    // Body features are intentionally absent from this exclusion test: two
    // different byte features can both occur in one bounded request body.
    left.method == right.method
        && left.path == right.path
        && optional_exact_overlap(
            left.content_type.as_deref(),
            right.content_type.as_deref(),
            true,
        )
        && optional_exact_overlap(
            left.soap_action.as_deref(),
            right.soap_action.as_deref(),
            false,
        )
}

fn optional_exact_overlap(
    left: Option<&str>,
    right: Option<&str>,
    ascii_case_insensitive: bool,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) if ascii_case_insensitive => left.eq_ignore_ascii_case(right),
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn parse_target(value: &str) -> Result<(String, Option<String>), HttpError> {
    if value.is_empty()
        || value.len() > MAX_HTTP_TARGET_BYTES
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.contains('#')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(error(
            "device_simulator.http.target_invalid",
            "HTTP target must be a bounded origin-form path",
        ));
    }
    let (path, query) = match value.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (value, None),
    };
    validate_route_path(path)?;
    Ok((path.to_owned(), query))
}

fn validate_route_path(path: &str) -> Result<(), HttpError> {
    if path.is_empty()
        || path.len() > MAX_HTTP_TARGET_BYTES
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('?')
        || path.contains('#')
        || path.contains('\\')
        || !path.is_ascii()
        || path.chars().any(char::is_control)
        || path.split('/').any(|segment| matches!(segment, "." | ".."))
    {
        return Err(error(
            "device_simulator.http.route_path_invalid",
            format!("HTTP route path '{path}' is unsafe"),
        ));
    }
    Ok(())
}

fn parse_content_length(headers: &BTreeMap<String, String>) -> Result<usize, HttpError> {
    headers
        .get("content-length")
        .map(|value| {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(error(
                    "device_simulator.http.content_length_invalid",
                    "Content-Length is not a valid unsigned integer",
                ));
            }
            value.parse::<usize>().map_err(|_| {
                error(
                    "device_simulator.http.content_length_invalid",
                    "Content-Length exceeds the supported integer range",
                )
            })
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn is_safe_content_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'/' | b'+' | b'-' | b'.' | b'_' | b';' | b'=' | b' ')
        })
}

fn is_safe_template_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

fn error(code: &'static str, message: impl Into<String>) -> HttpError {
    HttpError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::template::{
        TemplateManifest, TemplateVariableSpec, VariableEncoding,
    };

    fn request(value: &[u8]) -> HttpRequest {
        parse_http_request(value).unwrap()
    }

    fn template() -> CompiledTemplate {
        CompiledTemplate::compile(
            b"<Model>{{device.model}}</Model>",
            &TemplateManifest {
                relative_path: "templates/common/device-info.xml".into(),
                variables: vec![TemplateVariableSpec {
                    name: "device.model".into(),
                    encoding: VariableEncoding::XmlText,
                }],
                max_output_bytes: 1024,
            },
        )
        .unwrap()
    }

    fn route(id: &str, profiles: Vec<FirstReleaseProfileId>) -> RouteSpec {
        RouteSpec {
            id: id.into(),
            profiles,
            platforms: vec![TargetPlatform::Ums],
            matcher: RouteMatcher {
                method: HttpMethod::Post,
                path: "/onvif/device_service".into(),
                content_type: Some("application/soap+xml".into()),
                soap_action: Some(
                    "http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation".into(),
                ),
                body_contains: Some(b"GetDeviceInformation".to_vec()),
            },
            target: RouteTarget::Template {
                template_id: "common.device_info".into(),
            },
            response_content_type: "application/soap+xml; charset=utf-8".into(),
        }
    }

    #[test]
    fn parses_bounded_origin_form_request_and_normalizes_headers() {
        let parsed = request(
            b"POST /onvif/device_service?x=1 HTTP/1.1\r\nHost: 192.0.2.2\r\nContent-Type: application/soap+xml; charset=utf-8\r\nSOAPAction: \"urn:test\"\r\nContent-Length: 8\r\n\r\n<Probe/>",
        );
        assert_eq!(parsed.method, HttpMethod::Post);
        assert_eq!(parsed.path, "/onvif/device_service");
        assert_eq!(parsed.query.as_deref(), Some("x=1"));
        assert_eq!(parsed.soap_action(), Some("\"urn:test\""));
    }

    #[test]
    fn rejects_smuggling_traversal_unsupported_method_and_length_mismatch() {
        for (raw, code) in [
            (
                b"POST /x HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n".as_slice(),
                "device_simulator.http.header_duplicate",
            ),
            (
                b"GET /../secret HTTP/1.1\r\n\r\n".as_slice(),
                "device_simulator.http.route_path_invalid",
            ),
            (
                b"DELETE /x HTTP/1.1\r\n\r\n".as_slice(),
                "device_simulator.http.method_unsupported",
            ),
            (
                b"POST /x HTTP/1.1\r\nContent-Length: 2\r\n\r\nx".as_slice(),
                "device_simulator.http.body_length_invalid",
            ),
            (
                b"POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
                "device_simulator.http.transfer_encoding_unsupported",
            ),
        ] {
            assert_eq!(parse_http_request(raw).unwrap_err().code, code);
        }
    }

    #[test]
    fn profile_route_overrides_common_route_and_listener_ip_is_enforced() {
        let mut common = route("common.device_info", vec![]);
        let mut profile = route("smart.device_info", vec![FirstReleaseProfileId::IpcSmart]);
        profile.target = RouteTarget::StrongHandler(StrongHandlerId::SmartCapabilities);
        profile.response_content_type = "application/json; charset=utf-8".into();
        // Same matcher is intentional at different specificity.
        common.matcher.body_contains = None;
        profile.matcher.body_contains = None;
        let router = CompiledHttpRouter::compile(
            "192.0.2.2".parse().unwrap(),
            FirstReleaseProfileId::IpcSmart,
            TargetPlatform::Ums,
            vec![common, profile],
            BTreeMap::from([("common.device_info".into(), template())]),
        )
        .unwrap();
        let parsed = request(
            b"POST /onvif/device_service HTTP/1.1\r\nContent-Type: application/soap+xml\r\nSOAPAction: http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation\r\nContent-Length: 0\r\n\r\n",
        );
        assert_eq!(
            router
                .resolve("192.0.2.2".parse().unwrap(), &parsed)
                .unwrap(),
            RouteResolution::StrongHandler {
                route_id: "smart.device_info",
                handler: StrongHandlerId::SmartCapabilities,
            }
        );
        assert_eq!(
            router
                .resolve("192.0.2.3".parse().unwrap(), &parsed)
                .unwrap_err()
                .code,
            "device_simulator.http.listener_identity_mismatch"
        );
    }

    #[test]
    fn renders_only_precompiled_template_and_escapes_dynamic_value() {
        let spec = route("common.device_info", vec![]);
        let router = CompiledHttpRouter::compile(
            "192.0.2.2".parse().unwrap(),
            FirstReleaseProfileId::IpcCustom,
            TargetPlatform::Ums,
            vec![spec],
            BTreeMap::from([("common.device_info".into(), template())]),
        )
        .unwrap();
        let parsed = request(
            b"POST /onvif/device_service HTTP/1.1\r\nContent-Type: application/soap+xml\r\nSOAPAction: http://www.onvif.org/ver10/device/wsdl/GetDeviceInformation\r\nContent-Length: 20\r\n\r\nGetDeviceInformation",
        );
        let resolution = router
            .resolve("192.0.2.2".parse().unwrap(), &parsed)
            .unwrap();
        let response = router
            .render_template_response(
                &resolution,
                &BTreeMap::from([("device.model".into(), "IPC<&>".into())]),
            )
            .unwrap();
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "<Model>IPC&lt;&amp;&gt;</Model>"
        );
    }

    #[test]
    fn rejects_wildcard_bind_unknown_template_and_ambiguous_routes() {
        assert_eq!(
            HttpBindPlan {
                device_ip: Ipv4Addr::UNSPECIFIED,
                port: 81,
            }
            .validate()
            .unwrap_err()
            .code,
            "device_simulator.http.bind_ip_invalid"
        );
        assert_eq!(
            CompiledHttpRouter::compile(
                "192.0.2.2".parse().unwrap(),
                FirstReleaseProfileId::IpcSmart,
                TargetPlatform::Ums,
                vec![route("common.device_info", vec![])],
                BTreeMap::new(),
            )
            .unwrap_err()
            .code,
            "device_simulator.http.route_template_unknown"
        );
        let routes = vec![route("one", vec![]), route("two", vec![])];
        assert_eq!(
            CompiledHttpRouter::compile(
                "192.0.2.2".parse().unwrap(),
                FirstReleaseProfileId::IpcSmart,
                TargetPlatform::Ums,
                routes,
                BTreeMap::from([("common.device_info".into(), template())]),
            )
            .unwrap_err()
            .code,
            "device_simulator.http.route_ambiguous"
        );
    }

    #[test]
    fn response_encoder_has_fixed_headers_and_rejects_header_injection() {
        let encoded = encode_http_response(&HttpResponse {
            status: 200,
            content_type: "application/json; charset=utf-8".into(),
            body: b"{}".to_vec(),
        })
        .unwrap();
        assert!(encoded.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(encoded.ends_with(b"\r\n\r\n{}"));
        let proprietary = encode_http_response(&HttpResponse {
            status: 599,
            content_type: "application/json".into(),
            body: b"{}".to_vec(),
        })
        .unwrap();
        assert!(proprietary.starts_with(b"HTTP/1.1 599 OK\r\n"));
        assert_eq!(
            encode_http_response(&HttpResponse {
                status: 200,
                content_type: "text/xml\r\nX-Evil: yes".into(),
                body: vec![],
            })
            .unwrap_err()
            .code,
            "device_simulator.http.response_content_type_invalid"
        );
    }

    #[tokio::test]
    async fn listener_binds_only_the_planned_device_ip_and_releases_its_port() {
        let probe = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let listener = DeviceHttpListener::bind(
            HttpBindPlan {
                device_ip: Ipv4Addr::LOCALHOST,
                port,
            },
            Arc::new(ProtocolFailureMetrics::default()),
        )
        .await
        .unwrap();
        assert_eq!(listener.local_ip(), Ipv4Addr::LOCALHOST);
        assert_eq!(
            listener.local_addr().unwrap(),
            (Ipv4Addr::LOCALHOST, port).into()
        );
        drop(listener);
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).unwrap();
    }
}
