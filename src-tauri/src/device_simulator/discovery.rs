use crate::device_simulator::telemetry::ProtocolFailureMetrics;
use crate::device_simulator::template::CompiledTemplate;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Legacy-source-confirmed WS-Discovery transport baseline.
///
/// Evidence: `VirtualTools/script/Vsocket_ip.py:108-175` and
/// `VirtualTools/xml/Common/search*.xml`. Platform compatibility remains to be
/// confirmed by real VMS/UMS captures.
pub const DISCOVERY_MULTICAST_ADDRESS: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 252);
pub const DISCOVERY_LISTEN_PORT: u16 = 3702;
pub const DISCOVERY_RESPONSE_PORTS: [u16; 3] = [3705, 3706, 3707];
pub const MAX_DISCOVERY_DATAGRAM_BYTES: usize = 5120;
pub const MAX_DISCOVERY_RESPONSE_BYTES: usize = 60 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryBindPlan {
    pub device_ip: Ipv4Addr,
    pub multicast_address: Ipv4Addr,
    pub listen_port: u16,
}

impl DiscoveryBindPlan {
    pub fn legacy_ws_discovery(device_ip: Ipv4Addr) -> Self {
        Self {
            device_ip,
            multicast_address: DISCOVERY_MULTICAST_ADDRESS,
            listen_port: DISCOVERY_LISTEN_PORT,
        }
    }

    pub fn validate(self) -> Result<(), DiscoveryError> {
        if self.device_ip.is_unspecified()
            || self.device_ip.is_multicast()
            || self.device_ip == Ipv4Addr::BROADCAST
        {
            return Err(error(
                "device_simulator.discovery.bind_ip_invalid",
                "discovery listener must bind an explicit unicast device IPv4 address",
            ));
        }
        if !self.multicast_address.is_multicast() {
            return Err(error(
                "device_simulator.discovery.multicast_invalid",
                "discovery group must be an IPv4 multicast address",
            ));
        }
        if self.listen_port == 0 {
            return Err(error(
                "device_simulator.discovery.port_invalid",
                "discovery listen port must be non-zero",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryProbe {
    pub message_id: String,
    pub source: SocketAddrV4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DiscoveryError {}

pub fn parse_probe(datagram: &[u8], source: SocketAddr) -> Result<DiscoveryProbe, DiscoveryError> {
    if datagram.is_empty() || datagram.len() > MAX_DISCOVERY_DATAGRAM_BYTES {
        return Err(error(
            "device_simulator.discovery.datagram_size_invalid",
            format!(
                "discovery datagram must be between 1 and {MAX_DISCOVERY_DATAGRAM_BYTES} bytes"
            ),
        ));
    }
    let SocketAddr::V4(source) = source else {
        return Err(error(
            "device_simulator.discovery.source_invalid",
            "first-release discovery accepts IPv4 sources only",
        ));
    };
    if source.ip().is_unspecified() || source.ip().is_multicast() {
        return Err(error(
            "device_simulator.discovery.source_invalid",
            "discovery source must be a unicast IPv4 address",
        ));
    }
    let xml = std::str::from_utf8(datagram).map_err(|source| {
        error(
            "device_simulator.discovery.encoding_invalid",
            format!("discovery datagram is not UTF-8: {source}"),
        )
    })?;
    if xml.bytes().any(|byte| byte == 0) || !contains_xml_element(xml, "Probe") {
        return Err(error(
            "device_simulator.discovery.probe_invalid",
            "discovery datagram does not contain a Probe element",
        ));
    }
    let message_ids = extract_xml_text_values(xml, "MessageID")?;
    if message_ids.len() != 1 {
        return Err(error(
            "device_simulator.discovery.message_id_invalid",
            "Probe must contain exactly one MessageID element",
        ));
    }
    let message_id = message_ids[0].trim();
    if message_id.is_empty()
        || message_id.len() > 512
        || message_id.chars().any(|character| character.is_control())
    {
        return Err(error(
            "device_simulator.discovery.message_id_invalid",
            "Probe MessageID is empty, too large, or contains control characters",
        ));
    }
    Ok(DiscoveryProbe {
        message_id: message_id.to_owned(),
        source,
    })
}

pub fn render_probe_response(
    template: &CompiledTemplate,
    values: &BTreeMap<String, String>,
) -> Result<Vec<u8>, DiscoveryError> {
    let response = template.render(values).map_err(|source| {
        error(
            "device_simulator.discovery.response_render_failed",
            format!("failed to render discovery response: {source}"),
        )
    })?;
    if response.is_empty() || response.len() > MAX_DISCOVERY_RESPONSE_BYTES {
        return Err(error(
            "device_simulator.discovery.response_size_invalid",
            "discovery response is empty or larger than the UDP safety limit",
        ));
    }
    Ok(response)
}

pub struct DiscoveryListener {
    socket: UdpSocket,
    metrics: Arc<ProtocolFailureMetrics>,
}

impl DiscoveryListener {
    pub async fn bind(
        plan: DiscoveryBindPlan,
        metrics: Arc<ProtocolFailureMetrics>,
    ) -> Result<Self, DiscoveryError> {
        plan.validate()?;
        let address = SocketAddrV4::new(plan.device_ip, plan.listen_port);
        let socket = std::net::UdpSocket::bind(address).map_err(|source| {
            error(
                "device_simulator.discovery.bind_failed",
                format!("failed to bind discovery listener to {address}: {source}"),
            )
        })?;
        socket
            .join_multicast_v4(&plan.multicast_address, &plan.device_ip)
            .map_err(|source| {
                error(
                    "device_simulator.discovery.multicast_join_failed",
                    format!(
                        "failed to join discovery group {} through {}: {source}",
                        plan.multicast_address, plan.device_ip
                    ),
                )
            })?;
        socket.set_nonblocking(true).map_err(|source| {
            error(
                "device_simulator.discovery.nonblocking_failed",
                format!("failed to configure discovery socket: {source}"),
            )
        })?;
        let socket = UdpSocket::from_std(socket).map_err(|source| {
            error(
                "device_simulator.discovery.runtime_socket_failed",
                format!("failed to attach discovery socket to Tokio: {source}"),
            )
        })?;
        Ok(Self { socket, metrics })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, DiscoveryError> {
        self.socket.local_addr().map_err(|source| {
            error(
                "device_simulator.discovery.local_addr_failed",
                format!("failed to inspect discovery listener address: {source}"),
            )
        })
    }

    /// Receives one bounded datagram. The caller supplies monotonic
    /// milliseconds so failed parses can be counted while log emission is
    /// admitted at a bounded rate.
    pub async fn receive_probe(
        &self,
        now_ms: u64,
        log_interval_ms: u64,
    ) -> Result<(DiscoveryProbe, bool), DiscoveryError> {
        let mut buffer = [0_u8; MAX_DISCOVERY_DATAGRAM_BYTES + 1];
        let (length, source) = self.socket.recv_from(&mut buffer).await.map_err(|source| {
            error(
                "device_simulator.discovery.receive_failed",
                format!("failed to receive discovery datagram: {source}"),
            )
        })?;
        match parse_probe(&buffer[..length], source) {
            Ok(probe) => Ok((probe, false)),
            Err(source) => {
                let should_log = self.metrics.record_parse_failure(now_ms, log_interval_ms);
                Err(DiscoveryError {
                    code: source.code,
                    message: format!("{}; rate_limited_log_admitted={should_log}", source.message),
                })
            }
        }
    }

    pub async fn send_response(
        &self,
        probe: &DiscoveryProbe,
        response: &[u8],
        now_ms: u64,
        log_interval_ms: u64,
    ) -> Result<(), DiscoveryError> {
        if response.is_empty() || response.len() > MAX_DISCOVERY_RESPONSE_BYTES {
            return Err(error(
                "device_simulator.discovery.response_size_invalid",
                "discovery response is empty or larger than the UDP safety limit",
            ));
        }
        for port in DISCOVERY_RESPONSE_PORTS {
            let destination = SocketAddrV4::new(*probe.source.ip(), port);
            if let Err(source) = self.socket.send_to(response, destination).await {
                let should_log = self.metrics.record_send_failure(now_ms, log_interval_ms);
                return Err(error(
                    "device_simulator.discovery.send_failed",
                    format!(
                        "failed to send discovery response to {destination}: {source}; rate_limited_log_admitted={should_log}"
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn contains_xml_element(xml: &str, local_name: &str) -> bool {
    xml.match_indices('<').any(|(start, _)| {
        let remainder = &xml[start + 1..];
        if remainder.starts_with('/') || remainder.starts_with('!') || remainder.starts_with('?') {
            return false;
        }
        let name_end = remainder
            .find(|character: char| {
                character.is_ascii_whitespace() || matches!(character, '>' | '/')
            })
            .unwrap_or(remainder.len());
        let qualified_name = &remainder[..name_end];
        qualified_name.rsplit(':').next() == Some(local_name)
    })
}

fn extract_xml_text_values<'a>(
    xml: &'a str,
    local_name: &str,
) -> Result<Vec<&'a str>, DiscoveryError> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = xml[cursor..].find('<') {
        let start = cursor + relative_start;
        let remainder = &xml[start + 1..];
        if remainder.starts_with('/') || remainder.starts_with('!') || remainder.starts_with('?') {
            cursor = start + 1;
            continue;
        }
        let name_end = remainder
            .find(|character: char| {
                character.is_ascii_whitespace() || matches!(character, '>' | '/')
            })
            .ok_or_else(|| {
                error(
                    "device_simulator.discovery.xml_invalid",
                    "discovery XML contains an unterminated element name",
                )
            })?;
        let qualified_name = &remainder[..name_end];
        if qualified_name.rsplit(':').next() != Some(local_name) {
            cursor = start + 1;
            continue;
        }
        let open_end = remainder.find('>').ok_or_else(|| {
            error(
                "device_simulator.discovery.xml_invalid",
                "discovery XML contains an unterminated start tag",
            )
        })? + start
            + 1;
        if xml.as_bytes().get(open_end.wrapping_sub(1)) == Some(&b'/') {
            values.push("");
            cursor = open_end + 1;
            continue;
        }
        let close = format!("</{qualified_name}>");
        let value_start = open_end + 1;
        let relative_close = xml[value_start..].find(&close).ok_or_else(|| {
            error(
                "device_simulator.discovery.xml_invalid",
                format!("discovery XML is missing {close}"),
            )
        })?;
        values.push(&xml[value_start..value_start + relative_close]);
        cursor = value_start + relative_close + close.len();
    }
    Ok(values)
}

fn error(code: &'static str, message: impl Into<String>) -> DiscoveryError {
    DiscoveryError {
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

    fn source() -> SocketAddr {
        "192.0.2.44:49152".parse().unwrap()
    }

    #[test]
    fn parses_namespaced_probe_and_exact_message_id() {
        let probe = parse_probe(
            br#"<?xml version="1.0"?><s:Envelope><s:Body><d:Probe /></s:Body><wsa:MessageID>uuid:abc-123</wsa:MessageID></s:Envelope>"#,
            source(),
        )
        .unwrap();
        assert_eq!(probe.message_id, "uuid:abc-123");
        assert_eq!(SocketAddr::V4(probe.source), source());
    }

    #[test]
    fn rejects_substring_false_positive_duplicate_id_and_oversize() {
        assert_eq!(
            parse_probe(
                br#"<Envelope><NotAProbe/><MessageID>uuid:1</MessageID></Envelope>"#,
                source(),
            )
            .unwrap_err()
            .code,
            "device_simulator.discovery.probe_invalid"
        );
        assert_eq!(
            parse_probe(
                br#"<Probe/><MessageID>1</MessageID><w:MessageID>2</w:MessageID>"#,
                source(),
            )
            .unwrap_err()
            .code,
            "device_simulator.discovery.message_id_invalid"
        );
        assert_eq!(
            parse_probe(&vec![b'x'; MAX_DISCOVERY_DATAGRAM_BYTES + 1], source())
                .unwrap_err()
                .code,
            "device_simulator.discovery.datagram_size_invalid"
        );
    }

    #[test]
    fn bind_plan_never_allows_wildcard_or_non_multicast_group() {
        assert_eq!(
            DiscoveryBindPlan::legacy_ws_discovery(Ipv4Addr::UNSPECIFIED)
                .validate()
                .unwrap_err()
                .code,
            "device_simulator.discovery.bind_ip_invalid"
        );
        let mut plan = DiscoveryBindPlan::legacy_ws_discovery("192.0.2.2".parse().unwrap());
        plan.multicast_address = "192.0.2.1".parse().unwrap();
        assert_eq!(
            plan.validate().unwrap_err().code,
            "device_simulator.discovery.multicast_invalid"
        );
    }

    #[test]
    fn response_uses_compiled_template_and_escaped_values() {
        let template = CompiledTemplate::compile(
            b"<RelatesTo>{{request.message_id}}</RelatesTo>",
            &TemplateManifest {
                relative_path: "templates/common/search.xml".into(),
                variables: vec![TemplateVariableSpec {
                    name: "request.message_id".into(),
                    encoding: VariableEncoding::XmlText,
                }],
                max_output_bytes: 1024,
            },
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(
                render_probe_response(
                    &template,
                    &BTreeMap::from([("request.message_id".into(), "a<&b".into())]),
                )
                .unwrap(),
            )
            .unwrap(),
            "<RelatesTo>a&lt;&amp;b</RelatesTo>"
        );
    }

    #[test]
    fn constants_preserve_legacy_source_transport_without_claiming_platform_validation() {
        assert_eq!(DISCOVERY_MULTICAST_ADDRESS.to_string(), "239.255.255.252");
        assert_eq!(DISCOVERY_LISTEN_PORT, 3702);
        assert_eq!(DISCOVERY_RESPONSE_PORTS, [3705, 3706, 3707]);
    }
}
