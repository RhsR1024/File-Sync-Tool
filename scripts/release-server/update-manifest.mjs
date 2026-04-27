import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  findLatestVersionedExe,
  getReleaseDirCandidates,
  loadTauriBuildContext,
} from "../versioned-exe-utils.mjs";

function formatReleaseDate(date = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(
    date.getDate()
  )}`;
}

async function hashFileSha256(filePath) {
  const hash = createHash("sha256");

  await new Promise((resolve, reject) => {
    const stream = createReadStream(filePath);

    stream.on("data", (chunk) => {
      hash.update(chunk);
    });
    stream.on("error", reject);
    stream.on("end", resolve);
  });

  return hash.digest("hex");
}

function isManifestEntry(value) {
  return (
    value &&
    typeof value === "object" &&
    typeof value.version === "string" &&
    typeof value.url === "string" &&
    typeof value.sha256 === "string" &&
    typeof value.released_at === "string" &&
    Array.isArray(value.changelog) &&
    value.changelog.every((item) => typeof item === "string")
  );
}

function parseExistingManifest(text, manifestPath) {
  let parsed;

  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(`Existing manifest is invalid JSON: ${manifestPath}`);
  }

  if (
    !parsed ||
    typeof parsed !== "object" ||
    typeof parsed.latest !== "string" ||
    !Array.isArray(parsed.versions)
  ) {
    throw new Error(`Existing manifest has invalid shape: ${manifestPath}`);
  }

  if (!parsed.versions.every(isManifestEntry)) {
    throw new Error(`Existing manifest has invalid version entries: ${manifestPath}`);
  }

  return parsed;
}

async function loadManifest(manifestPath) {
  try {
    await access(manifestPath);
  } catch (error) {
    if (error && error.code === "ENOENT") {
      return null;
    }
    throw error;
  }

  const content = await readFile(manifestPath, "utf8");
  return parseExistingManifest(content, manifestPath);
}

function mergeManifest(existingManifest, currentEntry) {
  const changelog =
    existingManifest?.versions.find(
      (entry) => entry.version === currentEntry.version
    )?.changelog ?? [];

  const mergedEntry = {
    ...currentEntry,
    changelog,
  };

  const remainingEntries =
    existingManifest?.versions.filter(
      (entry) => entry.version !== currentEntry.version
    ) ?? [];

  return {
    latest: currentEntry.version,
    versions: [mergedEntry, ...remainingEntries],
  };
}

async function writeManifest(manifestPath, manifest) {
  await mkdir(path.dirname(manifestPath), { recursive: true });
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}

export async function updateManifest(root = process.cwd()) {
  const { productName, version } = await loadTauriBuildContext(root);
  const releaseDirs = getReleaseDirCandidates(root);
  const latestExe = await findLatestVersionedExe(releaseDirs, productName, version);
  const sha256 = await hashFileSha256(latestExe.filePath);

  const manifestPath = path.join(root, "scripts", "release-server", "manifest.json");
  const existingManifest = await loadManifest(manifestPath);
  const nextManifest = mergeManifest(existingManifest, {
    version,
    url: latestExe.fileName,
    sha256,
    released_at: formatReleaseDate(),
    changelog: [],
  });

  await writeManifest(manifestPath, nextManifest);

  return {
    manifestPath,
    version,
    url: latestExe.fileName,
  };
}

async function main() {
  const result = await updateManifest();
  console.log(
    `Updated manifest.json -> latest=${result.version}, url=${result.url}`
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
