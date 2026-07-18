use std::{fmt, str::FromStr};

use semver::Version;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// The versioned catalog served by the device-simulator asset server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogV1 {
    pub schema_version: u32,
    pub generated_at: String,
    pub engine_api: u32,
    pub packs: Vec<CatalogPack>,
    pub profiles: Vec<CatalogProfile>,
}

/// A downloadable, immutable pack advertised by the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPack {
    pub id: String,
    #[serde(with = "version_serde")]
    pub version: Version,
    pub kind: PackKind,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub unpacked_size: u64,
    pub dependencies: Vec<PackRef>,
    #[serde(with = "version_serde")]
    pub min_app_version: Version,
}

/// A selectable device profile and the packs needed to run it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogProfile {
    pub id: String,
    pub device_kind: DeviceKind,
    pub required_packs: Vec<PackRef>,
}

/// The manifest stored as `pack.json` at the root of each pack archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    pub schema_version: u32,
    pub id: String,
    #[serde(with = "version_serde")]
    pub version: Version,
    pub engine_api: u32,
    pub usage: PackUsagePolicy,
    pub files: Vec<PackFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackUsagePolicy {
    pub scope: PackUsageScope,
    pub notice: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackUsageScope {
    NonCommercial,
}

pub fn non_commercial_usage() -> PackUsagePolicy {
    PackUsagePolicy {
        scope: PackUsageScope::NonCommercial,
        notice: "Authorized for testing, learning, copying, and packaging; commercial use is prohibited."
            .into(),
    }
}

/// One file declared by a pack manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackKind {
    ProtocolCore,
    Media,
    DeviceProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    Ipc,
    Nvr,
}

/// An exact pack identity in the catalog dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackRef {
    pub id: String,
    pub version: Version,
}

impl fmt::Display for PackRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.id, self.version)
    }
}

impl FromStr for PackRef {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (id, version) = value
            .split_once('@')
            .ok_or_else(|| "pack reference must use <id>@<semver> format".to_string())?;

        if id.is_empty() {
            return Err("pack reference id must not be empty".to_string());
        }
        if version.is_empty() || version.contains('@') {
            return Err("pack reference must use <id>@<semver> format".to_string());
        }

        let version = Version::parse(version)
            .map_err(|error| format!("invalid pack reference version: {error}"))?;

        Ok(Self {
            id: id.to_string(),
            version,
        })
    }
}

impl Serialize for PackRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PackRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

mod version_serde {
    use super::*;

    pub fn serialize<S>(version: &Version, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(version)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Version, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Version::parse(&value).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CATALOG_JSON: &str = r#"
    {
      "schema_version": 1,
      "generated_at": "2026-07-18T12:00:00+08:00",
      "engine_api": 1,
      "packs": [
        {
          "id": "ipc-custom",
          "version": "1.0.0",
          "kind": "device-profile",
          "url": "packs/ipc-custom/1.0.0/ipc-custom-1.0.0.zip",
          "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
          "size": 5943210,
          "unpacked_size": 7210340,
          "dependencies": [
            "protocol-core@1.0.0",
            "media-h264-live@1.0.0"
          ],
          "min_app_version": "1.2.0"
        }
      ],
      "profiles": [
        {
          "id": "ipc-custom",
          "device_kind": "ipc",
          "required_packs": ["ipc-custom@1.0.0"]
        }
      ]
    }
    "#;

    #[test]
    fn catalog_example_round_trips() {
        let catalog: CatalogV1 = serde_json::from_str(CATALOG_JSON).unwrap();

        assert_eq!(catalog.packs[0].version, Version::new(1, 0, 0));
        assert_eq!(catalog.packs[0].kind, PackKind::DeviceProfile);
        assert_eq!(catalog.profiles[0].device_kind, DeviceKind::Ipc);
        assert_eq!(
            catalog.packs[0].dependencies[0].to_string(),
            "protocol-core@1.0.0"
        );

        let expected: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        let serialized = serde_json::to_value(catalog).unwrap();
        assert_eq!(serialized, expected);
    }

    #[test]
    fn pack_manifest_example_round_trips() {
        let source = json!({
            "schema_version": 1,
            "id": "ipc-custom",
            "version": "1.0.0",
            "engine_api": 1,
            "usage": {
                "scope": "non-commercial",
                "notice": "Authorized for testing, learning, copying, and packaging; commercial use is prohibited."
            },
            "files": [{
                "path": "profiles/ipc-custom.json",
                "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "size": 1234
            }]
        });

        let manifest: PackManifest = serde_json::from_value(source.clone()).unwrap();
        assert_eq!(serde_json::to_value(manifest).unwrap(), source);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value: serde_json::Value = serde_json::from_str(CATALOG_JSON).unwrap();
        value["packs"][0]["executable"] = json!("payload.exe");

        let error = serde_json::from_value::<CatalogV1>(value).unwrap_err();
        assert!(error.to_string().contains("unknown field `executable`"));

        let manifest = json!({
            "schema_version": 1,
            "id": "protocol-core",
            "version": "1.0.0",
            "engine_api": 1,
            "usage": {
                "scope": "non-commercial",
                "notice": "Authorized for testing, learning, copying, and packaging; commercial use is prohibited."
            },
            "files": [{
                "path": "profiles/schema.json",
                "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "size": 42,
                "source": "untrusted"
            }]
        });
        assert!(serde_json::from_value::<PackManifest>(manifest).is_err());
    }

    #[test]
    fn pack_ref_accepts_exact_id_and_semver_format() {
        let reference: PackRef = "media-h264-live@1.2.3-beta.1+build.7".parse().unwrap();

        assert_eq!(reference.id, "media-h264-live");
        assert_eq!(
            reference.version,
            Version::parse("1.2.3-beta.1+build.7").unwrap()
        );
        assert_eq!(
            reference.to_string(),
            "media-h264-live@1.2.3-beta.1+build.7"
        );
        assert_eq!(
            serde_json::to_string(&reference).unwrap(),
            "\"media-h264-live@1.2.3-beta.1+build.7\""
        );
    }

    #[test]
    fn pack_ref_rejects_invalid_formats() {
        for invalid in [
            "protocol-core",
            "@1.0.0",
            "protocol-core@",
            "protocol-core@latest",
            "protocol-core@1.0.0@extra",
        ] {
            assert!(invalid.parse::<PackRef>().is_err(), "accepted {invalid:?}");
        }
    }
}
