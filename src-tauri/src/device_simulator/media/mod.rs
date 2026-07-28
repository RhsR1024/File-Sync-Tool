//! Normalized, non-executable media pack loading.
//!
//! Runtime code consumes indexed elementary-stream bytes rather than replaying
//! captured network packets. Compatibility remains evidence-gated: synthetic
//! fixtures are accepted only by unit tests and never by the runtime loader.

mod loader;
mod manifest;
pub(crate) mod mf_h264;
pub(crate) mod watermark;

pub use loader::{
    load_media_pack, MediaPackCache, MediaPackError, SharedMediaFrame, SharedMediaNal,
    SharedMediaPack,
};
pub use manifest::{
    Codec, EvidenceDifference, EvidenceResolution, EvidenceSourceKind, FrameIndex,
    MediaCompatibility, MediaEvidence, MediaManifestV1, NalIndex, ParameterSetKind,
    ParameterSetRef, MEDIA_MANIFEST_SCHEMA_VERSION,
};
