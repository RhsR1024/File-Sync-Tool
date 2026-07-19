import assert from "node:assert/strict";
import test from "node:test";

import { extractPcapMedia, PcapMediaError } from "./extract-pcap-media.mjs";

function rtp({ sequence, timestamp, marker, payload, payloadType = 105, ssrc = 7 }) {
  const header = Buffer.alloc(12);
  header[0] = 0x80;
  header[1] = payloadType | (marker ? 0x80 : 0);
  header.writeUInt16BE(sequence, 2);
  header.writeUInt32BE(timestamp, 4);
  header.writeUInt32BE(ssrc, 8);
  return Buffer.concat([header, payload]);
}

function interleaved(packet, channel = 0) {
  const header = Buffer.alloc(4);
  header[0] = 0x24;
  header[1] = channel;
  header.writeUInt16BE(packet.length, 2);
  return Buffer.concat([header, packet]);
}

function tcpPacket(payload, sequence) {
  const packet = Buffer.alloc(14 + 20 + 20 + payload.length);
  packet.writeUInt16BE(0x0800, 12);
  const ip = 14;
  packet[ip] = 0x45;
  packet.writeUInt16BE(20 + 20 + payload.length, ip + 2);
  packet[ip + 8] = 64;
  packet[ip + 9] = 6;
  packet.set([192, 0, 2, 10], ip + 12);
  packet.set([192, 0, 2, 20], ip + 16);
  const tcp = ip + 20;
  packet.writeUInt16BE(554, tcp);
  packet.writeUInt16BE(50_000, tcp + 2);
  packet.writeUInt32BE(sequence, tcp + 4);
  packet[tcp + 12] = 0x50;
  packet.set(payload, tcp + 20);
  return packet;
}

function pcap(payloads) {
  const global = Buffer.alloc(24);
  global.writeUInt32LE(0xa1b2c3d4, 0);
  global.writeUInt16LE(2, 4);
  global.writeUInt16LE(4, 6);
  global.writeUInt32LE(65_535, 16);
  global.writeUInt32LE(1, 20);
  const records = [];
  let sequence = 1000;
  for (const payload of payloads) {
    const packet = tcpPacket(payload, sequence);
    sequence += payload.length;
    const header = Buffer.alloc(16);
    header.writeUInt32LE(packet.length, 8);
    header.writeUInt32LE(packet.length, 12);
    records.push(header, packet);
  }
  return Buffer.concat([global, ...records]);
}

const evidence = {
  source_kind: "authorized_pcap",
  pcap_source_id: "fixtures/capture.pcap",
  sdp_source_id: "fixtures/session.sdp",
  compatibility: "reviewed_static",
  verified_platforms: [],
  differences: [],
};

test("extracts single NAL, STAP-A, and FU-A packets into deterministic frame indexes", () => {
  const sps = Buffer.from([0x67, 0x42, 0x00, 0x1f]);
  const pps = Buffer.from([0x68, 0xce, 0x06, 0xe2]);
  const stap = Buffer.concat([
    Buffer.from([0x78, 0x00, sps.length]), sps,
    Buffer.from([0x00, pps.length]), pps,
  ]);
  const idr = Buffer.from([0x65, 1, 2, 3]);
  const fuStart = Buffer.from([0x7c, 0x85, 9, 8]);
  const fuEnd = Buffer.from([0x7c, 0x45, 7, 6]);
  const stream = Buffer.concat([
    Buffer.from("RTSP/1.0 200 OK\r\n\r\n"),
    interleaved(rtp({ sequence: 1, timestamp: 10_000, marker: false, payload: stap })),
    interleaved(rtp({ sequence: 2, timestamp: 10_000, marker: true, payload: idr })),
    interleaved(rtp({ sequence: 3, timestamp: 13_600, marker: false, payload: fuStart })),
    interleaved(rtp({ sequence: 4, timestamp: 13_600, marker: true, payload: fuEnd })),
    interleaved(rtp({ sequence: 5, timestamp: 17_200, marker: true, payload: Buffer.from([0x01]), payloadType: 107 })),
  ]);
  const capture = pcap([stream.subarray(0, 37), stream.subarray(37)]);
  const result = extractPcapMedia(capture, {
    id: "fixture-main",
    mediaFile: "main.h264",
    evidence,
    payloadType: 105,
  });
  assert.equal(result.frameCount, 2);
  assert.equal(result.manifest.frame_rate_numerator, 25);
  assert.equal(result.manifest.frame_rate_denominator, 1);
  assert.deepEqual(result.manifest.parameter_sets, [
    { kind: "sps", frame_index: 0, nal_index: 0 },
    { kind: "pps", frame_index: 0, nal_index: 1 },
  ]);
  assert.equal(result.manifest.frames[0].keyframe, true);
  assert.equal(result.manifest.frames[1].nals[0].nal_type, 5);
  assert.deepEqual(
    result.media.subarray(result.manifest.frames[1].offset),
    Buffer.from([0x65, 9, 8, 7, 6]),
  );
});

test("rejects captures without a complete interleaved RTP stream", () => {
  assert.throws(
    () => extractPcapMedia(pcap([Buffer.from("RTSP/1.0 200 OK\r\n\r\n")]), {
      id: "missing",
      mediaFile: "missing.h264",
      evidence,
    }),
    (error) => error instanceof PcapMediaError && error.code === "rtp_stream_missing",
  );
});
