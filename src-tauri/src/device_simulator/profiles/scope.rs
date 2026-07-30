use crate::device_simulator::assets::catalog::DeviceKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetPlatform {
    Ums,
}

pub const FIRST_RELEASE_PLATFORMS: [TargetPlatform; 1] = [TargetPlatform::Ums];

/// The only simulated device type. The custom-alarm, smart, face-access and the
/// two NVR profiles were removed along with their protocol, streaming and alarm
/// implementations; the enum is kept so profile identity stays explicit in the
/// catalog, pack and journal formats rather than becoming an implicit constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FirstReleaseProfileId {
    #[serde(rename = "ipc-structured")]
    IpcStructured,
}

impl FirstReleaseProfileId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IpcStructured => "ipc-structured",
        }
    }

    pub const fn legacy_device_type(self) -> &'static str {
        match self {
            Self::IpcStructured => "结构化相机",
        }
    }

    pub const fn device_kind(self) -> DeviceKind {
        match self {
            Self::IpcStructured => DeviceKind::Ipc,
        }
    }
}

pub const FIRST_RELEASE_PROFILES: [FirstReleaseProfileId; 1] =
    [FirstReleaseProfileId::IpcStructured];

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

/// Approved compatibility baseline for the structured camera profile.
/// Profile packs may narrow capabilities but may not silently expand them.
pub const FIRST_RELEASE_PROTOCOL_POLICY: FirstReleaseProtocolPolicy = FirstReleaseProtocolPolicy {
    device_authentication: DeviceAuthenticationPolicy::None,
    rtsp_authentication: RtspAuthenticationPolicy::None,
    rtsp_transport: RtspTransportPolicy::TcpInterleavedOnly,
    streams: StreamActivationPolicy::MainSubThird,
    audio: AudioPolicy::Disabled,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_release_scope_is_ums_only_and_structured_camera_only() {
        assert_eq!(FIRST_RELEASE_PLATFORMS, [TargetPlatform::Ums]);
        assert_eq!(
            FIRST_RELEASE_PROFILES.map(FirstReleaseProfileId::as_str),
            ["ipc-structured"]
        );
        assert_eq!(
            FirstReleaseProfileId::IpcStructured.legacy_device_type(),
            "结构化相机"
        );
        assert_eq!(
            FirstReleaseProfileId::IpcStructured.device_kind(),
            DeviceKind::Ipc
        );
    }

    #[test]
    fn scope_serializes_to_catalog_facing_ids() {
        assert_eq!(
            serde_json::to_string(&FirstReleaseProfileId::IpcStructured).unwrap(),
            "\"ipc-structured\""
        );
        assert_eq!(
            serde_json::to_string(&TargetPlatform::Ums).unwrap(),
            "\"ums\""
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
}
