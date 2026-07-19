use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::device_simulator::assets::catalog::DeviceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RtspStreamKind {
    Main,
    Sub,
    Third,
}

impl RtspStreamKind {
    fn index(self) -> u8 {
        match self {
            Self::Main => 0,
            Self::Sub => 1,
            Self::Third => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtspPorts {
    pub main: u16,
    pub sub: u16,
    pub third: u16,
}

impl RtspPorts {
    fn get(self, stream: RtspStreamKind) -> u16 {
        match stream {
            RtspStreamKind::Main => self.main,
            RtspStreamKind::Sub => self.sub,
            RtspStreamKind::Third => self.third,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtspRouteRole {
    Aggregate,
    VideoControl,
    MetadataControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtspRouteEvidence {
    LegacySourceConfirmedPlatformUnverified,
    LegacyConflictPreservedPlatformUnverified,
}

impl RtspRouteEvidence {
    pub fn runtime_activation_allowed(self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRtspRoute {
    pub stream: RtspStreamKind,
    pub channel: Option<u16>,
    pub role: RtspRouteRole,
    pub path: String,
    pub evidence: RtspRouteEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRtspListener {
    pub stream: RtspStreamKind,
    pub bind_addr: SocketAddr,
    pub routes: Vec<PlannedRtspRoute>,
}

impl PlannedRtspListener {
    pub fn runtime_paths(&self) -> Result<BTreeSet<&str>, RtspRoutePlanError> {
        Ok(self
            .routes
            .iter()
            .map(|route| route.path.as_str())
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspRoutePlanError {
    pub code: &'static str,
    pub message: String,
}

pub fn plan_rtsp_routes(
    device_kind: DeviceKind,
    device_ip: Ipv4Addr,
    ports: RtspPorts,
    nvr_channel_count: Option<u16>,
) -> Result<Vec<PlannedRtspListener>, RtspRoutePlanError> {
    validate_input(device_kind, device_ip, ports, nvr_channel_count)?;
    let streams = [
        RtspStreamKind::Main,
        RtspStreamKind::Sub,
        RtspStreamKind::Third,
    ];
    let mut listeners = Vec::with_capacity(streams.len());
    for stream in streams {
        let mut routes = match device_kind {
            DeviceKind::Ipc => vec![PlannedRtspRoute {
                stream,
                channel: None,
                role: RtspRouteRole::Aggregate,
                path: format!("/media/video{}", stream.index() + 1),
                evidence: RtspRouteEvidence::LegacySourceConfirmedPlatformUnverified,
            }],
            // The legacy implementation exposes the configured NVR channel
            // count through HTTP/ONVIF metadata but only starts c1 RTSP for
            // main/sub/third streams. Preserve that observable behavior;
            // c2..cN would be a new feature, not a language-porting detail.
            DeviceKind::Nvr => vec![PlannedRtspRoute {
                stream,
                channel: Some(1),
                role: RtspRouteRole::Aggregate,
                path: format!("/unicast/c1/s{}/live", stream.index()),
                evidence: RtspRouteEvidence::LegacySourceConfirmedPlatformUnverified,
            }],
        };
        routes.push(PlannedRtspRoute {
            stream,
            channel: matches!(device_kind, DeviceKind::Nvr).then_some(1),
            role: RtspRouteRole::VideoControl,
            path: "/media/video1/video".into(),
            evidence: if stream == RtspStreamKind::Main {
                RtspRouteEvidence::LegacySourceConfirmedPlatformUnverified
            } else {
                RtspRouteEvidence::LegacyConflictPreservedPlatformUnverified
            },
        });
        if device_kind == DeviceKind::Ipc {
            routes.push(PlannedRtspRoute {
                stream,
                channel: None,
                role: RtspRouteRole::MetadataControl,
                path: "/media/video1/metadata".into(),
                evidence: RtspRouteEvidence::LegacySourceConfirmedPlatformUnverified,
            });
        }
        listeners.push(PlannedRtspListener {
            stream,
            bind_addr: SocketAddr::new(IpAddr::V4(device_ip), ports.get(stream)),
            routes,
        });
    }
    Ok(listeners)
}

fn validate_input(
    device_kind: DeviceKind,
    device_ip: Ipv4Addr,
    ports: RtspPorts,
    nvr_channel_count: Option<u16>,
) -> Result<(), RtspRoutePlanError> {
    if device_ip.is_unspecified() || device_ip.is_multicast() || device_ip.is_broadcast() {
        return Err(route_error(
            "device_simulator.rtsp.device_ip_invalid",
            "RTSP route planning requires an explicit unicast device IPv4 address",
        ));
    }
    let ports = [ports.main, ports.sub, ports.third];
    if ports.contains(&0) || ports.into_iter().collect::<BTreeSet<_>>().len() != 3 {
        return Err(route_error(
            "device_simulator.rtsp.ports_invalid",
            "main, sub, and third RTSP ports must be non-zero and distinct",
        ));
    }
    match (device_kind, nvr_channel_count) {
        (DeviceKind::Ipc, None) => Ok(()),
        (DeviceKind::Ipc, Some(_)) => Err(route_error(
            "device_simulator.rtsp.ipc_channel_invalid",
            "IPC route plans do not accept an NVR channel count",
        )),
        (DeviceKind::Nvr, Some(1..=128)) => Ok(()),
        (DeviceKind::Nvr, _) => Err(route_error(
            "device_simulator.rtsp.nvr_channel_invalid",
            "NVR route plans require 1-128 channels",
        )),
    }
}

fn route_error(code: &'static str, message: impl Into<String>) -> RtspRoutePlanError {
    RtspRoutePlanError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_three_ipc_listeners_and_preserves_legacy_control_evidence() {
        let plan = plan_rtsp_routes(
            DeviceKind::Ipc,
            "192.0.2.10".parse().unwrap(),
            RtspPorts {
                main: 554,
                sub: 555,
                third: 556,
            },
            None,
        )
        .unwrap();
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].routes[0].path, "/media/video1");
        assert_eq!(plan[1].routes[0].path, "/media/video2");
        assert_eq!(plan[2].routes[0].path, "/media/video3");
        assert_eq!(plan[2].bind_addr.port(), 556);
        assert!(plan.iter().all(|listener| listener
            .routes
            .iter()
            .any(|route| route.path == "/media/video1/video")));
        assert!(plan.iter().all(|listener| listener.runtime_paths().is_ok()));
    }

    #[test]
    fn nvr_plan_preserves_legacy_c1_streaming_with_multi_channel_metadata() {
        let plan = plan_rtsp_routes(
            DeviceKind::Nvr,
            "198.51.100.20".parse().unwrap(),
            RtspPorts {
                main: 554,
                sub: 555,
                third: 556,
            },
            Some(8),
        )
        .unwrap();
        assert_eq!(plan[0].routes[0].path, "/unicast/c1/s0/live");
        assert_eq!(
            plan[0]
                .routes
                .iter()
                .filter(|route| route.role == RtspRouteRole::Aggregate)
                .count(),
            1
        );
        assert!(plan[0].runtime_paths().is_ok());
    }

    #[test]
    fn rejects_invalid_ports_and_channel_shapes() {
        let invalid = plan_rtsp_routes(
            DeviceKind::Ipc,
            "192.0.2.10".parse().unwrap(),
            RtspPorts {
                main: 554,
                sub: 554,
                third: 556,
            },
            None,
        )
        .unwrap_err();
        assert_eq!(invalid.code, "device_simulator.rtsp.ports_invalid");
        assert_eq!(
            plan_rtsp_routes(
                DeviceKind::Nvr,
                "192.0.2.10".parse().unwrap(),
                RtspPorts {
                    main: 554,
                    sub: 555,
                    third: 556,
                },
                Some(129),
            )
            .unwrap_err()
            .code,
            "device_simulator.rtsp.nvr_channel_invalid"
        );
    }
}
