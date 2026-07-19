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
    NvrCommonV1,
    NvrVehicleV1,
}

impl AlarmHandlerId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CustomV1 => "alarm.custom.v1",
            Self::SmartV1 => "alarm.smart.v1",
            Self::NvrCommonV1 => "alarm.nvr_common.v1",
            Self::NvrVehicleV1 => "alarm.nvr_vehicle.v1",
        }
    }

    pub const fn profile_id(self) -> FirstReleaseProfileId {
        match self {
            Self::CustomV1 => FirstReleaseProfileId::IpcCustom,
            Self::SmartV1 => FirstReleaseProfileId::IpcSmart,
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
    Reference,
    SubscriptionId,
    AlarmState,
    ImageBase64,
    ImageSize,
}

impl DynamicField {
    pub const fn token(self) -> &'static str {
        match self {
            Self::DeviceId => "device_id",
            Self::DeviceIp => "device_ip",
            Self::ChannelId => "channel_id",
            Self::Timestamp => "timestamp",
            Self::Reference => "reference",
            Self::SubscriptionId => "subscription_id",
            Self::AlarmState => "alarm_state",
            Self::ImageBase64 => "image_base64",
            Self::ImageSize => "image_size",
        }
    }

    fn parse(token: &str) -> AlarmResult<Self> {
        match token {
            "device_id" => Ok(Self::DeviceId),
            "device_ip" => Ok(Self::DeviceIp),
            "channel_id" => Ok(Self::ChannelId),
            "timestamp" => Ok(Self::Timestamp),
            "reference" => Ok(Self::Reference),
            "subscription_id" => Ok(Self::SubscriptionId),
            "alarm_state" => Ok(Self::AlarmState),
            "image_base64" => Ok(Self::ImageBase64),
            "image_size" => Ok(Self::ImageSize),
            _ => Err(AlarmError::new(
                "device_simulator.alarm.template_field_unknown",
                format!("unknown alarm template field '{token}'"),
            )),
        }
    }
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
    pub field_name: String,
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportDefinition {
    pub method: HttpMethod,
    pub path: String,
    pub source_binding: SourceBinding,
    pub body_encoding: BodyEncoding,
    pub success_rule: ResponseSuccessRule,
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
                    .map(|image| image.reference.clone())
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

#[derive(Debug, Clone, Default)]
pub struct AlarmBuildContext {
    pub source_ip: Option<Ipv4Addr>,
    pub fields: BTreeMap<DynamicField, String>,
    pub multipart_boundary: Option<String>,
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

pub fn build_alarm_request(
    definition: &AlarmHandlerDefinition,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<HttpAlarmRequest> {
    build_request_with_template(definition, &definition.template, context, image_cache)
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
            include_images,
            ..
        } => {
            if *include_images {
                build_request_with_template(definition, template, context, image_cache).map(Some)
            } else {
                let mut recovery_definition = definition.clone();
                recovery_definition.images.clear();
                recovery_definition.image_policy = ImagePolicy::Forbidden;
                build_request_with_template(&recovery_definition, template, context, image_cache)
                    .map(Some)
            }
        }
    }
}

fn build_request_with_template(
    definition: &AlarmHandlerDefinition,
    template: &CompiledTemplate,
    context: &AlarmBuildContext,
    image_cache: &ImageCache,
) -> AlarmResult<HttpAlarmRequest> {
    validate_definition(definition)?;
    validate_template_image_mapping(definition, template)?;
    let source_ip = context.source_ip.ok_or_else(|| {
        AlarmError::new(
            "device_simulator.alarm.source_ip_missing",
            "alarm request must bind to the simulated device IP",
        )
    })?;
    let mut fields = context.fields.clone();
    if template.fields().contains(&DynamicField::ImageBase64) {
        if definition.images.len() != 1 {
            return Err(AlarmError::new(
                "device_simulator.alarm.embedded_image_count_invalid",
                "embedded Base64 templates require exactly one image",
            ));
        }
        let image = image_cache.get(&definition.images[0].reference)?;
        fields.insert(
            DynamicField::ImageBase64,
            BASE64_STANDARD.encode(&image.bytes),
        );
        fields.insert(DynamicField::ImageSize, image.bytes.len().to_string());
    } else if template.fields().contains(&DynamicField::ImageSize) {
        if definition.images.len() != 1 {
            return Err(AlarmError::new(
                "device_simulator.alarm.embedded_image_count_invalid",
                "image-size templates require exactly one image",
            ));
        }
        let image = image_cache.get(&definition.images[0].reference)?;
        fields.insert(DynamicField::ImageSize, image.bytes.len().to_string());
    }
    let metadata = template.render(&fields)?;
    let (body, content_type) = match &definition.transport.body_encoding {
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
                &definition.images,
                image_cache,
            )?;
            (body, format!("multipart/form-data; boundary={boundary}"))
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
    Ok(HttpAlarmRequest {
        method: definition.transport.method,
        path: definition.transport.path.clone(),
        source_ip,
        headers,
        body: Arc::from(body),
        success_rule: definition.transport.success_rule.clone(),
    })
}

fn build_multipart_body(
    boundary: &str,
    metadata_name: &str,
    metadata_content_type: &str,
    metadata: &[u8],
    images: &[ImageAttachmentDefinition],
    cache: &ImageCache,
) -> AlarmResult<Vec<u8>> {
    validate_multipart_token(metadata_name, "metadata name")?;
    validate_header_value(metadata_content_type, "metadata content type")?;
    let mut body = Vec::new();
    append_part_prefix(
        &mut body,
        boundary,
        metadata_name,
        None,
        metadata_content_type,
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
            image.content_type,
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
    content_type: &str,
) {
    output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    output.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"").as_bytes());
    if let Some(file_name) = file_name {
        output.extend_from_slice(format!("; filename=\"{file_name}\"").as_bytes());
    }
    output.extend_from_slice(format!("\r\nContent-Type: {content_type}\r\n\r\n").as_bytes());
}

fn validate_definition(definition: &AlarmHandlerDefinition) -> AlarmResult<()> {
    if definition.handler_id.profile_id() != definition.profile_id {
        return Err(AlarmError::new(
            "device_simulator.alarm.handler_profile_mismatch",
            "alarm handler is registered for the wrong profile",
        ));
    }
    validate_http_path(&definition.transport.path)?;
    if definition.profile_id == FirstReleaseProfileId::NvrCommon
        && (definition.image_policy != ImagePolicy::Forbidden || !definition.images.is_empty())
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.nvr_common_image_forbidden",
            "ordinary NVR alarms cannot acquire image behavior without approved evidence",
        ));
    }
    if definition.image_policy == ImagePolicy::Forbidden && !definition.images.is_empty() {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_policy_invalid",
            "image attachments exist while the handler forbids images",
        ));
    }
    if definition.image_policy == ImagePolicy::Required && definition.images.is_empty() {
        return Err(AlarmError::new(
            "device_simulator.alarm.image_policy_invalid",
            "image handler requires at least one declared image",
        ));
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
    for image in &definition.images {
        validate_multipart_token(&image.field_name, "image field name")?;
        validate_file_name(&image.file_name)?;
    }
    match &definition.transport.body_encoding {
        BodyEncoding::Raw { content_type } => {
            validate_header_value(content_type, "content type")?;
            if !definition.images.is_empty()
                && !definition
                    .template
                    .fields()
                    .contains(&DynamicField::ImageBase64)
            {
                return Err(AlarmError::new(
                    "device_simulator.alarm.image_mapping_missing",
                    "raw alarm images require an explicit image_base64 template field",
                ));
            }
        }
        BodyEncoding::Multipart {
            metadata_name,
            metadata_content_type,
        } => {
            validate_multipart_token(metadata_name, "metadata name")?;
            validate_header_value(metadata_content_type, "metadata content type")?;
            if definition
                .template
                .fields()
                .contains(&DynamicField::ImageBase64)
            {
                return Err(AlarmError::new(
                    "device_simulator.alarm.image_mapping_ambiguous",
                    "multipart images cannot also use the image_base64 template field",
                ));
            }
        }
    }
    if let ResponseSuccessRule::StatusRange { minimum, maximum } = definition.transport.success_rule
    {
        if minimum < 100 || maximum > 599 || minimum > maximum {
            return Err(AlarmError::new(
                "device_simulator.alarm.success_rule_invalid",
                "HTTP success status range is invalid",
            ));
        }
    }
    Ok(())
}

fn validate_template_image_mapping(
    definition: &AlarmHandlerDefinition,
    template: &CompiledTemplate,
) -> AlarmResult<()> {
    match &definition.transport.body_encoding {
        BodyEncoding::Raw { .. }
            if !definition.images.is_empty()
                && !template.fields().contains(&DynamicField::ImageBase64) =>
        {
            Err(AlarmError::new(
                "device_simulator.alarm.image_mapping_missing",
                "raw alarm images require an explicit image_base64 template field",
            ))
        }
        BodyEncoding::Multipart { .. }
            if template.fields().contains(&DynamicField::ImageBase64) =>
        {
            Err(AlarmError::new(
                "device_simulator.alarm.image_mapping_ambiguous",
                "multipart images cannot also use the image_base64 template field",
            ))
        }
        _ => Ok(()),
    }
}

/// Provides one non-golden scaffold for each approved first-release handler.
/// These definitions must not be presented as platform-compatible.
pub fn synthetic_unverified_first_release_registry() -> AlarmResult<AlarmHandlerRegistry> {
    let mut registry = AlarmHandlerRegistry::default();
    for handler_id in [
        AlarmHandlerId::CustomV1,
        AlarmHandlerId::SmartV1,
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
            recovery: RecoveryDefinition::None,
            evidence: HandlerEvidence {
                legacy_sources: legacy_alarm_sources(profile_id)
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                template_source: "synthetic fixture; real template pending evidence gate".into(),
                fixture_provenance: FixtureProvenance::SyntheticUnverified,
                platforms: [TargetPlatform::Vms, TargetPlatform::Ums]
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
                platform: TargetPlatform::Vms,
                verification: PlatformVerification::SourceConfirmedPlatformUnverified,
            }],
            intentional_changes: vec![],
        }
    }

    fn raw_definition(profile_id: FirstReleaseProfileId) -> AlarmHandlerDefinition {
        let handler_id = match profile_id {
            FirstReleaseProfileId::IpcCustom => AlarmHandlerId::CustomV1,
            FirstReleaseProfileId::IpcSmart => AlarmHandlerId::SmartV1,
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
            field_name: "snapshot".into(),
            file_name: "alarm.jpg".into(),
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
        };
        let request = build_alarm_request(&definition, &context, &cache).unwrap();
        let body = &*request.body;
        assert!(body.starts_with(b"--fixture-boundary\r\n"));
        assert!(body
            .windows(image_bytes.len())
            .any(|window| window == image_bytes));
        assert!(body.ends_with(b"--fixture-boundary--\r\n"));
        assert_eq!(
            request.headers["Content-Type"],
            "multipart/form-data; boundary=fixture-boundary"
        );
    }

    #[test]
    fn missing_dynamic_field_and_invalid_boundary_are_rejected() {
        let mut definition = raw_definition(FirstReleaseProfileId::IpcSmart);
        let mut context = AlarmBuildContext {
            source_ip: Some(Ipv4Addr::LOCALHOST),
            fields: BTreeMap::new(),
            multipart_boundary: None,
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
        };
        assert!(
            build_recovery_request(&definition, &context, &ImageCache::default())
                .unwrap()
                .is_none()
        );
        definition.recovery = RecoveryDefinition::RenderWith {
            template: CompiledTemplate::compile(br#"{"state":"{{alarm_state}}"}"#).unwrap(),
            trigger: RecoveryTrigger::RequestedDelay,
            include_images: false,
        };
        let request = build_recovery_request(&definition, &context, &ImageCache::default())
            .unwrap()
            .unwrap();
        assert_eq!(&*request.body, br#"{"state":"recovered"}"#);
    }

    #[test]
    fn first_release_scaffolds_are_explicitly_unverified_for_vms_and_ums() {
        let registry = synthetic_unverified_first_release_registry().unwrap();
        assert_eq!(registry.len(), 4);
        for definition in registry.definitions() {
            assert_eq!(
                definition.evidence.fixture_provenance,
                FixtureProvenance::SyntheticUnverified
            );
            assert!(!definition
                .evidence
                .is_platform_verified(TargetPlatform::Vms));
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
