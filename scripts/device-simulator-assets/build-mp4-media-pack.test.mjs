import assert from "node:assert/strict";
import test from "node:test";

import { extractPcapMedia } from "./extract-pcap-media.mjs";
import {
  buildEncodeArguments,
  buildInterleavedPcap,
  DEFAULT_STREAMS,
  OFFLINE_H264_QUALITY,
  splitAnnexBAccessUnit,
} from "./build-mp4-media-pack.mjs";

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

test("offline quality profile improves detail without changing the media clock contract", () => {
  assert.deepEqual(
    DEFAULT_STREAMS.map(({ kind, width, height, fps, bitrate }) => ({ kind, width, height, fps, bitrate })),
    [
      { kind: "main", width: 1920, height: 1080, fps: 25, bitrate: 6_000_000 },
      { kind: "sub", width: 640, height: 360, fps: 20, bitrate: 1_000_000 },
      { kind: "third", width: 640, height: 360, fps: 20, bitrate: 1_000_000 },
    ],
  );
  assert.equal(OFFLINE_H264_QUALITY.preset, "medium");
  const args = buildEncodeArguments("input.mp4", "main.h264", DEFAULT_STREAMS[0]);
  assert.equal(args[args.indexOf("-preset") + 1], "medium");
  assert.match(args[args.indexOf("-vf") + 1], /flags=lanczos/);
  assert.equal(args[args.indexOf("-maxrate") + 1], "9000000");
  assert.equal(args[args.indexOf("-bufsize") + 1], "12000000");
  assert.equal(args[args.indexOf("-g") + 1], "50");
  assert.equal(args[args.indexOf("-keyint_min") + 1], "50");
  assert.equal(args[args.indexOf("-bf") + 1], "0");
  assert.equal(args[args.indexOf("-fps_mode") + 1], "cfr");
  assert.equal(args[args.indexOf("-x264-params") + 1], "repeat-headers=1:aud=1:open-gop=0");
});
