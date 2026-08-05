use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use sha2::{Digest, Sha256};

use crate::device_simulator::assets::validation::validate_pack_path;

use super::manifest::{
    EvidenceResolution, EvidenceSourceKind, MediaCompatibility, MediaManifestV1, ParameterSetKind,
    MAX_DYNAMIC_PAYLOAD_TYPE, MAX_FRAME_BYTES, MAX_MEDIA_BYTES, MAX_MEDIA_FRAMES,
    MAX_NALS_PER_FRAME, MAX_NAL_BYTES, MAX_RECOMMENDED_BITRATE_BPS, MEDIA_MANIFEST_SCHEMA_VERSION,
    MIN_DYNAMIC_PAYLOAD_TYPE, MIN_RECOMMENDED_BITRATE_BPS, VIDEO_CLOCK_RATE,
};

// Long local recordings carry one compact JSON entry per encoded frame.  The
// media itself is streamed from disk, so allowing a larger bounded index does
// not pull the recording into memory.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BITRATE_RATIO: u64 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPackError {
    pub code: &'static str,
    pub message: String,
}

impl MediaPackError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for MediaPackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for MediaPackError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedMediaNal {
    pub offset: usize,
    pub length: usize,
    pub nal_type: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedMediaFrame {
    pub duration_ticks: u32,
    pub keyframe: bool,
    pub nals: Arc<[SharedMediaNal]>,
}

#[derive(Debug)]
pub struct SharedMediaPack {
    manifest: Arc<MediaManifestV1>,
    media_file: Arc<File>,
    media_file_size: u64,
    frames: Arc<[SharedMediaFrame]>,
    actual_bitrate_bps: u64,
}

impl SharedMediaPack {
    pub fn manifest(&self) -> &MediaManifestV1 {
        &self.manifest
    }

    pub fn frames(&self) -> &[SharedMediaFrame] {
        &self.frames
    }

    pub fn actual_bitrate_bps(&self) -> u64 {
        self.actual_bitrate_bps
    }

    pub fn media_file_size(&self) -> u64 {
        self.media_file_size
    }

    pub fn read_frame_nals(
        &self,
        frame_index: usize,
    ) -> Result<(bool, Vec<Vec<u8>>), MediaPackError> {
        let frame = self.frames.get(frame_index).ok_or_else(|| {
            MediaPackError::new(
                "device_simulator.media.frame_index_invalid",
                format!("media frame index {frame_index} is out of bounds"),
            )
        })?;
        let nals = frame
            .nals
            .iter()
            .map(|nal| self.read_nal(nal))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((frame.keyframe, nals))
    }

    pub fn read_nal(&self, nal: &SharedMediaNal) -> Result<Vec<u8>, MediaPackError> {
        let mut bytes = vec![0_u8; nal.length];
        read_exact_at(&self.media_file, &mut bytes, nal.offset as u64).map_err(|error| {
            MediaPackError::new(
                "device_simulator.media.media_read_failed",
                format!("failed to read indexed media NAL: {error}"),
            )
        })?;
        Ok(bytes)
    }

    pub fn parameter_set(&self, kind: ParameterSetKind) -> Option<Vec<u8>> {
        let reference = self
            .manifest
            .parameter_sets
            .iter()
            .find(|reference| reference.kind == kind)?;
        let frame = self.frames.get(reference.frame_index)?;
        self.read_nal(frame.nals.get(reference.nal_index)?).ok()
    }
}

#[derive(Debug, Clone, Copy)]
enum EvidencePolicy {
    RuntimeVerifiedOnly,
    AllowLocalMaterial,
}

#[derive(Debug, Default)]
pub struct MediaPackCache {
    entries: Mutex<HashMap<PathBuf, Arc<SharedMediaPack>>>,
}

impl MediaPackCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(
        &self,
        pack_dir: &Path,
        manifest_relative_path: &str,
    ) -> Result<Arc<SharedMediaPack>, MediaPackError> {
        self.load_with_policy(
            pack_dir,
            manifest_relative_path,
            EvidencePolicy::RuntimeVerifiedOnly,
        )
    }

    /// Loads user-controlled loose media. It keeps all path, size, codec and
    /// frame-index validation, but deliberately does not require release-pack
    /// provenance or a signed catalog.
    pub fn load_local(
        &self,
        material_dir: &Path,
        manifest_relative_path: &str,
    ) -> Result<Arc<SharedMediaPack>, MediaPackError> {
        self.load_with_policy(
            material_dir,
            manifest_relative_path,
            EvidencePolicy::AllowLocalMaterial,
        )
    }

    fn load_with_policy(
        &self,
        pack_dir: &Path,
        manifest_relative_path: &str,
        policy: EvidencePolicy,
    ) -> Result<Arc<SharedMediaPack>, MediaPackError> {
        let manifest_path = resolve_pack_file(pack_dir, manifest_relative_path)?;
        let mut entries = self.entries.lock().map_err(|_| {
            MediaPackError::new(
                "device_simulator.media.cache_poisoned",
                "media cache lock is poisoned",
            )
        })?;
        if let Some(pack) = entries.get(&manifest_path) {
            return Ok(Arc::clone(pack));
        }

        let pack = Arc::new(load_media_pack_from_path(pack_dir, &manifest_path, policy)?);
        entries.insert(manifest_path, Arc::clone(&pack));
        Ok(pack)
    }

    #[cfg(test)]
    fn load_synthetic_fixture(
        &self,
        pack_dir: &Path,
        manifest_relative_path: &str,
    ) -> Result<Arc<SharedMediaPack>, MediaPackError> {
        self.load_with_policy(
            pack_dir,
            manifest_relative_path,
            EvidencePolicy::AllowLocalMaterial,
        )
    }
}

pub fn load_media_pack(
    pack_dir: &Path,
    manifest_relative_path: &str,
) -> Result<Arc<SharedMediaPack>, MediaPackError> {
    MediaPackCache::new().load(pack_dir, manifest_relative_path)
}

fn load_media_pack_from_path(
    pack_dir: &Path,
    manifest_path: &Path,
    policy: EvidencePolicy,
) -> Result<SharedMediaPack, MediaPackError> {
    ensure_regular_bounded_file(manifest_path, MAX_MANIFEST_BYTES, "manifest")?;
    let manifest_bytes = read_bounded_file(manifest_path, MAX_MANIFEST_BYTES, "manifest")?;
    let manifest: MediaManifestV1 = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.manifest_invalid",
            format!("invalid media manifest: {error}"),
        )
    })?;

    validate_manifest_metadata(&manifest, policy)?;
    let manifest_dir = manifest_path.parent().ok_or_else(|| {
        MediaPackError::new(
            "device_simulator.media.unsafe_path",
            "media manifest has no parent directory",
        )
    })?;
    let media_path = resolve_pack_file_from(pack_dir, manifest_dir, &manifest.media_file)?;
    ensure_regular_bounded_file(&media_path, MAX_MEDIA_BYTES, "media")?;
    let media_metadata = fs::metadata(&media_path).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.media_metadata_failed",
            format!("failed to inspect {}: {error}", media_path.display()),
        )
    })?;
    if media_metadata.len() != manifest.media_file_size {
        return Err(MediaPackError::new(
            "device_simulator.media.size_mismatch",
            format!(
                "media declares {} bytes but file has {} bytes",
                manifest.media_file_size,
                media_metadata.len()
            ),
        ));
    }

    let media_file = File::open(&media_path).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.media_read_failed",
            format!("failed to open {}: {error}", media_path.display()),
        )
    })?;
    if matches!(policy, EvidencePolicy::RuntimeVerifiedOnly) {
        let actual_sha256 = hash_file(&media_file)?;
        if actual_sha256 != manifest.media_file_sha256 {
            return Err(MediaPackError::new(
                "device_simulator.media.hash_mismatch",
                "media SHA-256 does not match manifest",
            ));
        }
    }

    let (frames, actual_bitrate_bps) =
        validate_index(&manifest, &media_file, media_metadata.len())?;
    Ok(SharedMediaPack {
        manifest: Arc::new(manifest),
        media_file: Arc::new(media_file),
        media_file_size: media_metadata.len(),
        frames: Arc::from(frames),
        actual_bitrate_bps,
    })
}

fn hash_file(file: &File) -> Result<String, MediaPackError> {
    let mut reader = BufReader::new(file.try_clone().map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.media_read_failed",
            format!("failed to clone media file handle: {error}"),
        )
    })?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            MediaPackError::new(
                "device_simulator.media.media_read_failed",
                format!("failed to hash media file: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_manifest_metadata(
    manifest: &MediaManifestV1,
    policy: EvidencePolicy,
) -> Result<(), MediaPackError> {
    if manifest.schema_version != MEDIA_MANIFEST_SCHEMA_VERSION {
        return Err(MediaPackError::new(
            "device_simulator.media.schema_unsupported",
            format!(
                "media schema {} is unsupported; expected {}",
                manifest.schema_version, MEDIA_MANIFEST_SCHEMA_VERSION
            ),
        ));
    }
    if !valid_id(&manifest.id) {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_id",
            "media id must be a lowercase ASCII token",
        ));
    }
    if manifest.clock_rate != VIDEO_CLOCK_RATE {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_clock_rate",
            format!("video clock rate must be {VIDEO_CLOCK_RATE}"),
        ));
    }
    if !(MIN_DYNAMIC_PAYLOAD_TYPE..=MAX_DYNAMIC_PAYLOAD_TYPE).contains(&manifest.payload_type) {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_payload_type",
            "RTP payload type must be in the dynamic range 96-127",
        ));
    }
    let duration_numerator = u64::from(manifest.clock_rate)
        .checked_mul(u64::from(manifest.frame_rate_denominator))
        .ok_or_else(|| {
            MediaPackError::new(
                "device_simulator.media.invalid_frame_rate",
                "frame rate overflows media clock calculation",
            )
        })?;
    if manifest.frame_rate_numerator == 0
        || manifest.frame_rate_denominator == 0
        || duration_numerator % u64::from(manifest.frame_rate_numerator) != 0
    {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_frame_rate",
            "frame rate must map exactly to the 90 kHz media clock",
        ));
    }
    if !(MIN_RECOMMENDED_BITRATE_BPS..=MAX_RECOMMENDED_BITRATE_BPS)
        .contains(&manifest.recommended_bitrate_bps)
    {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_bitrate",
            "recommended bitrate is outside safe limits",
        ));
    }
    if manifest.media_file_size == 0 || manifest.media_file_size > MAX_MEDIA_BYTES {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_size",
            "media size is zero or exceeds the one GiB limit",
        ));
    }
    if manifest.media_file_sha256.len() != 64
        || !manifest
            .media_file_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_hash",
            "media SHA-256 must contain 64 lowercase hexadecimal characters",
        ));
    }
    let expected_extension = manifest.codec.expected_extension();
    let actual_extension = Path::new(&manifest.media_file)
        .extension()
        .and_then(|extension| extension.to_str());
    if actual_extension != Some(expected_extension) {
        return Err(MediaPackError::new(
            "device_simulator.media.codec_file_mismatch",
            format!(
                "{:?} media must use the .{expected_extension} extension",
                manifest.codec
            ),
        ));
    }
    validate_evidence(manifest, policy)
}

fn validate_evidence(
    manifest: &MediaManifestV1,
    _policy: EvidencePolicy,
) -> Result<(), MediaPackError> {
    let evidence = &manifest.evidence;
    match evidence.source_kind {
        EvidenceSourceKind::SyntheticFixture => {
            if matches!(_policy, EvidencePolicy::AllowLocalMaterial)
                && evidence.compatibility == MediaCompatibility::Unverified
                && evidence.pcap_source_id.is_none()
                && evidence.sdp_source_id.is_none()
                && evidence.verified_platforms.is_empty()
                && evidence.differences.is_empty()
            {
                return Ok(());
            }
            Err(MediaPackError::new(
                "device_simulator.media.compatibility_unverified",
                "synthetic media fixtures are test-only and are not runtime-compatible evidence",
            ))
        }
        EvidenceSourceKind::AuthorizedPcap => {
            if !matches!(
                evidence.compatibility,
                MediaCompatibility::ReviewedStatic | MediaCompatibility::PlatformVerified
            ) || evidence
                .verified_platforms
                .iter()
                .any(|platform| platform.trim().is_empty())
                || evidence
                    .pcap_source_id
                    .as_deref()
                    .map_or(true, |value| value.trim().is_empty())
                || evidence
                    .sdp_source_id
                    .as_deref()
                    .map_or(true, |value| value.trim().is_empty())
            {
                return Err(MediaPackError::new(
                    "device_simulator.media.compatibility_unverified",
                    "authorized PCAP media must include reviewed static evidence and SDP provenance",
                ));
            }
            if evidence.compatibility == MediaCompatibility::PlatformVerified
                && evidence.verified_platforms.is_empty()
            {
                return Err(MediaPackError::new(
                    "device_simulator.media.compatibility_unverified",
                    "platform-verified media must name at least one verified platform",
                ));
            }
            for difference in &evidence.differences {
                if difference.field.trim().is_empty()
                    || difference.pcap_value.trim().is_empty()
                    || difference.sdp_value.trim().is_empty()
                    || difference
                        .selected_value
                        .as_deref()
                        .map_or(true, |value| value.trim().is_empty())
                    || match evidence.compatibility {
                        MediaCompatibility::ReviewedStatic => !matches!(
                            difference.resolution,
                            EvidenceResolution::UserApproved | EvidenceResolution::PlatformVerified
                        ),
                        MediaCompatibility::PlatformVerified => {
                            difference.resolution != EvidenceResolution::PlatformVerified
                        }
                        MediaCompatibility::Unverified => true,
                    }
                {
                    return Err(MediaPackError::new(
                        "device_simulator.media.evidence_difference_unresolved",
                        "PCAP and SDP differences must be explicitly user-approved or platform-verified",
                    ));
                }
            }
            Ok(())
        }
    }
}

fn validate_index(
    manifest: &MediaManifestV1,
    media_file: &File,
    media_file_size: u64,
) -> Result<(Vec<SharedMediaFrame>, u64), MediaPackError> {
    if manifest.frames.is_empty() || manifest.frames.len() > MAX_MEDIA_FRAMES {
        return Err(MediaPackError::new(
            "device_simulator.media.invalid_frame_count",
            "media must contain a bounded, non-empty frame index",
        ));
    }

    let expected_duration = u64::from(manifest.clock_rate)
        * u64::from(manifest.frame_rate_denominator)
        / u64::from(manifest.frame_rate_numerator);
    let mut next_frame_offset = 0_u64;
    let mut total_ticks = 0_u64;
    let mut frames = Vec::with_capacity(manifest.frames.len());

    for (frame_index, frame) in manifest.frames.iter().enumerate() {
        if frame.offset != next_frame_offset
            || frame.length == 0
            || frame.length > MAX_FRAME_BYTES
            || frame.duration_ticks == 0
            || u64::from(frame.duration_ticks) != expected_duration
            || frame.nals.is_empty()
            || frame.nals.len() > MAX_NALS_PER_FRAME
        {
            return Err(MediaPackError::new(
                "device_simulator.media.invalid_frame_index",
                format!("frame {frame_index} has invalid boundaries, duration, or NAL count"),
            ));
        }
        let frame_end = frame.offset.checked_add(frame.length).ok_or_else(|| {
            MediaPackError::new(
                "device_simulator.media.index_out_of_bounds",
                format!("frame {frame_index} overflows its byte range"),
            )
        })?;
        if frame_end > media_file_size {
            return Err(MediaPackError::new(
                "device_simulator.media.index_out_of_bounds",
                format!("frame {frame_index} exceeds the media buffer"),
            ));
        }

        let mut next_nal_offset = frame.offset;
        let mut has_keyframe_nal = false;
        let mut runtime_nals = Vec::with_capacity(frame.nals.len());
        for (nal_index, nal) in frame.nals.iter().enumerate() {
            if nal.offset != next_nal_offset || nal.length == 0 || nal.length > MAX_NAL_BYTES {
                return Err(MediaPackError::new(
                    "device_simulator.media.invalid_nal_index",
                    format!("NAL {nal_index} in frame {frame_index} is not contiguous or bounded"),
                ));
            }
            let nal_end = nal.offset.checked_add(nal.length).ok_or_else(|| {
                MediaPackError::new(
                    "device_simulator.media.index_out_of_bounds",
                    format!("NAL {nal_index} in frame {frame_index} overflows"),
                )
            })?;
            if nal_end > frame_end {
                return Err(MediaPackError::new(
                    "device_simulator.media.index_out_of_bounds",
                    format!("NAL {nal_index} in frame {frame_index} exceeds the frame"),
                ));
            }
            let start = usize::try_from(nal.offset).map_err(|_| {
                MediaPackError::new(
                    "device_simulator.media.index_out_of_bounds",
                    "NAL offset cannot be represented by this process",
                )
            })?;
            let end = usize::try_from(nal_end).map_err(|_| {
                MediaPackError::new(
                    "device_simulator.media.index_out_of_bounds",
                    "NAL end cannot be represented by this process",
                )
            })?;
            let header_len = match manifest.codec {
                super::manifest::Codec::H264 => 1,
                super::manifest::Codec::H265 => 2,
            };
            let mut header = [0_u8; 2];
            read_exact_at(media_file, &mut header[..header_len], nal.offset).map_err(|error| {
                MediaPackError::new(
                    "device_simulator.media.media_read_failed",
                    format!("failed to inspect NAL {nal_index} in frame {frame_index}: {error}"),
                )
            })?;
            let actual_nal_type =
                manifest
                    .codec
                    .nal_type(&header[..header_len])
                    .ok_or_else(|| {
                        MediaPackError::new(
                            "device_simulator.media.invalid_nal_header",
                            format!(
                                "NAL {nal_index} in frame {frame_index} has an incomplete header"
                            ),
                        )
                    })?;
            if actual_nal_type != nal.nal_type {
                return Err(MediaPackError::new(
                    "device_simulator.media.nal_type_mismatch",
                    format!(
                        "NAL {nal_index} in frame {frame_index} declares type {} but contains type {actual_nal_type}",
                        nal.nal_type
                    ),
                ));
            }
            has_keyframe_nal |= manifest.codec.is_keyframe_nal(actual_nal_type);
            runtime_nals.push(SharedMediaNal {
                offset: start,
                length: end - start,
                nal_type: actual_nal_type,
            });
            next_nal_offset = nal_end;
        }
        if next_nal_offset != frame_end || frame.keyframe != has_keyframe_nal {
            return Err(MediaPackError::new(
                "device_simulator.media.frame_keyframe_mismatch",
                format!("frame {frame_index} does not match its NAL coverage or keyframe flag"),
            ));
        }
        if frame_index == 0 && !frame.keyframe {
            return Err(MediaPackError::new(
                "device_simulator.media.loop_without_keyframe",
                "the first frame must be a keyframe so looping can restart safely",
            ));
        }

        frames.push(SharedMediaFrame {
            duration_ticks: frame.duration_ticks,
            keyframe: frame.keyframe,
            nals: Arc::from(runtime_nals),
        });
        next_frame_offset = frame_end;
        total_ticks = total_ticks
            .checked_add(u64::from(frame.duration_ticks))
            .ok_or_else(|| {
                MediaPackError::new(
                    "device_simulator.media.invalid_duration",
                    "total media duration overflows",
                )
            })?;
    }
    if next_frame_offset != media_file_size {
        return Err(MediaPackError::new(
            "device_simulator.media.index_coverage_mismatch",
            "frame index does not cover the entire media buffer",
        ));
    }

    validate_parameter_sets(manifest, &frames)?;
    let actual_bitrate_bps = (media_file_size as u128)
        .checked_mul(8)
        .and_then(|value| value.checked_mul(u128::from(manifest.clock_rate)))
        .and_then(|value| value.checked_div(u128::from(total_ticks)))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| {
            MediaPackError::new(
                "device_simulator.media.invalid_bitrate",
                "actual bitrate cannot be calculated safely",
            )
        })?;
    if !(MIN_RECOMMENDED_BITRATE_BPS..=MAX_RECOMMENDED_BITRATE_BPS).contains(&actual_bitrate_bps)
        || actual_bitrate_bps
            > manifest
                .recommended_bitrate_bps
                .saturating_mul(MAX_BITRATE_RATIO)
        || manifest.recommended_bitrate_bps > actual_bitrate_bps.saturating_mul(MAX_BITRATE_RATIO)
    {
        return Err(MediaPackError::new(
            "device_simulator.media.bitrate_mismatch",
            format!(
                "actual bitrate {actual_bitrate_bps} differs abnormally from recommended bitrate {}",
                manifest.recommended_bitrate_bps
            ),
        ));
    }

    Ok((frames, actual_bitrate_bps))
}

fn validate_parameter_sets(
    manifest: &MediaManifestV1,
    frames: &[SharedMediaFrame],
) -> Result<(), MediaPackError> {
    let mut kinds = HashSet::new();
    for reference in &manifest.parameter_sets {
        if !kinds.insert(reference.kind) {
            return Err(MediaPackError::new(
                "device_simulator.media.duplicate_parameter_set",
                format!("duplicate {:?} parameter set reference", reference.kind),
            ));
        }
        let nal = frames
            .get(reference.frame_index)
            .and_then(|frame| frame.nals.get(reference.nal_index))
            .ok_or_else(|| {
                MediaPackError::new(
                    "device_simulator.media.parameter_set_out_of_bounds",
                    format!(
                        "{:?} parameter set reference is out of bounds",
                        reference.kind
                    ),
                )
            })?;
        let expected_type = manifest
            .codec
            .parameter_set_nal_type(reference.kind)
            .ok_or_else(|| {
                MediaPackError::new(
                    "device_simulator.media.parameter_set_codec_mismatch",
                    format!("{:?} is not valid for {:?}", reference.kind, manifest.codec),
                )
            })?;
        if nal.nal_type != expected_type {
            return Err(MediaPackError::new(
                "device_simulator.media.parameter_set_type_mismatch",
                format!(
                    "{:?} must reference NAL type {expected_type}, not {}",
                    reference.kind, nal.nal_type
                ),
            ));
        }
    }
    for kind in manifest.codec.required_parameter_sets() {
        if !kinds.contains(kind) {
            return Err(MediaPackError::new(
                "device_simulator.media.parameter_set_missing",
                format!("required {:?} parameter set is missing", kind),
            ));
        }
    }
    Ok(())
}

fn resolve_pack_file(pack_dir: &Path, relative_path: &str) -> Result<PathBuf, MediaPackError> {
    resolve_pack_file_from(pack_dir, pack_dir, relative_path)
}

fn resolve_pack_file_from(
    pack_dir: &Path,
    base_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, MediaPackError> {
    validate_pack_path(relative_path).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.unsafe_path",
            format!("unsafe media pack path {relative_path:?}: {error}"),
        )
    })?;
    let pack_root = fs::canonicalize(pack_dir).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.pack_unavailable",
            format!("failed to resolve media pack directory: {error}"),
        )
    })?;
    let base_root = fs::canonicalize(base_dir).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.file_unavailable",
            format!("failed to resolve media base directory: {error}"),
        )
    })?;
    if !base_root.starts_with(&pack_root) {
        return Err(MediaPackError::new(
            "device_simulator.media.unsafe_path",
            "media base directory escapes the pack directory",
        ));
    }
    let candidate = base_root.join(relative_path);
    let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.file_unavailable",
            format!("failed to inspect {}: {error}", candidate.display()),
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(MediaPackError::new(
            "device_simulator.media.symlink_rejected",
            format!("symbolic links are not accepted: {}", candidate.display()),
        ));
    }
    let resolved = fs::canonicalize(&candidate).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.file_unavailable",
            format!("failed to resolve {}: {error}", candidate.display()),
        )
    })?;
    if !resolved.starts_with(&pack_root) {
        return Err(MediaPackError::new(
            "device_simulator.media.unsafe_path",
            "resolved media path escapes the pack directory",
        ));
    }
    Ok(resolved)
}

fn ensure_regular_bounded_file(
    path: &Path,
    maximum_bytes: u64,
    label: &str,
) -> Result<(), MediaPackError> {
    let metadata = fs::metadata(path).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.file_unavailable",
            format!("failed to inspect {}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum_bytes {
        return Err(MediaPackError::new(
            "device_simulator.media.file_size_rejected",
            format!(
                "{label} must be a non-empty regular file no larger than {maximum_bytes} bytes"
            ),
        ));
    }
    Ok(())
}

fn read_bounded_file(
    path: &Path,
    maximum_bytes: u64,
    label: &str,
) -> Result<Vec<u8>, MediaPackError> {
    let file = fs::File::open(path).map_err(|error| {
        MediaPackError::new(
            "device_simulator.media.file_read_failed",
            format!("failed to open {}: {error}", path.display()),
        )
    })?;
    let mut bytes = Vec::new();
    file.take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| {
            MediaPackError::new(
                "device_simulator.media.file_read_failed",
                format!("failed to read {}: {error}", path.display()),
            )
        })?;
    if bytes.is_empty() || bytes.len() as u64 > maximum_bytes {
        return Err(MediaPackError::new(
            "device_simulator.media.file_size_rejected",
            format!("{label} changed size or exceeds the {maximum_bytes}-byte limit"),
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    while !buffer.is_empty() {
        let read = file.read_at(buffer, offset)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "indexed media ended before the requested range",
            ));
        }
        offset = offset.saturating_add(read as u64);
        buffer = &mut buffer[read..];
    }
    Ok(())
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buffer.is_empty() {
        let read = file.seek_read(buffer, offset)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "indexed media ended before the requested range",
            ));
        }
        offset = offset.saturating_add(read as u64);
        buffer = &mut buffer[read..];
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;
    use crate::device_simulator::media::manifest::{
        Codec, EvidenceDifference, FrameIndex, MediaEvidence, NalIndex, ParameterSetRef,
    };
    use crate::device_simulator::rtsp::service::RtspStreamSource;

    struct Fixture {
        directory: TempDir,
        manifest: MediaManifestV1,
        bytes: Vec<u8>,
    }

    impl Fixture {
        fn new(codec: Codec) -> Self {
            let directory = tempfile::tempdir().expect("fixture directory");
            let (bytes, frames, parameter_sets, media_file) = fixture_stream(codec);
            let actual_bitrate = (bytes.len() as u64) * 8 * u64::from(VIDEO_CLOCK_RATE)
                / frames
                    .iter()
                    .map(|frame| u64::from(frame.duration_ticks))
                    .sum::<u64>();
            let manifest = MediaManifestV1 {
                schema_version: MEDIA_MANIFEST_SCHEMA_VERSION,
                id: "synthetic-loop".into(),
                codec,
                clock_rate: VIDEO_CLOCK_RATE,
                payload_type: 96,
                frame_rate_numerator: 25,
                frame_rate_denominator: 1,
                recommended_bitrate_bps: actual_bitrate,
                media_file: media_file.into(),
                media_file_size: bytes.len() as u64,
                media_file_sha256: format!("{:x}", Sha256::digest(&bytes)),
                frames,
                parameter_sets,
                evidence: MediaEvidence {
                    source_kind: EvidenceSourceKind::SyntheticFixture,
                    pcap_source_id: None,
                    sdp_source_id: None,
                    compatibility: MediaCompatibility::Unverified,
                    verified_platforms: vec![],
                    differences: vec![],
                },
            };
            let fixture = Self {
                directory,
                manifest,
                bytes,
            };
            fixture.write();
            fixture
        }

        fn write(&self) {
            fs::write(
                self.directory.path().join(&self.manifest.media_file),
                &self.bytes,
            )
            .expect("write media");
            fs::write(
                self.directory.path().join("media.json"),
                serde_json::to_vec_pretty(&self.manifest).expect("serialize manifest"),
            )
            .expect("write manifest");
        }

        fn load_synthetic(&self) -> Result<Arc<SharedMediaPack>, MediaPackError> {
            MediaPackCache::new().load_synthetic_fixture(self.directory.path(), "media.json")
        }
    }

    fn fixture_stream(
        codec: Codec,
    ) -> (Vec<u8>, Vec<FrameIndex>, Vec<ParameterSetRef>, &'static str) {
        let (frame_nals, parameter_sets, media_file): (
            Vec<Vec<Vec<u8>>>,
            Vec<(ParameterSetKind, usize, usize)>,
            &str,
        ) = match codec {
            Codec::H264 => (
                vec![
                    vec![vec![0x67, 0x42], vec![0x68, 0xce], nal_with_size(0x65, 100)],
                    vec![nal_with_size(0x41, 100)],
                ],
                vec![(ParameterSetKind::Sps, 0, 0), (ParameterSetKind::Pps, 0, 1)],
                "video.h264",
            ),
            Codec::H265 => (
                vec![vec![
                    vec![0x40, 0x01],
                    vec![0x42, 0x01],
                    vec![0x44, 0x01],
                    h265_nal_with_size(19, 100),
                ]],
                vec![
                    (ParameterSetKind::Vps, 0, 0),
                    (ParameterSetKind::Sps, 0, 1),
                    (ParameterSetKind::Pps, 0, 2),
                ],
                "video.h265",
            ),
        };

        let mut bytes = Vec::new();
        let mut frames = Vec::new();
        for nals in frame_nals {
            let frame_offset = bytes.len() as u64;
            let mut indexes = Vec::new();
            let mut keyframe = false;
            for nal in nals {
                let offset = bytes.len() as u64;
                let nal_type = codec.nal_type(&nal).expect("valid fixture NAL");
                keyframe |= codec.is_keyframe_nal(nal_type);
                indexes.push(NalIndex {
                    offset,
                    length: nal.len() as u64,
                    nal_type,
                });
                bytes.extend_from_slice(&nal);
            }
            frames.push(FrameIndex {
                offset: frame_offset,
                length: bytes.len() as u64 - frame_offset,
                duration_ticks: 3_600,
                keyframe,
                nals: indexes,
            });
        }
        let parameter_sets = parameter_sets
            .into_iter()
            .map(|(kind, frame_index, nal_index)| ParameterSetRef {
                kind,
                frame_index,
                nal_index,
            })
            .collect();
        (bytes, frames, parameter_sets, media_file)
    }

    fn nal_with_size(header: u8, size: usize) -> Vec<u8> {
        let mut nal = vec![0x55; size];
        nal[0] = header;
        nal
    }

    fn h265_nal_with_size(nal_type: u8, size: usize) -> Vec<u8> {
        let mut nal = vec![0x55; size];
        nal[0] = nal_type << 1;
        nal[1] = 0x01;
        nal
    }

    #[test]
    fn synthetic_fixture_loads_once_into_shared_file_backed_index() {
        let fixture = Fixture::new(Codec::H264);
        let cache = MediaPackCache::new();
        let first = cache
            .load_synthetic_fixture(fixture.directory.path(), "media.json")
            .expect("load fixture");
        let second = cache
            .load_synthetic_fixture(fixture.directory.path(), "media.json")
            .expect("load cached fixture");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.frames().len(), 2);
        assert_eq!(
            first.parameter_set(ParameterSetKind::Sps),
            Some(vec![0x67, 0x42])
        );
        assert_eq!(
            first.actual_bitrate_bps(),
            first.manifest().recommended_bitrate_bps
        );

        let source = RtspStreamSource::from_media(
            "synthetic-main",
            b"v=0\r\nm=video 0 RTP/AVP 96\r\na=rtpmap:96 H264/90000\r\n".as_slice(),
            Arc::clone(&first),
            8,
            1_200,
        )
        .expect("adapt shared media to RTSP");
        assert_eq!(source.codec, Codec::H264);
        assert!(source.scheduler.owns_indexed_producer());
        assert_eq!(first.read_frame_nals(0).unwrap().1[0], &[0x67, 0x42]);
    }

    #[test]
    fn resolves_media_file_relative_to_nested_manifest_directory() {
        let fixture = Fixture::new(Codec::H264);
        let nested = fixture.directory.path().join("media/main");
        fs::create_dir_all(&nested).expect("create nested media directory");
        fs::write(nested.join(&fixture.manifest.media_file), &fixture.bytes)
            .expect("write nested media");
        fs::write(
            nested.join("media.json"),
            serde_json::to_vec_pretty(&fixture.manifest).expect("serialize nested manifest"),
        )
        .expect("write nested manifest");
        fs::remove_file(fixture.directory.path().join(&fixture.manifest.media_file))
            .expect("remove root media copy");

        let loaded = MediaPackCache::new()
            .load_synthetic_fixture(fixture.directory.path(), "media/main/media.json")
            .expect("load nested media fixture");
        assert_eq!(loaded.media_file_size(), fixture.bytes.len() as u64);
        assert_eq!(loaded.read_frame_nals(0).unwrap().1[0], &[0x67, 0x42]);
    }

    #[test]
    fn runtime_rejects_synthetic_unverified_compatibility() {
        let fixture = Fixture::new(Codec::H264);
        let error = load_media_pack(fixture.directory.path(), "media.json")
            .expect_err("runtime must reject synthetic fixture");
        assert_eq!(
            error.code,
            "device_simulator.media.compatibility_unverified"
        );
    }

    #[test]
    fn h265_requires_vps_sps_pps_and_accepts_irap_keyframe() {
        let fixture = Fixture::new(Codec::H265);
        let pack = fixture.load_synthetic().expect("load H265 fixture");
        assert!(pack.frames()[0].keyframe);
        assert_eq!(
            pack.parameter_set(ParameterSetKind::Vps),
            Some(vec![0x40, 0x01])
        );
        assert!(pack.parameter_set(ParameterSetKind::Sps).is_some());
        assert!(pack.parameter_set(ParameterSetKind::Pps).is_some());
    }

    #[test]
    fn rejects_missing_parameter_sets_and_index_or_keyframe_drift() {
        let mut missing = Fixture::new(Codec::H264);
        missing.manifest.parameter_sets.pop();
        missing.write();
        assert_eq!(
            missing.load_synthetic().expect_err("missing PPS").code,
            "device_simulator.media.parameter_set_missing"
        );

        let mut out_of_bounds = Fixture::new(Codec::H264);
        out_of_bounds.manifest.frames[1].offset += 1;
        out_of_bounds.write();
        assert_eq!(
            out_of_bounds
                .load_synthetic()
                .expect_err("bad frame offset")
                .code,
            "device_simulator.media.invalid_frame_index"
        );

        let mut false_keyframe = Fixture::new(Codec::H264);
        false_keyframe.manifest.frames[0].keyframe = false;
        false_keyframe.write();
        assert_eq!(
            false_keyframe
                .load_synthetic()
                .expect_err("bad keyframe flag")
                .code,
            "device_simulator.media.frame_keyframe_mismatch"
        );

        let mut bad_nal_bounds = Fixture::new(Codec::H264);
        bad_nal_bounds.manifest.frames[0].nals[2].length += 1;
        bad_nal_bounds.write();
        assert_eq!(
            bad_nal_bounds
                .load_synthetic()
                .expect_err("NAL escapes frame")
                .code,
            "device_simulator.media.index_out_of_bounds"
        );

        let mut bad_nal_type = Fixture::new(Codec::H264);
        bad_nal_type.manifest.frames[0].nals[2].nal_type = 1;
        bad_nal_type.write();
        assert_eq!(
            bad_nal_type
                .load_synthetic()
                .expect_err("NAL type drift")
                .code,
            "device_simulator.media.nal_type_mismatch"
        );
    }

    #[test]
    fn validates_codec_clock_payload_size_and_bitrate_but_allows_local_hash_drift() {
        let mut invalid_payload = Fixture::new(Codec::H264);
        invalid_payload.manifest.payload_type = 95;
        invalid_payload.write();
        assert_eq!(
            invalid_payload.load_synthetic().expect_err("payload").code,
            "device_simulator.media.invalid_payload_type"
        );

        let mut invalid_clock = Fixture::new(Codec::H264);
        invalid_clock.manifest.clock_rate = 8_000;
        invalid_clock.write();
        assert_eq!(
            invalid_clock.load_synthetic().expect_err("clock").code,
            "device_simulator.media.invalid_clock_rate"
        );

        let mut invalid_codec = Fixture::new(Codec::H264);
        invalid_codec.manifest.codec = Codec::H265;
        invalid_codec.write();
        assert_eq!(
            invalid_codec.load_synthetic().expect_err("codec").code,
            "device_simulator.media.codec_file_mismatch"
        );

        let mut invalid_hash = Fixture::new(Codec::H264);
        invalid_hash.manifest.media_file_sha256 = "0".repeat(64);
        invalid_hash.write();
        invalid_hash
            .load_synthetic()
            .expect("loose local media does not require a matching release hash");

        let mut invalid_size = Fixture::new(Codec::H264);
        invalid_size.manifest.media_file_size = MAX_MEDIA_BYTES + 1;
        invalid_size.write();
        assert_eq!(
            invalid_size.load_synthetic().expect_err("size").code,
            "device_simulator.media.invalid_size"
        );

        let mut invalid_bitrate = Fixture::new(Codec::H264);
        invalid_bitrate.manifest.recommended_bitrate_bps = MIN_RECOMMENDED_BITRATE_BPS;
        invalid_bitrate.write();
        assert_eq!(
            invalid_bitrate.load_synthetic().expect_err("bitrate").code,
            "device_simulator.media.bitrate_mismatch"
        );
    }

    #[test]
    fn rejects_unresolved_pcap_sdp_difference() {
        let mut fixture = Fixture::new(Codec::H264);
        fixture.manifest.evidence = MediaEvidence {
            source_kind: EvidenceSourceKind::AuthorizedPcap,
            pcap_source_id: Some("capture-sha256:abc".into()),
            sdp_source_id: Some("camera-sdp-v1".into()),
            compatibility: MediaCompatibility::PlatformVerified,
            verified_platforms: vec!["target-platform-v1".into()],
            differences: vec![EvidenceDifference {
                field: "payload_type".into(),
                pcap_value: "96".into(),
                sdp_value: "98".into(),
                selected_value: None,
                resolution: EvidenceResolution::Unresolved,
            }],
        };
        fixture.write();

        let error = load_media_pack(fixture.directory.path(), "media.json")
            .expect_err("unresolved evidence difference");
        assert_eq!(
            error.code,
            "device_simulator.media.evidence_difference_unresolved"
        );
    }

    #[test]
    fn rejects_unknown_manifest_fields_and_unsafe_paths() {
        let fixture = Fixture::new(Codec::H264);
        let mut value = serde_json::to_value(&fixture.manifest).expect("manifest value");
        match &mut value {
            Value::Object(object) => {
                object.insert("unexpected".into(), Value::Bool(true));
            }
            _ => panic!("manifest must be an object"),
        }
        fs::write(
            fixture.directory.path().join("media.json"),
            serde_json::to_vec(&value).expect("serialize value"),
        )
        .expect("write manifest");
        assert_eq!(
            fixture.load_synthetic().expect_err("unknown field").code,
            "device_simulator.media.manifest_invalid"
        );

        assert_eq!(
            MediaPackCache::new()
                .load_synthetic_fixture(fixture.directory.path(), "../media.json")
                .expect_err("unsafe path")
                .code,
            "device_simulator.media.unsafe_path"
        );
    }
}
