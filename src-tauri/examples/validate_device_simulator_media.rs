use app_lib::device_simulator::media::load_media_pack;
use std::env;
use std::path::PathBuf;

fn main() {
    let pack_dir = env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: validate_device_simulator_media <unpacked media pack directory>");
        std::process::exit(2);
    });

    for kind in ["main", "sub", "third"] {
        let manifest_path = format!("media/{kind}/media.json");
        let pack = load_media_pack(&pack_dir, &manifest_path).unwrap_or_else(|error| {
            eprintln!("{kind}: {error}");
            std::process::exit(1);
        });
        let manifest = pack.manifest();
        let duration_ticks = pack
            .frames()
            .iter()
            .map(|frame| u64::from(frame.duration_ticks))
            .sum::<u64>();
        let duration_seconds = duration_ticks as f64 / f64::from(manifest.clock_rate);
        println!(
            "{kind}: id={} codec={:?} fps={}/{} frames={} duration={duration_seconds:.3}s bitrate={}bps",
            manifest.id,
            manifest.codec,
            manifest.frame_rate_numerator,
            manifest.frame_rate_denominator,
            pack.frames().len(),
            pack.actual_bitrate_bps(),
        );
    }
}
