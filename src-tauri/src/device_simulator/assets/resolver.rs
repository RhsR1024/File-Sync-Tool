use super::catalog::{CatalogPack, CatalogV1, PackRef};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetResolveError {
    pub code: &'static str,
    pub message: String,
}

impl AssetResolveError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AssetResolveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AssetResolveError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Visited,
}

/// Resolve the immutable pack closure for a set of profiles.
///
/// The result is dependency-first and deterministic. Callers may safely install
/// packs in the returned order. Catalog validation should normally run first,
/// but this function still rejects malformed or incomplete catalogs so it is
/// safe to use at trust boundaries.
pub fn resolve_profile_dependencies(
    catalog: &CatalogV1,
    profile_ids: &[String],
) -> Result<Vec<PackRef>, AssetResolveError> {
    let mut profile_index = HashMap::new();
    for profile in &catalog.profiles {
        if profile_index.insert(profile.id.as_str(), profile).is_some() {
            return Err(AssetResolveError::new(
                "device_simulator.assets.duplicate_profile",
                format!("catalog contains duplicate profile '{}'", profile.id),
            ));
        }
    }

    let mut pack_index: HashMap<String, &CatalogPack> = HashMap::new();
    for pack in &catalog.packs {
        let pack_ref = PackRef {
            id: pack.id.clone(),
            version: pack.version.clone(),
        };
        let key = canonical_pack_ref(&pack_ref, "catalog pack")?;
        if pack_index.insert(key.clone(), pack).is_some() {
            return Err(AssetResolveError::new(
                "device_simulator.assets.duplicate_pack",
                format!("catalog contains duplicate pack '{key}'"),
            ));
        }
    }

    let selected_profiles = profile_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut roots = BTreeSet::new();
    for profile_id in selected_profiles {
        let profile = profile_index.get(profile_id.as_str()).ok_or_else(|| {
            AssetResolveError::new(
                "device_simulator.assets.unknown_profile",
                format!("profile '{profile_id}' is not present in the catalog"),
            )
        })?;
        for required in &profile.required_packs {
            roots.insert(canonical_pack_ref(
                required,
                &format!("profile '{profile_id}' required pack"),
            )?);
        }
    }

    let mut states = HashMap::new();
    let mut stack = Vec::new();
    let mut resolved = Vec::new();
    for root in roots {
        visit_pack(&root, &pack_index, &mut states, &mut stack, &mut resolved)?;
    }
    Ok(resolved)
}

fn visit_pack(
    key: &str,
    pack_index: &HashMap<String, &CatalogPack>,
    states: &mut HashMap<String, VisitState>,
    stack: &mut Vec<String>,
    resolved: &mut Vec<PackRef>,
) -> Result<(), AssetResolveError> {
    match states.get(key) {
        Some(VisitState::Visited) => return Ok(()),
        Some(VisitState::Visiting) => {
            let start = stack.iter().position(|item| item == key).unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(key.to_string());
            return Err(AssetResolveError::new(
                "device_simulator.assets.dependency_cycle",
                format!("asset dependency cycle detected: {}", cycle.join(" -> ")),
            ));
        }
        None => {}
    }

    let pack = pack_index.get(key).ok_or_else(|| {
        AssetResolveError::new(
            "device_simulator.assets.missing_pack",
            format!("required pack '{key}' is not present in the catalog"),
        )
    })?;

    states.insert(key.to_string(), VisitState::Visiting);
    stack.push(key.to_string());

    let mut dependencies = BTreeSet::new();
    for dependency in &pack.dependencies {
        dependencies.insert(canonical_pack_ref(
            dependency,
            &format!("pack '{key}' dependency"),
        )?);
    }
    for dependency in dependencies {
        visit_pack(&dependency, pack_index, states, stack, resolved)?;
    }

    stack.pop();
    states.insert(key.to_string(), VisitState::Visited);
    let parsed = key.parse::<PackRef>().map_err(|error| {
        AssetResolveError::new(
            "device_simulator.assets.invalid_pack_ref",
            format!("resolved pack reference '{key}' is invalid: {error}"),
        )
    })?;
    resolved.push(parsed);
    Ok(())
}

fn canonical_pack_ref(pack_ref: &PackRef, context: &str) -> Result<String, AssetResolveError> {
    let rendered = pack_ref.to_string();
    rendered.parse::<PackRef>().map_err(|error| {
        AssetResolveError::new(
            "device_simulator.assets.invalid_pack_ref",
            format!("{context} '{rendered}' is invalid: {error}"),
        )
    })?;
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::catalog::{CatalogProfile, DeviceKind, PackKind};
    use semver::Version;

    fn reference(value: &str) -> PackRef {
        value.parse().unwrap()
    }

    fn pack(id: &str, dependencies: &[&str]) -> CatalogPack {
        CatalogPack {
            id: id.to_string(),
            version: Version::new(1, 0, 0),
            kind: if id == "protocol-core" {
                PackKind::ProtocolCore
            } else if id.starts_with("media-") {
                PackKind::Media
            } else {
                PackKind::DeviceProfile
            },
            url: format!("packs/{id}/1.0.0/{id}-1.0.0.zip"),
            sha256: "a".repeat(64),
            size: 100,
            unpacked_size: 200,
            dependencies: dependencies.iter().map(|value| reference(value)).collect(),
            min_app_version: Version::new(1, 0, 0),
        }
    }

    fn profile(id: &str, required_packs: &[&str]) -> CatalogProfile {
        CatalogProfile {
            id: id.to_string(),
            device_kind: DeviceKind::Ipc,
            required_packs: required_packs
                .iter()
                .map(|value| reference(value))
                .collect(),
        }
    }

    fn catalog(packs: Vec<CatalogPack>, profiles: Vec<CatalogProfile>) -> CatalogV1 {
        CatalogV1 {
            schema_version: 1,
            generated_at: "2026-07-18T12:00:00+08:00".to_string(),
            engine_api: 1,
            packs,
            profiles,
        }
    }

    fn rendered(result: Vec<PackRef>) -> Vec<String> {
        result.into_iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn resolves_dependency_first_closure_and_deduplicates_shared_packs() {
        let catalog = catalog(
            vec![
                pack("protocol-core", &[]),
                pack("media-h264-live", &["protocol-core@1.0.0"]),
                pack(
                    "ipc-custom",
                    &["protocol-core@1.0.0", "media-h264-live@1.0.0"],
                ),
                pack(
                    "nvr-common",
                    &["protocol-core@1.0.0", "media-h264-live@1.0.0"],
                ),
            ],
            vec![
                profile("ipc-custom", &["ipc-custom@1.0.0"]),
                profile("nvr-common", &["nvr-common@1.0.0"]),
            ],
        );

        let result = resolve_profile_dependencies(
            &catalog,
            &["nvr-common".to_string(), "ipc-custom".to_string()],
        )
        .unwrap();

        assert_eq!(
            rendered(result),
            vec![
                "protocol-core@1.0.0",
                "media-h264-live@1.0.0",
                "ipc-custom@1.0.0",
                "nvr-common@1.0.0",
            ]
        );
    }

    #[test]
    fn profile_input_order_does_not_change_resolution() {
        let catalog = catalog(
            vec![
                pack("protocol-core", &[]),
                pack("a", &["protocol-core@1.0.0"]),
                pack("b", &["protocol-core@1.0.0"]),
            ],
            vec![profile("a", &["a@1.0.0"]), profile("b", &["b@1.0.0"])],
        );
        let first = resolve_profile_dependencies(&catalog, &["a".into(), "b".into()]).unwrap();
        let second = resolve_profile_dependencies(&catalog, &["b".into(), "a".into()]).unwrap();
        assert_eq!(rendered(first), rendered(second));
    }

    #[test]
    fn rejects_unknown_profile_and_missing_pack() {
        let catalog = catalog(vec![], vec![profile("known", &["missing@1.0.0"])]);
        let error = resolve_profile_dependencies(&catalog, &["unknown".into()]).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.unknown_profile");

        let error = resolve_profile_dependencies(&catalog, &["known".into()]).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.missing_pack");
    }

    #[test]
    fn rejects_invalid_reference_and_dependency_cycle() {
        let invalid = catalog(
            vec![pack("bad@id", &[])],
            vec![CatalogProfile {
                id: "bad".to_string(),
                device_kind: DeviceKind::Ipc,
                required_packs: vec![PackRef {
                    id: "bad@id".to_string(),
                    version: Version::new(1, 0, 0),
                }],
            }],
        );
        let error = resolve_profile_dependencies(&invalid, &["bad".into()]).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.invalid_pack_ref");

        let cyclic = catalog(
            vec![pack("a", &["b@1.0.0"]), pack("b", &["a@1.0.0"])],
            vec![profile("cycle", &["a@1.0.0"])],
        );
        let error = resolve_profile_dependencies(&cyclic, &["cycle".into()]).unwrap_err();
        assert_eq!(error.code, "device_simulator.assets.dependency_cycle");
        assert!(error.message.contains("a@1.0.0 -> b@1.0.0 -> a@1.0.0"));
    }
}
