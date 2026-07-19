use super::schema::{validate_profile, DeviceProfileV1, ProfileSchemaError};
use super::scope::{FirstReleaseProfileId, FIRST_RELEASE_PROFILES};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Clone)]
pub struct ProfileRegistry {
    profiles: BTreeMap<String, DeviceProfileV1>,
}

impl ProfileRegistry {
    pub fn from_profiles(profiles: Vec<DeviceProfileV1>) -> Result<Self, ProfileSchemaError> {
        let mut registry = Self::default();
        for profile in profiles {
            validate_profile(&profile)?;
            let id = profile.id.clone();
            if registry.profiles.insert(id.clone(), profile).is_some() {
                return Err(error(
                    "device_simulator.validation.profile_duplicate",
                    format!("duplicate profile '{id}'"),
                ));
            }
        }
        Ok(registry)
    }

    pub fn get(&self, id: &str) -> Option<&DeviceProfileV1> {
        self.profiles.get(id)
    }

    pub fn list(&self) -> impl Iterator<Item = &DeviceProfileV1> {
        self.profiles.values()
    }

    pub fn validate_first_release_coverage(&self) -> Result<(), ProfileSchemaError> {
        let expected = FIRST_RELEASE_PROFILES
            .map(FirstReleaseProfileId::as_str)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let actual = self
            .profiles
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if actual != expected {
            let missing = expected.difference(&actual).copied().collect::<Vec<_>>();
            let unexpected = actual.difference(&expected).copied().collect::<Vec<_>>();
            return Err(error(
                "device_simulator.validation.profile_scope_incomplete",
                format!(
                    "first-release profile scope mismatch; missing=[{}], unexpected=[{}]",
                    missing.join(","),
                    unexpected.join(",")
                ),
            ));
        }
        for profile_id in FIRST_RELEASE_PROFILES {
            let profile = self.profiles.get(profile_id.as_str()).ok_or_else(|| {
                error(
                    "device_simulator.validation.profile_scope_incomplete",
                    format!("missing profile '{}'", profile_id.as_str()),
                )
            })?;
            if profile.device_kind != profile_id.device_kind()
                || profile.legacy_device_type != profile_id.legacy_device_type()
            {
                return Err(error(
                    "device_simulator.validation.profile_scope_mismatch",
                    format!(
                        "profile '{}' metadata does not match approved scope",
                        profile.id
                    ),
                ));
            }
        }
        Ok(())
    }
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
    use crate::device_simulator::profiles::schema::ProfileIdentityFacts;
    use crate::device_simulator::profiles::schema::{
        EvidenceStatus, EvidenceTopic, ProfileEvidence, ProfileHandlerBindings,
        PROFILE_SCHEMA_VERSION,
    };
    use crate::device_simulator::profiles::scope::TargetPlatform;

    fn profile(id: FirstReleaseProfileId) -> DeviceProfileV1 {
        DeviceProfileV1 {
            schema_version: PROFILE_SCHEMA_VERSION,
            id: id.as_str().into(),
            device_kind: id.device_kind(),
            legacy_device_type: id.legacy_device_type().into(),
            identity: ProfileIdentityFacts {
                model: "MODEL-STATIC-REVIEW".into(),
                firmware_version: "VERSION-STATIC-REVIEW".into(),
                nickname: "STATIC".into(),
                device_type_enum: matches!(id.device_kind(), DeviceKind::Nvr) as u16,
            },
            supported_platforms: vec![TargetPlatform::Ums],
            handlers: ProfileHandlerBindings {
                identity: "legacy.identity.v1".into(),
                discovery: match id.device_kind() {
                    DeviceKind::Ipc => "ws_discovery.ipc.v1",
                    DeviceKind::Nvr => "ws_discovery.nvr.v1",
                }
                .into(),
                http: "http.profile.v1".into(),
                rtsp: "rtsp.tcp_interleaved.v1".into(),
                alarms: vec![format!("alarm.{}.v1", id.as_str().replace('-', "_"))],
            },
            evidence: [
                (EvidenceTopic::Identity, "script/VSITool.py"),
                (EvidenceTopic::Discovery, "script/Vsocket_ip.py"),
                (EvidenceTopic::Http, "script/HTTPServer.py"),
                (EvidenceTopic::Rtsp, "script/IPCRtspLib.py"),
                (EvidenceTopic::Alarm, "script/AlarmHandler.py"),
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
    fn registry_requires_exact_approved_first_release_coverage() {
        let profiles = FIRST_RELEASE_PROFILES.map(profile).into_iter().collect();
        let registry = ProfileRegistry::from_profiles(profiles).unwrap();
        registry.validate_first_release_coverage().unwrap();
        assert_eq!(registry.list().count(), 6);
        assert!(registry.get("ipc-smart").is_some());
    }

    #[test]
    fn registry_rejects_duplicates_missing_and_metadata_drift() {
        let duplicate = vec![
            profile(FirstReleaseProfileId::IpcSmart),
            profile(FirstReleaseProfileId::IpcSmart),
        ];
        assert_eq!(
            ProfileRegistry::from_profiles(duplicate).unwrap_err().code,
            "device_simulator.validation.profile_duplicate"
        );

        let registry =
            ProfileRegistry::from_profiles(vec![profile(FirstReleaseProfileId::IpcSmart)]).unwrap();
        assert_eq!(
            registry.validate_first_release_coverage().unwrap_err().code,
            "device_simulator.validation.profile_scope_incomplete"
        );

        let mut profiles = FIRST_RELEASE_PROFILES.map(profile);
        profiles[0].legacy_device_type = "错误类型".into();
        let registry = ProfileRegistry::from_profiles(profiles.into_iter().collect()).unwrap();
        assert_eq!(
            registry.validate_first_release_coverage().unwrap_err().code,
            "device_simulator.validation.profile_scope_mismatch"
        );
    }
}
