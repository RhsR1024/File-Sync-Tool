//! Strongly typed alarm request construction infrastructure.
//!
//! The built-in first-release descriptors are deliberately marked as
//! synthetic and platform-unverified. They exercise the engine without
//! claiming compatibility before legacy-derived or captured golden fixtures
//! pass the evidence gate.

pub mod scheduler;

use crate::device_simulator::assets::catalog::PackManifest;
use crate::device_simulator::assets::validation::validate_pack_path;
use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

pub const MAX_ALARM_TEMPLATE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RENDERED_ALARM_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_ALARM_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ALARM_IMAGE_CACHE_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_ALARM_IMAGES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmError {
    pub code: &'static str,
    pub message: String,
}

impl AlarmError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for AlarmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AlarmError {}

pub type AlarmResult<T> = Result<T, AlarmError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlarmHandlerId {
    CustomV1,
    SmartV1,
    StructuredV1,
    FaceAccessV1,
    NvrCommonV1,
    NvrVehicleV1,
}

impl AlarmHandlerId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CustomV1 => "alarm.custom.v1",
            Self::SmartV1 => "alarm.smart.v1",
            Self::StructuredV1 => "alarm.structured.v1",
            Self::FaceAccessV1 => "alarm.face_access.v1",
            Self::NvrCommonV1 => "alarm.nvr_common.v1",
            Self::NvrVehicleV1 => "alarm.nvr_vehicle.v1",
        }
    }

    pub const fn profile_id(self) -> FirstReleaseProfileId {
        match self {
            Self::CustomV1 => FirstReleaseProfileId::IpcCustom,
            Self::SmartV1 => FirstReleaseProfileId::IpcSmart,
            Self::StructuredV1 => FirstReleaseProfileId::IpcStructured,
            Self::FaceAccessV1 => FirstReleaseProfileId::IpcFaceAccess,
            Self::NvrCommonV1 => FirstReleaseProfileId::NvrCommon,
            Self::NvrVehicleV1 => FirstReleaseProfileId::NvrVehicle,
        }
    }
}

impl FromStr for AlarmHandlerId {
    type Err = AlarmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "alarm.custom.v1" => Ok(Self::CustomV1),
            "alarm.smart.v1" => Ok(Self::SmartV1),
            "alarm.structured.v1" => Ok(Self::StructuredV1),
            "alarm.face_access.v1" => Ok(Self::FaceAccessV1),
            "alarm.nvr_common.v1" => Ok(Self::NvrCommonV1),
            "alarm.nvr_vehicle.v1" => Ok(Self::NvrVehicleV1),
            _ => Err(AlarmError::new(
                "device_simulator.alarm.handler_unknown",
                format!("unknown compiled alarm handler '{value}'"),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AlarmTypeId(String);

impl AlarmTypeId {
    pub fn new(value: impl Into<String>) -> AlarmResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
        {
            return Err(AlarmError::new(
                "device_simulator.alarm.type_id_invalid",
                "alarm type ID must be bounded lowercase ASCII",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicField {
    DeviceId,
    DeviceIp,
    ChannelId,
    Timestamp,
    CaptureTime,
    CaptureTimeText,
    Reference,
    SubscriptionId,
    EventId,
    RelatedId,
    PersonId,
    AlarmState,
    ImageBase64,
    ImageBase642,
    ImageBase643,
    ImageBase644,
    ImageBase645,
    ImageSize,
    ImageSize2,
    ImageSize3,
    ImageSize4,
    ImageSize5,
    ImageIndex,
    ImageIndex2,
    ImageIndex3,
    ImageIndex4,
    ImageIndex5,
}

impl DynamicField {
    pub const fn token(self) -> &'static str {
        match self {
            Self::DeviceId => "device_id",
            Self::DeviceIp => "device_ip",
            Self::ChannelId => "channel_id",
            Self::Timestamp => "timestamp",
            Self::CaptureTime => "capture_time",
            Self::CaptureTimeText => "capture_time_text",
            Self::Reference => "reference",
            Self::SubscriptionId => "subscription_id",
            Self::EventId => "event_id",
            Self::RelatedId => "related_id",
            Self::PersonId => "person_id",
            Self::AlarmState => "alarm_state",
            Self::ImageBase64 => "image_base64",
            Self::ImageBase642 => "image_base64_2",
            Self::ImageBase643 => "image_base64_3",
            Self::ImageBase644 => "image_base64_4",
            Self::ImageBase645 => "image_base64_5",
            Self::ImageSize => "image_size",
            Self::ImageSize2 => "image_size_2",
            Self::ImageSize3 => "image_size_3",
            Self::ImageSize4 => "image_size_4",
            Self::ImageSize5 => "image_size_5",
            Self::ImageIndex => "image_index",
            Self::ImageIndex2 => "image_index_2",
            Self::ImageIndex3 => "image_index_3",
            Self::ImageIndex4 => "image_index_4",
            Self::ImageIndex5 => "image_index_5",
        }
    }

    fn parse(token: &str) -> AlarmResult<Self> {
        match token {
            "device_id" => Ok(Self::DeviceId),
            "device_ip" => Ok(Self::DeviceIp),
            "channel_id" => Ok(Self::ChannelId),
            "timestamp" => Ok(Self::Timestamp),
            "capture_time" => Ok(Self::CaptureTime),
            "capture_time_text" => Ok(Self::CaptureTimeText),
            "reference" => Ok(Self::Reference),
            "subscription_id" => Ok(Self::SubscriptionId),
            "event_id" => Ok(Self::EventId),
            "related_id" => Ok(Self::RelatedId),
            "person_id" => Ok(Self::PersonId),
            "alarm_state" => Ok(Self::AlarmState),
            "image_base64" => Ok(Self::ImageBase64),
            "image_base64_2" => Ok(Self::ImageBase642),
            "image_base64_3" => Ok(Self::ImageBase643),
            "image_base64_4" => Ok(Self::ImageBase644),
            "image_base64_5" => Ok(Self::ImageBase645),
            "image_size" => Ok(Self::ImageSize),
            "image_size_2" => Ok(Self::ImageSize2),
            "image_size_3" => Ok(Self::ImageSize3),
            "image_size_4" => Ok(Self::ImageSize4),
            "image_size_5" => Ok(Self::ImageSize5),
            "image_index" => Ok(Self::ImageIndex),
            "image_index_2" => Ok(Self::ImageIndex2),
            "image_index_3" => Ok(Self::ImageIndex3),
            "image_index_4" => Ok(Self::ImageIndex4),
            "image_index_5" => Ok(Self::ImageIndex5),
            _ => Err(AlarmError::new(
                "device_simulator.alarm.template_field_unknown",
                format!("unknown alarm template field '{token}'"),
            )),
        }
    }
}

const IMAGE_BASE64_FIELDS: [DynamicField; 5] = [
    DynamicField::ImageBase64,
    DynamicField::ImageBase642,
    DynamicField::ImageBase643,
    DynamicField::ImageBase644,
    DynamicField::ImageBase645,
];
const IMAGE_SIZE_FIELDS: [DynamicField; 5] = [
    DynamicField::ImageSize,
    DynamicField::ImageSize2,
    DynamicField::ImageSize3,
    DynamicField::ImageSize4,
    DynamicField::ImageSize5,
];
const IMAGE_INDEX_FIELDS: [DynamicField; 5] = [
    DynamicField::ImageIndex,
    DynamicField::ImageIndex2,
    DynamicField::ImageIndex3,
    DynamicField::ImageIndex4,
    DynamicField::ImageIndex5,
];

pub(crate) fn embedded_image_count(template: &CompiledTemplate) -> usize {
    IMAGE_BASE64_FIELDS
        .iter()
        .filter(|field| template.fields().contains(field))
        .count()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledTemplate {
    source: Arc<str>,
    fields: BTreeSet<DynamicField>,
}

impl CompiledTemplate {
    pub fn compile(bytes: &[u8]) -> AlarmResult<Self> {
        if bytes.is_empty() || bytes.len() > MAX_ALARM_TEMPLATE_BYTES {
            return Err(AlarmError::new(
                "device_simulator.alarm.template_size_invalid",
                "alarm template must be non-empty and no larger than 2 MiB",
            ));
        }
        let source = std::str::from_utf8(bytes).map_err(|error| {
            AlarmError::new(
                "device_simulator.alarm.template_utf8_invalid",
                format!("alarm template is not UTF-8: {error}"),
            )
        })?;
        let mut fields = BTreeSet::new();
        let mut rest = source;
        while let Some(start) = rest.find("{{") {
            rest = &rest[start + 2..];
            let end = rest.find("}}").ok_or_else(|| {
                AlarmError::new(
                    "device_simulator.alarm.template_syntax_invalid",
                    "alarm template contains an unclosed field",
                )
            })?;
            fields.insert(DynamicField::parse(&rest[..end])?);
            rest = &rest[end + 2..];
        }
        Ok(Self {
            source: Arc::from(source),
            fields,
        })
    }

    pub fn fields(&self) -> &BTreeSet<DynamicField> {
        &self.fields
    }

    pub fn render(&self, values: &BTreeMap<DynamicField, String>) -> AlarmResult<Vec<u8>> {
        for field in &self.fields {
            if !values.contains_key(field) {
                return Err(AlarmError::new(
                    "device_simulator.alarm.template_field_missing",
                    format!("missing dynamic field '{}'", field.token()),
                ));
            }
        }
        let mut rendered = self.source.to_string();
        for field in &self.fields {
            let marker = format!("{{{{{}}}}}", field.token());
            rendered = rendered.replace(&marker, &values[field]);
            if rendered.len() > MAX_RENDERED_ALARM_BYTES {
                return Err(AlarmError::new(
                    "device_simulator.alarm.rendered_size_exceeded",
                    "rendered alarm body exceeds 32 MiB",
                ));
            }
        }
        Ok(rendered.into_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ImageAssetRef {
    Pack {
        pack_id: String,
        version: String,
        path: String,
    },
    UserAsset {
        image_id: String,
        extension: ImageExtension,
        sha256: String,
        size: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageExtension {
    Jpg,
    Jpeg,
    Png,
}

impl ImageExtension {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Jpeg => "jpeg",
            Self::Png => "png",
        }
    }

    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Jpg | Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackIdentity {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct CachedImage {
    pub bytes: Arc<[u8]>,
    pub content_type: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct ImageCache {
    images: BTreeMap<ImageAssetRef, CachedImage>,
    total_bytes: u64,
}

impl ImageCache {
    /// Reads and verifies every referenced image once at session/job start.
    /// Request construction only clones `Arc`s and never touches the disk.
    pub fn load_at_start(
        references: impl IntoIterator<Item = ImageAssetRef>,
        pack_root: &Path,
        user_asset_root: &Path,
        manifests: &BTreeMap<PackIdentity, PackManifest>,
    ) -> AlarmResult<Self> {
        let references = references.into_iter().collect::<BTreeSet<_>>();
        if references.len() > MAX_ALARM_IMAGES {
            return Err(AlarmError::new(
                "device_simulator.alarm.image_count_exceeded",
                "alarm image cache contains more than 256 images",
            ));
        }
        let mut cache = Self::default();
        for reference in references {
            let (path, expected_sha256, expected_size, content_type, root) = match &reference {
                ImageAssetRef::Pack {
                    pack_id,
                    version,
                    path,
                } => {
                    validate_pack_identity(pack_id, version)?;
                    validate_pack_path(path).map_err(|error| {
                        AlarmError::new("device_simulator.alarm.image_path_invalid", error.message)
                    })?;
                    let identity = PackIdentity {
                        id: pack_id.clone(),
                        version: version.clone(),
                    };
                    let manifest = manifests.get(&identity).ok_or_else(|| {
                        AlarmError::new(
                            "device_simulator.alarm.image_manifest_missing",
                            format!("manifest for {pack_id}@{version} is not active"),
                        )
                    })?;
                    if manifest.id != *pack_id || manifest.version.to_string() != *version {
                        return Err(AlarmError::new(
                            "device_simulator.alarm.image_manifest_mismatch",
                            "active image manifest identity does not match its index",
                        ));
                    }
                    let declared = manifest
                        .files
                        .iter()
                        .find(|file| file.path == *path)
                        .ok_or_else(|| {
                            AlarmError::new(
                                "device_simulator.alarm.image_not_declared",
                                format!("image '{path}' is not declared by its pack manifest"),
                            )
                        })?;
                    let extension = image_extension(path)?;
                    (
                        pack_root.join(pack_id).join(version).join(path),
                        declared.sha256.clone(),
                        declared.size,
                        extension.content_type(),
                        pack_root.to_path_buf(),
                    )
                }
                ImageAssetRef::UserAsset {
                    image_id,
                    extension,
                    sha256,
                    size,
                } => {
                    validate_user_asset_id(image_id)?;
                    validate_sha256(sha256)?;
                    (
                        user_asset_root.join(format!("{image_id}.{}", extension.as_str())),
                        sha256.clone(),
                        *size,
                        extension.content_type(),
                        user_asset_root.to_path_buf(),
                    )
                }
            };
            if expected_size == 0 || expected_size > MAX_ALARM_IMAGE_BYTES {
                return Err(AlarmError::new(
                    "device_simulator.alarm.image_size_invalid",
                    "alarm images must be non-empty and no larger than 16 MiB",
                ));
            }
            cache.total_bytes = cache
                .total_bytes
                .checked_add(expected_size)
                .filter(|total| *total <= MAX_ALARM_IMAGE_CACHE_BYTES)
                .ok_or_else(|| {
                    AlarmError::new(
                        "device_simulator.alarm.image_cache_size_exceeded",
                        "alarm image cache exceeds 128 MiB",
                    )
                })?;
            let bytes = read_verified_file(&root, &path, expected_size, &expected_sha256)?;
            cache.images.insert(
                reference,
                CachedImage {
                    bytes: Arc::from(bytes),
                    content_type,
                },
            );
        }
        Ok(cache)
    }

    pub fn get(&self, reference: &ImageAssetRef) -> AlarmResult<&CachedImage> {
        self.images.get(reference).ok_or_else(|| {
            AlarmError::new(
                "device_simulator.alarm.image_not_cached",
                "alarm image was not validated at task start",
            )
        })
    }

    pub fn get_by_token(&self, token: &str) -> Option<&CachedImage> {
        self.images.iter().find_map(|(reference, image)| {
            (image_reference_token(reference) == token).then_some(image)
        })
    }

    pub fn merged(&self, additional: Self) -> AlarmResult<Self> {
        let mut merged = self.clone();
        for (reference, image) in additional.images {
            if merged.images.contains_key(&reference) {
                continue;
            }
            merged.total_bytes = merged
                .total_bytes
                .checked_add(image.bytes.len() as u64)
                .filter(|total| *total <= MAX_ALARM_IMAGE_CACHE_BYTES)
                .ok_or_else(|| {
                    AlarmError::new(
                        "device_simulator.alarm.image_cache_size_exceeded",
                        "alarm image cache exceeds 128 MiB",
                    )
                })?;
            if merged.images.len() >= MAX_ALARM_IMAGES {
                return Err(AlarmError::new(
                    "device_simulator.alarm.image_count_exceeded",
                    "alarm image cache contains more than 256 images",
                ));
            }
            merged.images.insert(reference, image);
        }
        Ok(merged)
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

pub type SharedImageCache = Arc<RwLock<ImageCache>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceBinding {
    DeviceIp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseSuccessRule {
    /// Response semantics have not passed a platform evidence gate.
    Unverified,
    StatusRange {
        minimum: u16,
        maximum: u16,
    },
}

impl ResponseSuccessRule {
    pub fn evaluate(&self, status: u16) -> Option<bool> {
        match self {
            Self::Unverified => None,
            Self::StatusRange { minimum, maximum } => Some((*minimum..=*maximum).contains(&status)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyEncoding {
    Raw {
        content_type: String,
    },
    Multipart {
        metadata_name: String,
        metadata_content_type: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePolicy {
    Forbidden,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageAttachmentDefinition {
    pub reference: ImageAssetRef,
    /// Optional legacy picture-URL target when it intentionally differs from
    /// the image embedded in the JSON body.
    pub url_reference: Option<ImageAssetRef>,
    pub field_name: String,
    pub file_name: String,
    pub image_index: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportDefinition {
    pub method: HttpMethod,
    pub path: String,
    pub source_binding: SourceBinding,
    pub body_encoding: BodyEncoding,
    pub success_rule: ResponseSuccessRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRequestDefinition {
    pub template: CompiledTemplate,
    pub image_policy: ImagePolicy,
    pub images: Vec<ImageAttachmentDefinition>,
    pub transport: TransportDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryTrigger {
    RequestedDelay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryDefinition {
    None,
    RenderWith {
        template: CompiledTemplate,
        transport: TransportDefinition,
        trigger: RecoveryTrigger,
        include_images: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureProvenance {
    LegacyOrCaptureDerived,
    SyntheticUnverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformVerification {
    SourceConfirmedPlatformUnverified,
    PlatformVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformEvidence {
    pub platform: TargetPlatform,
    pub verification: PlatformVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerEvidence {
    pub legacy_sources: Vec<String>,
    pub template_source: String,
    pub fixture_provenance: FixtureProvenance,
    pub platforms: Vec<PlatformEvidence>,
    #[serde(default)]
    pub intentional_changes: Vec<String>,
}

impl HandlerEvidence {
    pub fn is_platform_verified(&self, platform: TargetPlatform) -> bool {
        self.platforms.iter().any(|evidence| {
            evidence.platform == platform
                && evidence.verification == PlatformVerification::PlatformVerified
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmHandlerDefinition {
    pub handler_id: AlarmHandlerId,
    pub alarm_type_id: AlarmTypeId,
    pub profile_id: FirstReleaseProfileId,
    pub template: CompiledTemplate,
    pub image_policy: ImagePolicy,
    pub images: Vec<ImageAttachmentDefinition>,
    pub transport: TransportDefinition,
    /// Additional HTTP requests emitted after the primary request for one
    /// logical legacy alarm. Their order is part of the migrated contract.
    pub follow_up_requests: Vec<AlarmRequestDefinition>,
    pub recovery: RecoveryDefinition,
    pub evidence: HandlerEvidence,
}

#[derive(Debug, Clone, Default)]
pub struct AlarmHandlerRegistry {
    handlers: BTreeMap<(AlarmHandlerId, AlarmTypeId), Arc<AlarmHandlerDefinition>>,
}

impl AlarmHandlerRegistry {
    pub fn register(&mut self, definition: AlarmHandlerDefinition) -> AlarmResult<()> {
        validate_definition(&definition)?;
        let key = (definition.handler_id, definition.alarm_type_id.clone());
        if self.handlers.contains_key(&key) {
            return Err(AlarmError::new(
                "device_simulator.alarm.handler_duplicate",
                "duplicate alarm handler and alarm type registration",
            ));
        }
        self.handlers.insert(key, Arc::new(definition));
        Ok(())
    }

    pub fn resolve(
        &self,
        profile_id: FirstReleaseProfileId,
        handler_id: &str,
        alarm_type_id: &str,
    ) -> AlarmResult<Arc<AlarmHandlerDefinition>> {
        let handler_id = AlarmHandlerId::from_str(handler_id)?;
        if handler_id.profile_id() != profile_id {
            return Err(AlarmError::new(
                "device_simulator.alarm.handler_profile_mismatch",
                "alarm handler is not compiled for the requested profile",
            ));
        }
        let alarm_type_id = AlarmTypeId::new(alarm_type_id)?;
        self.handlers
            .get(&(handler_id, alarm_type_id))
            .cloned()
            .ok_or_else(|| {
                AlarmError::new(
                    "device_simulator.alarm.type_unknown",
                    "alarm type is not registered for this handler",
                )
            })
    }

    pub fn image_references(&self) -> BTreeSet<ImageAssetRef> {
        self.handlers
            .values()
            .flat_map(|definition| {
                definition
                    .images
                    .iter()
                    .chain(
                        definition
                            .follow_up_requests
                            .iter()
                            .flat_map(|request| request.images.iter()),
                    )
                    .flat_map(|image| {
                        std::iter::once(image.reference.clone())
                            .chain(image.url_reference.iter().cloned())
                    })
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    pub fn definitions(&self) -> impl Iterator<Item = &AlarmHandlerDefinition> {
        self.handlers.values().map(Arc::as_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyAlarmValues {
    seed: u64,
}

impl LegacyAlarmValues {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    fn bounded(&self, salt: u64, minimum: u64, maximum: u64) -> u64 {
        debug_assert!(minimum <= maximum);
        let mut value = self.seed ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        minimum + value % (maximum - minimum + 1)
    }

    fn primary_subscription_id(&self) -> u16 {
        self.bounded(1, 1, 1_000) as u16
    }

    fn follow_up_subscription_id(&self) -> u16 {
        self.bounded(2, 1, 1_000) as u16
    }

    fn recovery_subscription_id(&self) -> u16 {
        self.bounded(3, 1, 1_000) as u16
    }

    pub fn related_id(&self) -> String {
        format!("16ID{}", self.bounded(4, 10_000_000_000, 99_999_999_999))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AlarmBuildContext {
    pub source_ip: Option<Ipv4Addr>,
    pub fields: BTreeMap<DynamicField, String>,
    pub multipart_boundary: Option<String>,
    pub legacy_values: Option<LegacyAlarmValues>,
}

#[derive(Debug, Clone)]
pub struct HttpAlarmRequest {
    pub method: HttpMethod,
    pub path: String,
    pub source_ip: Ipv4Addr,
    pub headers: BTreeMap<String, String>,
    pub body: Arc<[u8]>,
    pub success_rule: ResponseSuccessRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlarmRequestRole {
    Primary,
    FollowUp,
    Recovery,
}

pub fn build_alarm_request(
    definition: &AlarmHandlerDefinition,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<HttpAlarmRequest> {
    build_request_with_template(
        definition,
        &definition.template,
        AlarmRequestRole::Primary,
        context,
        image_cache,
    )
}

pub fn build_alarm_requests(
    definition: &AlarmHandlerDefinition,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<Vec<HttpAlarmRequest>> {
    let mut requests = Vec::with_capacity(1 + definition.follow_up_requests.len());
    requests.push(build_alarm_request(definition, context, image_cache)?);
    for follow_up in &definition.follow_up_requests {
        requests.push(build_request_from_parts(
            definition,
            &follow_up.template,
            follow_up.image_policy,
            &follow_up.images,
            &follow_up.transport,
            AlarmRequestRole::FollowUp,
            context,
            image_cache,
        )?);
    }
    Ok(requests)
}

pub fn build_recovery_request(
    definition: &AlarmHandlerDefinition,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<Option<HttpAlarmRequest>> {
    match &definition.recovery {
        RecoveryDefinition::None => Ok(None),
        RecoveryDefinition::RenderWith {
            template,
            transport,
            include_images,
            ..
        } => {
            let mut recovery_definition = definition.clone();
            recovery_definition.template = template.clone();
            recovery_definition.transport = transport.clone();
            recovery_definition.follow_up_requests.clear();
            if !*include_images {
                recovery_definition.images.clear();
                recovery_definition.image_policy = ImagePolicy::Forbidden;
            }
            build_request_with_template(
                &recovery_definition,
                template,
                AlarmRequestRole::Recovery,
                context,
                image_cache,
            )
            .map(Some)
        }
    }
}

fn build_request_with_template(
    definition: &AlarmHandlerDefinition,
    template: &CompiledTemplate,
    role: AlarmRequestRole,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<HttpAlarmRequest> {
    validate_definition(definition)?;
    build_request_from_parts(
        definition,
        template,
        definition.image_policy,
        &definition.images,
        &definition.transport,
        role,
        context,
        image_cache,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_request_from_parts(
    definition: &AlarmHandlerDefinition,
    template: &CompiledTemplate,
    image_policy: ImagePolicy,
    images: &[ImageAttachmentDefinition],
    transport: &TransportDefinition,
    role: AlarmRequestRole,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<HttpAlarmRequest> {
    validate_request_definition(
        definition.profile_id,
        template,
        image_policy,
        images,
        transport,
    )?;
    let source_ip = context.source_ip.ok_or_else(|| {
        AlarmError::new(
            "device_simulator.alarm.source_ip_missing",
            "alarm request must bind to the simulated device IP",
        )
    })?;
    let mut fields = context.fields.clone();
    for (index, attachment) in images.iter().enumerate() {
        let image = image_cache.get(&attachment.reference)?;
        if let Some(field) = IMAGE_BASE64_FIELDS.get(index) {
            if template.fields().contains(field) {
                fields.insert(*field, BASE64_STANDARD.encode(&image.bytes));
            }
        }
        if let Some(field) = IMAGE_SIZE_FIELDS.get(index) {
            if template.fields().contains(field) {
                // The legacy Python senders base64-encode embedded images first,
                // then put the encoded byte length in Size and in the picture URL.
                fields.insert(*field, base64_encoded_len(image.bytes.len()).to_string());
            }
        }
        if let Some(field) = IMAGE_INDEX_FIELDS.get(index) {
            if template.fields().contains(field) {
                let url_reference = attachment
                    .url_reference
                    .as_ref()
                    .unwrap_or(&attachment.reference);
                image_cache.get(url_reference)?;
                fields.insert(*field, image_reference_token(url_reference));
            }
        }
    }
    let metadata =
        apply_legacy_runtime_values(definition, role, context, template.render(&fields)?)?;
    let (body, content_type) = match &transport.body_encoding {
        BodyEncoding::Raw { content_type } => (metadata, content_type.clone()),
        BodyEncoding::Multipart {
            metadata_name,
            metadata_content_type,
        } => {
            let boundary = context.multipart_boundary.as_deref().ok_or_else(|| {
                AlarmError::new(
                    "device_simulator.alarm.multipart_boundary_missing",
                    "multipart alarm requires an injected boundary",
                )
            })?;
            validate_multipart_token(boundary, "boundary")?;
            let body = build_multipart_body(
                boundary,
                metadata_name,
                metadata_content_type,
                &metadata,
                images,
                image_cache,
                definition.profile_id,
            )?;
            let separator = if definition.profile_id == FirstReleaseProfileId::IpcSmart {
                ","
            } else {
                ";"
            };
            (
                body,
                format!("multipart/form-data{separator} boundary={boundary}"),
            )
        }
    };
    if body.len() > MAX_RENDERED_ALARM_BYTES {
        return Err(AlarmError::new(
            "device_simulator.alarm.rendered_size_exceeded",
            "rendered alarm body exceeds 32 MiB",
        ));
    }
    let mut headers = BTreeMap::new();
    headers.insert("Content-Type".into(), content_type);
    headers.insert("Content-Length".into(), body.len().to_string());
    match &transport.body_encoding {
        BodyEncoding::Multipart { .. } => {
            headers.insert("Accept".into(), "*/*".into());
            headers.insert("Accept-Encoding".into(), "gzip,deflate".into());
            headers.insert("Connection".into(), "keep-alive".into());
        }
        BodyEncoding::Raw { .. } => {
            if definition.profile_id == FirstReleaseProfileId::IpcSmart {
                headers.insert("Accept".into(), "*/*".into());
                if role == AlarmRequestRole::Primary
                    && !definition.follow_up_requests.is_empty()
                    && !images.is_empty()
                {
                    // SmartAlarm.py hand-writes the pictured V1.0 structure
                    // request with an Expect handshake.
                    headers.insert("Expect".into(), "100-continue".into());
                }
            } else if matches!(
                definition.profile_id,
                FirstReleaseProfileId::NvrCommon | FirstReleaseProfileId::NvrVehicle
            ) || (definition.profile_id == FirstReleaseProfileId::IpcFaceAccess
                && !transport.path.ends_with("/PersonVerification"))
            {
                headers.insert("Accept".into(), "*/*".into());
            }
            if definition.profile_id == FirstReleaseProfileId::IpcStructured
                || (definition.profile_id == FirstReleaseProfileId::NvrVehicle
                    && role == AlarmRequestRole::Primary)
                || (definition.profile_id == FirstReleaseProfileId::IpcFaceAccess
                    && transport.path.ends_with("/PersonVerification")
                    && role == AlarmRequestRole::Primary)
            {
                headers.insert("Connection".into(), "close".into());
            }
        }
    }
    Ok(HttpAlarmRequest {
        method: transport.method,
        path: transport.path.clone(),
        source_ip,
        headers,
        body: Arc::from(body),
        success_rule: transport.success_rule.clone(),
    })
}

fn apply_legacy_runtime_values(
    definition: &AlarmHandlerDefinition,
    role: AlarmRequestRole,
    context: &AlarmBuildContext,
    metadata: Vec<u8>,
) -> AlarmResult<Vec<u8>> {
    let Some(values) = context.legacy_values.as_ref() else {
        return Ok(metadata);
    };
    let mut document: serde_json::Value = serde_json::from_slice(&metadata).map_err(|source| {
        AlarmError::new(
            "device_simulator.alarm.rendered_json_invalid",
            format!("rendered legacy alarm is not valid JSON: {source}"),
        )
    })?;

    let event_id = context
        .fields
        .get(&DynamicField::EventId)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let timestamp = context_json_number(context, DynamicField::Timestamp);
    let capture_time = context
        .fields
        .get(&DynamicField::CaptureTime)
        .cloned()
        .unwrap_or_default();
    let reference = legacy_reference(definition, role, context, values);
    let related_id = values.related_id();

    match definition.profile_id {
        FirstReleaseProfileId::IpcCustom if role == AlarmRequestRole::Primary => {
            set_string(&mut document, "/EventInfo/Reference", &reference);
            set_value(&mut document, "/EventInfo/TimeStamp", timestamp.clone());
            set_number(&mut document, "/EventInfo/Seq", event_id);
            if document.pointer("/EventInfo/DeviceCode").is_some() {
                set_string(
                    &mut document,
                    "/EventInfo/DeviceCode",
                    context_string(context, DynamicField::DeviceId),
                );
            }
            set_string(&mut document, "/ImageList/0/CaptureTime", &capture_time);
        }
        FirstReleaseProfileId::IpcSmart => {
            if matches!(
                definition.transport.body_encoding,
                BodyEncoding::Multipart { .. }
            ) {
                if role == AlarmRequestRole::Primary {
                    set_string(&mut document, "/EventInfo/Reference", &reference);
                    set_number(&mut document, "/EventInfo/SrcID", 1);
                    set_value(&mut document, "/EventInfo/TimeStamp", timestamp.clone());
                    set_value(&mut document, "/ImageList/0/CaptureTime", timestamp.clone());
                    set_string(
                        &mut document,
                        "/ImageList/0/CaptureTimeStr",
                        context_string(context, DynamicField::CaptureTimeText),
                    );
                }
            } else if definition.follow_up_requests.is_empty() || role != AlarmRequestRole::Primary
            {
                set_string(&mut document, "/Reference", &reference);
                set_value(&mut document, "/AlarmInfo/TimeStamp", timestamp);
                set_number(&mut document, "/AlarmInfo/AlarmSeq", event_id);
                if role != AlarmRequestRole::Recovery {
                    set_string(&mut document, "/AlarmInfo/RelatedID", &related_id);
                }
            } else if role == AlarmRequestRole::Primary {
                set_string(&mut document, "/Reference", &reference);
                set_value(&mut document, "/TimeStamp", timestamp);
                set_number(&mut document, "/Seq", event_id);
                set_string(&mut document, "/RelatedID", &related_id);
                set_smart_v1_image_times(
                    &mut document,
                    context_json_number(context, DynamicField::Timestamp),
                );
            }

            if role == AlarmRequestRole::Primary {
                match definition.alarm_type_id.as_str() {
                    "dog-detection" => insert_json_object_field(
                        &mut document,
                        "EventDetail",
                        serde_json::json!({"UnLeashed": 1}),
                    ),
                    "dog-detection-2" => insert_json_object_field(
                        &mut document,
                        "EventDetail",
                        serde_json::json!({"NotAllowed": 1}),
                    ),
                    _ => {}
                }
            }
        }
        FirstReleaseProfileId::IpcStructured if role == AlarmRequestRole::Primary => {
            set_string(&mut document, "/Reference", &reference);
            set_value(&mut document, "/TimeStamp", timestamp);
            set_number(&mut document, "/Seq", event_id);
            set_capture_times(&mut document, "/StructureInfo/ImageInfoList", &capture_time);
            apply_structured_camera_values(
                &mut document,
                definition.alarm_type_id.as_str(),
                values,
                event_id,
            );
        }
        FirstReleaseProfileId::IpcFaceAccess => {
            let person_verification = definition.transport.path.ends_with("/PersonVerification");
            if person_verification && role == AlarmRequestRole::Primary {
                set_string(&mut document, "/Reference", &reference);
                set_number(&mut document, "/Seq", event_id);
                set_string(
                    &mut document,
                    "/DeviceCode",
                    context_string(context, DynamicField::DeviceId),
                );
                set_value(&mut document, "/Timestamp", timestamp.clone());
                set_number(&mut document, "/FaceInfoList/0/ID", event_id);
                set_value(&mut document, "/FaceInfoList/0/Timestamp", timestamp);
                set_number(&mut document, "/LibMatInfoList/0/ID", event_id);
                if definition.alarm_type_id.as_str() == "inlib" {
                    set_number_or_string(
                        &mut document,
                        "/LibMatInfoList/0/MatchPersonID",
                        context_string(context, DynamicField::PersonId),
                    );
                }
                apply_face_access_values(
                    &mut document,
                    definition.alarm_type_id.as_str(),
                    values,
                    context,
                );
            } else {
                set_string(&mut document, "/Reference", &reference);
                set_value(&mut document, "/AlarmInfo/TimeStamp", timestamp);
                set_number(&mut document, "/AlarmInfo/AlarmSeq", event_id);
            }
        }
        FirstReleaseProfileId::NvrCommon => {
            if definition.alarm_type_id.as_str() != "channel-deleted" {
                set_string(&mut document, "/Reference", &reference);
                set_value(&mut document, "/AlarmInfo/TimeStamp", timestamp);
            }
        }
        FirstReleaseProfileId::NvrVehicle => match definition.alarm_type_id.as_str() {
            "match" | "nomatch" if role == AlarmRequestRole::Primary => {
                set_string(&mut document, "/Reference", &reference);
                set_number(&mut document, "/VehicleEventInfo/ID", event_id);
                set_value(
                    &mut document,
                    "/VehicleEventInfo/Timestamp",
                    timestamp.clone(),
                );
                set_number(
                    &mut document,
                    "/VehicleEventInfo/VehicleInfoList/0/RecordID",
                    event_id,
                );
                set_value(
                    &mut document,
                    "/VehicleEventInfo/VehicleInfoList/0/PassingTime",
                    timestamp,
                );
                set_string(
                    &mut document,
                    "/VehicleEventInfo/VehicleInfoList/0/RelatedID",
                    &related_id,
                );
                let plate = if definition.alarm_type_id.as_str() == "match" {
                    format!("赣B{}BL", values.bounded(30, 100, 999))
                } else {
                    format!("赣A{}U8", values.bounded(31, 100, 999))
                };
                set_string(
                    &mut document,
                    "/VehicleEventInfo/VehicleInfoList/0/PlateAttr/Plate",
                    &plate,
                );
            }
            "match" | "nomatch" if role == AlarmRequestRole::FollowUp => {
                set_string(&mut document, "/Reference", &reference);
                set_value(&mut document, "/AlarmInfo/TimeStamp", timestamp);
                set_string(&mut document, "/AlarmInfo/RelatedID", &related_id);
            }
            "snap" if role == AlarmRequestRole::Primary => {
                set_string(&mut document, "/Reference", &reference);
                set_value(&mut document, "/TimeStamp", timestamp);
                set_number(
                    &mut document,
                    "/StructureInfo/ObjInfo/VehicleInfoList/0/ID",
                    event_id,
                );
                set_capture_times_count(
                    &mut document,
                    "/StructureInfo/ImageInfoList",
                    &capture_time,
                    2,
                );
                rewrite_picture_type(&mut document, "/StructureInfo/ImageInfoList/0/URL", "2");
                rewrite_picture_type(&mut document, "/StructureInfo/ImageInfoList/1/URL", "1");
            }
            _ => {}
        },
        _ => {}
    }

    serde_json::to_vec(&document).map_err(|source| {
        AlarmError::new(
            "device_simulator.alarm.rendered_json_serialize_failed",
            format!("rendered legacy alarm could not be serialized: {source}"),
        )
    })
}

fn legacy_reference(
    definition: &AlarmHandlerDefinition,
    role: AlarmRequestRole,
    context: &AlarmBuildContext,
    values: &LegacyAlarmValues,
) -> String {
    let base = context_string(context, DynamicField::Reference);
    let fixed = definition.profile_id == FirstReleaseProfileId::IpcStructured
        || (definition.profile_id == FirstReleaseProfileId::IpcFaceAccess
            && role == AlarmRequestRole::Primary
            && definition.transport.path.ends_with("/PersonVerification"));
    if fixed {
        return base.to_owned();
    }
    let subscription_id = match role {
        AlarmRequestRole::Primary => values.primary_subscription_id(),
        AlarmRequestRole::FollowUp => values.follow_up_subscription_id(),
        AlarmRequestRole::Recovery => match definition.profile_id {
            FirstReleaseProfileId::IpcSmart | FirstReleaseProfileId::NvrVehicle => {
                values.follow_up_subscription_id()
            }
            FirstReleaseProfileId::NvrCommon => values.primary_subscription_id(),
            FirstReleaseProfileId::IpcFaceAccess => values.recovery_subscription_id(),
            _ => values.primary_subscription_id(),
        },
    };
    base.rsplit_once("/Subscribers/")
        .map(|(prefix, _)| format!("{prefix}/Subscribers/{subscription_id}"))
        .unwrap_or_else(|| base.to_owned())
}

fn context_string(context: &AlarmBuildContext, field: DynamicField) -> &str {
    context.fields.get(&field).map(String::as_str).unwrap_or("")
}

fn context_json_number(context: &AlarmBuildContext, field: DynamicField) -> serde_json::Value {
    let raw = context_string(context, field);
    if let Ok(value) = raw.parse::<i64>() {
        return serde_json::Value::from(value);
    }
    if let Ok(value) = raw.parse::<f64>() {
        if let Some(number) = serde_json::Number::from_f64(value) {
            return serde_json::Value::Number(number);
        }
    }
    serde_json::Value::String(raw.to_owned())
}

fn set_value(document: &mut serde_json::Value, pointer: &str, value: serde_json::Value) {
    if let Some(slot) = document.pointer_mut(pointer) {
        *slot = value;
    }
}

fn set_string(document: &mut serde_json::Value, pointer: &str, value: &str) {
    set_value(
        document,
        pointer,
        serde_json::Value::String(value.to_owned()),
    );
}

fn set_number_or_string(document: &mut serde_json::Value, pointer: &str, value: &str) {
    if let Ok(number) = value.parse::<u64>() {
        set_number(document, pointer, number);
    } else {
        set_string(document, pointer, value);
    }
}

fn rewrite_picture_type(document: &mut serde_json::Value, pointer: &str, picture_type: &str) {
    let Some(slot) = document.pointer_mut(pointer) else {
        return;
    };
    let Some(url) = slot.as_str() else {
        return;
    };
    let Some((prefix, query)) = url.split_once("?") else {
        return;
    };
    let mut query_parts = query.split('&').map(str::to_owned).collect::<Vec<_>>();
    if let Some(type_part) = query_parts
        .iter_mut()
        .find(|part| part.starts_with("Type="))
    {
        *type_part = format!("Type={picture_type}");
        *slot = serde_json::Value::String(format!("{prefix}?{}", query_parts.join("&")));
    }
}

fn set_smart_v1_image_times(document: &mut serde_json::Value, timestamp: serde_json::Value) {
    for list in ["/StructureInfo/ImageInfoList", "/AlarmPicture/ImageList"] {
        for index in 0..8 {
            let pointer = format!("{list}/{index}/CaptureTime");
            if document.pointer(&pointer).is_some() {
                set_value(document, &pointer, timestamp.clone());
            }
        }
    }
}

fn set_capture_times(document: &mut serde_json::Value, list: &str, capture_time: &str) {
    set_capture_times_count(document, list, capture_time, 8);
}

fn set_capture_times_count(
    document: &mut serde_json::Value,
    list: &str,
    capture_time: &str,
    count: usize,
) {
    for index in 0..count {
        let pointer = format!("{list}/{index}/CaptureTime");
        if document.pointer(&pointer).is_some() {
            set_string(document, &pointer, capture_time);
        }
    }
}

fn apply_structured_camera_values(
    document: &mut serde_json::Value,
    alarm_type_id: &str,
    values: &LegacyAlarmValues,
    event_id: u64,
) {
    match alarm_type_id {
        "person" => {
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/ID",
                event_id,
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/Gender",
                values.bounded(10, 0, 2),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/AgeRange",
                values.bounded(11, 0, 6),
            );
        }
        "face" => {
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/FaceID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/FaceDoforPersonID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/PersonID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/PersonDoforFaceID",
                event_id,
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/AttributeInfo/Gender",
                values.bounded(12, 0, 2),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/AttributeInfo/AgeRange",
                values.bounded(13, 0, 6),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/AttributeInfo/GlassFlag",
                values.bounded(14, 0, 3),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/Gender",
                values.bounded(15, 0, 2),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/AgeRange",
                values.bounded(16, 0, 5),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/SleevesLength",
                values.bounded(17, 0, 3),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/HairLength",
                values.bounded(18, 0, 3),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/PersonInfoList/0/AttributeInfo/BagFlag",
                values.bounded(19, 0, 3),
            );
        }
        "car" => {
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/VehicleInfoList/0/ID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/FaceID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/FaceDoforVehicleID",
                event_id,
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/VehicleInfoList/0/VehicleAttributeInfo/SpeedType",
                values.bounded(20, 0, 5),
            );
            set_json_pointer(
                document,
                "/StructureInfo/ObjInfo/VehicleInfoList/0/PlateAttributeInfo/PlateNo",
                serde_json::Value::String(format!("UV{}", values.bounded(21, 100, 999))),
            );
        }
        "nonmotor" => {
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/NonMotorVehicleInfoList/0/ID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/FaceID",
                event_id,
            );
            set_number_create(
                document,
                "/StructureInfo/ObjInfo/FaceInfoList/0/FaceDoforNonMotorVehicleID",
                event_id,
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/NonMotorVehicleInfoList/0/AttributeInfo/SpeedType",
                values.bounded(22, 0, 5),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/NonMotorVehicleInfoList/0/AttributeInfo/ImageDirection",
                values.bounded(23, 0, 10),
            );
            set_number(
                document,
                "/StructureInfo/ObjInfo/NonMotorVehicleInfoList/0/AttributeInfo/NonVehicleType",
                values.bounded(24, 0, 6),
            );
        }
        _ => {}
    }
}

fn apply_face_access_values(
    document: &mut serde_json::Value,
    alarm_type_id: &str,
    values: &LegacyAlarmValues,
    context: &AlarmBuildContext,
) {
    if !matches!(alarm_type_id, "inlib" | "notinlib") {
        return;
    }
    let temperature_tenths = values.bounded(40, 300, 450);
    set_json_pointer(
        document,
        "/FaceInfoList/0/Temperature",
        serde_json::Value::String(format!(
            "{}.{:01}",
            temperature_tenths / 10,
            temperature_tenths % 10
        )),
    );
    set_number(
        document,
        "/FaceInfoList/0/MaskFlag",
        values.bounded(41, 0, 2),
    );
    let timestamp = context
        .fields
        .get(&DynamicField::Timestamp)
        .and_then(|value| value.split('.').next())
        .unwrap_or("0");
    let nonce = values.bounded(42, 1, 30_000);
    if alarm_type_id == "inlib" {
        set_json_pointer(
            document,
            "/FaceInfoList/0/PanoImage/Name",
            serde_json::Value::String(format!("{timestamp}_1_{nonce}.jpg")),
        );
        set_json_pointer(
            document,
            "/FaceInfoList/0/FaceImage/Name",
            serde_json::Value::String(format!("{timestamp}_2_{nonce}.jpg")),
        );
    } else {
        set_json_pointer(
            document,
            "/FaceInfoList/0/FaceImage/Name",
            serde_json::Value::String(format!("{timestamp}_1_{nonce}.jpg")),
        );
    }
}

fn set_number(document: &mut serde_json::Value, pointer: &str, value: u64) {
    set_json_pointer(document, pointer, serde_json::Value::from(value));
}

fn set_number_create(document: &mut serde_json::Value, pointer: &str, value: u64) {
    if document.pointer(pointer).is_some() {
        set_number(document, pointer, value);
        return;
    }
    let Some((parent, key)) = pointer.rsplit_once('/') else {
        return;
    };
    let Some(parent_value) = document.pointer_mut(parent) else {
        return;
    };
    if let serde_json::Value::Object(map) = parent_value {
        map.insert(key.replace("~1", "/").replace("~0", "~"), value.into());
    }
}

fn set_json_pointer(document: &mut serde_json::Value, pointer: &str, value: serde_json::Value) {
    if let Some(slot) = document.pointer_mut(pointer) {
        *slot = value;
    }
}

fn insert_json_object_field(document: &mut serde_json::Value, key: &str, value: serde_json::Value) {
    if let Some(object) = document.as_object_mut() {
        object.insert(key.to_owned(), value);
    }
}

pub(crate) fn image_reference_token(reference: &ImageAssetRef) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"file-sync-tool/device-simulator/picture/v1\0");
    match reference {
        ImageAssetRef::Pack {
            pack_id,
            version,
            path,
        } => {
            hasher.update(b"pack\0");
            hasher.update(pack_id.as_bytes());
            hasher.update(b"\0");
            hasher.update(version.as_bytes());
            hasher.update(b"\0");
            hasher.update(path.as_bytes());
        }
        ImageAssetRef::UserAsset {
            image_id,
            extension,
            ..
        } => {
            hasher.update(b"user\0");
            hasher.update(image_id.as_bytes());
            hasher.update(b"\0");
            hasher.update(extension.as_str().as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn build_multipart_body(
    boundary: &str,
    metadata_name: &str,
    metadata_content_type: &str,
    metadata: &[u8],
    images: &[ImageAttachmentDefinition],
    cache: &ImageCache,
    profile_id: FirstReleaseProfileId,
) -> AlarmResult<Vec<u8>> {
    validate_multipart_token(metadata_name, "metadata name")?;
    validate_header_value(metadata_content_type, "metadata content type")?;
    let mut body = Vec::new();
    append_part_prefix(
        &mut body,
        boundary,
        metadata_name,
        None,
        None,
        metadata_content_type,
        profile_id,
        metadata.len(),
    );
    body.extend_from_slice(metadata);
    body.extend_from_slice(b"\r\n");
    for attachment in images {
        validate_multipart_token(&attachment.field_name, "image field name")?;
        validate_file_name(&attachment.file_name)?;
        let image = cache.get(&attachment.reference)?;
        append_part_prefix(
            &mut body,
            boundary,
            &attachment.field_name,
            Some(&attachment.file_name),
            attachment.image_index,
            image.content_type,
            profile_id,
            image.bytes.len(),
        );
        body.extend_from_slice(&image.bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(body)
}

fn append_part_prefix(
    output: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    file_name: Option<&str>,
    image_index: Option<u16>,
    content_type: &str,
    profile_id: FirstReleaseProfileId,
    part_length: usize,
) {
    output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    if profile_id == FirstReleaseProfileId::IpcSmart && file_name.is_none() {
        // SmartAlarm.py keeps the semicolon inside the metadata field name.
        output.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name};\"\r\n").as_bytes(),
        );
    } else if profile_id == FirstReleaseProfileId::IpcCustom && file_name.is_some() {
        // Preserve CustomAlarm.py's historical parameter order and spacing.
        output.extend_from_slice(
            format!(
                "Content-Disposition: form-data;imageindex=1;name=\"{name}\";filename=\"{}\"\r\n",
                file_name.unwrap_or("picture.jpg")
            )
            .as_bytes(),
        );
    } else {
        output.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"").as_bytes(),
        );
        if let Some(image_index) = image_index {
            output.extend_from_slice(format!("; imageindex={image_index}").as_bytes());
        }
        if let Some(file_name) = file_name {
            output.extend_from_slice(format!("; filename=\"{file_name}\"").as_bytes());
        }
        output.extend_from_slice(b"\r\n");
    }
    output.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    // All six legacy senders declared each multipart part length explicitly.
    output.extend_from_slice(format!("Content-Length: {part_length}\r\n").as_bytes());
    output.extend_from_slice(b"\r\n");
}

fn base64_encoded_len(raw_len: usize) -> usize {
    raw_len.saturating_add(2) / 3 * 4
}

fn validate_definition(definition: &AlarmHandlerDefinition) -> AlarmResult<()> {
    if definition.handler_id.profile_id() != definition.profile_id {
        return Err(AlarmError::new(
            "device_simulator.alarm.handler_profile_mismatch",
            "alarm handler is registered for the wrong profile",
        ));
    }
    validate_request_definition(
        definition.profile_id,
        &definition.template,
        definition.image_policy,
        &definition.images,
        &definition.transport,
    )?;
    for follow_up in &definition.follow_up_requests {
        validate_request_definition(
            definition.profile_id,
            &follow_up.template,
            follow_up.image_policy,
            &follow_up.images,
            &follow_up.transport,
        )?;
    }
    if let RecoveryDefinition::RenderWith {
        template,
        transport,
        include_images,
        ..
    } = &definition.recovery
    {
        validate_request_definition(
            definition.profile_id,
            template,
            if *include_images {
                definition.image_policy
            } else {
                ImagePolicy::Forbidden
            },
            if *include_images {
                &definition.images
            } else {
                &[]
            },
            transport,
        )?;
    }
    if definition.evidence.legacy_sources.is_empty()
        || definition.evidence.template_source.trim().is_empty()
        || definition.evidence.platforms.is_empty()
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.evidence_incomplete",
            "alarm handler evidence must identify sources, template provenance, and platforms",
        ));
    }
    if definition.evidence.fixture_provenance == FixtureProvenance::SyntheticUnverified
        && definition
            .evidence
            .platforms
            .iter()
            .any(|evidence| evidence.verification == PlatformVerification::PlatformVerified)
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.evidence_contradiction",
            "a synthetic fixture cannot claim platform verification",
        ));
    }
    Ok(())
}

fn validate_request_definition(
    profile_id: FirstReleaseProfileId,
    template: &CompiledTemplate,
    image_policy: ImagePolicy,
    images: &[ImageAttachmentDefinition],
    transport: &TransportDefinition,
) -> AlarmResult<()> {
    validate_http_path(&transport.path)?;
    if profile_id == FirstReleaseProfileId::NvrCommon
        && (image_policy != ImagePolicy::Forbidden || !images.is_empty())
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.nvr_common_image_forbidden",
            "ordinary NVR alarms cannot acquire image behavior without approved evidence",
        ));
    }
    if image_policy == ImagePolicy::Forbidden && !images.is_empty() {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_policy_invalid",
            "image attachments exist while the handler forbids images",
        ));
    }
    if image_policy == ImagePolicy::Required && images.is_empty() {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_policy_invalid",
            "image handler requires at least one declared image",
        ));
    }
    for image in images {
        validate_multipart_token(&image.field_name, "image field name")?;
        validate_file_name(&image.file_name)?;
        if image.image_index == Some(0) {
            return Err(AlarmError::new(
                "device_simulator.alarm.multipart_image_index_invalid",
                "multipart image indexes are one-based",
            ));
        }
    }
    let embedded_images = embedded_image_count(template);
    match &transport.body_encoding {
        BodyEncoding::Raw { content_type } => {
            validate_header_value(content_type, "content type")?;
            if embedded_images != images.len() && (!images.is_empty() || embedded_images != 0) {
                return Err(AlarmError::new(
                    "device_simulator.alarm.image_mapping_missing",
                    "raw alarm image attachments must match the explicit Base64 template slots",
                ));
            }
        }
        BodyEncoding::Multipart {
            metadata_name,
            metadata_content_type,
        } => {
            validate_multipart_token(metadata_name, "metadata name")?;
            validate_header_value(metadata_content_type, "metadata content type")?;
            if embedded_images != 0 {
                return Err(AlarmError::new(
                    "device_simulator.alarm.image_mapping_ambiguous",
                    "multipart images cannot also use the image_base64 template field",
                ));
            }
        }
    }
    if let ResponseSuccessRule::StatusRange { minimum, maximum } = transport.success_rule {
        if minimum < 100 || maximum > 599 || minimum > maximum {
            return Err(AlarmError::new(
                "device_simulator.alarm.success_rule_invalid",
                "HTTP success status range is invalid",
            ));
        }
    }
    Ok(())
}

/// Provides one non-golden scaffold for each approved first-release handler.
/// These definitions must not be presented as platform-compatible.
pub fn synthetic_unverified_first_release_registry() -> AlarmResult<AlarmHandlerRegistry> {
    let mut registry = AlarmHandlerRegistry::default();
    for handler_id in [
        AlarmHandlerId::CustomV1,
        AlarmHandlerId::SmartV1,
        AlarmHandlerId::StructuredV1,
        AlarmHandlerId::FaceAccessV1,
        AlarmHandlerId::NvrCommonV1,
        AlarmHandlerId::NvrVehicleV1,
    ] {
        let profile_id = handler_id.profile_id();
        let image_policy = if profile_id == FirstReleaseProfileId::NvrCommon {
            ImagePolicy::Forbidden
        } else {
            // Real mappings are intentionally absent until legacy-derived
            // fixtures and pack manifests pass review.
            ImagePolicy::Optional
        };
        registry.register(AlarmHandlerDefinition {
            handler_id,
            alarm_type_id: AlarmTypeId::new("synthetic-fixture")?,
            profile_id,
            template: CompiledTemplate::compile(
                br#"{"fixture":"synthetic_unverified","device":"{{device_id}}","time":"{{timestamp}}"}"#,
            )?,
            image_policy,
            images: vec![],
            transport: TransportDefinition {
                method: HttpMethod::Post,
                path: "/synthetic-unverified/alarm".into(),
                source_binding: SourceBinding::DeviceIp,
                body_encoding: BodyEncoding::Raw {
                    content_type: "application/json; charset=utf-8".into(),
                },
                success_rule: ResponseSuccessRule::Unverified,
            },
            follow_up_requests: vec![],
            recovery: RecoveryDefinition::None,
            evidence: HandlerEvidence {
                legacy_sources: legacy_alarm_sources(profile_id)
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                template_source: "synthetic fixture; real template pending evidence gate".into(),
                fixture_provenance: FixtureProvenance::SyntheticUnverified,
                platforms: [TargetPlatform::Ums]
                    .into_iter()
                    .map(|platform| PlatformEvidence {
                        platform,
                        verification: PlatformVerification::SourceConfirmedPlatformUnverified,
                    })
                    .collect(),
                intentional_changes: vec![
                    "No vendor URL, fields, boundary, or success rule is inferred".into(),
                ],
            },
        })?;
    }
    Ok(registry)
}

fn legacy_alarm_sources(profile_id: FirstReleaseProfileId) -> Vec<&'static str> {
    match profile_id {
        FirstReleaseProfileId::IpcCustom => vec![
            "data/alarms_info.yml",
            "script/CustomAlarm.py",
            "object/CustomStruct/",
            "pic/CUSTOM/",
        ],
        FirstReleaseProfileId::IpcSmart => vec![
            "data/alarms_info.yml",
            "script/SmartAlarm.py",
            "object/SmartStruct/",
            "pic/SMART/",
        ],
        FirstReleaseProfileId::IpcStructured => vec![
            "data/alarms_info.yml",
            "script/StructureAlarm.py",
            "object/StructStruct/",
            "pic/STRUCT/",
        ],
        FirstReleaseProfileId::IpcFaceAccess => vec![
            "data/alarms_info.yml",
            "script/ACSAlarm.py",
            "object/ACSStruct/",
            "pic/ACS/",
        ],
        FirstReleaseProfileId::NvrCommon => vec![
            "data/alarms_info.yml",
            "script/NormalAlarm.py",
            "object/NormalStruct/",
        ],
        FirstReleaseProfileId::NvrVehicle => vec![
            "data/alarms_info.yml",
            "script/VehicleAlarm.py",
            "object/VehicleStruct/",
            "pic/VEHICLE/",
        ],
    }
}

fn validate_http_path(path: &str) -> AlarmResult<()> {
    if path.is_empty()
        || path.len() > 2048
        || !path.starts_with('/')
        || path.starts_with("//")
        || path
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\0'))
        || path.contains("://")
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.http_path_invalid",
            "alarm transport path must be an origin-relative HTTP path",
        ));
    }
    Ok(())
}

fn validate_header_value(value: &str, subject: &str) -> AlarmResult<()> {
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte == b'\r' || byte == b'\n' || byte == 0)
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.header_invalid",
            format!("{subject} is not a safe HTTP header value"),
        ));
    }
    Ok(())
}

fn validate_multipart_token(value: &str, subject: &str) -> AlarmResult<()> {
    if value.is_empty()
        || value.len() > 70
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.multipart_token_invalid",
            format!("{subject} is not a safe multipart token"),
        ));
    }
    Ok(())
}

fn validate_file_name(value: &str) -> AlarmResult<()> {
    if value.is_empty()
        || value.len() > 255
        || value
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '\r' | '\n' | '\0' | '"'))
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_filename_invalid",
            "image filename is not safe for Content-Disposition",
        ));
    }
    image_extension(value)?;
    Ok(())
}

fn image_extension(path: &str) -> AlarmResult<ImageExtension> {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    match extension.map(str::to_ascii_lowercase).as_deref() {
        Some("jpg") => Ok(ImageExtension::Jpg),
        Some("jpeg") => Ok(ImageExtension::Jpeg),
        Some("png") => Ok(ImageExtension::Png),
        _ => Err(AlarmError::new(
            "device_simulator.alarm.image_type_unsupported",
            "alarm images must be JPEG or PNG",
        )),
    }
}

fn validate_pack_identity(id: &str, version: &str) -> AlarmResult<()> {
    let value = format!("{id}@{version}");
    value
        .parse::<crate::device_simulator::assets::catalog::PackRef>()
        .map_err(|_| {
            AlarmError::new(
                "device_simulator.alarm.image_pack_invalid",
                "image pack identity is invalid",
            )
        })?;
    Ok(())
}

fn validate_user_asset_id(id: &str) -> AlarmResult<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.user_image_id_invalid",
            "user image ID must be bounded lowercase ASCII",
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> AlarmResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_hash_invalid",
            "alarm image SHA-256 must be lowercase hexadecimal",
        ));
    }
    Ok(())
}

fn read_verified_file(
    root: &Path,
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> AlarmResult<Vec<u8>> {
    validate_sha256(expected_sha256)?;
    let canonical_root = fs::canonicalize(root).map_err(|error| {
        AlarmError::new(
            "device_simulator.alarm.image_root_invalid",
            format!("failed to resolve image root: {error}"),
        )
    })?;
    reject_symlink_components(root, path)?;
    let canonical_path = fs::canonicalize(path).map_err(|error| {
        AlarmError::new(
            "device_simulator.alarm.image_read_failed",
            format!("failed to resolve alarm image: {error}"),
        )
    })?;
    if !canonical_path.starts_with(&canonical_root) {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_path_escape",
            "alarm image escaped its approved asset root",
        ));
    }
    let metadata = fs::metadata(&canonical_path).map_err(|error| {
        AlarmError::new(
            "device_simulator.alarm.image_read_failed",
            format!("failed to inspect alarm image: {error}"),
        )
    })?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_size_mismatch",
            "alarm image size differs from its declaration",
        ));
    }
    let bytes = fs::read(&canonical_path).map_err(|error| {
        AlarmError::new(
            "device_simulator.alarm.image_read_failed",
            format!("failed to read alarm image: {error}"),
        )
    })?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected_sha256 {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_hash_mismatch",
            "alarm image hash differs from its declaration",
        ));
    }
    Ok(bytes)
}

fn reject_symlink_components(root: &Path, path: &Path) -> AlarmResult<()> {
    let relative = path.strip_prefix(root).map_err(|_| {
        AlarmError::new(
            "device_simulator.alarm.image_path_escape",
            "alarm image is outside its approved asset root",
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            AlarmError::new(
                "device_simulator.alarm.image_read_failed",
                format!("failed to inspect alarm image path: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(AlarmError::new(
                "device_simulator.alarm.image_symlink_forbidden",
                "alarm image paths may not contain symbolic links",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{non_commercial_usage, PackFile, PackManifest};
    use semver::Version;
    use tempfile::TempDir;

    fn evidence() -> HandlerEvidence {
        HandlerEvidence {
            legacy_sources: vec!["script/TestAlarm.py".into()],
            template_source: "synthetic fixture, not a golden fixture".into(),
            fixture_provenance: FixtureProvenance::SyntheticUnverified,
            platforms: vec![PlatformEvidence {
                platform: TargetPlatform::Ums,
                verification: PlatformVerification::SourceConfirmedPlatformUnverified,
            }],
            intentional_changes: vec![],
        }
    }

    fn raw_definition(profile_id: FirstReleaseProfileId) -> AlarmHandlerDefinition {
        let handler_id = match profile_id {
            FirstReleaseProfileId::IpcCustom => AlarmHandlerId::CustomV1,
            FirstReleaseProfileId::IpcSmart => AlarmHandlerId::SmartV1,
            FirstReleaseProfileId::IpcStructured => AlarmHandlerId::StructuredV1,
            FirstReleaseProfileId::IpcFaceAccess => AlarmHandlerId::FaceAccessV1,
            FirstReleaseProfileId::NvrCommon => AlarmHandlerId::NvrCommonV1,
            FirstReleaseProfileId::NvrVehicle => AlarmHandlerId::NvrVehicleV1,
        };
        AlarmHandlerDefinition {
            handler_id,
            alarm_type_id: AlarmTypeId::new("fixture-motion").unwrap(),
            profile_id,
            template: CompiledTemplate::compile(
                br#"{"device":"{{device_id}}","time":"{{timestamp}}"}"#,
            )
            .unwrap(),
            image_policy: ImagePolicy::Forbidden,
            images: vec![],
            transport: TransportDefinition {
                method: HttpMethod::Post,
                path: "/fixture/alarm".into(),
                source_binding: SourceBinding::DeviceIp,
                body_encoding: BodyEncoding::Raw {
                    content_type: "application/json; charset=utf-8".into(),
                },
                success_rule: ResponseSuccessRule::Unverified,
            },
            follow_up_requests: vec![],
            recovery: RecoveryDefinition::None,
            evidence: evidence(),
        }
    }

    #[test]
    fn template_compiler_accepts_adjacent_literal_json_object_terminators() {
        let template = CompiledTemplate::compile(br#"{"outer":{"value":1}}"#).unwrap();
        assert!(template.fields().is_empty());
        assert_eq!(
            template.render(&BTreeMap::new()).unwrap(),
            br#"{"outer":{"value":1}}"#
        );
    }

    #[test]
    fn registry_uses_compiled_handler_ids_and_exact_profile_binding() {
        let mut registry = AlarmHandlerRegistry::default();
        registry
            .register(raw_definition(FirstReleaseProfileId::IpcSmart))
            .unwrap();
        assert!(registry
            .resolve(
                FirstReleaseProfileId::IpcSmart,
                "alarm.smart.v1",
                "fixture-motion"
            )
            .is_ok());
        assert_eq!(
            registry
                .resolve(
                    FirstReleaseProfileId::IpcCustom,
                    "alarm.smart.v1",
                    "fixture-motion"
                )
                .unwrap_err()
                .code,
            "device_simulator.alarm.handler_profile_mismatch"
        );
        assert_eq!(
            AlarmHandlerId::from_str("alarm.downloaded.script")
                .unwrap_err()
                .code,
            "device_simulator.alarm.handler_unknown"
        );
    }

    #[test]
    fn ordinary_nvr_cannot_register_an_image_alarm() {
        let mut definition = raw_definition(FirstReleaseProfileId::NvrCommon);
        definition.image_policy = ImagePolicy::Optional;
        assert_eq!(
            AlarmHandlerRegistry::default()
                .register(definition)
                .unwrap_err()
                .code,
            "device_simulator.alarm.nvr_common_image_forbidden"
        );
    }

    fn pack_fixture() -> (
        TempDir,
        BTreeMap<PackIdentity, PackManifest>,
        ImageAssetRef,
        Vec<u8>,
    ) {
        let root = TempDir::new().unwrap();
        let bytes = b"synthetic-image-bytes".to_vec();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let image_path = root.path().join("packs/ipc-smart/1.0.0/images/alarm.jpg");
        fs::create_dir_all(image_path.parent().unwrap()).unwrap();
        fs::write(&image_path, &bytes).unwrap();
        fs::create_dir_all(root.path().join("user-assets")).unwrap();
        let manifest = PackManifest {
            schema_version: 1,
            id: "ipc-smart".into(),
            version: Version::new(1, 0, 0),
            engine_api: 1,
            usage: non_commercial_usage(),
            files: vec![PackFile {
                path: "images/alarm.jpg".into(),
                sha256: hash,
                size: bytes.len() as u64,
            }],
        };
        let identity = PackIdentity {
            id: "ipc-smart".into(),
            version: "1.0.0".into(),
        };
        let reference = ImageAssetRef::Pack {
            pack_id: identity.id.clone(),
            version: identity.version.clone(),
            path: "images/alarm.jpg".into(),
        };
        (
            root,
            BTreeMap::from([(identity, manifest)]),
            reference,
            bytes,
        )
    }

    #[test]
    fn images_are_verified_once_deduplicated_and_arc_shared() {
        let (root, manifests, reference, bytes) = pack_fixture();
        let cache = ImageCache::load_at_start(
            [reference.clone(), reference.clone()],
            &root.path().join("packs"),
            &root.path().join("user-assets"),
            &manifests,
        )
        .unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_bytes(), bytes.len() as u64);
        let first = cache.get(&reference).unwrap().bytes.clone();
        fs::remove_file(root.path().join("packs/ipc-smart/1.0.0/images/alarm.jpg")).unwrap();
        let second = cache.get(&reference).unwrap().bytes.clone();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(&*second, bytes);
    }

    #[test]
    fn image_caches_merge_verified_user_assets_without_reloading_pack_images() {
        let (root, manifests, pack_reference, pack_bytes) = pack_fixture();
        let pack_cache = ImageCache::load_at_start(
            [pack_reference.clone()],
            &root.path().join("packs"),
            &root.path().join("user-assets"),
            &manifests,
        )
        .unwrap();
        let original_pack_bytes = pack_cache.get(&pack_reference).unwrap().bytes.clone();
        let user_bytes = b"user-image";
        let image_id = format!("{:x}", Sha256::digest(user_bytes));
        fs::write(
            root.path()
                .join("user-assets")
                .join(format!("{image_id}.png")),
            user_bytes,
        )
        .unwrap();
        let user_reference = ImageAssetRef::UserAsset {
            image_id: image_id.clone(),
            extension: ImageExtension::Png,
            sha256: image_id,
            size: user_bytes.len() as u64,
        };
        let user_cache = ImageCache::load_at_start(
            [user_reference.clone()],
            &root.path().join("packs"),
            &root.path().join("user-assets"),
            &manifests,
        )
        .unwrap();

        let merged = pack_cache.merged(user_cache).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(
            merged.total_bytes(),
            (pack_bytes.len() + user_bytes.len()) as u64
        );
        assert!(Arc::ptr_eq(
            &original_pack_bytes,
            &merged.get(&pack_reference).unwrap().bytes
        ));
        assert_eq!(&*merged.get(&user_reference).unwrap().bytes, user_bytes);
    }

    #[test]
    fn image_cache_rejects_undeclared_paths_and_size_limits() {
        let (root, manifests, _, _) = pack_fixture();
        let undeclared = ImageAssetRef::Pack {
            pack_id: "ipc-smart".into(),
            version: "1.0.0".into(),
            path: "images/other.jpg".into(),
        };
        assert_eq!(
            ImageCache::load_at_start(
                [undeclared],
                &root.path().join("packs"),
                &root.path().join("user-assets"),
                &manifests,
            )
            .unwrap_err()
            .code,
            "device_simulator.alarm.image_not_declared"
        );

        let oversized = ImageAssetRef::UserAsset {
            image_id: "fixture".into(),
            extension: ImageExtension::Jpg,
            sha256: "a".repeat(64),
            size: MAX_ALARM_IMAGE_BYTES + 1,
        };
        assert_eq!(
            ImageCache::load_at_start(
                [oversized],
                &root.path().join("packs"),
                &root.path().join("user-assets"),
                &manifests,
            )
            .unwrap_err()
            .code,
            "device_simulator.alarm.image_size_invalid"
        );
    }

    #[test]
    fn raw_http_request_uses_injected_fields_source_ip_and_unverified_success() {
        let definition = raw_definition(FirstReleaseProfileId::IpcSmart);
        let context = AlarmBuildContext {
            source_ip: Some(Ipv4Addr::new(10, 0, 0, 8)),
            fields: BTreeMap::from([
                (DynamicField::DeviceId, "device-8".into()),
                (DynamicField::Timestamp, "2026-07-18T12:00:00+08:00".into()),
            ]),
            multipart_boundary: None,
            legacy_values: None,
        };
        let request = build_alarm_request(&definition, &context, &ImageCache::default()).unwrap();
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.path, "/fixture/alarm");
        assert_eq!(request.source_ip, Ipv4Addr::new(10, 0, 0, 8));
        assert_eq!(request.success_rule.evaluate(200), None);
        assert_eq!(
            &*request.body,
            br#"{"device":"device-8","time":"2026-07-18T12:00:00+08:00"}"#
        );
    }

    #[test]
    fn multipart_request_uses_cached_image_and_exact_injected_boundary() {
        let (root, manifests, reference, image_bytes) = pack_fixture();
        let cache = ImageCache::load_at_start(
            [reference.clone()],
            &root.path().join("packs"),
            &root.path().join("user-assets"),
            &manifests,
        )
        .unwrap();
        let mut definition = raw_definition(FirstReleaseProfileId::IpcSmart);
        definition.image_policy = ImagePolicy::Required;
        definition.images.push(ImageAttachmentDefinition {
            reference,
            url_reference: None,
            field_name: "snapshot".into(),
            file_name: "alarm.jpg".into(),
            image_index: Some(1),
        });
        definition.transport.body_encoding = BodyEncoding::Multipart {
            metadata_name: "metadata".into(),
            metadata_content_type: "application/json; charset=utf-8".into(),
        };
        let context = AlarmBuildContext {
            source_ip: Some(Ipv4Addr::new(10, 0, 0, 9)),
            fields: BTreeMap::from([
                (DynamicField::DeviceId, "device-9".into()),
                (DynamicField::Timestamp, "fixed-time".into()),
            ]),
            multipart_boundary: Some("fixture-boundary".into()),
            legacy_values: None,
        };
        let request = build_alarm_request(&definition, &context, &cache).unwrap();
        let body = &*request.body;
        assert!(body.starts_with(b"--fixture-boundary\r\n"));
        assert!(body
            .windows(image_bytes.len())
            .any(|window| window == image_bytes));
        assert!(String::from_utf8_lossy(body)
            .contains("name=\"snapshot\"; imageindex=1; filename=\"alarm.jpg\""));
        assert!(body.ends_with(b"--fixture-boundary--\r\n"));
        assert_eq!(
            request.headers["Content-Type"],
            "multipart/form-data, boundary=fixture-boundary"
        );
    }

    #[test]
    fn missing_dynamic_field_and_invalid_boundary_are_rejected() {
        let mut definition = raw_definition(FirstReleaseProfileId::IpcSmart);
        let mut context = AlarmBuildContext {
            source_ip: Some(Ipv4Addr::LOCALHOST),
            fields: BTreeMap::new(),
            multipart_boundary: None,
            legacy_values: None,
        };
        assert_eq!(
            build_alarm_request(&definition, &context, &ImageCache::default())
                .unwrap_err()
                .code,
            "device_simulator.alarm.template_field_missing"
        );
        context.fields.insert(DynamicField::DeviceId, "x".into());
        context.fields.insert(DynamicField::Timestamp, "x".into());
        context.multipart_boundary = Some("bad\r\nboundary".into());
        definition.transport.body_encoding = BodyEncoding::Multipart {
            metadata_name: "metadata".into(),
            metadata_content_type: "application/json".into(),
        };
        assert_eq!(
            build_alarm_request(&definition, &context, &ImageCache::default())
                .unwrap_err()
                .code,
            "device_simulator.alarm.multipart_token_invalid"
        );
    }

    #[test]
    fn recovery_template_is_explicit_and_optional() {
        let mut definition = raw_definition(FirstReleaseProfileId::IpcSmart);
        let context = AlarmBuildContext {
            source_ip: Some(Ipv4Addr::LOCALHOST),
            fields: BTreeMap::from([
                (DynamicField::DeviceId, "x".into()),
                (DynamicField::Timestamp, "t".into()),
                (DynamicField::AlarmState, "recovered".into()),
            ]),
            multipart_boundary: None,
            legacy_values: None,
        };
        assert!(
            build_recovery_request(&definition, &context, &ImageCache::default())
                .unwrap()
                .is_none()
        );
        definition.recovery = RecoveryDefinition::RenderWith {
            template: CompiledTemplate::compile(br#"{"state":"{{alarm_state}}"}"#).unwrap(),
            transport: definition.transport.clone(),
            trigger: RecoveryTrigger::RequestedDelay,
            include_images: false,
        };
        let request = build_recovery_request(&definition, &context, &ImageCache::default())
            .unwrap()
            .unwrap();
        assert_eq!(&*request.body, br#"{"state":"recovered"}"#);
    }

    #[test]
    fn first_release_scaffolds_are_explicitly_unverified_for_ums() {
        let registry = synthetic_unverified_first_release_registry().unwrap();
        assert_eq!(registry.len(), 6);
        for definition in registry.definitions() {
            assert_eq!(
                definition.evidence.fixture_provenance,
                FixtureProvenance::SyntheticUnverified
            );
            assert!(!definition
                .evidence
                .is_platform_verified(TargetPlatform::Ums));
            assert_eq!(definition.transport.success_rule.evaluate(200), None);
        }
        let ordinary = registry
            .resolve(
                FirstReleaseProfileId::NvrCommon,
                "alarm.nvr_common.v1",
                "synthetic-fixture",
            )
            .unwrap();
        assert_eq!(ordinary.image_policy, ImagePolicy::Forbidden);
        assert!(ordinary.images.is_empty());
    }
}
