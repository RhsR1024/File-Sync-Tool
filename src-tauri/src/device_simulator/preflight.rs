use crate::device_simulator::api::{
    preview_devices, DevicePreview, PreflightCheck, PreflightCheckSeverity, PreflightCheckStatus,
    PreflightReport, SimulatorStartRequest,
};
use crate::device_simulator::windows::interfaces::NetworkInterfaceInfo;
use crate::device_simulator::windows::ip_alias::{AddressConflictAssessment, ConflictVerdict};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Default)]
pub struct PreflightEnvironment {
    pub interfaces: Vec<NetworkInterfaceInfo>,
    pub local_addresses: HashSet<Ipv4Addr>,
    pub conflict_assessments: Vec<AddressConflictAssessment>,
    pub unavailable_tcp_ports: BTreeSet<u16>,
    pub assets_ready: bool,
    pub asset_details: Option<String>,
    pub profiles_static_reviewed: bool,
    pub profiles_platform_verified: bool,
    pub worker_available: bool,
    pub firewall_required: bool,
    pub firewall_available: bool,
    pub residual_session_id: Option<String>,
    /// `Some(true/false)` means a bounded probe completed; `None` remains explicit.
    pub platform_connectivity: BTreeMap<String, Option<bool>>,
}

pub fn run_preflight(
    request: &SimulatorStartRequest,
    environment: &PreflightEnvironment,
) -> PreflightReport {
    let mut checks = Vec::new();
    let device_preview = match preview_devices(request) {
        Ok(preview) => {
            checks.push(passed(
                "request",
                "deviceSimulator.preflight.checks.request",
                Some(format!(
                    "{} devices / {} channels",
                    preview.total_devices, preview.total_channels
                )),
            ));
            preview
        }
        Err(error) => {
            checks.push(failed(
                "request",
                "deviceSimulator.preflight.checks.request",
                error.details.or(Some(error.code)),
            ));
            empty_preview()
        }
    };

    checks.push(if environment.assets_ready {
        passed(
            "assets",
            "deviceSimulator.preflight.checks.assets",
            environment.asset_details.clone(),
        )
    } else {
        failed(
            "assets",
            "deviceSimulator.preflight.checks.assets",
            environment
                .asset_details
                .clone()
                .or_else(|| Some("signed profile dependencies are not ready".into())),
        )
    });

    checks.push(if environment.profiles_platform_verified {
        passed(
            "profile-evidence",
            "deviceSimulator.preflight.checks.profileEvidence",
            None,
        )
    } else if environment.profiles_static_reviewed {
        warning(
            "profile-evidence",
            "deviceSimulator.preflight.checks.profileEvidence",
            Some(
                "static legacy evidence was reviewed and approved for local execution; the selected profile/platform combination remains unverified on a real platform"
                    .into(),
            ),
        )
    } else {
        failed(
            "profile-evidence",
            "deviceSimulator.preflight.checks.profileEvidence",
            Some(
                "the selected profile/platform combination still requires approved golden fixtures and real-platform evidence"
                    .into(),
            ),
        )
    });

    checks.push(match environment.residual_session_id.as_deref() {
        Some(session_id) => failed(
            "recovery",
            "deviceSimulator.preflight.checks.recovery",
            Some(format!("session {session_id} must be reconciled first")),
        ),
        None => passed(
            "recovery",
            "deviceSimulator.preflight.checks.recovery",
            None,
        ),
    });

    let selected_interface = environment
        .interfaces
        .iter()
        .find(|item| item.id.as_str() == request.interface_id);
    checks.push(match selected_interface {
        Some(interface) if interface.is_enabled && interface.is_up => passed(
            "interface",
            "deviceSimulator.preflight.checks.interface",
            Some(format!("{} ({})", interface.name, interface.description)),
        ),
        Some(interface) => failed(
            "interface",
            "deviceSimulator.preflight.checks.interface",
            Some(format!("{} is not enabled and operational", interface.name)),
        ),
        None => failed(
            "interface",
            "deviceSimulator.preflight.checks.interface",
            Some("the selected stable adapter id is not present".into()),
        ),
    });

    let local_conflicts = device_preview
        .devices
        .iter()
        .map(|device| device.ip)
        .filter(|address| environment.local_addresses.contains(address))
        .collect::<Vec<_>>();
    checks.push(if local_conflicts.is_empty() {
        passed(
            "local-addresses",
            "deviceSimulator.preflight.checks.localAddresses",
            None,
        )
    } else {
        failed(
            "local-addresses",
            "deviceSimulator.preflight.checks.localAddresses",
            Some(format!(
                "already assigned locally: {}",
                join_addresses(&local_conflicts)
            )),
        )
    });

    let assessed = environment
        .conflict_assessments
        .iter()
        .filter(|assessment| {
            device_preview
                .devices
                .iter()
                .any(|device| device.ip == assessment.address)
        })
        .collect::<Vec<_>>();
    let conflicts = assessed
        .iter()
        .filter(|assessment| assessment.verdict == ConflictVerdict::Conflict)
        .map(|assessment| assessment.address)
        .collect::<Vec<_>>();
    checks.push(if !conflicts.is_empty() {
        failed(
            "address-conflicts",
            "deviceSimulator.preflight.checks.addressConflicts",
            Some(format!("conflict evidence: {}", join_addresses(&conflicts))),
        )
    } else if assessed.len() == device_preview.devices.len() && !assessed.is_empty() {
        if assessed
            .iter()
            .all(|assessment| assessment.verdict == ConflictVerdict::Clear)
        {
            passed(
                "address-conflicts",
                "deviceSimulator.preflight.checks.addressConflicts",
                None,
            )
        } else {
            warning(
                "address-conflicts",
                "deviceSimulator.preflight.checks.addressConflicts",
                Some("one or more addresses remain inconclusive".into()),
            )
        }
    } else {
        warning(
            "address-conflicts",
            "deviceSimulator.preflight.checks.addressConflicts",
            Some(
                "LAN neighbor/ARP conflict probing must be completed by the elevated Worker before mutation"
                    .into(),
            ),
        )
    });

    let requested_ports = BTreeSet::from([
        request.device_http_port,
        request.rtsp_ports.main,
        request.rtsp_ports.sub,
        request.rtsp_ports.third,
    ]);
    let unavailable_ports = requested_ports
        .intersection(&environment.unavailable_tcp_ports)
        .copied()
        .collect::<Vec<_>>();
    checks.push(if unavailable_ports.is_empty() {
        passed("ports", "deviceSimulator.preflight.checks.ports", None)
    } else {
        failed(
            "ports",
            "deviceSimulator.preflight.checks.ports",
            Some(format!("unavailable TCP ports: {unavailable_ports:?}")),
        )
    });

    let invalid_servers = request
        .platform
        .servers
        .iter()
        .filter(|server| {
            server.id.trim().is_empty() || server.host.trim().is_empty() || server.port == 0
        })
        .map(|server| server.id.clone())
        .collect::<Vec<_>>();
    checks.push(if request.platform.servers.is_empty() {
        failed(
            "platform-config",
            "deviceSimulator.preflight.checks.platformConfig",
            Some("at least one target platform server is required".into()),
        )
    } else if invalid_servers.is_empty() {
        passed(
            "platform-config",
            "deviceSimulator.preflight.checks.platformConfig",
            None,
        )
    } else {
        failed(
            "platform-config",
            "deviceSimulator.preflight.checks.platformConfig",
            Some(format!("invalid server entries: {invalid_servers:?}")),
        )
    });

    let failed_connectivity = environment
        .platform_connectivity
        .iter()
        .filter_map(|(id, reachable)| (*reachable == Some(false)).then_some(id.as_str()))
        .collect::<Vec<_>>();
    let unknown_connectivity = request.platform.servers.iter().any(|server| {
        environment
            .platform_connectivity
            .get(&server.id)
            .copied()
            .flatten()
            .is_none()
    });
    checks.push(if !failed_connectivity.is_empty() {
        warning(
            "platform-connectivity",
            "deviceSimulator.preflight.checks.platformConnectivity",
            Some(format!(
                "bounded connection probe failed: {failed_connectivity:?}"
            )),
        )
    } else if unknown_connectivity {
        warning(
            "platform-connectivity",
            "deviceSimulator.preflight.checks.platformConnectivity",
            Some("connectivity has not been verified in the current environment".into()),
        )
    } else {
        passed(
            "platform-connectivity",
            "deviceSimulator.preflight.checks.platformConnectivity",
            None,
        )
    });

    checks.push(if environment.worker_available {
        passed(
            "worker",
            "deviceSimulator.preflight.checks.worker",
            Some("the same executable can enter isolated Worker mode; UAC is requested only at start".into()),
        )
    } else {
        failed(
            "worker",
            "deviceSimulator.preflight.checks.worker",
            Some("elevated Worker mode is unavailable on this platform".into()),
        )
    });

    checks.push(if !environment.firewall_required {
        warning(
            "firewall",
            "deviceSimulator.preflight.checks.firewall",
            Some("automatic firewall management is disabled; LAN reachability must be verified manually".into()),
        )
    } else if environment.firewall_available {
        passed(
            "firewall",
            "deviceSimulator.preflight.checks.firewall",
            None,
        )
    } else {
        failed(
            "firewall",
            "deviceSimulator.preflight.checks.firewall",
            Some("the precise Windows Firewall backend is not available".into()),
        )
    });

    let ok = !checks.iter().any(|check| {
        check.severity == PreflightCheckSeverity::Error
            && check.status == PreflightCheckStatus::Failed
    });
    PreflightReport {
        ok,
        checks,
        device_preview,
    }
}

fn empty_preview() -> DevicePreview {
    DevicePreview {
        total_devices: 0,
        total_channels: 0,
        devices: Vec::new(),
    }
}

fn passed(id: &str, message_key: &str, details: Option<String>) -> PreflightCheck {
    check(
        id,
        PreflightCheckSeverity::Info,
        PreflightCheckStatus::Passed,
        message_key,
        details,
    )
}

fn warning(id: &str, message_key: &str, details: Option<String>) -> PreflightCheck {
    check(
        id,
        PreflightCheckSeverity::Warning,
        PreflightCheckStatus::Warning,
        message_key,
        details,
    )
}

fn failed(id: &str, message_key: &str, details: Option<String>) -> PreflightCheck {
    check(
        id,
        PreflightCheckSeverity::Error,
        PreflightCheckStatus::Failed,
        message_key,
        details,
    )
}

fn check(
    id: &str,
    severity: PreflightCheckSeverity,
    status: PreflightCheckStatus,
    message_key: &str,
    details: Option<String>,
) -> PreflightCheck {
    PreflightCheck {
        id: id.into(),
        severity,
        status,
        message_key: message_key.into(),
        details,
    }
}

fn join_addresses(addresses: &[Ipv4Addr]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::api::{
        DeviceGroupDraft, DeviceSimulatorStreamKind, RtspPorts, StreamRuntimeConfig,
        StreamTransport, TargetPlatformConfig, TargetPlatformServer,
    };
    use crate::device_simulator::profiles::scope::TargetPlatform;
    use crate::device_simulator::windows::interfaces::StableInterfaceId;

    fn request() -> SimulatorStartRequest {
        SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Vms,
                servers: vec![TargetPlatformServer {
                    id: "vms-1".into(),
                    host: "192.0.2.10".into(),
                    port: 80,
                }],
                alarm_receiver_url: None,
            },
            interface_id: "guid:a0b1c2d3-1234-5678-90ab-010203040506".into(),
            start_ip: "192.168.50.10".parse().unwrap(),
            subnet_prefix: 24,
            device_http_port: 81,
            rtsp_ports: RtspPorts::default(),
            groups: vec![DeviceGroupDraft {
                id: "ipc".into(),
                profile_id: "ipc-custom".into(),
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
        }
    }

    fn interface() -> NetworkInterfaceInfo {
        NetworkInterfaceInfo {
            id: StableInterfaceId::from_adapter_guid("a0b1c2d3-1234-5678-90ab-010203040506")
                .unwrap(),
            name: "Ethernet".into(),
            description: "Test adapter".into(),
            interface_index: 7,
            is_enabled: true,
            is_up: true,
            mac_address: Some("001122334455".into()),
            ipv4_addresses: Vec::new(),
        }
    }

    #[test]
    fn unresolved_evidence_and_assets_block_start_but_risk_checks_remain_structured() {
        let report = run_preflight(
            &request(),
            &PreflightEnvironment {
                interfaces: vec![interface()],
                worker_available: true,
                firewall_required: true,
                firewall_available: true,
                ..Default::default()
            },
        );
        assert!(!report.ok);
        assert_eq!(report.device_preview.total_devices, 1);
        assert!(report.checks.iter().any(|check| {
            check.id == "address-conflicts" && check.status == PreflightCheckStatus::Warning
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "profile-evidence" && check.status == PreflightCheckStatus::Failed
        }));
    }

    #[test]
    fn local_conflict_and_residual_session_are_blocking() {
        let mut local_addresses = HashSet::new();
        local_addresses.insert("192.168.50.10".parse().unwrap());
        let report = run_preflight(
            &request(),
            &PreflightEnvironment {
                interfaces: vec![interface()],
                local_addresses,
                assets_ready: true,
                profiles_static_reviewed: true,
                profiles_platform_verified: true,
                worker_available: true,
                firewall_required: true,
                firewall_available: true,
                residual_session_id: Some("session-previous".into()),
                platform_connectivity: BTreeMap::from([("vms-1".into(), Some(true))]),
                ..Default::default()
            },
        );
        assert!(!report.ok);
        assert!(report.checks.iter().any(|check| {
            check.id == "local-addresses" && check.status == PreflightCheckStatus::Failed
        }));
        assert!(report.checks.iter().any(|check| {
            check.id == "recovery" && check.status == PreflightCheckStatus::Failed
        }));
    }
}
