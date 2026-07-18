use serde::{Deserialize, Serialize};

pub const MEDIA_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const VIDEO_CLOCK_RATE: u32 = 90_000;
pub const MIN_DYNAMIC_PAYLOAD_TYPE: u8 = 96;
pub const MAX_DYNAMIC_PAYLOAD_TYPE: u8 = 127;
pub const MIN_RECOMMENDED_BITRATE_BPS: u64 = 1_000;
pub const MAX_RECOMMENDED_BITRATE_BPS: u64 = 100_000_000;
pub const MAX_MEDIA_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_MEDIA_FRAMES: usize = 1_000_000;
pub const MAX_NALS_PER_FRAME: usize = 1024;
pub const MAX_NAL_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_FRAME_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    H265,
}

impl Codec {
    pub(crate) fn expected_extension(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
        }
    }

    pub(crate) fn nal_type(self, nal: &[u8]) -> Option<u8> {
        match self {
            Self::H264 => nal.first().map(|header| header & 0x1f),
            Self::H265 => (nal.len() >= 2).then(|| (nal[0] >> 1) & 0x3f),
        }
    }

    pub(crate) fn is_keyframe_nal(self, nal_type: u8) -> bool {
        match self {
            Self::H264 => nal_type == 5,
            Self::H265 => (16..=21).contains(&nal_type),
        }
    }

    pub(crate) fn parameter_set_nal_type(self, kind: ParameterSetKind) -> Option<u8> {
        match (self, kind) {
            (Self::H264, ParameterSetKind::Sps) => Some(7),
            (Self::H264, ParameterSetKind::Pps) => Some(8),
            (Self::H265, ParameterSetKind::Vps) => Some(32),
            (Self::H265, ParameterSetKind::Sps) => Some(33),
            (Self::H265, ParameterSetKind::Pps) => Some(34),
            _ => None,
        }
    }

    pub(crate) fn required_parameter_sets(self) -> &'static [ParameterSetKind] {
        const H264: &[ParameterSetKind] = &[ParameterSetKind::Sps, ParameterSetKind::Pps];
        const H265: &[ParameterSetKind] = &[
            ParameterSetKind::Vps,
            ParameterSetKind::Sps,
            ParameterSetKind::Pps,
        ];
        match self {
            Self::H264 => H264,
            Self::H265 => H265,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterSetKind {
    Vps,
    Sps,
    Pps,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NalIndex {
    /// Absolute byte offset into the normalized elementary stream.
    pub offset: u64,
    pub length: u64,
    /// Codec-specific NAL type extracted by the pack builder.
    pub nal_type: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameIndex {
    pub offset: u64,
    pub length: u64,
    pub duration_ticks: u32,
    pub keyframe: bool,
    pub nals: Vec<NalIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterSetRef {
    pub kind: ParameterSetKind,
    pub frame_index: usize,
    pub nal_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSourceKind {
    AuthorizedPcap,
    SyntheticFixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCompatibility {
    Unverified,
    PlatformVerified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceResolution {
    Unresolved,
    PlatformVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDifference {
    pub field: String,
    pub pcap_value: String,
    pub sdp_value: String,
    pub selected_value: Option<String>,
    pub resolution: EvidenceResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaEvidence {
    pub source_kind: EvidenceSourceKind,
    pub pcap_source_id: Option<String>,
    pub sdp_source_id: Option<String>,
    pub compatibility: MediaCompatibility,
    pub verified_platforms: Vec<String>,
    pub differences: Vec<EvidenceDifference>,
}

/// Strict `media.json` contract for one normalized elementary stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaManifestV1 {
    pub schema_version: u32,
    pub id: String,
    pub codec: Codec,
    pub clock_rate: u32,
    pub payload_type: u8,
    pub frame_rate_numerator: u32,
    pub frame_rate_denominator: u32,
    pub recommended_bitrate_bps: u64,
    pub media_file: String,
    pub media_file_size: u64,
    pub media_file_sha256: String,
    pub frames: Vec<FrameIndex>,
    pub parameter_sets: Vec<ParameterSetRef>,
    pub evidence: MediaEvidence,
}
