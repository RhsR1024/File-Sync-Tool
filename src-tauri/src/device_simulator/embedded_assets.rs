use crate::device_simulator::runtime_assets::PinnedPackDirectory;
use rust_embed::RustEmbed;
use std::fs;
use std::path::{Component, Path};
use std::sync::OnceLock;

#[derive(RustEmbed)]
#[folder = "resources/device-simulator-base/"]
struct BuiltInDeviceSimulatorAssets;

static EXTRACTED: OnceLock<Result<Vec<PinnedPackDirectory>, EmbeddedAssetError>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedAssetError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for EmbeddedAssetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EmbeddedAssetError {}

/// Materializes the protocol/profile baseline bundled with the executable.
///
/// These files are deliberately not release packs: there is no catalog,
/// signature, hash allow-list, or user-facing version. They are refreshed once
/// per process so an application update also updates the built-in baseline.
pub fn ensure_built_in_assets(
    app_data_dir: &Path,
) -> Result<Vec<PinnedPackDirectory>, EmbeddedAssetError> {
    EXTRACTED
        .get_or_init(|| extract_built_in_assets(app_data_dir))
        .clone()
}

fn extract_built_in_assets(
    app_data_dir: &Path,
) -> Result<Vec<PinnedPackDirectory>, EmbeddedAssetError> {
    let root = app_data_dir
        .join("device-simulator")
        .join("built-in-assets");
    fs::create_dir_all(&root).map_err(|source| {
        error(
            "device_simulator.assets.builtin_directory_failed",
            format!("failed to create '{}': {source}", root.display()),
        )
    })?;

    // Earlier builds embedded large alarm pictures and two H.264 themes here.
    // They now come from the upgrade server. Remove only these app-owned legacy
    // subtrees so upgrading from a larger EXE cannot keep exposing stale media.
    for obsolete in ["media-h264-live/media", "ipc-structured/pic"] {
        let obsolete = root.join(obsolete);
        if obsolete.is_dir() {
            fs::remove_dir_all(&obsolete).map_err(|source| {
                error(
                    "device_simulator.assets.builtin_cleanup_failed",
                    format!("failed to remove '{}': {source}", obsolete.display()),
                )
            })?;
        }
    }

    for embedded_path in BuiltInDeviceSimulatorAssets::iter() {
        let relative = Path::new(embedded_path.as_ref());
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(error(
                "device_simulator.assets.builtin_path_invalid",
                format!("embedded asset path '{}' is invalid", relative.display()),
            ));
        }
        let asset = BuiltInDeviceSimulatorAssets::get(embedded_path.as_ref()).ok_or_else(|| {
            error(
                "device_simulator.assets.builtin_read_failed",
                format!("embedded asset '{}' disappeared", relative.display()),
            )
        })?;
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                error(
                    "device_simulator.assets.builtin_directory_failed",
                    format!("failed to create '{}': {source}", parent.display()),
                )
            })?;
        }
        let bytes = asset.data.as_ref();
        let unchanged = fs::read(&target)
            .ok()
            .is_some_and(|current| current == bytes);
        if !unchanged {
            fs::write(&target, bytes).map_err(|source| {
                error(
                    "device_simulator.assets.builtin_write_failed",
                    format!("failed to write '{}': {source}", target.display()),
                )
            })?;
        }
    }

    ["protocol-core", "media-h264-live", "ipc-structured"]
        .into_iter()
        .map(|id| {
            let directory = root.join(id);
            if !directory.is_dir() {
                return Err(error(
                    "device_simulator.assets.builtin_directory_missing",
                    format!(
                        "built-in asset directory '{}' is missing",
                        directory.display()
                    ),
                ));
            }
            Ok(PinnedPackDirectory {
                id: id.to_owned(),
                version: String::new(),
                directory,
            })
        })
        .collect()
}

fn error(code: &'static str, message: impl Into<String>) -> EmbeddedAssetError {
    EmbeddedAssetError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_baseline_contains_all_runtime_roots() {
        let paths = BuiltInDeviceSimulatorAssets::iter().collect::<Vec<_>>();
        for id in ["protocol-core", "media-h264-live", "ipc-structured"] {
            assert!(
                paths.iter().any(|path| path.starts_with(&format!("{id}/"))),
                "missing embedded root {id}"
            );
        }
    }
}
