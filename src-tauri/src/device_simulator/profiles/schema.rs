use super::scope::{TargetPlatform, FIRST_RELEASE_PLATFORMS};
use crate::device_simulator::assets::catalog::DeviceKind;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const PROFILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceProfileV1 {
    pub schema_version: u32,
    pub id: String,
    pub device_kind: DeviceKind,
    pub legacy_device_type: String,
    pub supported_platforms: Vec<TargetPlatform>,
    pub handlers: ProfileHandlerBindings,
    pub evidence: Vec<ProfileEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileHandlerBindings {
    pub identity: String,
    pub discovery: String,
    pub http: String,
    pub rtsp: String,
    pub alarms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileEvidence {
    pub topic: EvidenceTopic,
    pub status: EvidenceStatus,
    pub sources: Vec<String>,
    #[serde(default)]
    pub verified_platforms: Vec<TargetPlatform>,
    #[serde(default)]
    pub intentional_changes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTopic {
    Identity,
    Discovery,
    Http,
    Rtsp,
    Alarm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    LegacySourceConfirmed,
    RecommendedFallback,
    PlatformVerified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSchemaError {
    pub code: &'static str,
    pub message: String,
}

impl ProfileSchemaError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProfileSchemaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProfileSchemaError {}

pub fn validate_profile(profile: &DeviceProfileV1) -> Result<(), ProfileSchemaError> {
    if profile.schema_version != PROFILE_SCHEMA_VERSION {
        return Err(ProfileSchemaError::new(
            "device_simulator.validation.profile_schema_unsupported",
            format!(
                "unsupported profile schema version {}",
                profile.schema_version
            ),
        ));
    }
    validate_token(&profile.id, "profile id")?;
    if profile.legacy_device_type.trim().is_empty() {
        return Err(ProfileSchemaError::new(
            "device_simulator.validation.profile_legacy_type_missing",
            "legacy device type is required for evidence traceability",
        ));
    }

    validate_platforms(&profile.supported_platforms, "profile")?;
    validate_token(&profile.handlers.identity, "identity handler")?;
    validate_token(&profile.handlers.discovery, "discovery handler")?;
    validate_token(&profile.handlers.http, "HTTP handler")?;
    validate_token(&profile.handlers.rtsp, "RTSP handler")?;
    if profile.handlers.alarms.is_empty() {
        return Err(ProfileSchemaError::new(
            "device_simulator.validation.profile_alarm_handler_missing",
            "first-release profile must declare at least one alarm handler",
        ));
    }
    let mut alarm_handlers = HashSet::new();
    for handler in &profile.handlers.alarms {
        validate_token(handler, "alarm handler")?;
        if !alarm_handlers.insert(handler) {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_handler_duplicate",
                format!("duplicate alarm handler '{handler}'"),
            ));
        }
    }

    let mut topics = HashSet::new();
    for evidence in &profile.evidence {
        if !topics.insert(evidence.topic) {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_evidence_duplicate",
                format!("duplicate evidence topic '{:?}'", evidence.topic),
            ));
        }
        if evidence.sources.is_empty() {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_evidence_source_missing",
                format!("evidence topic '{:?}' has no sources", evidence.topic),
            ));
        }
        for source in &evidence.sources {
            validate_evidence_source(source)?;
        }
        validate_platforms(&evidence.verified_platforms, "evidence")?;
        if evidence.status == EvidenceStatus::PlatformVerified
            && evidence.verified_platforms.is_empty()
        {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_verification_missing",
                "platform_verified evidence must name at least one verified platform",
            ));
        }
        if evidence
            .verified_platforms
            .iter()
            .any(|platform| !profile.supported_platforms.contains(platform))
        {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_verification_platform_mismatch",
                "evidence references a platform outside the profile scope",
            ));
        }
    }
    for topic in [
        EvidenceTopic::Identity,
        EvidenceTopic::Discovery,
        EvidenceTopic::Http,
        EvidenceTopic::Rtsp,
        EvidenceTopic::Alarm,
    ] {
        if !topics.contains(&topic) {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_evidence_incomplete",
                format!("profile is missing '{topic:?}' evidence"),
            ));
        }
    }
    Ok(())
}

fn validate_platforms(
    platforms: &[TargetPlatform],
    subject: &str,
) -> Result<(), ProfileSchemaError> {
    if subject == "profile" && platforms.is_empty() {
        return Err(ProfileSchemaError::new(
            "device_simulator.validation.profile_platform_missing",
            "profile must support at least one approved target platform",
        ));
    }
    let mut unique = HashSet::new();
    for platform in platforms {
        if !FIRST_RELEASE_PLATFORMS.contains(platform) {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_platform_out_of_scope",
                format!("{subject} contains a platform outside the first-release scope"),
            ));
        }
        if !unique.insert(platform) {
            return Err(ProfileSchemaError::new(
                "device_simulator.validation.profile_platform_duplicate",
                format!("{subject} contains a duplicate platform"),
            ));
        }
    }
    Ok(())
}

fn validate_token(value: &str, subject: &str) -> Result<(), ProfileSchemaError> {
    if value.is_empty()
        || value.len() > 96
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(ProfileSchemaError::new(
            "device_simulator.validation.profile_token_invalid",
            format!("{subject} must be a lowercase identifier"),
        ));
    }
    Ok(())
}

fn validate_evidence_source(source: &str) -> Result<(), ProfileSchemaError> {
    if source.is_empty()
        || source.len() > 512
        || source.contains('\0')
        || source.contains('\\')
        || source.starts_with('/')
        || source.contains(':')
        || source
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(ProfileSchemaError::new(
            "device_simulator.validation.profile_evidence_path_invalid",
            format!("evidence source must be a normalized relative path: '{source}'"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> DeviceProfileV1 {
        DeviceProfileV1 {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: "ipc-smart".into(),
            device_kind: DeviceKind::Ipc,
            legacy_device_type: "智能相机".into(),
            supported_platforms: vec![TargetPlatform::Vms, TargetPlatform::Ums],
            handlers: ProfileHandlerBindings {
                identity: "legacy.identity.v1".into(),
                discovery: "ws_discovery.ipc.v1".into(),
                http: "http.smart_ipc.v1".into(),
                rtsp: "rtsp.tcp_interleaved.v1".into(),
                alarms: vec!["alarm.smart.v1".into()],
            },
            evidence: [
                (EvidenceTopic::Identity, "script/VSITool.py"),
                (EvidenceTopic::Discovery, "script/Vsocket_ip.py"),
                (EvidenceTopic::Http, "script/HTTPServer.py"),
                (EvidenceTopic::Rtsp, "script/IPCRtspLib.py"),
                (EvidenceTopic::Alarm, "script/SmartAlarm.py"),
            ]
            .into_iter()
            .map(|(topic, source)| ProfileEvidence {
                topic,
                status: EvidenceStatus::LegacySourceConfirmed,
                sources: vec![source.into()],
                verified_platforms: vec![],
                intentional_changes: vec![],
            })
            .collect(),
        }
    }

    #[test]
    fn accepts_traceable_first_release_profile() {
        assert!(validate_profile(&profile()).is_ok());
        let encoded = serde_json::to_string(&profile()).unwrap();
        let decoded: DeviceProfileV1 = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, profile());
    }

    #[test]
    fn rejects_missing_evidence_and_unsafe_source_paths() {
        let mut candidate = profile();
        candidate.evidence.pop();
        assert_eq!(
            validate_profile(&candidate).unwrap_err().code,
            "device_simulator.validation.profile_evidence_incomplete"
        );

        let mut candidate = profile();
        candidate.evidence[0].sources = vec!["../secrets.ini".into()];
        assert_eq!(
            validate_profile(&candidate).unwrap_err().code,
            "device_simulator.validation.profile_evidence_path_invalid"
        );
    }

    #[test]
    fn platform_verified_status_requires_matching_platforms() {
        let mut candidate = profile();
        candidate.evidence[0].status = EvidenceStatus::PlatformVerified;
        assert_eq!(
            validate_profile(&candidate).unwrap_err().code,
            "device_simulator.validation.profile_verification_missing"
        );
        candidate.evidence[0].verified_platforms = vec![TargetPlatform::Vms];
        assert!(validate_profile(&candidate).is_ok());
    }
}
