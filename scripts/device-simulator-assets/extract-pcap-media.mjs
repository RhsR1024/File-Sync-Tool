#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const PCAP_GLOBAL_HEADER_BYTES = 24;
const PCAP_PACKET_HEADER_BYTES = 16;
const ETHERNET_HEADER_BYTES = 14;
const RTP_FIXED_HEADER_BYTES = 12;
const VIDEO_CLOCK_RATE = 90_000;

export class PcapMediaError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "PcapMediaError";
    this.code = code;
  }
}

function fail(code, message) {
  throw new PcapMediaError(code, message);
}

function parsePcapHeader(bytes) {
  if (bytes.length < PCAP_GLOBAL_HEADER_BYTES) {
    fail("pcap_truncated", "PCAP global header is truncated");
  }
  const magic = bytes.readUInt32LE(0);
  if (magic === 0xa1b2c3d4 || magic === 0xa1b23c4d) {
    return { littleEndian: true, nanosecond: magic === 0xa1b23c4d };
  }
  const bigMagic = bytes.readUInt32BE(0);
  if (bigMagic === 0xa1b2c3d4 || bigMagic === 0xa1b23c4d) {
    return { littleEndian: false, nanosecond: bigMagic === 0xa1b23c4d };
  }
  fail("pcap_magic_unsupported", "only classic PCAP files are supported");
}

function readU32(bytes, offset, littleEndian) {
  return littleEndian ? bytes.readUInt32LE(offset) : bytes.readUInt32BE(offset);
}

function ipv4Text(bytes, offset) {
  return `${bytes[offset]}.${bytes[offset + 1]}.${bytes[offset + 2]}.${bytes[offset + 3]}`;
}

function parseTcpSegments(bytes) {
  const { littleEndian } = parsePcapHeader(bytes);
  const flows = new Map();
  let cursor = PCAP_GLOBAL_HEADER_BYTES;
  let packetIndex = 0;
  while (cursor < bytes.length) {
    if (cursor + PCAP_PACKET_HEADER_BYTES > bytes.length) {
      fail("pcap_truncated", `packet header ${packetIndex} is truncated`);
    }
    const capturedLength = readU32(bytes, cursor + 8, littleEndian);
    cursor += PCAP_PACKET_HEADER_BYTES;
    if (capturedLength === 0 || cursor + capturedLength > bytes.length) {
      fail("pcap_truncated", `packet ${packetIndex} has an invalid captured length`);
    }
    const packet = bytes.subarray(cursor, cursor + capturedLength);
    cursor += capturedLength;
    packetIndex += 1;

    if (packet.length < ETHERNET_HEADER_BYTES || packet.readUInt16BE(12) !== 0x0800) continue;
    const ipOffset = ETHERNET_HEADER_BYTES;
    const version = packet[ipOffset] >> 4;
    const ipHeaderLength = (packet[ipOffset] & 0x0f) * 4;
    if (version !== 4 || ipHeaderLength < 20 || ipOffset + ipHeaderLength > packet.length) continue;
    if (packet[ipOffset + 9] !== 6) continue;
    const totalLength = packet.readUInt16BE(ipOffset + 2);
    const ipEnd = Math.min(packet.length, ipOffset + totalLength);
    const tcpOffset = ipOffset + ipHeaderLength;
    if (tcpOffset + 20 > ipEnd) continue;
    const tcpHeaderLength = (packet[tcpOffset + 12] >> 4) * 4;
    if (tcpHeaderLength < 20 || tcpOffset + tcpHeaderLength > ipEnd) continue;
    const payload = packet.subarray(tcpOffset + tcpHeaderLength, ipEnd);
    if (payload.length === 0) continue;

    const sourceIp = ipv4Text(packet, ipOffset + 12);
    const destinationIp = ipv4Text(packet, ipOffset + 16);
    const sourcePort = packet.readUInt16BE(tcpOffset);
    const destinationPort = packet.readUInt16BE(tcpOffset + 2);
    const sequence = packet.readUInt32BE(tcpOffset + 4);
    const key = `${sourceIp}:${sourcePort}>${destinationIp}:${destinationPort}`;
    const flow = flows.get(key) ?? { key, segments: [] };
    flow.segments.push({ sequence, payload: Buffer.from(payload), packetIndex });
    flows.set(key, flow);
  }
  return [...flows.values()];
}

function reassembleFlow(flow) {
  const segments = [...flow.segments].sort((left, right) => (
    left.sequence - right.sequence || left.packetIndex - right.packetIndex
  ));
  if (segments.length === 0) return Buffer.alloc(0);
  const chunks = [];
  let expected = segments[0].sequence;
  for (const segment of segments) {
    const segmentEnd = segment.sequence + segment.payload.length;
    if (segmentEnd <= expected) continue;
    if (segment.sequence > expected) {
      fail(
        "tcp_stream_gap",
        `TCP flow ${flow.key} has a capture gap before sequence ${segment.sequence}`,
      );
    }
    const overlap = expected - segment.sequence;
    chunks.push(segment.payload.subarray(overlap));
    expected = segmentEnd;
  }
  return Buffer.concat(chunks);
}

function parseInterleavedRtp(stream, channel) {
  const packets = [];
  let cursor = 0;
  while (cursor + 4 <= stream.length) {
    const marker = stream.indexOf(0x24, cursor);
    if (marker < 0 || marker + 4 > stream.length) break;
    const frameChannel = stream[marker + 1];
    const length = stream.readUInt16BE(marker + 2);
    const end = marker + 4 + length;
    if (length < RTP_FIXED_HEADER_BYTES || end > stream.length) {
      cursor = marker + 1;
      continue;
    }
    const packet = stream.subarray(marker + 4, end);
    if ((packet[0] >> 6) !== 2) {
      cursor = marker + 1;
      continue;
    }
    if (frameChannel === channel) packets.push(parseRtpPacket(packet));
    cursor = end;
  }
  return packets;
}

function parseRtpPacket(packet) {
  const csrcCount = packet[0] & 0x0f;
  const hasPadding = (packet[0] & 0x20) !== 0;
  const hasExtension = (packet[0] & 0x10) !== 0;
  let offset = RTP_FIXED_HEADER_BYTES + csrcCount * 4;
  if (offset > packet.length) fail("rtp_header_invalid", "RTP CSRC list is truncated");
  if (hasExtension) {
    if (offset + 4 > packet.length) fail("rtp_header_invalid", "RTP extension is truncated");
    const extensionWords = packet.readUInt16BE(offset + 2);
    offset += 4 + extensionWords * 4;
    if (offset > packet.length) fail("rtp_header_invalid", "RTP extension payload is truncated");
  }
  let end = packet.length;
  if (hasPadding) {
    const padding = packet[packet.length - 1];
    if (padding === 0 || padding > end - offset) fail("rtp_padding_invalid", "RTP padding is invalid");
    end -= padding;
  }
  if (offset >= end) fail("rtp_payload_empty", "RTP packet has no H.264 payload");
  return {
    marker: (packet[1] & 0x80) !== 0,
    payloadType: packet[1] & 0x7f,
    sequence: packet.readUInt16BE(2),
    timestamp: packet.readUInt32BE(4),
    ssrc: packet.readUInt32BE(8),
    payload: Buffer.from(packet.subarray(offset, end)),
  };
}

function extendSequenceNumbers(packets) {
  let cycles = 0;
  let previous = null;
  for (const packet of packets) {
    if (previous !== null && previous > 60_000 && packet.sequence < 5_000) cycles += 65_536;
    packet.extendedSequence = cycles + packet.sequence;
    previous = packet.sequence;
  }
  packets.sort((left, right) => left.extendedSequence - right.extendedSequence);
}

function depacketizeH264(packets) {
  extendSequenceNumbers(packets);
  const frames = new Map();
  const fragmentState = new Map();
  const invalidTimestamps = new Set();
  for (const packet of packets) {
    const frame = frames.get(packet.timestamp) ?? {
      timestamp: packet.timestamp,
      firstSequence: packet.extendedSequence,
      markerSeen: false,
      nals: [],
    };
    frame.firstSequence = Math.min(frame.firstSequence, packet.extendedSequence);
    frame.markerSeen ||= packet.marker;
    frames.set(packet.timestamp, frame);

    const payload = packet.payload;
    const nalType = payload[0] & 0x1f;
    if (nalType >= 1 && nalType <= 23) {
      frame.nals.push({ sequence: packet.extendedSequence, bytes: payload });
      continue;
    }
    if (nalType === 24) {
      let offset = 1;
      let ordinal = 0;
      while (offset + 2 <= payload.length) {
        const length = payload.readUInt16BE(offset);
        offset += 2;
        if (length === 0 || offset + length > payload.length) {
          fail("h264_stap_invalid", "STAP-A contains an invalid NAL length");
        }
        frame.nals.push({
          sequence: packet.extendedSequence + ordinal / 1000,
          bytes: Buffer.from(payload.subarray(offset, offset + length)),
        });
        ordinal += 1;
        offset += length;
      }
      if (offset !== payload.length) fail("h264_stap_invalid", "STAP-A has trailing bytes");
      continue;
    }
    if (nalType === 28) {
      if (payload.length < 3) fail("h264_fua_invalid", "FU-A packet is too short");
      const fuHeader = payload[1];
      const start = (fuHeader & 0x80) !== 0;
      const end = (fuHeader & 0x40) !== 0;
      const reconstructedType = fuHeader & 0x1f;
      const key = `${packet.ssrc}:${packet.timestamp}:${reconstructedType}`;
      if (start) {
        if (fragmentState.has(key)) invalidTimestamps.add(packet.timestamp);
        fragmentState.set(key, {
          timestamp: packet.timestamp,
          firstSequence: packet.extendedSequence,
          chunks: [Buffer.from([(payload[0] & 0xe0) | reconstructedType]), payload.subarray(2)],
        });
      } else {
        const state = fragmentState.get(key);
        if (!state) {
          invalidTimestamps.add(packet.timestamp);
          continue;
        }
        state.chunks.push(payload.subarray(2));
      }
      if (end) {
        const state = fragmentState.get(key);
        if (!state) {
          invalidTimestamps.add(packet.timestamp);
          continue;
        }
        frame.nals.push({ sequence: state.firstSequence, bytes: Buffer.concat(state.chunks) });
        fragmentState.delete(key);
      }
      continue;
    }
    fail("h264_packetization_unsupported", `unsupported H.264 RTP NAL type ${nalType}`);
  }
  for (const state of fragmentState.values()) invalidTimestamps.add(state.timestamp);
  return [...frames.values()]
    .filter((frame) => frame.markerSeen && frame.nals.length > 0 && !invalidTimestamps.has(frame.timestamp))
    .sort((left, right) => left.firstSequence - right.firstSequence)
    .map((frame) => ({
      ...frame,
      nals: frame.nals.sort((left, right) => left.sequence - right.sequence).map((nal) => nal.bytes),
    }));
}

function timestampDelta(current, next) {
  return (next - current) >>> 0;
}

function mostCommonDelta(frames) {
  const counts = new Map();
  for (let index = 1; index < frames.length; index += 1) {
    const delta = timestampDelta(frames[index - 1].timestamp, frames[index].timestamp);
    if (delta > 0 && delta <= VIDEO_CLOCK_RATE * 10) counts.set(delta, (counts.get(delta) ?? 0) + 1);
  }
  if (counts.size === 0) fail("rtp_timestamps_invalid", "capture does not contain a stable frame interval");
  return [...counts].sort((left, right) => right[1] - left[1] || left[0] - right[0])[0][0];
}

function gcd(left, right) {
  while (right !== 0) [left, right] = [right, left % right];
  return left;
}

function normalizedMedia(frames, metadata) {
  if (frames.length < 2) fail("media_frames_insufficient", "at least two complete H.264 frames are required");
  const durationTicks = mostCommonDelta(frames);
  const divisor = gcd(VIDEO_CLOCK_RATE, durationTicks);
  const frameRateNumerator = VIDEO_CLOCK_RATE / divisor;
  const frameRateDenominator = durationTicks / divisor;
  const chunks = [];
  const frameIndex = [];
  const parameterSets = [];
  let offset = 0;
  for (let index = 0; index < frames.length; index += 1) {
    const frame = frames[index];
    const nals = [];
    const frameOffset = offset;
    let keyframe = false;
    for (let nalIndex = 0; nalIndex < frame.nals.length; nalIndex += 1) {
      const bytes = frame.nals[nalIndex];
      const nalType = bytes[0] & 0x1f;
      if (nalType === 5) keyframe = true;
      nals.push({ offset, length: bytes.length, nal_type: nalType });
      chunks.push(bytes);
      if (nalType === 7 && !parameterSets.some((entry) => entry.kind === "sps")) {
        parameterSets.push({ kind: "sps", frame_index: index, nal_index: nalIndex });
      }
      if (nalType === 8 && !parameterSets.some((entry) => entry.kind === "pps")) {
        parameterSets.push({ kind: "pps", frame_index: index, nal_index: nalIndex });
      }
      offset += bytes.length;
    }
    frameIndex.push({
      offset: frameOffset,
      length: offset - frameOffset,
      duration_ticks: durationTicks,
      keyframe,
      nals,
    });
  }
  if (!parameterSets.some((entry) => entry.kind === "sps") || !parameterSets.some((entry) => entry.kind === "pps")) {
    fail("h264_parameter_sets_missing", "capture does not contain both H.264 SPS and PPS NAL units");
  }
  const media = Buffer.concat(chunks);
  const durationSeconds = (frames.length * durationTicks) / VIDEO_CLOCK_RATE;
  const bitrate = Math.max(1_000, Math.round((media.length * 8) / durationSeconds));
  const manifest = {
    schema_version: 1,
    id: metadata.id,
    codec: "h264",
    clock_rate: VIDEO_CLOCK_RATE,
    payload_type: metadata.payloadType,
    frame_rate_numerator: frameRateNumerator,
    frame_rate_denominator: frameRateDenominator,
    recommended_bitrate_bps: bitrate,
    media_file: metadata.mediaFile,
    media_file_size: media.length,
    media_file_sha256: createHash("sha256").update(media).digest("hex"),
    frames: frameIndex,
    parameter_sets: parameterSets,
    evidence: metadata.evidence,
  };
  return { media, manifest, durationTicks, bitrate };
}

export function extractPcapMedia(bytes, options) {
  const channel = options.channel ?? 0;
  const flows = parseTcpSegments(bytes);
  const candidates = [];
  for (const flow of flows) {
    let stream;
    try {
      stream = reassembleFlow(flow);
    } catch (error) {
      if (error instanceof PcapMediaError && error.code === "tcp_stream_gap") continue;
      throw error;
    }
    const packets = parseInterleavedRtp(stream, channel);
    if (packets.length > 0) candidates.push({ flow: flow.key, packets });
  }
  if (candidates.length === 0) fail("rtp_stream_missing", `no interleaved RTP packets found on channel ${channel}`);
  candidates.sort((left, right) => right.packets.length - left.packets.length);
  const selected = candidates[0];
  const payloadCounts = new Map();
  for (const packet of selected.packets) {
    payloadCounts.set(packet.payloadType, (payloadCounts.get(packet.payloadType) ?? 0) + 1);
  }
  const payloadType = options.payloadType ?? [...payloadCounts]
    .sort((left, right) => right[1] - left[1] || left[0] - right[0])[0][0];
  const videoPackets = selected.packets.filter((packet) => packet.payloadType === payloadType);
  if (videoPackets.length === 0) {
    fail("rtp_payload_type_mismatch", `RTP payload type ${payloadType} is absent from the selected flow`);
  }
  const frames = depacketizeH264(videoPackets);
  const normalized = normalizedMedia(frames, {
    id: options.id,
    payloadType,
    mediaFile: options.mediaFile,
    evidence: options.evidence,
  });
  return {
    ...normalized,
    flow: selected.flow,
    rtpPackets: videoPackets.length,
    frameCount: frames.length,
  };
}

export async function extractPcapMediaFile({ input, outputDirectory, id, mediaFile, evidence, channel = 0, payloadType }) {
  const bytes = await readFile(input);
  const result = extractPcapMedia(bytes, { id, mediaFile, evidence, channel, payloadType });
  await mkdir(outputDirectory, { recursive: true });
  await writeFile(path.join(outputDirectory, mediaFile), result.media);
  await writeFile(
    path.join(outputDirectory, "media.json"),
    `${JSON.stringify(result.manifest, null, 2)}\n`,
  );
  return result;
}

function parseCli(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!option?.startsWith("--") || value === undefined) fail("invalid_arguments", `invalid option near ${option ?? "end"}`);
    values[option.slice(2)] = value;
  }
  for (const required of ["input", "output-dir", "id", "media-file", "pcap-source", "sdp-source"]) {
    if (!values[required]) fail("invalid_arguments", `--${required} is required`);
  }
  return values;
}

async function main() {
  const values = parseCli(process.argv.slice(2));
  const result = await extractPcapMediaFile({
    input: path.resolve(values.input),
    outputDirectory: path.resolve(values["output-dir"]),
    id: values.id,
    mediaFile: values["media-file"],
    channel: values.channel ? Number(values.channel) : 0,
    payloadType: values["payload-type"] ? Number(values["payload-type"]) : undefined,
    evidence: {
      source_kind: "authorized_pcap",
      pcap_source_id: values["pcap-source"],
      sdp_source_id: values["sdp-source"],
      compatibility: "reviewed_static",
      verified_platforms: [],
      differences: values.difference
        ? [JSON.parse(values.difference)]
        : [],
    },
  });
  process.stdout.write(
    `Extracted ${result.frameCount} H.264 frames from ${result.rtpPackets} RTP packets\n` +
    `flow=${result.flow}\nfps=${result.manifest.frame_rate_numerator}/${result.manifest.frame_rate_denominator}\n` +
    `bitrate=${result.bitrate}\nsha256=${result.manifest.media_file_sha256}\n`,
  );
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    const code = error instanceof PcapMediaError ? error.code : "unexpected_error";
    process.stderr.write(`[${code}] ${error.message}\n`);
    process.exitCode = 1;
  });
}
