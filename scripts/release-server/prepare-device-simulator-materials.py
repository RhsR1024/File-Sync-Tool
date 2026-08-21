#!/usr/bin/env python3
"""Prepare MP4 files for direct device-simulator download and playback.

This publisher-side tool intentionally uses only Python's standard library and
the system FFmpeg executable, so it can run on openEuler without the Windows
desktop application or a Rust toolchain.
"""

import argparse
import hashlib
import json
import mmap
import os
import shutil
import subprocess
import sys
import uuid
from pathlib import Path


VIDEO_CLOCK_RATE = 90_000
MAX_MEDIA_BYTES = 1024 * 1024 * 1024
OFFLINE_H264_PRESET = "medium"
OFFLINE_H264_PEAK_BITRATE_NUMERATOR = 3
OFFLINE_H264_PEAK_BITRATE_DENOMINATOR = 2
OFFLINE_H264_BUFFER_SECONDS = 2
DEFAULT_VIDEO = "车流测试视频.mp4"
RENDITIONS = (
    {
        "kind": "main",
        "width": 1920,
        "height": 1080,
        "fps": 25,
        "bitrate": 6_000_000,
        "payload_type": 105,
    },
    {
        "kind": "sub",
        "width": 640,
        "height": 360,
        "fps": 20,
        "bitrate": 1_000_000,
        "payload_type": 105,
    },
    {
        "kind": "third",
        "width": 640,
        "height": 360,
        "fps": 20,
        "bitrate": 1_000_000,
        "payload_type": 105,
    },
)


def parse_args():
    script_root = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(
        description="Convert server-side MP4 files into indexed H.264 materials."
    )
    parser.add_argument(
        "--source",
        type=Path,
        default=script_root / "virtual-device-assets" / "source-videos",
        help="directory containing source MP4 files",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=script_root / "virtual-device-assets" / "prepared-videos",
        help="directory receiving prepared-catalog.json and media files",
    )
    parser.add_argument(
        "--default-video",
        default=DEFAULT_VIDEO,
        help="exact source MP4 filename selected by default",
    )
    parser.add_argument(
        "--ffmpeg",
        default="ffmpeg",
        help="FFmpeg executable path (default: ffmpeg from PATH)",
    )
    return parser.parse_args()


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while True:
            chunk = source.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def read_previous_catalog(path):
    if not path.is_file():
        return None
    try:
        catalog = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print("warning: ignoring unreadable previous catalog: {}".format(error), file=sys.stderr)
        return None
    if catalog.get("schema_version") != 1 or not isinstance(catalog.get("themes"), list):
        print("warning: ignoring incompatible previous prepared catalog", file=sys.stderr)
        return None
    return catalog


def stream_paths(theme_id):
    return {
        rendition["kind"]: "media/themes/{}/{}/media.json".format(
            theme_id, rendition["kind"]
        )
        for rendition in RENDITIONS
    }


def cached_streams_exist(output, theme):
    streams = theme.get("streams")
    if not isinstance(streams, dict):
        return False
    for rendition in RENDITIONS:
        manifest_relative = streams.get(rendition["kind"])
        if not isinstance(manifest_relative, str):
            return False
        manifest_path = output / manifest_relative
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            media_path = manifest_path.parent / manifest["media_file"]
            if (
                not media_path.is_file()
                or media_path.stat().st_size != manifest["media_file_size"]
                or not manifest.get("frames")
            ):
                return False
        except (OSError, KeyError, TypeError, json.JSONDecodeError):
            return False
    return True


def find_start_code(data, start):
    # Searching for the three-byte suffix also finds a four-byte Annex-B marker;
    # the unused leading zero is removed when the preceding NAL is trimmed.
    index = data.find(b"\x00\x00\x01", start)
    return None if index < 0 else (index, index + 3)


def finish_frame(frames, nals, frame_offset, fps):
    if not nals:
        return
    end = nals[-1]["offset"] + nals[-1]["length"]
    frames.append(
        {
            "offset": frame_offset,
            "length": end - frame_offset,
            "duration_ticks": VIDEO_CLOCK_RATE // fps,
            "keyframe": any(nal["nal_type"] == 5 for nal in nals),
            "nals": list(nals),
        }
    )
    nals.clear()


def normalize_annex_b(raw_path, target_directory, theme_id, rendition):
    target_directory.mkdir(parents=True, exist_ok=True)
    media_name = "{}.h264".format(rendition["kind"])
    media_path = target_directory / media_name
    frames = []
    current_nals = []
    parameter_sets = []
    normalized_offset = 0
    frame_offset = 0

    with raw_path.open("rb") as source, media_path.open("wb") as target:
        with mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as data:
            marker = find_start_code(data, 0)
            if marker is None:
                raise RuntimeError("FFmpeg H.264 output has no Annex-B start code")
            while marker is not None:
                begin = marker[1]
                next_marker = find_start_code(data, begin)
                end = len(data) if next_marker is None else next_marker[0]
                while end > begin and data[end - 1] == 0:
                    end -= 1
                if end > begin:
                    nal_type = data[begin] & 0x1F
                    if nal_type == 9 and current_nals:
                        finish_frame(
                            frames, current_nals, frame_offset, rendition["fps"]
                        )
                        frame_offset = normalized_offset
                    nal_index = len(current_nals)
                    length = end - begin
                    current_nals.append(
                        {
                            "offset": normalized_offset,
                            "length": length,
                            "nal_type": nal_type,
                        }
                    )
                    if nal_type in (7, 8):
                        kind = "sps" if nal_type == 7 else "pps"
                        if not any(item["kind"] == kind for item in parameter_sets):
                            parameter_sets.append(
                                {
                                    "kind": kind,
                                    "frame_index": len(frames),
                                    "nal_index": nal_index,
                                }
                            )
                    target.write(data[begin:end])
                    normalized_offset += length
                marker = next_marker

    finish_frame(frames, current_nals, frame_offset, rendition["fps"])
    if len(frames) < 2 or not frames[0]["keyframe"]:
        raise RuntimeError("encoded H.264 stream has no usable keyframe sequence")
    if {item["kind"] for item in parameter_sets} != {"sps", "pps"}:
        raise RuntimeError("encoded H.264 stream is missing SPS or PPS")
    if normalized_offset > MAX_MEDIA_BYTES:
        raise RuntimeError(
            "prepared stream exceeds the client 1 GiB per-stream limit: {}".format(
                media_path
            )
        )

    manifest = {
        "schema_version": 1,
        "id": "{}-{}".format(theme_id, rendition["kind"]),
        "codec": "h264",
        "clock_rate": VIDEO_CLOCK_RATE,
        "payload_type": rendition["payload_type"],
        "frame_rate_numerator": rendition["fps"],
        "frame_rate_denominator": 1,
        "recommended_bitrate_bps": rendition["bitrate"],
        "media_file": media_name,
        "media_file_size": normalized_offset,
        "media_file_sha256": sha256_file(media_path),
        "frames": frames,
        "parameter_sets": parameter_sets,
        "evidence": {
            "source_kind": "synthetic_fixture",
            "pcap_source_id": None,
            "sdp_source_id": None,
            "compatibility": "unverified",
            "verified_platforms": [],
            "differences": [],
        },
    }
    (target_directory / "media.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def build_encode_command(ffmpeg, source, raw_path, rendition):
    scale = (
        "scale={width}:{height}:force_original_aspect_ratio=decrease:flags=lanczos,"
        "pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black,fps={fps}"
    ).format(**rendition)
    gop = rendition["fps"] * 2
    peak_bitrate = (
        rendition["bitrate"] * OFFLINE_H264_PEAK_BITRATE_NUMERATOR
        // OFFLINE_H264_PEAK_BITRATE_DENOMINATOR
    )
    command = [
        ffmpeg,
        "-hide_banner",
        "-loglevel",
        "warning",
        "-y",
        "-i",
        str(source),
        "-map",
        "0:v:0",
        "-an",
        "-vf",
        scale,
        "-c:v",
        "libx264",
        "-preset",
        OFFLINE_H264_PRESET,
        "-profile:v",
        "high",
        "-pix_fmt",
        "yuv420p",
        "-b:v",
        str(rendition["bitrate"]),
        "-maxrate",
        str(peak_bitrate),
        "-bufsize",
        str(rendition["bitrate"] * OFFLINE_H264_BUFFER_SECONDS),
        "-g",
        str(gop),
        "-keyint_min",
        str(gop),
        "-sc_threshold",
        "0",
        "-bf",
        "0",
        "-x264-params",
        "repeat-headers=1:aud=1:open-gop=0",
        # FFmpeg 4.2 (as shipped by the target openEuler server) predates
        # -fps_mode. The legacy output option is equivalent here and remains
        # accepted by newer FFmpeg releases.
        "-vsync",
        "cfr",
        "-f",
        "h264",
        str(raw_path),
    ]
    return command


def encode_rendition(ffmpeg, source, raw_path, rendition):
    subprocess.run(
        build_encode_command(ffmpeg, source, raw_path, rendition), check=True
    )


def prepare_theme(ffmpeg, source, output, theme_id):
    themes_root = output / "media" / "themes"
    # Use a hidden sibling under the final theme parent so directory activation
    # is a same-parent rename on Windows and Linux. serve.py explicitly excludes
    # hidden work directories from files.json.
    staging_root = themes_root / (".material-builder-" + uuid.uuid4().hex)
    staged_theme = staging_root / theme_id
    target_theme = themes_root / theme_id
    backup_theme = staging_root / "previous"
    try:
        staged_theme.mkdir(parents=True)
        for rendition in RENDITIONS:
            raw_path = staging_root / (rendition["kind"] + ".annexb.h264")
            print(
                "  encoding {} {}p@{}fps".format(
                    rendition["kind"], rendition["height"], rendition["fps"]
                ),
                flush=True,
            )
            encode_rendition(ffmpeg, source, raw_path, rendition)
            normalize_annex_b(
                raw_path, staged_theme / rendition["kind"], theme_id, rendition
            )
            raw_path.unlink()

        themes_root.mkdir(parents=True, exist_ok=True)
        if target_theme.exists():
            os.rename(str(target_theme), str(backup_theme))
        os.rename(str(staged_theme), str(target_theme))
        if backup_theme.exists():
            shutil.rmtree(str(backup_theme))
    except Exception:
        if backup_theme.exists() and not target_theme.exists():
            os.rename(str(backup_theme), str(target_theme))
        raise
    finally:
        shutil.rmtree(str(staging_root), ignore_errors=True)


def write_catalog_atomic(path, catalog):
    temporary = path.with_name(path.name + ".tmp-" + uuid.uuid4().hex)
    temporary.write_text(
        json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    os.replace(str(temporary), str(path))


def main():
    args = parse_args()
    source = args.source.expanduser().resolve()
    output = args.output.expanduser().resolve()
    if not source.is_dir():
        raise RuntimeError("source directory does not exist: {}".format(source))
    videos = sorted(
        (path for path in source.iterdir() if path.is_file() and path.suffix.lower() == ".mp4"),
        key=lambda path: path.name,
    )
    if not videos:
        raise RuntimeError("no MP4 files found in: {}".format(source))
    if shutil.which(args.ffmpeg) is None and not Path(args.ffmpeg).is_file():
        raise RuntimeError("FFmpeg was not found: {}".format(args.ffmpeg))
    subprocess.run(
        [args.ffmpeg, "-hide_banner", "-version"],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    output.mkdir(parents=True, exist_ok=True)
    catalog_path = output / "prepared-catalog.json"
    previous = read_previous_catalog(catalog_path)
    previous_themes = [] if previous is None else previous["themes"]
    previous_by_content = {
        theme.get("source_content_id"): theme
        for theme in previous_themes
        if theme.get("source_content_id")
    }

    themes = []
    seen_ids = set()
    for index, video in enumerate(videos, 1):
        print("[{}/{}] hashing {}".format(index, len(videos), video.name), flush=True)
        content_id = sha256_file(video)
        theme_id = "local-" + content_id[:12]
        if theme_id in seen_ids:
            raise RuntimeError("duplicate MP4 content is not allowed: {}".format(video.name))
        seen_ids.add(theme_id)
        cached = previous_by_content.get(content_id)
        if cached is not None and cached_streams_exist(output, cached):
            streams = cached["streams"]
            print("  reused prepared streams", flush=True)
        elif cached_streams_exist(output, {"streams": stream_paths(theme_id)}):
            # Recover a completed theme left behind when a prior run finished
            # encoding but was interrupted before catalog activation.
            streams = stream_paths(theme_id)
            print("  recovered completed prepared streams", flush=True)
        else:
            prepare_theme(args.ffmpeg, video, output, theme_id)
            streams = stream_paths(theme_id)
        stat = video.stat()
        themes.append(
            {
                "id": theme_id,
                "display_name": video.stem,
                "source_file": video.name,
                "source_size": stat.st_size,
                "source_modified_ms": stat.st_mtime_ns // 1_000_000,
                "source_content_id": content_id,
                "streams": streams,
            }
        )

    requested_default = args.default_video.strip()
    default_theme = next(
        (
            theme
            for theme in themes
            if theme["source_file"].casefold() == requested_default.casefold()
        ),
        None,
    )
    if default_theme is None:
        raise RuntimeError(
            "default video was not found: {} (use --default-video)".format(
                requested_default
            )
        )

    catalog = {
        "schema_version": 1,
        "default_theme_id": default_theme["id"],
        "themes": themes,
        "alarm_images": {},
    }
    write_catalog_atomic(catalog_path, catalog)

    # A reused stream directory can intentionally differ from the current
    # theme id (for example after an older catalog migration). Keep the actual
    # directory referenced by each stream, matching the Rust publisher.
    active_ids = {
        Path(theme["streams"]["main"]).parts[2]
        for theme in themes
    }
    themes_root = output / "media" / "themes"
    if themes_root.is_dir():
        for child in themes_root.iterdir():
            if child.is_dir() and child.name not in active_ids:
                shutil.rmtree(str(child))

    print("Preparation completed: {}".format(output))
    print("Default theme: {}".format(default_theme["display_name"]))
    print("Clients download prepared-videos directly and do not need FFmpeg.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print("error: {}".format(error), file=sys.stderr)
        sys.exit(1)
