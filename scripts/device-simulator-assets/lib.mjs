import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign as ed25519Sign,
  verify as ed25519Verify,
} from "node:crypto";
import {
  createReadStream,
  createWriteStream,
} from "node:fs";
import {
  access,
  lstat,
  mkdir,
  open,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

export const SCHEMA_VERSION = 1;
export const ENGINE_API = 1;
export const SIGNATURE_VERSION = 1;
export const NON_COMMERCIAL_USAGE = Object.freeze({
  scope: "non-commercial",
  notice:
    "Authorized for testing, learning, copying, and packaging; commercial use is prohibited.",
});

const MAX_PACK_BYTES = 2 * 1024 * 1024 * 1024;
const MAX_UNPACKED_BYTES = 8 * 1024 * 1024 * 1024;
const MAX_FILE_BYTES = 1024 * 1024 * 1024;
const MAX_FILES = 10_000;
const MAX_PATH_LENGTH = 512;
const MAX_PATH_SEGMENT_LENGTH = 255;
const FORBIDDEN_EXTENSIONS = new Set([
  "exe",
  "dll",
  "py",
  "js",
  "bat",
  "cmd",
  "ps1",
  "wasm",
  "msi",
  "scr",
  "com",
]);
const PACK_KINDS = new Set(["protocol-core", "media", "device-profile"]);
const DEVICE_KINDS = new Set(["ipc", "nvr"]);
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;
const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;
const ID = /^[a-z](?:[a-z0-9-]{0,62}[a-z0-9])?$/;
const SHA256 = /^[0-9a-f]{64}$/;
const DOS_DATE_1980_01_01 = 0x21;
const ZIP_UTF8_FLAG = 0x0800;
const ZIP_LOCAL_FILE = 0x04034b50;
const ZIP_CENTRAL_FILE = 0x02014b50;
const ZIP_END = 0x06054b50;

let crcTable;

export class AssetReleaseError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "AssetReleaseError";
    this.code = code;
  }
}

function fail(code, message) {
  throw new AssetReleaseError(code, message);
}

function assertObject(value, subject) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    fail("invalid_document", `${subject} must be a JSON object`);
  }
}

function assertExactKeys(value, allowed, subject) {
  assertObject(value, subject);
  const extra = Object.keys(value).filter((key) => !allowed.includes(key));
  const missing = allowed.filter((key) => !(key in value));
  if (extra.length || missing.length) {
    fail(
      "invalid_document",
      `${subject} keys are invalid (missing: ${missing.join(", ") || "none"}; extra: ${extra.join(", ") || "none"})`,
    );
  }
}

function assertInteger(value, subject, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum) {
    fail("invalid_document", `${subject} must be an integer >= ${minimum}`);
  }
}

function assertId(value, subject) {
  if (typeof value !== "string" || !ID.test(value)) {
    fail(
      "invalid_id",
      `${subject} must be 1-64 lowercase ASCII letters, digits, or internal hyphens and start with a letter`,
    );
  }
}

function assertSemver(value, subject) {
  const match = typeof value === "string" && value.length <= 128 ? SEMVER.exec(value) : null;
  const prerelease = match?.[4]?.split(".") ?? [];
  const invalidNumericPrerelease = prerelease.some(
    (identifier) => /^\d+$/.test(identifier) && identifier.length > 1 && identifier.startsWith("0"),
  );
  if (!match || invalidNumericPrerelease) {
    fail("invalid_version", `${subject} must be a bounded semantic version`);
  }
}

function assertRfc3339(value, subject) {
  if (typeof value !== "string" || !RFC3339.test(value) || Number.isNaN(Date.parse(value))) {
    fail("invalid_generated_at", `${subject} must be RFC 3339 with an explicit timezone`);
  }
}

function assertSha256(value, subject) {
  if (typeof value !== "string" || !SHA256.test(value)) {
    fail("invalid_sha256", `${subject} must be 64 lowercase hexadecimal characters`);
  }
}

function assertUsage(value, subject = "usage") {
  assertExactKeys(value, ["scope", "notice"], subject);
  if (
    value.scope !== NON_COMMERCIAL_USAGE.scope ||
    value.notice !== NON_COMMERCIAL_USAGE.notice
  ) {
    fail(
      "usage_policy_invalid",
      `${subject} must preserve the approved non-commercial usage scope and notice`,
    );
  }
}

function isWindowsDeviceName(component) {
  const stem = component.split(".")[0].toLowerCase();
  return (
    ["con", "prn", "aux", "nul"].includes(stem) ||
    /^(?:com|lpt)[1-9]$/.test(stem)
  );
}

export function validatePackPath(relativePath) {
  if (
    typeof relativePath !== "string" ||
    relativePath.length === 0 ||
    relativePath.length > MAX_PATH_LENGTH
  ) {
    fail("invalid_pack_path", `pack path is empty or longer than ${MAX_PATH_LENGTH}`);
  }
  if (
    relativePath.startsWith("/") ||
    relativePath.startsWith("//") ||
    relativePath.includes("\\") ||
    /^[A-Za-z]:/.test(relativePath) ||
    [...relativePath].some((character) => {
      const code = character.charCodeAt(0);
      return code === 0 || code < 0x20 || code === 0x7f;
    })
  ) {
    fail("invalid_pack_path", `pack path is not normalized: ${relativePath}`);
  }

  const components = relativePath.split("/");
  for (const component of components) {
    if (
      !component ||
      component === "." ||
      component === ".." ||
      component.length > MAX_PATH_SEGMENT_LENGTH ||
      component.endsWith(".") ||
      component.endsWith(" ") ||
      /[<>:"|?*]/.test(component) ||
      isWindowsDeviceName(component)
    ) {
      fail("invalid_pack_path", `invalid Windows-safe pack path: ${relativePath}`);
    }
  }

  const fileName = components.at(-1);
  const extension = fileName.includes(".")
    ? fileName.slice(fileName.lastIndexOf(".") + 1).toLowerCase()
    : "";
  if (FORBIDDEN_EXTENSIONS.has(extension)) {
    fail("forbidden_file_type", `executable asset file type is forbidden: ${relativePath}`);
  }
}

function jsonBytes(value) {
  return Buffer.from(`${JSON.stringify(value, null, 2)}\n`, "utf8");
}

function compareUtf8(left, right) {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

async function parseJsonFile(filePath, subject) {
  let bytes;
  try {
    bytes = await readFile(filePath);
  } catch (error) {
    fail("file_read_failed", `failed to read ${subject} ${filePath}: ${error.message}`);
  }
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    fail("invalid_json", `${subject} ${filePath} is invalid JSON: ${error.message}`);
  }
}

async function pathExists(filePath) {
  try {
    await access(filePath);
    return true;
  } catch {
    return false;
  }
}

function crc32Update(crc, chunk) {
  if (!crcTable) {
    crcTable = new Uint32Array(256);
    for (let value = 0; value < 256; value += 1) {
      let current = value;
      for (let bit = 0; bit < 8; bit += 1) {
        current = current & 1 ? 0xedb88320 ^ (current >>> 1) : current >>> 1;
      }
      crcTable[value] = current >>> 0;
    }
  }
  let current = crc;
  for (const byte of chunk) {
    current = crcTable[(current ^ byte) & 0xff] ^ (current >>> 8);
  }
  return current >>> 0;
}

async function digestFile(filePath, expectedSize) {
  const hash = createHash("sha256");
  let crc = 0xffffffff;
  let size = 0;
  for await (const chunk of createReadStream(filePath)) {
    size += chunk.length;
    if (size > MAX_FILE_BYTES) {
      fail("size_limit_exceeded", `${filePath} exceeds ${MAX_FILE_BYTES} bytes`);
    }
    hash.update(chunk);
    crc = crc32Update(crc, chunk);
  }
  if (size !== expectedSize) {
    fail("source_changed", `source file changed while hashing: ${filePath}`);
  }
  return {
    crc32: (crc ^ 0xffffffff) >>> 0,
    sha256: hash.digest("hex"),
    size,
  };
}

async function collectSourceFiles(sourceRoot) {
  const rootInfo = await lstat(sourceRoot).catch((error) => {
    fail("source_invalid", `cannot inspect source directory ${sourceRoot}: ${error.message}`);
  });
  if (!rootInfo.isDirectory() || rootInfo.isSymbolicLink()) {
    fail("source_invalid", `pack source must be a real directory: ${sourceRoot}`);
  }

  const collected = [];
  const caseInsensitivePaths = new Set();
  async function visit(directory, prefix) {
    const entries = await readdir(directory, { withFileTypes: true });
    entries.sort((left, right) => compareUtf8(left.name, right.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isSymbolicLink()) {
        fail("symlink_forbidden", `symbolic links are forbidden in packs: ${relative}`);
      }
      if (entry.isDirectory()) {
        await visit(absolute, relative);
        continue;
      }
      if (!entry.isFile()) {
        fail("source_invalid", `unsupported source entry type: ${relative}`);
      }
      if (relative.toLowerCase() === "pack.json") {
        fail("reserved_path", "source directory must not contain pack.json");
      }
      validatePackPath(relative);
      const key = relative.toLowerCase();
      if (caseInsensitivePaths.has(key)) {
        fail("duplicate_file", `case-insensitive duplicate pack path: ${relative}`);
      }
      caseInsensitivePaths.add(key);
      const info = await lstat(absolute);
      if (info.size <= 0 || info.size > MAX_FILE_BYTES) {
        fail(
          "size_limit_exceeded",
          `${relative} size ${info.size} must be between 1 and ${MAX_FILE_BYTES}`,
        );
      }
      const digest = await digestFile(absolute, info.size);
      collected.push({ absolute, relative, ...digest });
      if (collected.length > MAX_FILES) {
        fail("too_many_files", `pack contains more than ${MAX_FILES} payload files`);
      }
    }
  }
  await visit(sourceRoot, "");
  if (collected.length === 0) {
    fail("empty_pack", "pack must contain at least one payload file");
  }
  collected.sort((left, right) => compareUtf8(left.relative, right.relative));
  const unpackedSize = collected.reduce((total, file) => total + file.size, 0);
  if (unpackedSize > MAX_UNPACKED_BYTES) {
    fail("size_limit_exceeded", `pack payload exceeds ${MAX_UNPACKED_BYTES} bytes`);
  }
  return { files: collected, unpackedSize };
}

function localHeader(nameBytes, file) {
  const header = Buffer.alloc(30 + nameBytes.length);
  header.writeUInt32LE(ZIP_LOCAL_FILE, 0);
  header.writeUInt16LE(20, 4);
  header.writeUInt16LE(ZIP_UTF8_FLAG, 6);
  header.writeUInt16LE(0, 8);
  header.writeUInt16LE(0, 10);
  header.writeUInt16LE(DOS_DATE_1980_01_01, 12);
  header.writeUInt32LE(file.crc32, 14);
  header.writeUInt32LE(file.size, 18);
  header.writeUInt32LE(file.size, 22);
  header.writeUInt16LE(nameBytes.length, 26);
  header.writeUInt16LE(0, 28);
  nameBytes.copy(header, 30);
  return header;
}

function centralHeader(nameBytes, file, offset) {
  const header = Buffer.alloc(46 + nameBytes.length);
  header.writeUInt32LE(ZIP_CENTRAL_FILE, 0);
  header.writeUInt16LE(0x0314, 4);
  header.writeUInt16LE(20, 6);
  header.writeUInt16LE(ZIP_UTF8_FLAG, 8);
  header.writeUInt16LE(0, 10);
  header.writeUInt16LE(0, 12);
  header.writeUInt16LE(DOS_DATE_1980_01_01, 14);
  header.writeUInt32LE(file.crc32, 16);
  header.writeUInt32LE(file.size, 20);
  header.writeUInt32LE(file.size, 24);
  header.writeUInt16LE(nameBytes.length, 28);
  header.writeUInt16LE(0, 30);
  header.writeUInt16LE(0, 32);
  header.writeUInt16LE(0, 34);
  header.writeUInt16LE(0, 36);
  header.writeUInt32LE((0o100644 << 16) >>> 0, 38);
  header.writeUInt32LE(offset, 42);
  nameBytes.copy(header, 46);
  return header;
}

function endHeader(entryCount, centralSize, centralOffset) {
  const header = Buffer.alloc(22);
  header.writeUInt32LE(ZIP_END, 0);
  header.writeUInt16LE(0, 4);
  header.writeUInt16LE(0, 6);
  header.writeUInt16LE(entryCount, 8);
  header.writeUInt16LE(entryCount, 10);
  header.writeUInt32LE(centralSize, 12);
  header.writeUInt32LE(centralOffset, 16);
  header.writeUInt16LE(0, 20);
  return header;
}

async function writeChunk(stream, chunk) {
  if (!stream.write(chunk)) {
    await new Promise((resolve, reject) => {
      const cleanup = () => {
        stream.off("drain", onDrain);
        stream.off("error", onError);
      };
      const onDrain = () => {
        cleanup();
        resolve();
      };
      const onError = (error) => {
        cleanup();
        reject(error);
      };
      stream.once("drain", onDrain);
      stream.once("error", onError);
    });
  }
}

async function createStoredZip(outputPath, entries) {
  await mkdir(path.dirname(outputPath), { recursive: true });
  const stream = createWriteStream(outputPath, { flags: "wx" });
  const central = [];
  let offset = 0;
  try {
    for (const entry of entries) {
      const nameBytes = Buffer.from(entry.relative, "utf8");
      const header = localHeader(nameBytes, entry);
      await writeChunk(stream, header);
      const entryOffset = offset;
      offset += header.length;
      if (entry.bytes) {
        await writeChunk(stream, entry.bytes);
      } else {
        for await (const chunk of createReadStream(entry.absolute)) {
          await writeChunk(stream, chunk);
        }
      }
      offset += entry.size;
      central.push(centralHeader(nameBytes, entry, entryOffset));
      if (offset > 0xffffffff) {
        fail("zip64_unsupported", "pack exceeds deterministic ZIP32 limits");
      }
    }
    const centralOffset = offset;
    for (const header of central) {
      await writeChunk(stream, header);
      offset += header.length;
    }
    const centralSize = offset - centralOffset;
    await writeChunk(stream, endHeader(entries.length, centralSize, centralOffset));
    await new Promise((resolve, reject) => {
      stream.end(resolve);
      stream.once("error", reject);
    });
  } catch (error) {
    stream.destroy();
    await rm(outputPath, { force: true });
    throw error;
  }
}

function manifestEntry(manifestBytes) {
  let crc = 0xffffffff;
  crc = crc32Update(crc, manifestBytes);
  return {
    relative: "pack.json",
    bytes: manifestBytes,
    size: manifestBytes.length,
    crc32: (crc ^ 0xffffffff) >>> 0,
  };
}

export async function loadPackDefinition(definitionPath) {
  const definition = await parseJsonFile(definitionPath, "pack definition");
  assertExactKeys(
    definition,
    ["schema_version", "id", "version", "engine_api", "source_dir", "usage"],
    "pack definition",
  );
  if (definition.schema_version !== SCHEMA_VERSION) {
    fail("schema_unsupported", `pack definition schema must be ${SCHEMA_VERSION}`);
  }
  if (definition.engine_api !== ENGINE_API) {
    fail("engine_api_unsupported", `pack engine_api must be ${ENGINE_API}`);
  }
  assertId(definition.id, "pack id");
  assertSemver(definition.version, "pack version");
  if (typeof definition.source_dir !== "string" || !definition.source_dir.trim()) {
    fail("source_invalid", "pack source_dir must be a non-empty path");
  }
  assertUsage(definition.usage, "pack definition usage");
  return definition;
}

export async function buildPack({ definitionPath, releaseRoot }) {
  const definition = await loadPackDefinition(definitionPath);
  const sourceRoot = path.resolve(path.dirname(definitionPath), definition.source_dir);
  const { files, unpackedSize } = await collectSourceFiles(sourceRoot);
  const manifest = {
    schema_version: SCHEMA_VERSION,
    id: definition.id,
    version: definition.version,
    engine_api: ENGINE_API,
    usage: { ...NON_COMMERCIAL_USAGE },
    files: files.map(({ relative, sha256, size }) => ({
      path: relative,
      sha256,
      size,
    })),
  };
  const manifestBytes = jsonBytes(manifest);
  const target = packArchivePath(releaseRoot, definition.id, definition.version);
  const temporary = `${target}.building-${process.pid}-${Date.now()}`;
  await createStoredZip(temporary, [manifestEntry(manifestBytes), ...files]);
  try {
    const archiveInfo = await stat(temporary);
    if (archiveInfo.size <= 0 || archiveInfo.size > MAX_PACK_BYTES) {
      fail("size_limit_exceeded", `pack archive size exceeds ${MAX_PACK_BYTES} bytes`);
    }
    const inspected = await inspectPackArchive(temporary, {
      expectedId: definition.id,
      expectedVersion: definition.version,
      expectedEngineApi: ENGINE_API,
    });
    if (inspected.unpackedSize !== unpackedSize) {
      fail("source_changed", "pack source changed while the archive was being generated");
    }
    const sha256 = await hashFile(temporary);

    if (await pathExists(target)) {
      const existingHash = await hashFile(target);
      if (existingHash !== sha256) {
        fail(
          "immutable_pack_conflict",
          `${definition.id}@${definition.version} already exists with different bytes; publish a new version`,
        );
      }
    } else {
      await rename(temporary, target);
    }

    return {
      archive: target,
      id: definition.id,
      version: definition.version,
      sha256,
      size: archiveInfo.size,
      unpacked_size: unpackedSize,
    };
  } finally {
    await rm(temporary, { force: true });
  }
}

export function packArchivePath(releaseRoot, id, version) {
  return path.join(releaseRoot, "packs", id, version, `${id}-${version}.zip`);
}

export async function hashFile(filePath) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) {
    hash.update(chunk);
  }
  return hash.digest("hex");
}

function assertPackRef(value, subject) {
  if (typeof value !== "string" || !value.includes("@")) {
    fail("invalid_pack_ref", `${subject} must use id@version`);
  }
  const at = value.lastIndexOf("@");
  const id = value.slice(0, at);
  const version = value.slice(at + 1);
  assertId(id, `${subject} id`);
  assertSemver(version, `${subject} version`);
  return { id, version };
}

function assertUnique(values, key, subject) {
  const seen = new Set();
  for (const value of values) {
    const identity = key(value).toLowerCase();
    if (seen.has(identity)) {
      fail("duplicate_entry", `${subject} repeats ${identity}`);
    }
    seen.add(identity);
  }
}

function validateCatalogSource(source) {
  assertExactKeys(
    source,
    ["schema_version", "generated_at", "engine_api", "packs", "profiles"],
    "catalog source",
  );
  if (source.schema_version !== SCHEMA_VERSION) {
    fail("schema_unsupported", `catalog schema must be ${SCHEMA_VERSION}`);
  }
  if (source.engine_api !== ENGINE_API) {
    fail("engine_api_unsupported", `catalog engine_api must be ${ENGINE_API}`);
  }
  assertRfc3339(source.generated_at, "catalog generated_at");
  if (!Array.isArray(source.packs) || !Array.isArray(source.profiles)) {
    fail("invalid_document", "catalog packs and profiles must be arrays");
  }
  for (const pack of source.packs) {
    assertExactKeys(
      pack,
      ["id", "version", "kind", "dependencies", "min_app_version"],
      "catalog source pack",
    );
    assertId(pack.id, "pack id");
    assertSemver(pack.version, "pack version");
    assertSemver(pack.min_app_version, "minimum application version");
    if (!PACK_KINDS.has(pack.kind)) {
      fail("invalid_pack_kind", `invalid pack kind ${pack.kind}`);
    }
    if (!Array.isArray(pack.dependencies)) {
      fail("invalid_document", `dependencies for ${pack.id} must be an array`);
    }
    pack.dependencies.forEach((reference) => assertPackRef(reference, "dependency"));
    assertUnique(pack.dependencies, (value) => value, `dependencies for ${pack.id}`);
  }
  assertUnique(source.packs, (pack) => `${pack.id}@${pack.version}`, "catalog packs");

  for (const profile of source.profiles) {
    assertExactKeys(profile, ["id", "device_kind", "required_packs"], "catalog profile");
    assertId(profile.id, "profile id");
    if (!DEVICE_KINDS.has(profile.device_kind)) {
      fail("invalid_device_kind", `invalid device kind ${profile.device_kind}`);
    }
    if (!Array.isArray(profile.required_packs) || profile.required_packs.length === 0) {
      fail("invalid_document", `profile ${profile.id} must require at least one pack`);
    }
    profile.required_packs.forEach((reference) => assertPackRef(reference, "required pack"));
    assertUnique(profile.required_packs, (value) => value, `required packs for ${profile.id}`);
  }
  assertUnique(source.profiles, (profile) => profile.id, "catalog profiles");

  const known = new Set(source.packs.map((pack) => `${pack.id}@${pack.version}`));
  for (const pack of source.packs) {
    for (const dependency of pack.dependencies) {
      if (!known.has(dependency)) {
        fail("pack_ref_missing", `${pack.id}@${pack.version} depends on missing ${dependency}`);
      }
    }
  }
  for (const profile of source.profiles) {
    for (const required of profile.required_packs) {
      if (!known.has(required)) {
        fail("pack_ref_missing", `${profile.id} requires missing ${required}`);
      }
    }
  }
  validateDependencyCycles(source.packs);
}

function validateDependencyCycles(packs) {
  const byRef = new Map(packs.map((pack) => [`${pack.id}@${pack.version}`, pack]));
  const states = new Map();
  function visit(reference, stack) {
    if (states.get(reference) === 2) return;
    if (states.get(reference) === 1) {
      fail("dependency_cycle", `pack dependency cycle: ${[...stack, reference].join(" -> ")}`);
    }
    states.set(reference, 1);
    const pack = byRef.get(reference);
    for (const dependency of pack.dependencies) visit(dependency, [...stack, reference]);
    states.set(reference, 2);
  }
  for (const reference of byRef.keys()) visit(reference, []);
}

export async function buildCatalog({ definitionPath, releaseRoot, outputPath }) {
  const source = await parseJsonFile(definitionPath, "catalog source");
  validateCatalogSource(source);
  const packs = [];
  const sortedPacks = [...source.packs].sort((left, right) =>
    compareUtf8(`${left.id}@${left.version}`, `${right.id}@${right.version}`),
  );
  for (const sourcePack of sortedPacks) {
    const archive = packArchivePath(releaseRoot, sourcePack.id, sourcePack.version);
    const validated = await inspectPackArchive(archive, {
      expectedId: sourcePack.id,
      expectedVersion: sourcePack.version,
      expectedEngineApi: source.engine_api,
    });
    packs.push({
      id: sourcePack.id,
      version: sourcePack.version,
      kind: sourcePack.kind,
      url: `packs/${sourcePack.id}/${sourcePack.version}/${sourcePack.id}-${sourcePack.version}.zip`,
      sha256: await hashFile(archive),
      size: validated.archiveSize,
      unpacked_size: validated.unpackedSize,
      dependencies: [...sourcePack.dependencies].sort(),
      min_app_version: sourcePack.min_app_version,
    });
  }
  const catalog = {
    schema_version: SCHEMA_VERSION,
    generated_at: source.generated_at,
    engine_api: source.engine_api,
    packs,
    profiles: [...source.profiles]
      .sort((left, right) => compareUtf8(left.id, right.id))
      .map((profile) => ({
        id: profile.id,
        device_kind: profile.device_kind,
        required_packs: [...profile.required_packs].sort(),
      })),
  };
  validateCatalogDocument(catalog);
  const target = outputPath ?? path.join(releaseRoot, "staging", "catalog-v1.json");
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, jsonBytes(catalog), { flag: "w" });
  return { catalog, output: target };
}

export function validateCatalogDocument(catalog) {
  assertExactKeys(
    catalog,
    ["schema_version", "generated_at", "engine_api", "packs", "profiles"],
    "catalog",
  );
  if (catalog.schema_version !== SCHEMA_VERSION) {
    fail("schema_unsupported", `catalog schema must be ${SCHEMA_VERSION}`);
  }
  if (catalog.engine_api !== ENGINE_API) {
    fail("engine_api_unsupported", `catalog engine_api must be ${ENGINE_API}`);
  }
  assertRfc3339(catalog.generated_at, "catalog generated_at");
  if (!Array.isArray(catalog.packs) || !Array.isArray(catalog.profiles)) {
    fail("invalid_document", "catalog packs and profiles must be arrays");
  }
  for (const pack of catalog.packs) {
    assertExactKeys(
      pack,
      [
        "id",
        "version",
        "kind",
        "url",
        "sha256",
        "size",
        "unpacked_size",
        "dependencies",
        "min_app_version",
      ],
      "catalog pack",
    );
    assertId(pack.id, "pack id");
    assertSemver(pack.version, "pack version");
    assertSemver(pack.min_app_version, "minimum application version");
    if (!PACK_KINDS.has(pack.kind)) {
      fail("invalid_pack_kind", `invalid pack kind ${pack.kind}`);
    }
    if (!Array.isArray(pack.dependencies)) {
      fail("invalid_document", `dependencies for ${pack.id} must be an array`);
    }
    pack.dependencies.forEach((reference) => assertPackRef(reference, "dependency"));
    assertUnique(pack.dependencies, (value) => value, `dependencies for ${pack.id}`);
    const expectedUrl = `packs/${pack.id}/${pack.version}/${pack.id}-${pack.version}.zip`;
    if (pack.url !== expectedUrl) {
      fail("invalid_url", `pack URL must be immutable release path ${expectedUrl}`);
    }
    assertSha256(pack.sha256, "pack SHA-256");
    assertInteger(pack.size, "pack size", 1);
    assertInteger(pack.unpacked_size, "pack unpacked_size", 1);
    if (pack.size > MAX_PACK_BYTES || pack.unpacked_size > MAX_UNPACKED_BYTES) {
      fail("size_limit_exceeded", `pack ${pack.id}@${pack.version} exceeds runtime limits`);
    }
  }
  assertUnique(catalog.packs, (pack) => `${pack.id}@${pack.version}`, "catalog packs");

  for (const profile of catalog.profiles) {
    assertExactKeys(profile, ["id", "device_kind", "required_packs"], "catalog profile");
    assertId(profile.id, "profile id");
    if (!DEVICE_KINDS.has(profile.device_kind)) {
      fail("invalid_device_kind", `invalid device kind ${profile.device_kind}`);
    }
    if (!Array.isArray(profile.required_packs) || profile.required_packs.length === 0) {
      fail("invalid_document", `profile ${profile.id} must require at least one pack`);
    }
    profile.required_packs.forEach((reference) => assertPackRef(reference, "required pack"));
    assertUnique(profile.required_packs, (value) => value, `required packs for ${profile.id}`);
  }
  assertUnique(catalog.profiles, (profile) => profile.id, "catalog profiles");

  const known = new Set(catalog.packs.map((pack) => `${pack.id}@${pack.version}`));
  for (const pack of catalog.packs) {
    for (const dependency of pack.dependencies) {
      if (!known.has(dependency)) {
        fail("pack_ref_missing", `${pack.id}@${pack.version} depends on missing ${dependency}`);
      }
    }
  }
  for (const profile of catalog.profiles) {
    for (const required of profile.required_packs) {
      if (!known.has(required)) {
        fail("pack_ref_missing", `${profile.id} requires missing ${required}`);
      }
    }
  }
  validateDependencyCycles(catalog.packs);
}

function parseSignatureEnvelope(signatureBytes) {
  let envelope;
  try {
    envelope = JSON.parse(signatureBytes.toString("utf8"));
  } catch (error) {
    fail("signature_invalid", `signature envelope is invalid JSON: ${error.message}`);
  }
  assertExactKeys(
    envelope,
    ["version", "algorithm", "key_id", "catalog_sha256", "signature"],
    "signature envelope",
  );
  if (envelope.version !== SIGNATURE_VERSION || envelope.algorithm !== "ed25519") {
    fail("signature_invalid", "signature envelope must use version 1 Ed25519");
  }
  if (typeof envelope.key_id !== "string" || !ID.test(envelope.key_id)) {
    fail("signature_invalid", "signature key_id must be a stable lowercase identifier");
  }
  assertSha256(envelope.catalog_sha256, "catalog signature hash");
  if (typeof envelope.signature !== "string" || !/^[A-Za-z0-9+/]+={0,2}$/.test(envelope.signature)) {
    fail("signature_invalid", "signature must be standard base64");
  }
  const raw = Buffer.from(envelope.signature, "base64");
  if (raw.length !== 64 || raw.toString("base64") !== envelope.signature) {
    fail("signature_invalid", "Ed25519 signature must be canonical base64 for 64 bytes");
  }
  return { envelope, raw };
}

function loadPrivateKey(bytes) {
  let key;
  try {
    key = createPrivateKey(bytes);
  } catch (error) {
    fail("private_key_invalid", `private key must be PKCS#8 PEM: ${error.message}`);
  }
  if (key.asymmetricKeyType !== "ed25519") {
    fail("private_key_invalid", "private key must be Ed25519");
  }
  return key;
}

function loadPublicKey(bytes) {
  let key;
  try {
    key = createPublicKey(bytes);
  } catch (error) {
    fail("public_key_invalid", `public key must be SPKI PEM: ${error.message}`);
  }
  if (key.asymmetricKeyType !== "ed25519") {
    fail("public_key_invalid", "public key must be Ed25519");
  }
  return key;
}

export function publicKeyRawBase64(publicKey) {
  const jwk = publicKey.export({ format: "jwk" });
  if (jwk.kty !== "OKP" || jwk.crv !== "Ed25519" || typeof jwk.x !== "string") {
    fail("public_key_invalid", "unable to export raw Ed25519 public key");
  }
  return Buffer.from(jwk.x, "base64url").toString("base64");
}

export async function signCatalog({ catalogPath, privateKeyPath, keyId, outputPath }) {
  assertId(keyId, "signature key id");
  const catalogBytes = await readFile(catalogPath);
  const catalog = JSON.parse(catalogBytes.toString("utf8"));
  validateCatalogDocument(catalog);
  const privateKey = loadPrivateKey(await readFile(privateKeyPath));
  const signature = ed25519Sign(null, catalogBytes, privateKey);
  const envelope = {
    version: SIGNATURE_VERSION,
    algorithm: "ed25519",
    key_id: keyId,
    catalog_sha256: createHash("sha256").update(catalogBytes).digest("hex"),
    signature: signature.toString("base64"),
  };
  const target = outputPath ?? `${catalogPath}.sig`;
  await writeFile(target, jsonBytes(envelope), { flag: "w" });
  return {
    envelope,
    output: target,
    publicKeyRawBase64: publicKeyRawBase64(createPublicKey(privateKey)),
  };
}

export async function verifyCatalogSignature({
  catalogPath,
  signaturePath,
  publicKeyPath,
  expectedKeyId,
}) {
  const catalogBytes = await readFile(catalogPath);
  const signatureBytes = await readFile(signaturePath);
  const { envelope, raw } = parseSignatureEnvelope(signatureBytes);
  if (expectedKeyId && envelope.key_id !== expectedKeyId) {
    fail(
      "signature_key_mismatch",
      `signature key ${envelope.key_id} does not match expected ${expectedKeyId}`,
    );
  }
  const digest = createHash("sha256").update(catalogBytes).digest("hex");
  if (digest !== envelope.catalog_sha256) {
    fail("signature_hash_mismatch", "catalog bytes do not match signature envelope hash");
  }
  const publicKey = loadPublicKey(await readFile(publicKeyPath));
  if (!ed25519Verify(null, catalogBytes, publicKey, raw)) {
    fail("signature_verification_failed", "catalog Ed25519 signature verification failed");
  }
  const catalog = JSON.parse(catalogBytes.toString("utf8"));
  validateCatalogDocument(catalog);
  return { catalog, envelope, publicKeyRawBase64: publicKeyRawBase64(publicKey) };
}

async function readExactly(handle, length, position) {
  const buffer = Buffer.alloc(length);
  let offset = 0;
  while (offset < length) {
    const { bytesRead } = await handle.read(buffer, offset, length - offset, position + offset);
    if (bytesRead === 0) fail("zip_invalid", "unexpected end of ZIP archive");
    offset += bytesRead;
  }
  return buffer;
}

async function readZipEntries(archivePath) {
  const archiveStat = await stat(archivePath);
  if (archiveStat.size <= 0 || archiveStat.size > MAX_PACK_BYTES) {
    fail("size_limit_exceeded", `archive ${archivePath} exceeds runtime limits`);
  }
  const handle = await open(archivePath, "r");
  try {
    const tailLength = Math.min(archiveStat.size, 65_557);
    const tail = await readExactly(handle, tailLength, archiveStat.size - tailLength);
    let endIndex = -1;
    for (let index = tail.length - 22; index >= 0; index -= 1) {
      if (tail.readUInt32LE(index) === ZIP_END) {
        endIndex = index;
        break;
      }
    }
    if (endIndex < 0) fail("zip_invalid", "ZIP end record is missing");
    const endOffset = archiveStat.size - tailLength + endIndex;
    if (endOffset + 22 !== archiveStat.size) {
      fail("zip_invalid", "ZIP end record must be the final archive bytes");
    }
    const disk = tail.readUInt16LE(endIndex + 4);
    const centralDisk = tail.readUInt16LE(endIndex + 6);
    const entryCount = tail.readUInt16LE(endIndex + 10);
    const centralSize = tail.readUInt32LE(endIndex + 12);
    const centralOffset = tail.readUInt32LE(endIndex + 16);
    const commentLength = tail.readUInt16LE(endIndex + 20);
    if (
      disk !== 0 ||
      centralDisk !== 0 ||
      commentLength !== 0 ||
      entryCount === 0 ||
      entryCount > MAX_FILES + 1
    ) {
      fail("zip_invalid", "multi-disk, comments, or empty ZIP archives are not supported");
    }
    if (centralOffset + centralSize !== endOffset) {
      fail("zip_invalid", "ZIP central directory is outside the archive");
    }
    const central = await readExactly(handle, centralSize, centralOffset);
    const entries = [];
    const names = new Set();
    let cursor = 0;
    for (let count = 0; count < entryCount; count += 1) {
      if (cursor + 46 > central.length || central.readUInt32LE(cursor) !== ZIP_CENTRAL_FILE) {
        fail("zip_invalid", "ZIP central directory entry is invalid");
      }
      const flags = central.readUInt16LE(cursor + 8);
      const method = central.readUInt16LE(cursor + 10);
      const crc32 = central.readUInt32LE(cursor + 16);
      const compressedSize = central.readUInt32LE(cursor + 20);
      const size = central.readUInt32LE(cursor + 24);
      const nameLength = central.readUInt16LE(cursor + 28);
      const extraLength = central.readUInt16LE(cursor + 30);
      const comment = central.readUInt16LE(cursor + 32);
      const localOffset = central.readUInt32LE(cursor + 42);
      const end = cursor + 46 + nameLength + extraLength + comment;
      if (end > central.length) fail("zip_invalid", "truncated ZIP central directory");
      const name = central.subarray(cursor + 46, cursor + 46 + nameLength).toString("utf8");
      if (
        flags !== ZIP_UTF8_FLAG ||
        method !== 0 ||
        compressedSize !== size ||
        extraLength !== 0 ||
        comment !== 0
      ) {
        fail("zip_invalid", `release ZIP entry ${name} must be UTF-8 and stored without compression`);
      }
      if (name.endsWith("/")) fail("zip_invalid", `directory entries are not allowed: ${name}`);
      if (name !== "pack.json") validatePackPath(name);
      const key = name.toLowerCase();
      if (names.has(key)) fail("duplicate_file", `duplicate ZIP entry ${name}`);
      names.add(key);
      const local = await readExactly(handle, 30, localOffset);
      if (local.readUInt32LE(0) !== ZIP_LOCAL_FILE) fail("zip_invalid", `invalid local header for ${name}`);
      const localFlags = local.readUInt16LE(6);
      const localMethod = local.readUInt16LE(8);
      const localCrc32 = local.readUInt32LE(14);
      const localCompressedSize = local.readUInt32LE(18);
      const localSize = local.readUInt32LE(22);
      const localNameLength = local.readUInt16LE(26);
      const localExtraLength = local.readUInt16LE(28);
      if (
        localFlags !== flags ||
        localMethod !== method ||
        localCrc32 !== crc32 ||
        localCompressedSize !== compressedSize ||
        localSize !== size ||
        localExtraLength !== 0
      ) {
        fail("zip_invalid", `local and central ZIP headers differ for ${name}`);
      }
      const localName = await readExactly(handle, localNameLength, localOffset + 30);
      if (!localName.equals(central.subarray(cursor + 46, cursor + 46 + nameLength))) {
        fail("zip_invalid", `local and central ZIP names differ for ${name}`);
      }
      const dataOffset = localOffset + 30 + localNameLength + localExtraLength;
      if (dataOffset + size > centralOffset) fail("zip_invalid", `entry ${name} overlaps central directory`);
      entries.push({ name, size, crc32, dataOffset });
      cursor = end;
    }
    if (cursor !== central.length) fail("zip_invalid", "unexpected bytes in ZIP central directory");
    return { archiveSize: archiveStat.size, entries, handle };
  } catch (error) {
    await handle.close();
    throw error;
  }
}

async function hashZipEntry(handle, entry, includeBytes = false) {
  const hash = createHash("sha256");
  let crc = 0xffffffff;
  let bytes = includeBytes ? Buffer.alloc(entry.size) : null;
  let readOffset = 0;
  const chunk = Buffer.alloc(Math.min(1024 * 1024, Math.max(1, entry.size)));
  while (readOffset < entry.size) {
    const length = Math.min(chunk.length, entry.size - readOffset);
    const { bytesRead } = await handle.read(chunk, 0, length, entry.dataOffset + readOffset);
    if (bytesRead !== length) fail("zip_invalid", `truncated ZIP entry ${entry.name}`);
    const slice = chunk.subarray(0, bytesRead);
    hash.update(slice);
    crc = crc32Update(crc, slice);
    if (bytes) slice.copy(bytes, readOffset);
    readOffset += bytesRead;
  }
  const actualCrc = (crc ^ 0xffffffff) >>> 0;
  if (actualCrc !== entry.crc32) fail("zip_crc_mismatch", `CRC mismatch for ${entry.name}`);
  return { sha256: hash.digest("hex"), bytes };
}

function validateManifest(manifest, expected) {
  assertExactKeys(
    manifest,
    ["schema_version", "id", "version", "engine_api", "usage", "files"],
    "pack manifest",
  );
  if (manifest.schema_version !== SCHEMA_VERSION || manifest.engine_api !== expected.expectedEngineApi) {
    fail("manifest_invalid", "pack manifest schema or engine_api is unsupported");
  }
  assertId(manifest.id, "pack manifest id");
  assertSemver(manifest.version, "pack manifest version");
  if (manifest.id !== expected.expectedId || manifest.version !== expected.expectedVersion) {
    fail("manifest_identity_mismatch", "pack manifest does not match catalog identity");
  }
  assertUsage(manifest.usage, "pack manifest usage");
  if (!Array.isArray(manifest.files) || manifest.files.length === 0 || manifest.files.length > MAX_FILES) {
    fail("manifest_invalid", `pack manifest must declare 1-${MAX_FILES} files`);
  }
  assertUnique(manifest.files, (file) => file.path, "pack manifest files");
  for (const file of manifest.files) {
    assertExactKeys(file, ["path", "sha256", "size"], "pack manifest file");
    validatePackPath(file.path);
    assertSha256(file.sha256, `SHA-256 for ${file.path}`);
    assertInteger(file.size, `size for ${file.path}`, 1);
    if (file.size > MAX_FILE_BYTES) fail("size_limit_exceeded", `${file.path} exceeds runtime limits`);
  }
}

export async function inspectPackArchive(archivePath, expected) {
  const archive = await readZipEntries(archivePath);
  try {
    const manifestEntryFound = archive.entries.find((entry) => entry.name === "pack.json");
    if (!manifestEntryFound || manifestEntryFound.size > 8 * 1024 * 1024) {
      fail("manifest_invalid", "pack.json is missing or too large");
    }
    const manifestResult = await hashZipEntry(archive.handle, manifestEntryFound, true);
    let manifest;
    try {
      manifest = JSON.parse(manifestResult.bytes.toString("utf8"));
    } catch (error) {
      fail("manifest_invalid", `pack.json is invalid JSON: ${error.message}`);
    }
    validateManifest(manifest, expected);
    const payloadEntries = archive.entries.filter((entry) => entry.name !== "pack.json");
    if (payloadEntries.length !== manifest.files.length) {
      fail("undeclared_file", "ZIP payload entries do not exactly match pack manifest");
    }
    const byName = new Map(payloadEntries.map((entry) => [entry.name.toLowerCase(), entry]));
    let unpackedSize = 0;
    for (const declared of manifest.files) {
      const entry = byName.get(declared.path.toLowerCase());
      if (!entry || entry.name !== declared.path || entry.size !== declared.size) {
        fail("file_mismatch", `ZIP entry does not match manifest: ${declared.path}`);
      }
      const result = await hashZipEntry(archive.handle, entry);
      if (result.sha256 !== declared.sha256) {
        fail("file_hash_mismatch", `ZIP entry hash does not match manifest: ${declared.path}`);
      }
      unpackedSize += entry.size;
    }
    return { archiveSize: archive.archiveSize, unpackedSize, manifest };
  } finally {
    await archive.handle.close();
  }
}

export async function validateRelease({
  releaseRoot,
  catalogPath = path.join(releaseRoot, "catalog-v1.json"),
  signaturePath = `${catalogPath}.sig`,
  publicKeyPath,
  expectedKeyId,
}) {
  const verified = await verifyCatalogSignature({
    catalogPath,
    signaturePath,
    publicKeyPath,
    expectedKeyId,
  });
  for (const pack of verified.catalog.packs) {
    const archivePath = packArchivePath(releaseRoot, pack.id, pack.version);
    const archiveStat = await stat(archivePath).catch((error) => {
      fail("pack_missing", `catalog pack is missing ${archivePath}: ${error.message}`);
    });
    if (archiveStat.size !== pack.size) {
      fail("pack_size_mismatch", `catalog size does not match ${pack.id}@${pack.version}`);
    }
    const archiveHash = await hashFile(archivePath);
    if (archiveHash !== pack.sha256) {
      fail("pack_hash_mismatch", `catalog hash does not match ${pack.id}@${pack.version}`);
    }
    const inspected = await inspectPackArchive(archivePath, {
      expectedId: pack.id,
      expectedVersion: pack.version,
      expectedEngineApi: verified.catalog.engine_api,
    });
    if (inspected.unpackedSize !== pack.unpacked_size) {
      fail("pack_size_mismatch", `catalog unpacked_size does not match ${pack.id}@${pack.version}`);
    }
  }
  return {
    catalog: verified.catalog,
    envelope: verified.envelope,
    publicKeyRawBase64: verified.publicKeyRawBase64,
    packCount: verified.catalog.packs.length,
  };
}

async function writeSynced(filePath, bytes) {
  const handle = await open(filePath, "wx");
  try {
    await handle.writeFile(bytes);
    await handle.sync();
  } finally {
    await handle.close();
  }
}

async function atomicRename(source, target) {
  try {
    await rename(source, target);
  } catch (error) {
    fail(
      "atomic_replace_failed",
      `atomic rename to ${target} failed; destination filesystem must support same-directory replacement: ${error.message}`,
    );
  }
}

export async function publishCatalog({
  releaseRoot,
  candidateCatalogPath,
  candidateSignaturePath = `${candidateCatalogPath}.sig`,
  publicKeyPath,
  expectedKeyId,
}) {
  await validateRelease({
    releaseRoot,
    catalogPath: candidateCatalogPath,
    signaturePath: candidateSignaturePath,
    publicKeyPath,
    expectedKeyId,
  });
  const catalogBytes = await readFile(candidateCatalogPath);
  const signatureBytes = await readFile(candidateSignaturePath);
  await mkdir(releaseRoot, { recursive: true });
  const nonce = `${process.pid}-${Date.now()}`;
  const catalogTarget = path.join(releaseRoot, "catalog-v1.json");
  const signatureTarget = `${catalogTarget}.sig`;
  const catalogTemp = path.join(releaseRoot, `.catalog-v1.json.${nonce}.new`);
  const signatureTemp = path.join(releaseRoot, `.catalog-v1.json.sig.${nonce}.new`);
  try {
    await writeSynced(catalogTemp, catalogBytes);
    await writeSynced(signatureTemp, signatureBytes);
    // The detached signature is installed first. The signed catalog is the
    // externally visible commit point and is replaced last.
    await atomicRename(signatureTemp, signatureTarget);
    await atomicRename(catalogTemp, catalogTarget);
  } finally {
    await rm(catalogTemp, { force: true });
    await rm(signatureTemp, { force: true });
  }
  return { catalog: catalogTarget, signature: signatureTarget };
}

export function isPathInside(parent, candidate) {
  const relative = path.relative(path.resolve(parent), path.resolve(candidate));
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== "..");
}
