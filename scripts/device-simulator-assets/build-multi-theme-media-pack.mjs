#!/usr/bin/env node

import { execFile } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";

import { buildMp4MediaPack, DEFAULT_STREAMS } from "./build-mp4-media-pack.mjs";
import { AssetReleaseError, buildPack } from "./lib.mjs";

const execFileAsync = promisify(execFile);
const THEME_ID = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/;
const NON_COMMERCIAL_USAGE = {
  scope: "non-commercial",
  notice: "Authorized for testing, learning, copying, and packaging; commercial use is prohibited.",
};

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
  for (const required of ["definition", "output-root", "version"]) {
    if (!values[required]) fail("invalid_arguments", `--${required} is required`);
  }
  return {
    definition: path.resolve(values.definition),
    outputRoot: path.resolve(values["output-root"]),
    version: values.version,
    legacyPack: values["legacy-pack"] ? path.resolve(values["legacy-pack"]) : null,
    ffmpeg: values.ffmpeg ? path.resolve(values.ffmpeg) : "ffmpeg",
    ffprobe: values.ffprobe ? path.resolve(values.ffprobe) : "ffprobe",
  };
}

async function readDefinition(definitionPath) {
  const document = JSON.parse(await readFile(definitionPath, "utf8"));
  if (document.schema_version !== 1 || !Array.isArray(document.themes) || document.themes.length === 0) {
    fail("definition_invalid", "multi-theme definition must contain at least one theme");
  }
  const ids = new Set();
  for (const theme of document.themes) {
    if (!THEME_ID.test(theme.id) || ids.has(theme.id)) {
      fail("definition_invalid", `invalid or duplicate theme id ${theme.id}`);
    }
    ids.add(theme.id);
    if (typeof theme.display_name_key !== "string" || !theme.display_name_key.trim()) {
      fail("definition_invalid", `theme ${theme.id} has no display_name_key`);
    }
    if (typeof theme.input !== "string" || !theme.input.trim()) {
      fail("definition_invalid", `theme ${theme.id} has no input MP4`);
    }
    theme.input = path.resolve(path.dirname(definitionPath), theme.input);
  }
  if (!ids.has(document.default_theme_id)) {
    fail("definition_invalid", "default_theme_id is absent from themes");
  }
  return document;
}

function streamPaths(themeId) {
  return {
    main: `media/themes/${themeId}/main/media.json`,
    sub: `media/themes/${themeId}/sub/media.json`,
    third: `media/themes/${themeId}/third/media.json`,
  };
}

export function themeCatalogDocument(definition, includeClassic) {
  const themes = definition.themes.map((theme) => ({
    id: theme.id,
    display_name_key: theme.display_name_key,
    streams: streamPaths(theme.id),
  }));
  if (includeClassic) {
    themes.unshift({
      id: "classic",
      display_name_key: "deviceSimulator.mediaThemes.classic",
      streams: streamPaths("classic"),
    });
  }
  return {
    schema_version: 1,
    default_theme_id: definition.default_theme_id,
    themes,
  };
}

async function rewriteEvidence(manifestPath, themeId, kind) {
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  manifest.evidence.pcap_source_id = `provenance/${themeId}/${kind}.pcap`;
  manifest.evidence.sdp_source_id = `provenance/${themeId}/${kind}.sdp`;
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
}

async function addClassicTheme(legacyPack, sourceMediaRoot, temporaryRoot) {
  const extracted = path.join(temporaryRoot, "legacy");
  await mkdir(extracted, { recursive: true });
  try {
    await execFileAsync("tar", ["-xf", legacyPack, "-C", extracted], { windowsHide: true });
  } catch (error) {
    fail("legacy_pack_extract_failed", String(error.stderr || error.message).trim());
  }
  for (const kind of ["main", "sub", "third"]) {
    await cp(
      path.join(extracted, "media", kind),
      path.join(sourceMediaRoot, "themes", "classic", kind),
      { recursive: true },
    );
  }
}

export async function buildMultiThemeMediaPack(options) {
  const definition = await readDefinition(options.definition);
  const temporaryRoot = await mkdtemp(path.join(os.tmpdir(), "fst-media-themes-"));
  const sourceRoot = path.join(options.outputRoot, "source", "media-h264-live");
  const sourceMediaRoot = path.join(sourceRoot, "media");
  const provenanceRoot = path.join(options.outputRoot, "provenance");
  const releaseRoot = path.join(options.outputRoot, "release");
  await Promise.all([sourceMediaRoot, provenanceRoot, releaseRoot].map((directory) => mkdir(directory, { recursive: true })));
  const results = [];
  try {
    if (options.legacyPack) await addClassicTheme(options.legacyPack, sourceMediaRoot, temporaryRoot);
    for (const theme of definition.themes) {
      const intermediateRoot = path.join(temporaryRoot, theme.id);
      const generated = await buildMp4MediaPack({
        input: theme.input,
        outputRoot: intermediateRoot,
        version: options.version,
        ffmpeg: options.ffmpeg,
        ffprobe: options.ffprobe,
        streams: DEFAULT_STREAMS.map((stream) => ({
          ...stream,
          id: `${theme.id}-${stream.kind}`,
        })),
      });
      for (const kind of ["main", "sub", "third"]) {
        const target = path.join(sourceMediaRoot, "themes", theme.id, kind);
        await cp(
          path.join(intermediateRoot, "source", "media-h264-live", "media", kind),
          target,
          { recursive: true },
        );
        await rewriteEvidence(path.join(target, "media.json"), theme.id, kind);
      }
      await cp(path.join(intermediateRoot, "provenance"), path.join(provenanceRoot, theme.id), { recursive: true });
      results.push({
        id: theme.id,
        display_name_key: theme.display_name_key,
        input: theme.input,
        input_sha256: generated.input_sha256,
        streams: generated.streams,
      });
    }

    const catalog = themeCatalogDocument(definition, Boolean(options.legacyPack));
    await writeFile(path.join(sourceMediaRoot, "themes.json"), `${JSON.stringify(catalog, null, 2)}\n`, "utf8");
    const packDefinition = path.join(options.outputRoot, "media-h264-live.pack.json");
    await writeFile(packDefinition, `${JSON.stringify({
      schema_version: 1,
      id: "media-h264-live",
      version: options.version,
      engine_api: 1,
      source_dir: sourceRoot,
      usage: NON_COMMERCIAL_USAGE,
    }, null, 2)}\n`, "utf8");
    const pack = await buildPack({ definitionPath: packDefinition, releaseRoot });
    const result = {
      pack,
      default_theme_id: catalog.default_theme_id,
      themes: catalog.themes,
      generated_themes: results,
      provenance_directory: provenanceRoot,
    };
    await writeFile(path.join(options.outputRoot, "build-result.json"), `${JSON.stringify(result, null, 2)}\n`, "utf8");
    return result;
  } finally {
    const resolvedTemporary = path.resolve(temporaryRoot);
    const resolvedSystemTemporary = `${path.resolve(os.tmpdir())}${path.sep}`;
    if (resolvedTemporary.startsWith(resolvedSystemTemporary) && path.basename(resolvedTemporary).startsWith("fst-media-themes-")) {
      await rm(resolvedTemporary, { recursive: true, force: true });
    }
  }
}

async function main() {
  const result = await buildMultiThemeMediaPack(parseArguments(process.argv.slice(2)));
  process.stdout.write(
    `Built multi-theme media pack ${result.pack.id}@${result.pack.version}\n` +
    `${result.pack.archive}\nsha256=${result.pack.sha256}\n` +
    `themes=${result.themes.map((theme) => theme.id).join(",")}\n`,
  );
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    const code = error instanceof AssetReleaseError ? error.code : error.code ?? "unexpected_error";
    process.stderr.write(`[${code}] ${error.message}\n`);
    process.exitCode = 1;
  });
}
