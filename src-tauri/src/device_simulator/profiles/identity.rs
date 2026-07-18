use super::scope::{validate_nvr_channel_count, FirstReleaseProfileId};
use crate::device_simulator::assets::catalog::DeviceKind;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::net::Ipv4Addr;

pub const MAX_PREVIEW_DEVICES: u16 = 500;
pub const DEFAULT_HTTP_PORT: u16 = 81;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityPlan {
    pub profile_id: FirstReleaseProfileId,
    pub network: Ipv4Net,
    pub start_ip: Ipv4Addr,
    pub device_count: u16,
    pub deterministic_seed: [u8; 32],
    pub http_port: u16,
    pub nvr_channel_count: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentityPreview {
    pub ordinal: u16,
    pub ip: Ipv4Addr,
    pub mac_compact: String,
    pub serial_number: String,
    pub hardware_id: String,
    pub http_url: String,
    pub streams: Vec<StreamPreview>,
    pub nvr_channel_count: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamPreview {
    pub name: StreamName,
    pub port: u16,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamName {
    Main,
    Sub,
    Third,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityPlanError {
    pub code: &'static str,
    pub message: String,
}

impl IdentityPlanError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for IdentityPlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for IdentityPlanError {}

pub fn generate_device_previews(
    plan: &IdentityPlan,
    occupied_addresses: &HashSet<Ipv4Addr>,
) -> Result<Vec<DeviceIdentityPreview>, IdentityPlanError> {
    validate_plan(plan)?;
    let first = u32::from(plan.start_ip);
    let last = u32::from(plan.network.broadcast()).saturating_sub(1);
    let requested_last = first
        .checked_add(u32::from(plan.device_count).saturating_sub(1))
        .ok_or_else(|| capacity_error("device address range overflowed IPv4"))?;
    if requested_last > last {
        return Err(capacity_error(format!(
            "{} devices from {} do not fit in {}",
            plan.device_count, plan.start_ip, plan.network
        )));
    }

    let mut result = Vec::with_capacity(plan.device_count as usize);
    let mut identities = HashSet::new();
    for offset in 0..u32::from(plan.device_count) {
        let ip = Ipv4Addr::from(first + offset);
        if occupied_addresses.contains(&ip) {
            return Err(IdentityPlanError::new(
                "device_simulator.validation.ip_conflict",
                format!("planned device address {ip} is already in use"),
            ));
        }
        let ordinal = offset as u16 + 1;
        let digest = identity_digest(plan, ip, ordinal);
        let mac_compact = format!(
            "02{:02x}{:02x}{:02x}{:02x}{:02x}",
            digest[0], digest[1], digest[2], digest[3], digest[4]
        );
        let serial_number = digest[5..13]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        let hardware_id = format!("210235{}{:06}", &serial_number[..10], ordinal);
        if !identities.insert((
            mac_compact.clone(),
            serial_number.clone(),
            hardware_id.clone(),
        )) {
            return Err(IdentityPlanError::new(
                "device_simulator.validation.identity_duplicate",
                "deterministic identity generation produced a duplicate",
            ));
        }
        result.push(DeviceIdentityPreview {
            ordinal,
            ip,
            mac_compact,
            serial_number,
            hardware_id,
            http_url: format!("http://{ip}:{}", plan.http_port),
            streams: stream_previews(plan.profile_id.device_kind(), ip),
            nvr_channel_count: plan.nvr_channel_count,
        });
    }
    Ok(result)
}

fn validate_plan(plan: &IdentityPlan) -> Result<(), IdentityPlanError> {
    if plan.device_count == 0 || plan.device_count > MAX_PREVIEW_DEVICES {
        return Err(IdentityPlanError::new(
            "device_simulator.validation.device_count_invalid",
            format!("device count must be between 1 and {MAX_PREVIEW_DEVICES}"),
        ));
    }
    if plan.http_port == 0 {
        return Err(IdentityPlanError::new(
            "device_simulator.validation.http_port_invalid",
            "HTTP port must be non-zero",
        ));
    }
    if plan.network.prefix_len() > 30 {
        return Err(capacity_error("device network must provide host addresses"));
    }
    if !plan.network.contains(&plan.start_ip)
        || plan.start_ip == plan.network.network()
        || plan.start_ip == plan.network.broadcast()
    {
        return Err(IdentityPlanError::new(
            "device_simulator.validation.start_ip_invalid",
            format!(
                "start IP {} is not a usable host in {}",
                plan.start_ip, plan.network
            ),
        ));
    }
    match (plan.profile_id.device_kind(), plan.nvr_channel_count) {
        (DeviceKind::Nvr, Some(channels)) => {
            validate_nvr_channel_count(channels).map_err(|code| {
                IdentityPlanError::new(code, format!("invalid NVR channel count {channels}"))
            })?
        }
        (DeviceKind::Nvr, None) => {
            return Err(IdentityPlanError::new(
                "device_simulator.validation.nvr_channel_count_missing",
                "NVR profile requires a channel count",
            ));
        }
        (DeviceKind::Ipc, Some(_)) => {
            return Err(IdentityPlanError::new(
                "device_simulator.validation.ipc_channel_count_invalid",
                "IPC profile must not declare an NVR channel count",
            ));
        }
        (DeviceKind::Ipc, None) => {}
    }
    Ok(())
}

fn identity_digest(plan: &IdentityPlan, ip: Ipv4Addr, ordinal: u16) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(plan.deterministic_seed);
    hasher.update(plan.profile_id.as_str().as_bytes());
    hasher.update(ip.octets());
    hasher.update(ordinal.to_be_bytes());
    hasher.finalize().into()
}

fn stream_previews(kind: DeviceKind, ip: Ipv4Addr) -> Vec<StreamPreview> {
    [
        (StreamName::Main, 554, 1_u8),
        (StreamName::Sub, 555, 2),
        (StreamName::Third, 556, 3),
    ]
    .into_iter()
    .map(|(name, port, number)| {
        let url = match kind {
            DeviceKind::Ipc => format!("rtsp://{ip}:{port}/media/video{number}"),
            DeviceKind::Nvr => {
                format!("rtsp://{ip}:{port}/unicast/c1/s{}/live", number - 1)
            }
        };
        StreamPreview { name, port, url }
    })
    .collect()
}

fn capacity_error(message: impl Into<String>) -> IdentityPlanError {
    IdentityPlanError::new(
        "device_simulator.validation.network_capacity_insufficient",
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(
        profile_id: FirstReleaseProfileId,
        network: &str,
        start: &str,
        count: u16,
    ) -> IdentityPlan {
        IdentityPlan {
            profile_id,
            network: network.parse().unwrap(),
            start_ip: start.parse().unwrap(),
            device_count: count,
            deterministic_seed: [3_u8; 32],
            http_port: DEFAULT_HTTP_PORT,
            nvr_channel_count: (profile_id.device_kind() == DeviceKind::Nvr).then_some(8),
        }
    }

    #[test]
    fn supports_non_24_networks_and_crosses_octet_boundaries() {
        let plan = plan(
            FirstReleaseProfileId::IpcSmart,
            "10.20.0.0/23",
            "10.20.0.254",
            3,
        );
        let previews = generate_device_previews(&plan, &HashSet::new()).unwrap();
        assert_eq!(
            previews.iter().map(|item| item.ip).collect::<Vec<_>>(),
            ["10.20.0.254", "10.20.0.255", "10.20.1.0"]
                .map(|value| value.parse::<Ipv4Addr>().unwrap())
        );
        assert_eq!(previews[0].mac_compact.len(), 12);
        assert_eq!(previews[0].streams[2].port, 556);
    }

    #[test]
    fn generation_is_deterministic_and_profile_specific() {
        let smart = plan(
            FirstReleaseProfileId::IpcSmart,
            "10.0.0.0/24",
            "10.0.0.2",
            2,
        );
        let custom = plan(
            FirstReleaseProfileId::IpcCustom,
            "10.0.0.0/24",
            "10.0.0.2",
            2,
        );
        let first = generate_device_previews(&smart, &HashSet::new()).unwrap();
        assert_eq!(
            first,
            generate_device_previews(&smart, &HashSet::new()).unwrap()
        );
        assert_ne!(
            first[0].serial_number,
            generate_device_previews(&custom, &HashSet::new()).unwrap()[0].serial_number
        );
    }

    #[test]
    fn rejects_capacity_conflicts_and_kind_mismatches() {
        let small = plan(
            FirstReleaseProfileId::IpcSmart,
            "192.0.2.0/29",
            "192.0.2.5",
            3,
        );
        assert_eq!(
            generate_device_previews(&small, &HashSet::new())
                .unwrap_err()
                .code,
            "device_simulator.validation.network_capacity_insufficient"
        );
        let valid = plan(
            FirstReleaseProfileId::IpcSmart,
            "192.0.2.0/29",
            "192.0.2.2",
            2,
        );
        assert_eq!(
            generate_device_previews(&valid, &HashSet::from(["192.0.2.3".parse().unwrap()]))
                .unwrap_err()
                .code,
            "device_simulator.validation.ip_conflict"
        );
        let mut nvr = plan(
            FirstReleaseProfileId::NvrCommon,
            "192.0.2.0/24",
            "192.0.2.2",
            1,
        );
        nvr.nvr_channel_count = None;
        assert_eq!(
            generate_device_previews(&nvr, &HashSet::new())
                .unwrap_err()
                .code,
            "device_simulator.validation.nvr_channel_count_missing"
        );
    }

    #[test]
    fn nvr_preview_uses_approved_channel_and_stream_summary() {
        let plan = plan(
            FirstReleaseProfileId::NvrVehicle,
            "198.51.100.0/24",
            "198.51.100.10",
            1,
        );
        let preview = generate_device_previews(&plan, &HashSet::new())
            .unwrap()
            .remove(0);
        assert_eq!(preview.nvr_channel_count, Some(8));
        assert_eq!(
            preview.streams[0].url,
            "rtsp://198.51.100.10:554/unicast/c1/s0/live"
        );
    }
}
