use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};

use chrono::DateTime;
use semver::Version;

use super::catalog::{non_commercial_usage, CatalogPack, CatalogV1, PackManifest, PackRef};

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;
pub const SUPPORTED_ENGINE_API: u32 = 1;

/// Hard limits applied before a pack is downloaded or unpacked.
pub const MAX_PACK_SIZE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_UNPACKED_SIZE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_FILE_SIZE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_FILES_PER_PACK: usize = 10_000;

const MAX_ID_LENGTH: usize = 64;
const MAX_PATH_LENGTH: usize = 512;
const MAX_PATH_SEGMENT_LENGTH: usize = 255;
const FORBIDDEN_EXTENSIONS: &[&str] = &[
    "exe", "dll", "py", "js", "bat", "cmd", "ps1", "wasm", "msi", "scr", "com",
];

/// A machine-readable validation failure. `code` is stable and `message` is
/// intended for diagnostics rather than program flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetValidationError {
    pub code: &'static str,
    pub message: String,
}

impl AssetValidationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for AssetValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for AssetValidationError {}

type ValidationResult = Result<(), AssetValidationError>;
type PackKey = (String, Version);

/// Validates a catalog without performing network or filesystem operations.
pub fn validate_catalog(catalog: &CatalogV1, current_app_version: &Version) -> ValidationResult {
    validate_schema(catalog.schema_version, "catalog")?;
    validate_engine_api(catalog.engine_api, "catalog")?;

    DateTime::parse_from_rfc3339(&catalog.generated_at).map_err(|error| {
        AssetValidationError::new(
            "device_simulator.assets.invalid_generated_at",
            format!("catalog generated_at must be RFC 3339: {error}"),
        )
    })?;

    let mut packs = HashMap::<PackKey, &CatalogPack>::new();
    for pack in &catalog.packs {
        validate_id(&pack.id, "pack")?;
        validate_version(&pack.version, "pack")?;
        validate_version(&pack.min_app_version, "minimum application")?;
        validate_pack_url(&pack.url)?;
        validate_sha256(&pack.sha256, "pack")?;
        validate_size(pack.size, MAX_PACK_SIZE_BYTES, "pack compressed")?;
        validate_size(pack.unpacked_size, MAX_UNPACKED_SIZE_BYTES, "pack unpacked")?;

        if pack.min_app_version > *current_app_version {
            return Err(AssetValidationError::new(
                "device_simulator.assets.app_version_unsupported",
                format!(
                    "pack {}@{} requires application {}, current application is {}",
                    pack.id, pack.version, pack.min_app_version, current_app_version
                ),
            ));
        }

        let key = (pack.id.clone(), pack.version.clone());
        if packs.insert(key, pack).is_some() {
            return Err(AssetValidationError::new(
                "device_simulator.assets.duplicate_pack",
                format!("duplicate pack {}@{}", pack.id, pack.version),
            ));
        }

        validate_unique_refs(&pack.dependencies, "dependency", &pack.id)?;
        for dependency in &pack.dependencies {
            validate_pack_ref(dependency, "dependency")?;
        }
    }

    for pack in &catalog.packs {
        for dependency in &pack.dependencies {
            ensure_ref_exists(dependency, &packs, "dependency")?;
        }
    }
    validate_dependency_cycles(&packs)?;

    let mut profile_ids = HashSet::new();
    for profile in &catalog.profiles {
        validate_id(&profile.id, "profile")?;
        if !profile_ids.insert(profile.id.as_str()) {
            return Err(AssetValidationError::new(
                "device_simulator.assets.duplicate_profile",
                format!("duplicate profile {}", profile.id),
            ));
        }

        validate_unique_refs(&profile.required_packs, "required pack", &profile.id)?;
        for required in &profile.required_packs {
            validate_pack_ref(required, "required pack")?;
            ensure_ref_exists(required, &packs, "required pack")?;
        }
    }

    Ok(())
}

/// Validates a pack's internal manifest against its catalog entry.
pub fn validate_pack_manifest(manifest: &PackManifest, expected: &CatalogPack) -> ValidationResult {
    validate_schema(manifest.schema_version, "pack manifest")?;
    validate_engine_api(manifest.engine_api, "pack manifest")?;
    validate_id(&manifest.id, "pack manifest")?;
    validate_version(&manifest.version, "pack manifest")?;
    if manifest.usage != non_commercial_usage() {
        return Err(AssetValidationError::new(
            "device_simulator.assets.usage_policy_invalid",
            "first-release packs must preserve the approved non-commercial usage notice",
        ));
    }

    if manifest.id != expected.id || manifest.version != expected.version {
        return Err(AssetValidationError::new(
            "device_simulator.assets.manifest_identity_mismatch",
            format!(
                "manifest {}@{} does not match catalog pack {}@{}",
                manifest.id, manifest.version, expected.id, expected.version
            ),
        ));
    }
    if manifest.files.len() > MAX_FILES_PER_PACK {
        return Err(AssetValidationError::new(
            "device_simulator.assets.too_many_files",
            format!(
                "manifest contains {} files, maximum is {}",
                manifest.files.len(),
                MAX_FILES_PER_PACK
            ),
        ));
    }

    let mut paths = HashSet::new();
    let mut declared_size = 0_u64;
    for file in &manifest.files {
        validate_pack_path(&file.path)?;
        validate_sha256(&file.sha256, "pack file")?;
        validate_size(file.size, MAX_FILE_SIZE_BYTES, "pack file")?;

        // Installation targets Windows, whose normal filesystem lookup is
        // case-insensitive. Reject names which would overwrite each other.
        let path_key = file.path.to_ascii_lowercase();
        if !paths.insert(path_key) {
            return Err(AssetValidationError::new(
                "device_simulator.assets.duplicate_file",
                format!("duplicate manifest file path {}", file.path),
            ));
        }

        declared_size = declared_size.checked_add(file.size).ok_or_else(|| {
            AssetValidationError::new(
                "device_simulator.assets.size_limit_exceeded",
                "manifest file sizes overflow u64",
            )
        })?;
    }

    if declared_size > expected.unpacked_size || declared_size > MAX_UNPACKED_SIZE_BYTES {
        return Err(AssetValidationError::new(
            "device_simulator.assets.size_limit_exceeded",
            format!(
                "manifest declares {declared_size} bytes, catalog unpacked size is {}",
                expected.unpacked_size
            ),
        ));
    }

    Ok(())
}

/// Validates a ZIP entry or manifest path using platform-independent rules.
/// Archive extraction must separately reject entries whose ZIP metadata marks
/// them as symbolic links; a path string alone cannot establish file type.
pub fn validate_pack_path(path: &str) -> ValidationResult {
    if path.is_empty() || path.len() > MAX_PATH_LENGTH {
        return invalid_path(path, "path is empty or too long");
    }
    if path.starts_with('/') || path.starts_with("//") {
        return invalid_path(path, "absolute and UNC paths are not allowed");
    }
    if path.contains('\\') {
        return invalid_path(path, "backslashes are not allowed");
    }
    if path
        .bytes()
        .any(|byte| byte == 0 || byte < 0x20 || byte == 0x7f)
    {
        return invalid_path(path, "NUL and control characters are not allowed");
    }

    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        if component.is_empty() || component == "." || component == ".." {
            return invalid_path(path, "path must be relative and normalized");
        }
        if component.len() > MAX_PATH_SEGMENT_LENGTH {
            return invalid_path(path, "path segment is too long");
        }
        if component.ends_with('.') || component.ends_with(' ') {
            return invalid_path(
                path,
                "Windows-normalized trailing dots or spaces are not allowed",
            );
        }
        if component
            .chars()
            .any(|character| matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            return invalid_path(path, "Windows-reserved path characters are not allowed");
        }
        if is_windows_device_name(component) {
            return invalid_path(path, "Windows device names are not allowed");
        }

        if components.peek().is_none() {
            validate_file_extension(component)?;
        }
    }

    Ok(())
}

fn validate_schema(version: u32, subject: &str) -> ValidationResult {
    if version != SUPPORTED_SCHEMA_VERSION {
        return Err(AssetValidationError::new(
            "device_simulator.assets.schema_unsupported",
            format!(
                "{subject} schema version {version} is unsupported; expected {SUPPORTED_SCHEMA_VERSION}"
            ),
        ));
    }
    Ok(())
}

fn validate_engine_api(version: u32, subject: &str) -> ValidationResult {
    if version != SUPPORTED_ENGINE_API {
        return Err(AssetValidationError::new(
            "device_simulator.assets.engine_api_unsupported",
            format!(
                "{subject} engine API {version} is unsupported; expected {SUPPORTED_ENGINE_API}"
            ),
        ));
    }
    Ok(())
}

fn validate_id(id: &str, subject: &str) -> ValidationResult {
    let bytes = id.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= MAX_ID_LENGTH
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');

    if !valid {
        return Err(AssetValidationError::new(
            "device_simulator.assets.invalid_id",
            format!(
                "{subject} id {id:?} must be 1-{MAX_ID_LENGTH} lowercase ASCII letters, digits, or internal hyphens and start with a letter"
            ),
        ));
    }
    Ok(())
}

fn validate_version(version: &Version, subject: &str) -> ValidationResult {
    // Versions arrive as `semver::Version`; checking the canonical text also
    // bounds attacker-controlled prerelease/build metadata.
    let canonical = version.to_string();
    if canonical.len() > 128 || Version::parse(&canonical).is_err() {
        return Err(AssetValidationError::new(
            "device_simulator.assets.invalid_version",
            format!("{subject} version is not a bounded semantic version"),
        ));
    }
    Ok(())
}

fn validate_pack_ref(reference: &PackRef, subject: &str) -> ValidationResult {
    validate_id(&reference.id, subject)?;
    validate_version(&reference.version, subject)
}

fn validate_unique_refs(references: &[PackRef], subject: &str, owner: &str) -> ValidationResult {
    let mut seen = HashSet::new();
    for reference in references {
        let key = (reference.id.as_str(), &reference.version);
        if !seen.insert(key) {
            return Err(AssetValidationError::new(
                "device_simulator.assets.duplicate_pack_ref",
                format!("{owner} repeats {subject} {reference}"),
            ));
        }
    }
    Ok(())
}

fn ensure_ref_exists(
    reference: &PackRef,
    packs: &HashMap<PackKey, &CatalogPack>,
    subject: &str,
) -> ValidationResult {
    let key = (reference.id.clone(), reference.version.clone());
    if !packs.contains_key(&key) {
        return Err(AssetValidationError::new(
            "device_simulator.assets.pack_ref_missing",
            format!("{subject} {reference} does not exist in the catalog"),
        ));
    }
    Ok(())
}

fn validate_dependency_cycles(packs: &HashMap<PackKey, &CatalogPack>) -> ValidationResult {
    fn visit(
        key: &PackKey,
        packs: &HashMap<PackKey, &CatalogPack>,
        states: &mut HashMap<PackKey, u8>,
    ) -> ValidationResult {
        match states.get(key) {
            Some(1) => {
                return Err(AssetValidationError::new(
                    "device_simulator.assets.dependency_cycle",
                    format!("dependency cycle includes {}@{}", key.0, key.1),
                ));
            }
            Some(2) => return Ok(()),
            _ => {}
        }

        states.insert(key.clone(), 1);
        if let Some(pack) = packs.get(key) {
            for dependency in &pack.dependencies {
                let dependency_key = (dependency.id.clone(), dependency.version.clone());
                visit(&dependency_key, packs, states)?;
            }
        }
        states.insert(key.clone(), 2);
        Ok(())
    }

    let mut states = HashMap::new();
    for key in packs.keys() {
        visit(key, packs, &mut states)?;
    }
    Ok(())
}

fn validate_sha256(value: &str, subject: &str) -> ValidationResult {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AssetValidationError::new(
            "device_simulator.assets.invalid_sha256",
            format!("{subject} SHA-256 must be exactly 64 lowercase hexadecimal characters"),
        ));
    }
    Ok(())
}

fn validate_size(size: u64, maximum: u64, subject: &str) -> ValidationResult {
    if size == 0 || size > maximum {
        return Err(AssetValidationError::new(
            "device_simulator.assets.size_limit_exceeded",
            format!("{subject} size {size} must be between 1 and {maximum} bytes"),
        ));
    }
    Ok(())
}

fn validate_pack_url(value: &str) -> ValidationResult {
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return invalid_url("URL is empty, padded, or contains control characters");
    }

    match reqwest::Url::parse(value) {
        Ok(url) => {
            if !matches!(url.scheme(), "http" | "https") {
                return invalid_url("absolute pack URLs must use HTTP or HTTPS");
            }
            if url.host_str().is_none() {
                return invalid_url("absolute pack URL must have a host");
            }
            if !url.username().is_empty() || url.password().is_some() {
                return invalid_url("pack URL credentials are not allowed");
            }
            if url.fragment().is_some() {
                return invalid_url("pack URL fragments are not allowed");
            }

            let scheme_separator = value.find("://").ok_or_else(|| {
                AssetValidationError::new(
                    "device_simulator.assets.invalid_url",
                    "absolute URL lacks ://",
                )
            })?;
            let after_scheme = &value[scheme_separator + 3..];
            let authority_end = after_scheme
                .find(|character| matches!(character, '/' | '?' | '#'))
                .unwrap_or(after_scheme.len());
            let authority = &after_scheme[..authority_end];
            if authority.contains('@') || authority.contains('\\') {
                return invalid_url("pack URL authority contains credentials or backslashes");
            }

            let raw_path_and_suffix = &after_scheme[authority_end..];
            let raw_path_end = raw_path_and_suffix
                .find(|character| matches!(character, '?' | '#'))
                .unwrap_or(raw_path_and_suffix.len());
            let raw_path = &raw_path_and_suffix[..raw_path_end];
            let relative_path = raw_path.strip_prefix('/').unwrap_or(raw_path);
            validate_decoded_url_path(relative_path)
        }
        Err(_) => {
            if value.contains('?') || value.contains('#') {
                return invalid_url("relative pack URLs cannot contain query strings or fragments");
            }
            validate_decoded_url_path(value)
        }
    }
}

fn validate_decoded_url_path(value: &str) -> ValidationResult {
    let mut decoded = value.to_string();
    for _ in 0..3 {
        let next = percent_decode(&decoded)?;
        if next == decoded {
            break;
        }
        decoded = next;
    }
    if contains_percent_escape(&decoded) {
        return invalid_url("multiply encoded URL path is not allowed");
    }

    validate_pack_path(&decoded).map_err(|error| {
        AssetValidationError::new(
            "device_simulator.assets.invalid_url",
            format!("unsafe pack URL path: {}", error.message),
        )
    })?;
    if !decoded.to_ascii_lowercase().ends_with(".zip") {
        return invalid_url("pack URL path must name a .zip file");
    }
    Ok(())
}

fn percent_decode(value: &str) -> Result<String, AssetValidationError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(AssetValidationError::new(
                "device_simulator.assets.invalid_url",
                "URL path contains an incomplete percent escape",
            ));
        }
        let high = hex_value(bytes[index + 1]);
        let low = hex_value(bytes[index + 2]);
        let (Some(high), Some(low)) = (high, low) else {
            return Err(AssetValidationError::new(
                "device_simulator.assets.invalid_url",
                "URL path contains an invalid percent escape",
            ));
        };
        output.push(high * 16 + low);
        index += 3;
    }

    String::from_utf8(output).map_err(|_| {
        AssetValidationError::new(
            "device_simulator.assets.invalid_url",
            "URL path is not valid UTF-8",
        )
    })
}

fn contains_percent_escape(value: &str) -> bool {
    value.as_bytes().windows(3).any(|window| {
        window[0] == b'%' && hex_value(window[1]).is_some() && hex_value(window[2]).is_some()
    })
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn validate_file_extension(file_name: &str) -> ValidationResult {
    let extension = file_name.rsplit_once('.').map(|(_, extension)| extension);
    if extension.is_some_and(|extension| {
        FORBIDDEN_EXTENSIONS
            .iter()
            .any(|forbidden| extension.eq_ignore_ascii_case(forbidden))
    }) {
        return Err(AssetValidationError::new(
            "device_simulator.assets.forbidden_file_type",
            format!("executable asset file type is forbidden: {file_name}"),
        ));
    }
    Ok(())
}

fn is_windows_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let stem_bytes = stem.as_bytes();
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "con" | "prn" | "aux" | "nul"
    ) || (stem_bytes.len() == 4
        && (stem_bytes[..3].eq_ignore_ascii_case(b"com")
            || stem_bytes[..3].eq_ignore_ascii_case(b"lpt"))
        && matches!(stem_bytes[3], b'1'..=b'9'))
}

fn invalid_path(path: &str, reason: &str) -> ValidationResult {
    Err(AssetValidationError::new(
        "device_simulator.assets.invalid_pack_path",
        format!("invalid pack path {path:?}: {reason}"),
    ))
}

fn invalid_url(reason: &str) -> ValidationResult {
    Err(AssetValidationError::new(
        "device_simulator.assets.invalid_url",
        reason,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{
        non_commercial_usage, CatalogProfile, DeviceKind, PackFile, PackKind,
    };

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn pack(id: &str, dependencies: &[(&str, &str)]) -> CatalogPack {
        CatalogPack {
            id: id.to_string(),
            version: Version::new(1, 0, 0),
            kind: PackKind::DeviceProfile,
            url: format!("packs/{id}/1.0.0/{id}-1.0.0.zip"),
            sha256: HASH.to_string(),
            size: 1024,
            unpacked_size: 2048,
            dependencies: dependencies
                .iter()
                .map(|(id, version)| PackRef {
                    id: (*id).to_string(),
                    version: Version::parse(version).unwrap(),
                })
                .collect(),
            min_app_version: Version::new(1, 0, 0),
        }
    }

    fn valid_catalog() -> CatalogV1 {
        CatalogV1 {
            schema_version: SUPPORTED_SCHEMA_VERSION,
            generated_at: "2026-07-18T12:00:00+08:00".to_string(),
            engine_api: SUPPORTED_ENGINE_API,
            packs: vec![
                pack("protocol-core", &[]),
                pack("media-h264-live", &[]),
                pack(
                    "ipc-custom",
                    &[("protocol-core", "1.0.0"), ("media-h264-live", "1.0.0")],
                ),
            ],
            profiles: vec![CatalogProfile {
                id: "ipc-custom".to_string(),
                device_kind: DeviceKind::Ipc,
                required_packs: vec![PackRef {
                    id: "ipc-custom".to_string(),
                    version: Version::new(1, 0, 0),
                }],
            }],
        }
    }

    fn valid_manifest() -> PackManifest {
        PackManifest {
            schema_version: SUPPORTED_SCHEMA_VERSION,
            id: "ipc-custom".to_string(),
            version: Version::new(1, 0, 0),
            engine_api: SUPPORTED_ENGINE_API,
            usage: non_commercial_usage(),
            files: vec![PackFile {
                path: "profiles/ipc-custom.json".to_string(),
                sha256: HASH.to_string(),
                size: 512,
            }],
        }
    }

    #[test]
    fn accepts_valid_catalog_and_manifest() {
        let catalog = valid_catalog();
        validate_catalog(&catalog, &Version::new(1, 2, 0)).unwrap();
        validate_pack_manifest(&valid_manifest(), &catalog.packs[2]).unwrap();
    }

    #[test]
    fn rejects_schema_engine_time_and_incompatible_application() {
        let mut catalog = valid_catalog();
        catalog.schema_version = 2;
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.schema_unsupported"
        );

        catalog = valid_catalog();
        catalog.engine_api = 2;
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.engine_api_unsupported"
        );

        catalog = valid_catalog();
        catalog.generated_at = "2026-07-18 12:00:00".to_string();
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.invalid_generated_at"
        );

        catalog = valid_catalog();
        catalog.packs[0].min_app_version = Version::new(2, 0, 0);
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.app_version_unsupported"
        );
    }

    #[test]
    fn rejects_invalid_ids_hashes_and_sizes() {
        for invalid in ["", "Upper", "-leading", "trailing-", "two--NO"] {
            let mut catalog = valid_catalog();
            catalog.packs[0].id = invalid.to_string();
            assert_eq!(
                validate_catalog(&catalog, &Version::new(1, 2, 0))
                    .unwrap_err()
                    .code,
                "device_simulator.assets.invalid_id",
                "accepted {invalid:?}"
            );
        }

        let mut catalog = valid_catalog();
        catalog.packs[0].sha256 = "A".repeat(64);
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.invalid_sha256"
        );

        catalog = valid_catalog();
        catalog.packs[0].size = 0;
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.size_limit_exceeded"
        );

        catalog = valid_catalog();
        catalog.packs[0].unpacked_size = MAX_UNPACKED_SIZE_BYTES + 1;
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.size_limit_exceeded"
        );
    }

    #[test]
    fn rejects_duplicate_and_missing_catalog_references() {
        let mut catalog = valid_catalog();
        catalog.packs.push(catalog.packs[0].clone());
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.duplicate_pack"
        );

        catalog = valid_catalog();
        catalog.profiles.push(catalog.profiles[0].clone());
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.duplicate_profile"
        );

        catalog = valid_catalog();
        catalog.profiles[0].required_packs[0].id = "missing-pack".to_string();
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.pack_ref_missing"
        );

        catalog = valid_catalog();
        catalog.packs[2].dependencies.push(PackRef {
            id: "missing-pack".to_string(),
            version: Version::new(1, 0, 0),
        });
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.pack_ref_missing"
        );
    }

    #[test]
    fn rejects_duplicate_refs_and_dependency_cycles() {
        let mut catalog = valid_catalog();
        let duplicate = catalog.packs[2].dependencies[0].clone();
        catalog.packs[2].dependencies.push(duplicate);
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.duplicate_pack_ref"
        );

        catalog = valid_catalog();
        catalog.packs[0].dependencies.push(PackRef {
            id: "ipc-custom".to_string(),
            version: Version::new(1, 0, 0),
        });
        assert_eq!(
            validate_catalog(&catalog, &Version::new(1, 2, 0))
                .unwrap_err()
                .code,
            "device_simulator.assets.dependency_cycle"
        );
    }

    #[test]
    fn validates_http_https_and_relative_zip_urls() {
        for valid in [
            "packs/ipc/1.0.0/ipc.zip",
            "https://assets.example.test/packs/ipc.zip",
            "http://127.0.0.1:8080/packs/ipc.zip?token=opaque",
        ] {
            assert!(validate_pack_url(valid).is_ok(), "rejected {valid:?}");
        }

        for invalid in [
            "ftp://assets.example.test/ipc.zip",
            "https://user:password@assets.example.test/ipc.zip",
            "https://assets.example.test/ipc.zip#fragment",
            "https://assets.example.test/../secret.zip",
            "https://assets.example.test/%2e%2e/secret.zip",
            "https://assets.example.test/%252e%252e/secret.zip",
            "https://assets.example.test//ipc.zip",
            "//assets.example.test/ipc.zip",
            "C:/packs/ipc.zip",
            "packs/ipc.json",
            "packs/ipc.zip?token=relative",
        ] {
            assert_eq!(
                validate_pack_url(invalid).unwrap_err().code,
                "device_simulator.assets.invalid_url",
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn rejects_windows_and_zip_slip_paths() {
        for invalid in [
            "../secret.xml",
            "profiles/../../secret.xml",
            "/absolute/file.xml",
            "//server/share/file.xml",
            r"C:\temp\file.xml",
            r"profiles\file.xml",
            "profiles//file.xml",
            "profiles/./file.xml",
            "profiles/../file.xml",
            "profiles/file.xml/",
            "profiles/file.xml.",
            "profiles/file.xml ",
            "profiles/file.xml:stream",
            "profiles/CON.xml",
            "profiles/aux",
            "profiles/COM1.json",
            "profiles/nu\0l.json",
        ] {
            assert_eq!(
                validate_pack_path(invalid).unwrap_err().code,
                "device_simulator.assets.invalid_pack_path",
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn rejects_all_forbidden_executable_extensions_case_insensitively() {
        for extension in FORBIDDEN_EXTENSIONS {
            for path in [
                format!("payload/run.{extension}"),
                format!("payload/RUN.{}", extension.to_ascii_uppercase()),
            ] {
                assert_eq!(
                    validate_pack_path(&path).unwrap_err().code,
                    "device_simulator.assets.forbidden_file_type",
                    "accepted {path:?}"
                );
            }
        }

        for safe in [
            "profiles/ipc-custom.json",
            "templates/device.xml",
            "images/alarm.jpeg",
            "media/live.h264",
        ] {
            validate_pack_path(safe).unwrap();
        }
    }

    #[test]
    fn validates_manifest_identity_files_hashes_and_declared_size() {
        let expected = valid_catalog().packs.pop().unwrap();

        let mut manifest = valid_manifest();
        manifest.id = "other-pack".to_string();
        assert_eq!(
            validate_pack_manifest(&manifest, &expected)
                .unwrap_err()
                .code,
            "device_simulator.assets.manifest_identity_mismatch"
        );

        manifest = valid_manifest();
        manifest.usage.notice = "commercial use allowed".into();
        assert_eq!(
            validate_pack_manifest(&manifest, &expected)
                .unwrap_err()
                .code,
            "device_simulator.assets.usage_policy_invalid"
        );

        manifest = valid_manifest();
        manifest.files.push(PackFile {
            path: "PROFILES/IPC-CUSTOM.JSON".to_string(),
            sha256: HASH.to_string(),
            size: 1,
        });
        assert_eq!(
            validate_pack_manifest(&manifest, &expected)
                .unwrap_err()
                .code,
            "device_simulator.assets.duplicate_file"
        );

        manifest = valid_manifest();
        manifest.files[0].sha256 = "0".repeat(63);
        assert_eq!(
            validate_pack_manifest(&manifest, &expected)
                .unwrap_err()
                .code,
            "device_simulator.assets.invalid_sha256"
        );

        manifest = valid_manifest();
        manifest.files[0].size = expected.unpacked_size + 1;
        assert_eq!(
            validate_pack_manifest(&manifest, &expected)
                .unwrap_err()
                .code,
            "device_simulator.assets.size_limit_exceeded"
        );
    }

    #[test]
    fn invalid_semver_is_rejected_by_the_strong_catalog_model() {
        let json = r#"{
            "schema_version":1,
            "generated_at":"2026-07-18T12:00:00Z",
            "engine_api":1,
            "packs":[{
                "id":"ipc-custom","version":"latest","kind":"device-profile",
                "url":"packs/ipc.zip","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "size":1,"unpacked_size":1,"dependencies":[],"min_app_version":"1.0.0"
            }],
            "profiles":[]
        }"#;
        assert!(serde_json::from_str::<CatalogV1>(json).is_err());
    }
}
