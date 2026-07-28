use app_lib::device_simulator::assets::catalog::PackManifest;
use app_lib::device_simulator::runtime_assets::{
    list_media_themes, PinnedPackDirectory, RuntimeAssetLayout, RuntimeMediaKind,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn pack_pin(id: &str, directory: PathBuf) -> PinnedPackDirectory {
    let manifest: PackManifest = serde_json::from_slice(
        &fs::read(directory.join("pack.json")).expect("pack.json should be readable"),
    )
    .expect("pack.json should be valid");
    assert_eq!(manifest.id, id, "pack directory identity mismatch");
    PinnedPackDirectory {
        id: id.into(),
        version: manifest.version.to_string(),
        directory,
    }
}

fn required_directory(arguments: &mut impl Iterator<Item = std::ffi::OsString>) -> PathBuf {
    let directory = arguments.next().map(PathBuf::from).unwrap_or_else(|| {
        eprintln!(
            "usage: validate_device_simulator_themes <media-pack> <protocol-pack> <profile-pack>"
        );
        std::process::exit(2);
    });
    fs::canonicalize(directory).expect("pack directory should exist")
}

fn main() {
    let mut arguments = env::args_os().skip(1);
    let media = required_directory(&mut arguments);
    let protocol = required_directory(&mut arguments);
    let profile = required_directory(&mut arguments);
    let pins = vec![
        pack_pin("media-h264-live", media.clone()),
        pack_pin("protocol-core", protocol),
        pack_pin("ipc-smart", profile),
    ];

    for theme in list_media_themes(Path::new(&media)).expect("theme catalog should load") {
        let layout = RuntimeAssetLayout::load_for_theme(&pins, &["ipc-smart".into()], &theme.id)
            .unwrap_or_else(|error| panic!("theme {} failed: {error}", theme.id));
        let main = layout.media(RuntimeMediaKind::Main);
        let sub = layout.media(RuntimeMediaKind::Sub);
        let third = layout.media(RuntimeMediaKind::Third);
        println!(
            "{}: main={} sub={} third={} frames",
            theme.id,
            main.frames().len(),
            sub.frames().len(),
            third.frames().len(),
        );
    }
}
