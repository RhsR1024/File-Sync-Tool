use app_lib::device_simulator::local_materials::{
    load_local_media_theme, refresh_local_media, LocalMaterialPaths,
};
use std::path::PathBuf;

fn main() {
    let app_data_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: refresh_device_simulator_materials <app-data-directory>");
    let paths = LocalMaterialPaths::from_app_data_dir(&app_data_dir);
    let themes = refresh_local_media(&paths).unwrap_or_else(|error| {
        panic!("failed to refresh local materials: {error}");
    });
    for theme in themes {
        load_local_media_theme(&paths, &theme.id)
            .unwrap_or_else(|error| panic!("failed to validate '{}': {error}", theme.id))
            .unwrap_or_else(|| panic!("refreshed theme '{}' is missing", theme.id));
        println!("{}\t{}", theme.id, theme.display_name.unwrap_or_default());
    }
}
