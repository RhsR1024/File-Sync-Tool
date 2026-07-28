import assert from "node:assert/strict";
import test from "node:test";

import { extractPcapMedia } from "./extract-pcap-media.mjs";
import { buildInterleavedPcap, splitAnnexBAccessUnit } from "./build-mp4-media-pack.mjs";

const evidence = {
  source_kind: "authorized_pcap",
  pcap_source_id: "provenance/test.pcap",
  sdp_source_id: "provenance/test.sdp",
  compatibility: "reviewed_static",
  verified_platforms: [],
  differences: [],
};

test("splits mixed Annex B start codes without retaining delimiters", () => {
  const bytes = Buffer.from([
    0, 0, 0, 1, 0x67, 1, 2, 3,
    0, 0, 1, 0x68, 4,
    0, 0, 0, 1, 0x65, 5, 6,
  ]);
  assert.deepEqual(splitAnnexBAccessUnit(bytes), [
    Buffer.from([0x67, 1, 2, 3]),
    Buffer.from([0x68, 4]),
    Buffer.from([0x65, 5, 6]),
  ]);
});

test("writes a TCP-interleaved PCAP that the approved extractor round-trips", () => {
  const largeIdr = Buffer.concat([Buffer.from([0x65]), Buffer.alloc(3_000, 0x5a)]);
  const frames = [
    {
      nals: [Buffer.from([0x09, 0xf0]), Buffer.from([0x67, 0x64, 0, 0x28]), Buffer.from([0x68, 0xee]), largeIdr],
      keyframe: true,
      durationTicks: 3_600,
    },
    {
      nals: [Buffer.from([0x09, 0x30]), Buffer.from([0x41, 1, 2, 3])],
      keyframe: false,
      durationTicks: 3_600,
    },
  ];
  const result = extractPcapMedia(buildInterleavedPcap(frames), {
    id: "roundtrip-main",
    mediaFile: "main.h264",
    payloadType: 105,
    evidence,
  });
  assert.equal(result.frameCount, 2);
  assert.equal(result.manifest.frame_rate_numerator, 25);
  assert.equal(result.manifest.frames[0].keyframe, true);
  assert.deepEqual(result.manifest.parameter_sets, [
    { kind: "sps", frame_index: 0, nal_index: 1 },
    { kind: "pps", frame_index: 0, nal_index: 2 },
  ]);
  assert.deepEqual(result.media, Buffer.concat(frames.flatMap((frame) => frame.nals)));
});
