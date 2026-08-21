#!/usr/bin/env node

import { execFile } from "node:child_process";
import { mkdtemp, open, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

const execFileAsync = promisify(execFile);
const ANNEX_B_START_CODE = Buffer.from([0, 0, 0, 1]);

function fail(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

export function parseQualityArguments(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
      fail("invalid_arguments", `invalid option near ${option ?? "end"}`);
    }
    values[option.slice(2)] = value;
  }
  for (const required of ["source", "manifest"]) {
    if (!values[required]) fail("invalid_arguments", `--${required} is required`);
  }
  const sampleSeconds = Number(values["sample-seconds"] ?? 5);
  const minimumVmaf = Number(values["minimum-vmaf"] ?? 80);
  if (!Number.isFinite(sampleSeconds) || sampleSeconds <= 0) {
    fail("invalid_arguments", "--sample-seconds must be greater than zero");
  }
  if (!Number.isFinite(minimumVmaf) || minimumVmaf < 0 || minimumVmaf > 100) {
    fail("invalid_arguments", "--minimum-vmaf must be between 0 and 100");
  }
  return {
    source: path.resolve(values.source),
    manifest: path.resolve(values.manifest),
    ffmpeg: values.ffmpeg ? path.resolve(values.ffmpeg) : "ffmpeg",
    ffprobe: values.ffprobe ? path.resolve(values.ffprobe) : "ffprobe",
    sampleSeconds,
    minimumVmaf,
  };
}

async function run(command, args, label) {
  try {
    return await execFileAsync(command, args, {
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
      windowsHide: true,
    });
  } catch (error) {
    const details = String(error.stderr || error.stdout || error.message).trim();
    fail("quality_tool_failed", `${label} failed${details ? `: ${details}` : ""}`);
  }
}

function positiveInteger(value, label) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    fail("quality_manifest_invalid", `${label} must be a positive integer`);
  }
  return parsed;
}

export async function reconstructAnnexBSample(manifestPath, outputPath, sampleSeconds) {
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  if (manifest.codec !== "h264") {
    fail("quality_codec_unsupported", `quality gate requires H.264, received ${manifest.codec}`);
  }
  const numerator = positiveInteger(manifest.frame_rate_numerator, "frame_rate_numerator");
  const denominator = positiveInteger(manifest.frame_rate_denominator, "frame_rate_denominator");
  const frames = Array.isArray(manifest.frames) ? manifest.frames : [];
  const requestedFrames = Math.max(2, Math.ceil(sampleSeconds * numerator / denominator));
  const selectedFrames = frames.slice(0, requestedFrames);
  if (selectedFrames.length < 2) {
    fail("quality_frames_insufficient", "media manifest has fewer than two frames");
  }
  const manifestDirectory = path.dirname(path.resolve(manifestPath));
  const mediaFile = String(manifest.media_file ?? "");
  if (!mediaFile) fail("quality_manifest_invalid", "media_file is required");
  const mediaPath = path.resolve(manifestDirectory, mediaFile);
  const relativeMediaPath = path.relative(manifestDirectory, mediaPath);
  if (
    relativeMediaPath === ".." ||
    relativeMediaPath.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relativeMediaPath)
  ) {
    fail("quality_manifest_invalid", "media_file must remain inside the manifest directory");
  }
  const input = await open(mediaPath, "r");
  const output = await open(outputPath, "w");
  try {
    for (const [frameIndex, frame] of selectedFrames.entries()) {
      if (!Array.isArray(frame.nals) || frame.nals.length === 0) {
        fail("quality_manifest_invalid", `frame ${frameIndex} has no NAL units`);
      }
      for (const [nalIndex, nal] of frame.nals.entries()) {
        const offset = Number(nal.offset);
        const length = positiveInteger(nal.length, `frame ${frameIndex} NAL ${nalIndex} length`);
        if (!Number.isSafeInteger(offset) || offset < 0) {
          fail("quality_manifest_invalid", `frame ${frameIndex} NAL ${nalIndex} offset is invalid`);
        }
        const bytes = Buffer.allocUnsafe(length);
        const read = await input.read(bytes, 0, length, offset);
        if (read.bytesRead !== length) {
          fail("quality_media_truncated", `frame ${frameIndex} NAL ${nalIndex} is truncated`);
        }
        await output.write(ANNEX_B_START_CODE);
        await output.write(bytes);
      }
    }
  } finally {
    await Promise.allSettled([input.close(), output.close()]);
  }
  return {
    frameCount: selectedFrames.length,
    frameRateNumerator: numerator,
    frameRateDenominator: denominator,
  };
}

export function buildVmafFilter({ width, height, frameCount, numerator, denominator }) {
  const frameRate = `${numerator}/${denominator}`;
  const timeline = `N*${denominator}/(${numerator}*TB)`;
  return `[0:v]scale=${width}:${height}:force_original_aspect_ratio=decrease:flags=lanczos,` +
    `pad=${width}:${height}:(ow-iw)/2:(oh-ih)/2:black,fps=${frameRate},` +
    `trim=end_frame=${frameCount},setpts=${timeline}[reference];` +
    `[1:v]trim=end_frame=${frameCount},setpts=${timeline}[candidate];` +
    `[candidate][reference]libvmaf=shortest=1:n_threads=1`;
}

function vmafScore(stderr) {
  const match = String(stderr).match(/VMAF score:\s*([0-9]+(?:\.[0-9]+)?)/i);
  if (!match) fail("quality_score_missing", "FFmpeg did not report a VMAF score");
  return Number(match[1]);
}

export async function verifyMediaQuality(options) {
  const workDirectory = await mkdtemp(path.join(os.tmpdir(), "fst-media-quality-"));
  const samplePath = path.join(workDirectory, "candidate.h264");
  try {
    const sample = await reconstructAnnexBSample(
      options.manifest,
      samplePath,
      options.sampleSeconds,
    );
    const probe = await run(options.ffprobe, [
      "-v", "error", "-f", "h264", "-select_streams", "v:0",
      "-show_entries", "stream=width,height", "-of", "json", samplePath,
    ], "FFprobe candidate inspection");
    const stream = JSON.parse(probe.stdout).streams?.[0];
    const width = positiveInteger(stream?.width, "candidate width");
    const height = positiveInteger(stream?.height, "candidate height");
    const filter = buildVmafFilter({
      width,
      height,
      frameCount: sample.frameCount,
      numerator: sample.frameRateNumerator,
      denominator: sample.frameRateDenominator,
    });
    const result = await run(options.ffmpeg, [
      "-hide_banner", "-loglevel", "info",
      "-i", options.source,
      "-f", "h264", "-i", samplePath,
      "-filter_complex", filter,
      "-an", "-f", "null", "-",
    ], "FFmpeg VMAF quality gate");
    const score = vmafScore(result.stderr);
    const summary = {
      source: options.source,
      manifest: options.manifest,
      width,
      height,
      frame_rate: `${sample.frameRateNumerator}/${sample.frameRateDenominator}`,
      frames: sample.frameCount,
      vmaf: score,
      minimum_vmaf: options.minimumVmaf,
      passed: score >= options.minimumVmaf,
    };
    if (!summary.passed) {
      fail(
        "quality_gate_failed",
        `VMAF ${score.toFixed(3)} is below the required ${options.minimumVmaf.toFixed(3)}`,
      );
    }
    return summary;
  } finally {
    await rm(workDirectory, { recursive: true, force: true });
  }
}

async function main() {
  const summary = await verifyMediaQuality(parseQualityArguments(process.argv.slice(2)));
  process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    process.stderr.write(`[${error.code ?? "unexpected_error"}] ${error.message}\n`);
    process.exitCode = 1;
  });
}
