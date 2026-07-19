use crate::device_simulator::assets::catalog::DeviceKind;
use serde::{Deserialize, Serialize};

pub const DEFAULT_NVR_CHANNEL_COUNT: u16 = 8;

/// Engineering ceiling for the first release.
///
/// The legacy configuration establishes a default of 8 but has no maximum
/// guard. Its fixed GetProfiles asset covers channels 1 through 128, so the
/// first release treats 128 as a product safety boundary, not a vendor limit.
pub const MAX_NVR_CHANNEL_COUNT: u16 = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetPlatform {
    Vms,
    Ums,
}

pub const FIRST_RELEASE_PLATFORMS: [TargetPlatform; 2] = [TargetPlatform::Vms, TargetPlatform::Ums];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FirstReleaseProfileId {
    #[serde(rename = "ipc-custom")]
    IpcCustom,
    #[serde(rename = "ipc-smart")]
    IpcSmart,
    #[serde(rename = "nvr-common")]
    NvrCommon,
    #[serde(rename = "nvr-vehicle")]
    NvrVehicle,
}

impl FirstReleaseProfileId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IpcCustom => "ipc-custom",
            Self::IpcSmart => "ipc-smart",
            Self::NvrCommon => "nvr-common",
            Self::NvrVehicle => "nvr-vehicle",
        }
    }

    pub const fn legacy_device_type(self) -> &'static str {
        match self {
            Self::IpcCustom => "自定义报警相机",
            Self::IpcSmart => "智能相机",
            Self::NvrCommon => "普通NVR",
            Self::NvrVehicle => "车辆识别NVR",
        }
    }

    pub const fn device_kind(self) -> DeviceKind {
        match self {
            Self::IpcCustom | Self::IpcSmart => DeviceKind::Ipc,
            Self::NvrCommon | Self::NvrVehicle => DeviceKind::Nvr,
        }
    }
}

pub const FIRST_RELEASE_PROFILES: [FirstReleaseProfileId; 4] = [
    FirstReleaseProfileId::IpcCustom,
    FirstReleaseProfileId::IpcSmart,
    FirstReleaseProfileId::NvrCommon,
    FirstReleaseProfileId::NvrVehicle,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAuthenticationPolicy {
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RtspAuthenticationPolicy {
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RtspTransportPolicy {
    TcpInterleavedOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamActivationPolicy {
    MainSubThird,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioPolicy {
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirstReleaseProtocolPolicy {
    pub device_authentication: DeviceAuthenticationPolicy,
    pub rtsp_authentication: RtspAuthenticationPolicy,
    pub rtsp_transport: RtspTransportPolicy,
    pub streams: StreamActivationPolicy,
    pub audio: AudioPolicy,
}

/// Approved compatibility baseline for all four first-release profiles.
/// Profile packs may narrow capabilities but may not silently expand them.
pub const FIRST_RELEASE_PROTOCOL_POLICY: FirstReleaseProtocolPolicy = FirstReleaseProtocolPolicy {
    device_authentication: DeviceAuthenticationPolicy::None,
    rtsp_authentication: RtspAuthenticationPolicy::None,
    rtsp_transport: RtspTransportPolicy::TcpInterleavedOnly,
    streams: StreamActivationPolicy::MainSubThird,
    audio: AudioPolicy::Disabled,
};

pub fn validate_nvr_channel_count(channel_count: u16) -> Result<(), &'static str> {
    if channel_count == 0 {
        return Err("device_simulator.validation.nvr_channel_count_zero");
    }
    if channel_count > MAX_NVR_CHANNEL_COUNT {
        return Err("device_simulator.validation.nvr_channel_count_too_large");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_release_scope_is_vms_ums_and_four_independent_profiles() {
        assert_eq!(
            FIRST_RELEASE_PLATFORMS,
            [TargetPlatform::Vms, TargetPlatform::Ums]
        );
        assert_eq!(
            FIRST_RELEASE_PROFILES.map(FirstReleaseProfileId::as_str),
            ["ipc-custom", "ipc-smart", "nvr-common", "nvr-vehicle"]
        );
        assert_eq!(
            FirstReleaseProfileId::IpcSmart.legacy_device_type(),
            "智能相机"
        );
        assert_eq!(
            FirstReleaseProfileId::IpcSmart.device_kind(),
            DeviceKind::Ipc
        );
        assert_eq!(
            FirstReleaseProfileId::NvrCommon.device_kind(),
            DeviceKind::Nvr
        );
    }

    #[test]
    fn scope_serializes_to_catalog_facing_ids() {
        assert_eq!(
            serde_json::to_string(&FirstReleaseProfileId::IpcSmart).unwrap(),
            "\"ipc-smart\""
        );
        assert_eq!(
            serde_json::to_string(&TargetPlatform::Vms).unwrap(),
            "\"vms\""
        );
    }

    #[test]
    fn protocol_baseline_matches_legacy_rtsp_runtime() {
        assert_eq!(
            FIRST_RELEASE_PROTOCOL_POLICY,
            FirstReleaseProtocolPolicy {
                device_authentication: DeviceAuthenticationPolicy::None,
                rtsp_authentication: RtspAuthenticationPolicy::None,
                rtsp_transport: RtspTransportPolicy::TcpInterleavedOnly,
                streams: StreamActivationPolicy::MainSubThird,
                audio: AudioPolicy::Disabled,
            }
        );
    }

    #[test]
    fn nvr_channel_policy_keeps_legacy_default_but_rejects_unbounded_counts() {
        assert_eq!(DEFAULT_NVR_CHANNEL_COUNT, 8);
        assert!(validate_nvr_channel_count(DEFAULT_NVR_CHANNEL_COUNT).is_ok());
        assert!(validate_nvr_channel_count(MAX_NVR_CHANNEL_COUNT).is_ok());
        assert_eq!(
            validate_nvr_channel_count(0),
            Err("device_simulator.validation.nvr_channel_count_zero")
        );
        assert_eq!(
            validate_nvr_channel_count(MAX_NVR_CHANNEL_COUNT + 1),
            Err("device_simulator.validation.nvr_channel_count_too_large")
        );
    }
}
