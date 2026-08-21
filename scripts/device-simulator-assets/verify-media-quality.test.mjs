import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildVmafFilter,
  parseQualityArguments,
  reconstructAnnexBSample,
} from "./verify-media-quality.mjs";

test("reconstructs a bounded Annex B sample from indexed NAL bytes", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "fst-quality-test-"));
  try {
    const media = Buffer.from([0x09, 0xf0, 0x65, 1, 2, 0x09, 0x30, 0x41, 3]);
    await writeFile(path.join(directory, "main.h264"), media);
    const manifestPath = path.join(directory, "media.json");
    await writeFile(manifestPath, JSON.stringify({
      codec: "h264",
      frame_rate_numerator: 25,
      frame_rate_denominator: 1,
      media_file: "main.h264",
      frames: [
        { nals: [{ offset: 0, length: 2 }, { offset: 2, length: 3 }] },
        { nals: [{ offset: 5, length: 2 }, { offset: 7, length: 2 }] },
      ],
    }));
    const outputPath = path.join(directory, "sample.h264");
    const summary = await reconstructAnnexBSample(manifestPath, outputPath, 1);
    assert.equal(summary.frameCount, 2);
    assert.deepEqual(
      await readFile(outputPath),
      Buffer.from([
        0, 0, 0, 1, 0x09, 0xf0,
        0, 0, 0, 1, 0x65, 1, 2,
        0, 0, 0, 1, 0x09, 0x30,
        0, 0, 0, 1, 0x41, 3,
      ]),
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("VMAF filter compares the declared CFR timeline with Lanczos-scaled source", () => {
  const filter = buildVmafFilter({
    width: 1920,
    height: 1080,
    frameCount: 125,
    numerator: 25,
    denominator: 1,
  });
  assert.match(filter, /flags=lanczos/);
  assert.match(filter, /fps=25\/1/);
  assert.match(filter, /trim=end_frame=125/);
  assert.match(filter, /setpts=N\*1\/\(25\*TB\)/);
  assert.match(filter, /\[candidate\]\[reference\]libvmaf=shortest=1/);
});

test("rejects a manifest that tries to read media outside its directory", async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), "fst-quality-path-test-"));
  try {
    const manifestDirectory = path.join(directory, "manifest");
    await mkdir(manifestDirectory);
    const manifestPath = path.join(manifestDirectory, "media.json");
    await writeFile(manifestPath, JSON.stringify({
      codec: "h264",
      frame_rate_numerator: 25,
      frame_rate_denominator: 1,
      media_file: "../outside.h264",
      frames: [
        { nals: [{ offset: 0, length: 1 }] },
        { nals: [{ offset: 1, length: 1 }] },
      ],
    }));
    await assert.rejects(
      reconstructAnnexBSample(manifestPath, path.join(directory, "sample.h264"), 1),
      /media_file must remain inside/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("quality CLI defaults to a short, explicit VMAF gate", () => {
  const parsed = parseQualityArguments(["--source", "source.mp4", "--manifest", "media.json"]);
  assert.equal(parsed.sampleSeconds, 5);
  assert.equal(parsed.minimumVmaf, 80);
  assert.equal(parsed.ffmpeg, "ffmpeg");
  assert.equal(parsed.ffprobe, "ffprobe");
});
