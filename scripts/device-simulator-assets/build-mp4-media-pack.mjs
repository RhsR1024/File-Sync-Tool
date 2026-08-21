#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

import { extractPcapMediaFile, PcapMediaError } from "./extract-pcap-media.mjs";
import { AssetReleaseError, buildPack } from "./lib.mjs";

const execFileAsync = promisify(execFile);
const VIDEO_CLOCK_RATE = 90_000;
const RTP_PAYLOAD_TYPE = 105;
const RTP_MAX_NAL_PAYLOAD_BYTES = 1_200;
const NON_COMMERCIAL_USAGE = {
  scope: "non-commercial",
  notice: "Authorized for testing, learning, copying, and packaging; commercial use is prohibited.",
};

export const DEFAULT_STREAMS = [
  { kind: "main", id: "mp4-main", width: 1920, height: 1080, fps: 25, bitrate: 6_000_000, pathIndex: 1 },
  { kind: "sub", id: "mp4-sub", width: 640, height: 360, fps: 20, bitrate: 1_000_000, pathIndex: 2 },
  { kind: "third", id: "mp4-third", width: 640, height: 360, fps: 20, bitrate: 1_000_000, pathIndex: 3 },
];

export const OFFLINE_H264_QUALITY = Object.freeze({
  preset: "medium",
  scaleFlags: "lanczos",
  peakBitrateNumerator: 3,
  peakBitrateDenominator: 2,
  bufferSeconds: 2,
});

function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

function parseArguments(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
      fail("invalid_arguments", `invalid option near ${option ?? "end"}`);
    }
    values[option.slice(2)] = value;
  }
  for (const required of ["input", "output-root", "version"]) {
    if (!values[required]) fail("invalid_arguments", `--${required} is required`);
  }
  return {
    input: path.resolve(values.input),
    outputRoot: path.resolve(values["output-root"]),
    version: values.version,
    ffmpeg: values.ffmpeg ? path.resolve(values.ffmpeg) : "ffmpeg",
    ffprobe: values.ffprobe ? path.resolve(values.ffprobe) : "ffprobe",
  };
}

async function run(command, args, label) {
  try {
    const result = await execFileAsync(command, args, {
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
      windowsHide: true,
    });
    return result.stdout;
  } catch (error) {
    const details = String(error.stderr || error.stdout || error.message).trim();
    fail("tool_failed", `${label} failed${details ? `: ${details}` : ""}`);
  }
}

function findStartCode(bytes, from) {
  for (let index = from; index + 2 < bytes.length; index += 1) {
    if (bytes[index] !== 0 || bytes[index + 1] !== 0) continue;
    if (bytes[index + 2] === 1) return { index, length: 3 };
    if (index + 3 < bytes.length && bytes[index + 2] === 0 && bytes[index + 3] === 1) {
      return { index, length: 4 };
    }
  }
  return null;
}

export function splitAnnexBAccessUnit(bytes) {
  const nals = [];
  let marker = findStartCode(bytes, 0);
  if (!marker) fail("annex_b_start_code_missing", "H.264 access unit has no Annex B start code");
  if (marker.index !== 0 && bytes.subarray(0, marker.index).some((byte) => byte !== 0)) {
    fail("annex_b_prefix_invalid", "H.264 access unit has bytes before its first Annex B start code");
  }
  while (marker) {
    const nalStart = marker.index + marker.length;
    const next = findStartCode(bytes, nalStart);
    const nalEnd = next?.index ?? bytes.length;
    if (nalEnd > nalStart) nals.push(Buffer.from(bytes.subarray(nalStart, nalEnd)));
    marker = next;
  }
  if (nals.length === 0) fail("annex_b_empty", "H.264 access unit contains no NAL units");
  return nals;
}

function normalizeFrameRate(value) {
  const [numeratorText, denominatorText = "1"] = String(value).split("/");
  const numerator = Number(numeratorText);
  const denominator = Number(denominatorText);
  if (!Number.isInteger(numerator) || !Number.isInteger(denominator) || numerator <= 0 || denominator <= 0) {
    fail("frame_rate_invalid", `invalid frame rate ${value}`);
  }
  const scaled = VIDEO_CLOCK_RATE * denominator;
  if (scaled % numerator !== 0) {
    fail("frame_rate_invalid", `frame rate ${value} does not map exactly to the 90 kHz clock`);
  }
  return { numerator, denominator, durationTicks: scaled / numerator };
}

async function inspectInput(ffprobe, input) {
  const output = await run(ffprobe, [
    "-v", "error",
    "-select_streams", "v:0",
    "-show_entries", "format=duration,size:stream=codec_name,width,height,pix_fmt,avg_frame_rate",
    "-of", "json",
    input,
  ], "FFprobe input inspection");
  const document = JSON.parse(output);
  const stream = document.streams?.[0];
  if (!stream) fail("video_stream_missing", "input MP4 has no video stream");
  return { stream, format: document.format };
}

export function buildEncodeArguments(input, output, config) {
  const gop = config.fps * 2;
  const peakBitrate = Math.floor(
    config.bitrate * OFFLINE_H264_QUALITY.peakBitrateNumerator /
      OFFLINE_H264_QUALITY.peakBitrateDenominator,
  );
  const scale = `scale=${config.width}:${config.height}:force_original_aspect_ratio=decrease:` +
    `flags=${OFFLINE_H264_QUALITY.scaleFlags},` +
    `pad=${config.width}:${config.height}:(ow-iw)/2:(oh-ih)/2:black,fps=${config.fps}`;
  return [
    "-hide_banner", "-loglevel", "warning", "-y",
    "-i", input,
    "-map", "0:v:0", "-an",
    "-vf", scale,
    "-c:v", "libx264", "-preset", OFFLINE_H264_QUALITY.preset, "-profile:v", "high", "-pix_fmt", "yuv420p",
    "-threads", "1",
    "-b:v", String(config.bitrate), "-maxrate", String(peakBitrate),
    "-bufsize", String(config.bitrate * OFFLINE_H264_QUALITY.bufferSeconds),
    "-g", String(gop), "-keyint_min", String(gop), "-sc_threshold", "0", "-bf", "0",
    "-x264-params", "repeat-headers=1:aud=1:open-gop=0",
    "-fps_mode", "cfr", "-f", "h264",
    output,
  ];
}

async function encodeStream(ffmpeg, input, output, config) {
  await run(
    ffmpeg,
    buildEncodeArguments(input, output, config),
    `FFmpeg ${config.kind} encode`,
  );
}

async function readAccessUnits(ffprobe, mediaPath, fps) {
  const output = await run(ffprobe, [
    "-v", "error", "-f", "h264",
    "-select_streams", "v:0",
    "-show_packets", "-show_entries", "packet=pos,size,flags",
    "-of", "json", mediaPath,
  ], "FFprobe access-unit indexing");
  const document = JSON.parse(output);
  const media = await readFile(mediaPath);
  const packets = document.packets ?? [];
  if (packets.length < 2) fail("frames_insufficient", "encoded stream contains fewer than two frames");
  const frameRate = normalizeFrameRate(`${fps}/1`);
  const frames = packets.map((packet, index) => {
    const position = Number(packet.pos);
    const size = Number(packet.size);
    if (!Number.isSafeInteger(position) || !Number.isSafeInteger(size) || position < 0 || size <= 0 || position + size > media.length) {
      fail("packet_index_invalid", `FFprobe returned an invalid packet range for frame ${index}`);
    }
    const nals = splitAnnexBAccessUnit(media.subarray(position, position + size));
    const keyframe = nals.some((nal) => (nal[0] & 0x1f) === 5);
    if (index === 0 && !keyframe) fail("first_frame_not_keyframe", "encoded stream does not begin with an IDR frame");
    return { nals, keyframe, durationTicks: frameRate.durationTicks };
  });
  return frames;
}

function rtpPacketsForNal(nal, state) {
  if (nal.length <= RTP_MAX_NAL_PAYLOAD_BYTES) return [nal];
  const nalType = nal[0] & 0x1f;
  const indicator = (nal[0] & 0xe0) | 28;
  const packets = [];
  let offset = 1;
  const chunkBytes = RTP_MAX_NAL_PAYLOAD_BYTES - 2;
  while (offset < nal.length) {
    const end = Math.min(nal.length, offset + chunkBytes);
    const startBit = offset === 1 ? 0x80 : 0;
    const endBit = end === nal.length ? 0x40 : 0;
    packets.push(Buffer.concat([
      Buffer.from([indicator, startBit | endBit | nalType]),
      nal.subarray(offset, end),
    ]));
    offset = end;
  }
  if (packets.length === 0) fail("nal_packetization_failed", `failed to packetize NAL for sequence ${state.sequence}`);
  return packets;
}

function rtpPacket(payload, state, marker) {
  const packet = Buffer.alloc(12 + payload.length);
  packet[0] = 0x80;
  packet[1] = RTP_PAYLOAD_TYPE | (marker ? 0x80 : 0);
  packet.writeUInt16BE(state.sequence & 0xffff, 2);
  packet.writeUInt32BE(state.timestamp >>> 0, 4);
  packet.writeUInt32BE(state.ssrc >>> 0, 8);
  payload.copy(packet, 12);
  state.sequence = (state.sequence + 1) & 0xffff;
  return packet;
}

function tcpRecord(interleaved, state, recordIndex) {
  const ethernet = Buffer.from([
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
    0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb,
    0x08, 0x00,
  ]);
  const ipv4 = Buffer.alloc(20);
  ipv4[0] = 0x45;
  ipv4.writeUInt16BE(20 + 20 + interleaved.length, 2);
  ipv4.writeUInt16BE(recordIndex & 0xffff, 4);
  ipv4[8] = 64;
  ipv4[9] = 6;
  Buffer.from([192, 0, 2, 10]).copy(ipv4, 12);
  Buffer.from([192, 0, 2, 20]).copy(ipv4, 16);
  const tcp = Buffer.alloc(20);
  tcp.writeUInt16BE(554, 0);
  tcp.writeUInt16BE(50_000, 2);
  tcp.writeUInt32BE(state.tcpSequence >>> 0, 4);
  tcp[12] = 0x50;
  tcp[13] = 0x18;
  tcp.writeUInt16BE(65_535, 14);
  state.tcpSequence = (state.tcpSequence + interleaved.length) >>> 0;
  const packet = Buffer.concat([ethernet, ipv4, tcp, interleaved]);
  const header = Buffer.alloc(16);
  const totalMicroseconds = recordIndex * 1_000;
  header.writeUInt32LE(1_700_000_000 + Math.floor(totalMicroseconds / 1_000_000), 0);
  header.writeUInt32LE(totalMicroseconds % 1_000_000, 4);
  header.writeUInt32LE(packet.length, 8);
  header.writeUInt32LE(packet.length, 12);
  return Buffer.concat([header, packet]);
}

export function buildInterleavedPcap(frames, options = {}) {
  const state = {
    sequence: options.sequence ?? 1,
    timestamp: options.timestamp ?? 0,
    ssrc: options.ssrc ?? 0x10203040,
    tcpSequence: options.tcpSequence ?? 1,
  };
  const global = Buffer.alloc(24);
  global.writeUInt32LE(0xa1b2c3d4, 0);
  global.writeUInt16LE(2, 4);
  global.writeUInt16LE(4, 6);
  global.writeUInt32LE(65_535, 16);
  global.writeUInt32LE(1, 20);
  const records = [global];
  let recordIndex = 0;
  for (const frame of frames) {
    const payloads = frame.nals.flatMap((nal) => rtpPacketsForNal(nal, state));
    for (let index = 0; index < payloads.length; index += 1) {
      const packet = rtpPacket(payloads[index], state, index === payloads.length - 1);
      const interleaved = Buffer.alloc(4 + packet.length);
      interleaved[0] = 0x24;
      interleaved[1] = 0;
      interleaved.writeUInt16BE(packet.length, 2);
      packet.copy(interleaved, 4);
      records.push(tcpRecord(interleaved, state, recordIndex));
      recordIndex += 1;
    }
    state.timestamp = (state.timestamp + frame.durationTicks) >>> 0;
  }
  return Buffer.concat(records);
}

function parameterSet(frames, nalType) {
  return frames.flatMap((frame) => frame.nals).find((nal) => (nal[0] & 0x1f) === nalType);
}

function generatedSdp(frames, config) {
  const sps = parameterSet(frames, 7);
  const pps = parameterSet(frames, 8);
  if (!sps || !pps || sps.length < 4) fail("parameter_sets_missing", `${config.kind} stream has no usable SPS/PPS`);
  const profileLevelId = sps.subarray(1, 4).toString("hex");
  return [
    "v=0",
    "o=- 0 0 IN IP4 127.0.0.1",
    `s=Generated ${config.kind} stream`,
    "t=0 0",
    `m=video 0 RTP/AVP ${RTP_PAYLOAD_TYPE}`,
    `a=rtpmap:${RTP_PAYLOAD_TYPE} H264/${VIDEO_CLOCK_RATE}`,
    `a=fmtp:${RTP_PAYLOAD_TYPE} packetization-mode=1;profile-level-id=${profileLevelId};sprop-parameter-sets=${sps.toString("base64")},${pps.toString("base64")}`,
    `a=framerate:${config.fps}`,
    `a=control:/media/video${config.pathIndex}`,
    "",
  ].join("\r\n");
}

async function sha256(filePath) {
  return createHash("sha256").update(await readFile(filePath)).digest("hex");
}

export async function buildMp4MediaPack(options) {
  const sourceRoot = path.join(options.outputRoot, "source", "media-h264-live");
  const workRoot = path.join(options.outputRoot, "work");
  const provenanceRoot = path.join(options.outputRoot, "provenance");
  const releaseRoot = path.join(options.outputRoot, "release");
  await Promise.all([sourceRoot, workRoot, provenanceRoot, releaseRoot].map((directory) => mkdir(directory, { recursive: true })));

  const inputInfo = await inspectInput(options.ffprobe, options.input);
  const ffmpegVersion = (await run(options.ffmpeg, ["-version"], "FFmpeg version check")).split(/\r?\n/, 1)[0];
  const streamResults = [];
  for (const config of options.streams ?? DEFAULT_STREAMS) {
    const elementaryPath = path.join(workRoot, `${config.kind}.annexb.h264`);
    await encodeStream(options.ffmpeg, options.input, elementaryPath, config);
    const frames = await readAccessUnits(options.ffprobe, elementaryPath, config.fps);
    const pcapPath = path.join(provenanceRoot, `${config.kind}.pcap`);
    const sdpPath = path.join(provenanceRoot, `${config.kind}.sdp`);
    await writeFile(pcapPath, buildInterleavedPcap(frames, { ssrc: 0x10203040 + config.pathIndex }));
    await writeFile(sdpPath, generatedSdp(frames, config), "utf8");
    const extracted = await extractPcapMediaFile({
      input: pcapPath,
      outputDirectory: path.join(sourceRoot, "media", config.kind),
      id: config.id,
      mediaFile: `${config.kind}.h264`,
      payloadType: RTP_PAYLOAD_TYPE,
      evidence: {
        source_kind: "authorized_pcap",
        pcap_source_id: `provenance/${config.kind}.pcap`,
        sdp_source_id: `provenance/${config.kind}.sdp`,
        compatibility: "reviewed_static",
        verified_platforms: [],
        differences: [],
      },
    });
    streamResults.push({
      kind: config.kind,
      width: config.width,
      height: config.height,
      fps: config.fps,
      target_bitrate_bps: config.bitrate,
      actual_bitrate_bps: extracted.bitrate,
      frames: extracted.frameCount,
      duration_seconds: extracted.frameCount / config.fps,
      media_sha256: extracted.manifest.media_file_sha256,
    });
  }

  const definitionPath = path.join(options.outputRoot, "media-h264-live.pack.json");
  await writeFile(definitionPath, `${JSON.stringify({
    schema_version: 1,
    id: "media-h264-live",
    version: options.version,
    engine_api: 1,
    source_dir: sourceRoot,
    usage: NON_COMMERCIAL_USAGE,
  }, null, 2)}\n`, "utf8");
  const pack = await buildPack({ definitionPath, releaseRoot });
  const result = {
    input: options.input,
    input_sha256: await sha256(options.input),
    input_media: inputInfo,
    ffmpeg_version: ffmpegVersion,
    pack,
    streams: streamResults,
    provenance_directory: provenanceRoot,
  };
  await writeFile(path.join(options.outputRoot, "build-result.json"), `${JSON.stringify(result, null, 2)}\n`, "utf8");
  return result;
}

async function main() {
  const result = await buildMp4MediaPack(parseArguments(process.argv.slice(2)));
  process.stdout.write(
    `Built MP4-derived media pack ${result.pack.id}@${result.pack.version}\n` +
    `${result.pack.archive}\nsha256=${result.pack.sha256}\n`,
  );
  for (const stream of result.streams) {
    process.stdout.write(
      `${stream.kind}: ${stream.width}x${stream.height} ${stream.fps}fps ` +
      `${stream.frames} frames ${stream.actual_bitrate_bps}bps\n`,
    );
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    const code = error instanceof PcapMediaError || error instanceof AssetReleaseError
      ? error.code
      : error.code ?? "unexpected_error";
    process.stderr.write(`[${code}] ${error.message}\n`);
    process.exitCode = 1;
  });
}
