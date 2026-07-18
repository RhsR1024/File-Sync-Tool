use super::schema::{validate_profile, DeviceProfileV1, ProfileSchemaError};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_PROFILE_BYTES: u64 = 1024 * 1024;

const KNOWN_IDENTITY_HANDLERS: &[&str] = &["legacy.identity.v1"];
const KNOWN_DISCOVERY_HANDLERS: &[&str] = &["ws_discovery.ipc.v1", "ws_discovery.nvr.v1"];
const KNOWN_HTTP_HANDLERS: &[&str] = &[
    "http.custom_ipc.v1",
    "http.smart_ipc.v1",
    "http.nvr_common.v1",
    "http.nvr_vehicle.v1",
];
const KNOWN_RTSP_HANDLERS: &[&str] = &["rtsp.tcp_interleaved.v1"];
const KNOWN_ALARM_HANDLERS: &[&str] = &[
    "alarm.custom.v1",
    "alarm.smart.v1",
    "alarm.nvr_common.v1",
    "alarm.nvr_vehicle.v1",
];

pub fn load_profile_from_pack(
    pack_directory: &Path,
    expected_id: &str,
) -> Result<DeviceProfileV1, ProfileSchemaError> {
    if expected_id.is_empty()
        || !expected_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(error(
            "device_simulator.validation.profile_id_invalid",
            "profile id is not a safe catalog identifier",
        ));
    }
    let path = pack_directory
        .join("profiles")
        .join(format!("{expected_id}.json"));
    ensure_profile_path(pack_directory, &path)?;
    let metadata = fs::metadata(&path).map_err(|source| {
        error(
            "device_simulator.validation.profile_read_failed",
            format!("failed to inspect profile '{}': {source}", path.display()),
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PROFILE_BYTES {
        return Err(error(
            "device_simulator.validation.profile_size_invalid",
            "profile must be a regular JSON file no larger than 1 MiB",
        ));
    }
    let bytes = fs::read(&path).map_err(|source| {
        error(
            "device_simulator.validation.profile_read_failed",
            format!("failed to read profile '{}': {source}", path.display()),
        )
    })?;
    let profile: DeviceProfileV1 = serde_json::from_slice(&bytes).map_err(|source| {
        error(
            "device_simulator.validation.profile_json_invalid",
            format!("profile JSON is invalid: {source}"),
        )
    })?;
    validate_profile(&profile)?;
    if profile.id != expected_id {
        return Err(error(
            "device_simulator.validation.profile_identity_mismatch",
            format!("profile '{}' was loaded as '{expected_id}'", profile.id),
        ));
    }
    validate_known_handlers(&profile)?;
    Ok(profile)
}

fn validate_known_handlers(profile: &DeviceProfileV1) -> Result<(), ProfileSchemaError> {
    for (kind, handler, known) in [
        (
            "identity",
            profile.handlers.identity.as_str(),
            KNOWN_IDENTITY_HANDLERS,
        ),
        (
            "discovery",
            profile.handlers.discovery.as_str(),
            KNOWN_DISCOVERY_HANDLERS,
        ),
        ("HTTP", profile.handlers.http.as_str(), KNOWN_HTTP_HANDLERS),
        ("RTSP", profile.handlers.rtsp.as_str(), KNOWN_RTSP_HANDLERS),
    ] {
        if !known.contains(&handler) {
            return Err(error(
                "device_simulator.validation.profile_handler_unknown",
                format!("unknown {kind} handler '{handler}'"),
            ));
        }
    }
    for handler in &profile.handlers.alarms {
        if !KNOWN_ALARM_HANDLERS.contains(&handler.as_str()) {
            return Err(error(
                "device_simulator.validation.profile_handler_unknown",
                format!("unknown alarm handler '{handler}'"),
            ));
        }
    }
    Ok(())
}

fn ensure_profile_path(root: &Path, path: &Path) -> Result<(), ProfileSchemaError> {
    let relative: PathBuf = path
        .strip_prefix(root)
        .map_err(|_| {
            error(
                "device_simulator.validation.profile_path_invalid",
                "profile path escaped its pack directory",
            )
        })?
        .into();
    if relative.components().count() != 2 {
        return Err(error(
            "device_simulator.validation.profile_path_invalid",
            "profile must be stored directly under profiles/",
        ));
    }
    Ok(())
}

fn error(code: &'static str, message: impl Into<String>) -> ProfileSchemaError {
    ProfileSchemaError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::DeviceKind;
    use crate::device_simulator::profiles::schema::{
        EvidenceStatus, EvidenceTopic, ProfileEvidence, ProfileHandlerBindings,
        PROFILE_SCHEMA_VERSION,
    };
    use crate::device_simulator::profiles::scope::TargetPlatform;
    use tempfile::TempDir;

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
    fn loads_only_expected_profiles_with_compiled_handlers() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("profiles")).unwrap();
        fs::write(
            root.path().join("profiles/ipc-smart.json"),
            serde_json::to_vec(&profile()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_profile_from_pack(root.path(), "ipc-smart").unwrap(),
            profile()
        );
    }

    #[test]
    fn rejects_unknown_handlers_and_identity_drift() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("profiles")).unwrap();
        let mut candidate = profile();
        candidate.handlers.http = "http.downloaded_code.v1".into();
        fs::write(
            root.path().join("profiles/ipc-smart.json"),
            serde_json::to_vec(&candidate).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_profile_from_pack(root.path(), "ipc-smart")
                .unwrap_err()
                .code,
            "device_simulator.validation.profile_handler_unknown"
        );

        candidate = profile();
        candidate.id = "ipc-custom".into();
        fs::write(
            root.path().join("profiles/ipc-smart.json"),
            serde_json::to_vec(&candidate).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load_profile_from_pack(root.path(), "ipc-smart")
                .unwrap_err()
                .code,
            "device_simulator.validation.profile_identity_mismatch"
        );
    }
}
