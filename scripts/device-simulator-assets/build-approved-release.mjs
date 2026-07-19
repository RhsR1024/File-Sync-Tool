#!/usr/bin/env node

import { generateKeyPairSync } from "node:crypto";
import {
  cp,
  mkdtemp,
  mkdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

import {
  buildCatalog,
  buildPack,
  NON_COMMERCIAL_USAGE,
  publishCatalog,
  signCatalog,
  validateRelease,
} from "./lib.mjs";
import { extractPcapMediaFile } from "./extract-pcap-media.mjs";

const PACK_VERSION = "1.0.2";
const APP_VERSION = "1.2.1";
const DEFAULT_KEY_ID = "device-assets-static-review-2026";

const PROFILE_DEFINITIONS = Object.freeze({
  "ipc-custom": {
    deviceKind: "ipc",
    legacyDeviceType: "自定义报警相机",
    discovery: "ws_discovery.ipc.v1",
    http: "http.custom_ipc.v1",
    alarm: "alarm.custom.v1",
    sourceCopies: [
      ["xml/Custom", "xml/Custom"],
      ["object/CustomStruct", "object/CustomStruct"],
      ["pic/CUSTOM", "pic/CUSTOM"],
    ],
  },
  "ipc-smart": {
    deviceKind: "ipc",
    legacyDeviceType: "智能相机",
    discovery: "ws_discovery.ipc.v1",
    http: "http.smart_ipc.v1",
    alarm: "alarm.smart.v1",
    sourceCopies: [
      ["xml/Smart", "xml/Smart"],
      ["object/SmartStruct", "object/SmartStruct"],
      ["pic/SMART", "pic/SMART"],
    ],
  },
  "nvr-common": {
    deviceKind: "nvr",
    legacyDeviceType: "普通NVR",
    discovery: "ws_discovery.nvr.v1",
    http: "http.nvr_common.v1",
    alarm: "alarm.nvr_common.v1",
    sourceCopies: [
      ["object/NormalStruct", "object/NormalStruct"],
    ],
  },
  "nvr-vehicle": {
    deviceKind: "nvr",
    legacyDeviceType: "车辆识别NVR",
    discovery: "ws_discovery.nvr.v1",
    http: "http.nvr_vehicle.v1",
    alarm: "alarm.nvr_vehicle.v1",
    sourceCopies: [
      ["xml/Vehicle", "xml/Vehicle"],
      ["object/VehicleStruct", "object/VehicleStruct"],
      ["pic/VEHICLE", "pic/VEHICLE", "images-only"],
    ],
  },
});

const SECTION_BY_PROFILE = Object.freeze({
  "ipc-custom": "自定义报警相机",
  "ipc-smart": "智能相机",
  "nvr-common": "普通NVR",
  "nvr-vehicle": "车辆识别NVR",
});

const IMAGE_FOLDER_BY_PROFILE = Object.freeze({
  "ipc-custom": "CUSTOM",
  "ipc-smart": "SMART",
  "nvr-vehicle": "VEHICLE",
});

const OBJECT_FOLDER_BY_PROFILE = Object.freeze({
  "ipc-custom": "CustomStruct",
  "ipc-smart": "SmartStruct",
  "nvr-common": "NormalStruct",
  "nvr-vehicle": "VehicleStruct",
});

function fail(message) {
  throw new Error(message);
}

function parseArgs(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 2) {
    const option = argv[index];
    const value = argv[index + 1];
    if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
      fail(`invalid option near ${option ?? "end"}`);
    }
    values[option.slice(2)] = value;
  }
  for (const required of ["legacy-root", "release-root", "private-key", "public-key"]) {
    if (!values[required]) fail(`--${required} is required`);
  }
  return {
    legacyRoot: path.resolve(values["legacy-root"]),
    releaseRoot: path.resolve(values["release-root"]),
    privateKey: path.resolve(values["private-key"]),
    publicKey: path.resolve(values["public-key"]),
    keyId: values["key-id"] ?? DEFAULT_KEY_ID,
    generatedAt: values["generated-at"] ?? new Date().toISOString(),
  };
}

function stringField(line, field) {
  const match = new RegExp(`${field}\\s*:\\s*(['\"])(.*?)\\1`).exec(line);
  return match?.[2] ?? null;
}

function numericField(line, field) {
  const match = new RegExp(`${field}\\s*:\\s*(\\d+)`).exec(line);
  return match ? Number(match[1]) : null;
}

function arrayField(line, field) {
  const match = new RegExp(`${field}\\s*:\\s*\\[([^\\]]*)\\]`).exec(line);
  if (!match) return [];
  return [...match[1].matchAll(/(['\"])(.*?)\1/g)].map((item) => item[2]);
}

function normalizeAssetPath(value, prefix, profileId) {
  if (!value) return null;
  const normalized = value
    .replaceAll("\\", "/")
    .split("/")
    .filter(Boolean)
    .join("/");
  const objectFolder = prefix === "object" ? OBJECT_FOLDER_BY_PROFILE[profileId] : null;
  const relativePath = objectFolder && !normalized.includes("/")
    ? `${objectFolder}/${normalized}`
    : normalized;
  return `${prefix}/${relativePath}`;
}

function asciiId(value) {
  if (!value) return null;
  const id = value
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return id || null;
}

function definitionId(fields, index) {
  const templateName = fields.alarmData ?? fields.picData ?? fields.eventAlarm;
  return asciiId(fields.desc ?? fields.eventType ?? fields.picName ?? templateName)
    ?? `legacy-${String(index + 1).padStart(3, "0")}`;
}

function parseAlarmLine(line, profileId, index) {
  const fields = {
    displayName: stringField(line, "AlarmType"),
    desc: stringField(line, "Desc"),
    eventType: stringField(line, "EventType"),
    picName: stringField(line, "picName"),
    picData: stringField(line, "picData"),
    picDataVms: stringField(line, "picData-vms"),
    picHeader: stringField(line, "picHeader"),
    alarmData: stringField(line, "alarmData"),
    eventAlarm: stringField(line, "EventAlarm"),
    protocol: stringField(line, "alarmProtocol"),
    recoveryType: stringField(line, "alarmTypeOff"),
    sourceType: stringField(line, "type"),
    supportsPictures: numericField(line, "issupportpic"),
    serverSupport: arrayField(line, "serverSupport"),
  };
  const platforms = [];
  if (fields.serverSupport.includes("VMS系列")) platforms.push("vms");
  if (fields.serverSupport.includes("UMS")) platforms.push("ums");
  const imageFolder = IMAGE_FOLDER_BY_PROFILE[profileId];
  const imageRoot = fields.picName && imageFolder
    ? `pic/${imageFolder}/normal/${fields.picName}`
    : null;
  return {
    id: definitionId(fields, index),
    display_name: fields.displayName,
    platforms,
    protocol: fields.protocol?.toLowerCase().replace(".", "_") ?? "v1_0",
    event_type: fields.eventType ?? fields.desc ?? definitionId(fields, index),
    alarm_template: normalizeAssetPath(fields.alarmData ?? fields.eventAlarm, "object", profileId),
    structure_template: normalizeAssetPath(fields.picData, "object", profileId),
    structure_template_vms: normalizeAssetPath(fields.picDataVms, "object", profileId),
    structure_path: fields.picHeader,
    image_root: imageRoot,
    supports_pictures: fields.supportsPictures === null ? Boolean(imageRoot) : fields.supportsPictures === 1,
    recovery_event_type: fields.recoveryType,
    source_type: fields.sourceType,
    evidence: {
      status: "reviewed_static",
      source: "data/alarms_info.yml",
      line: index + 1,
    },
  };
}

export function parseApprovedAlarmDefinitions(yamlText, profileId) {
  const section = SECTION_BY_PROFILE[profileId];
  if (!section) fail(`unknown profile ${profileId}`);
  const lines = yamlText.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === `${section}:`);
  if (start < 0) fail(`alarm section ${section} is missing`);
  const definitions = [];
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\S.*:\s*$/.test(line)) break;
    if (!/^\s+-\s+\{/.test(line)) continue;
    definitions.push(parseAlarmLine(line, profileId, index));
  }
  if (definitions.length === 0) fail(`alarm section ${section} contains no definitions`);
  const ids = new Map();
  for (const definition of definitions) {
    const count = ids.get(definition.id) ?? 0;
    ids.set(definition.id, count + 1);
    if (count > 0) definition.id = `${definition.id}-${count + 1}`;
  }
  return definitions;
}

export function parseApprovedDeviceIdentity(yamlText, legacyDeviceType) {
  const lines = yamlText.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === `${legacyDeviceType}:`);
  if (start < 0) fail(`device identity section ${legacyDeviceType} is missing`);
  const values = {};
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (/^\S.*:\s*$/.test(line)) break;
    const match = /^\s+([a-z_]+):\s*(.*?)\s*$/.exec(line);
    if (!match) continue;
    values[match[1]] = match[2].replace(/^(['"])(.*)\1$/, "$2");
  }
  const deviceTypeEnum = Number(values.dev_typeenum);
  if (!values.dev_type || !values.dev_version || !values.nick_name || !Number.isInteger(deviceTypeEnum)) {
    fail(`device identity section ${legacyDeviceType} is incomplete`);
  }
  return {
    model: values.dev_type,
    firmware_version: values.dev_version,
    nickname: values.nick_name,
    device_type_enum: deviceTypeEnum,
  };
}

function profileDocument(profileId, definition, identity) {
  const source = (topic, sources, intentionalChanges = []) => ({
    topic,
    status: "reviewed_static",
    sources,
    verified_platforms: [],
    intentional_changes: intentionalChanges,
  });
  return {
    schema_version: 1,
    id: profileId,
    device_kind: definition.deviceKind,
    legacy_device_type: definition.legacyDeviceType,
    identity,
    supported_platforms: ["vms", "ums"],
    handlers: {
      identity: "legacy.identity.v1",
      discovery: definition.discovery,
      http: definition.http,
      rtsp: "rtsp.tcp_interleaved.v1",
      alarms: [definition.alarm],
    },
    evidence: [
      source("identity", ["data/dev_type.yml", "script/VSITool.py"]),
      source("discovery", ["script/Vsocket_ip.py", definition.deviceKind === "ipc" ? "xml/Common/search.xml" : "xml/Common/search-aibox.xml"]),
      source("http", ["script/HTTPServer.py"], ["Static review permits local implementation; platform compatibility remains unverified"]),
      source("rtsp", ["script/IPCRtspLib.py", "mediafile/mainstream.pcap", "mediafile/substream.pcap", "mediafile/thirdstream.pcap"], ["Third-stream capture duplicates substream bytes; /media/video3 is the user-approved static selection"]),
      source("alarm", ["data/alarms_info.yml", `script/${profileId === "ipc-custom" ? "Custom" : profileId === "ipc-smart" ? "Smart" : profileId === "nvr-common" ? "Normal" : "Vehicle"}Alarm.py`], ["HTTP response success semantics remain real-platform gated"]),
    ],
  };
}

async function copyDirectory(source, target, mode) {
  const forbiddenExtensions = new Set([
    ".exe", ".dll", ".py", ".js", ".bat", ".cmd", ".ps1", ".wasm", ".msi", ".scr", ".com",
  ]);
  await cp(source, target, {
    recursive: true,
    force: true,
    filter: (entry) => {
      if (forbiddenExtensions.has(path.extname(entry).toLowerCase())) return false;
      if (mode !== "images-only") return true;
      const extension = path.extname(entry).toLowerCase();
      return extension === "" || [".jpg", ".jpeg", ".png"].includes(extension);
    },
  });
}

async function writeJson(filePath, value) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

async function buildSourcePacks(workRoot, legacyRoot) {
  const alarmYaml = await readFile(path.join(legacyRoot, "data", "alarms_info.yml"), "utf8");
  const deviceTypeYaml = await readFile(path.join(legacyRoot, "data", "dev_type.yml"), "utf8");

  const protocolRoot = path.join(workRoot, "protocol-core");
  await copyDirectory(path.join(legacyRoot, "xml", "Common"), path.join(protocolRoot, "xml", "Common"));
  await copyDirectory(path.join(legacyRoot, "xml", "AIBOX"), path.join(protocolRoot, "xml", "AIBOX"));

  for (const [profileId, definition] of Object.entries(PROFILE_DEFINITIONS)) {
    const profileRoot = path.join(workRoot, profileId);
    for (const [source, target, mode] of definition.sourceCopies) {
      await copyDirectory(path.join(legacyRoot, source), path.join(profileRoot, target), mode);
    }
    const identity = parseApprovedDeviceIdentity(deviceTypeYaml, definition.legacyDeviceType);
    await writeJson(path.join(profileRoot, "profiles", `${profileId}.json`), profileDocument(profileId, definition, identity));
    await writeJson(path.join(profileRoot, "runtime", "alarm-types.json"), {
      schema_version: 1,
      profile_id: profileId,
      handler_id: definition.alarm,
      definitions: parseApprovedAlarmDefinitions(alarmYaml, profileId),
    });
  }

  const mediaRoot = path.join(workRoot, "media-h264-live", "media");
  const commonEvidence = (pcap, differences = []) => ({
    source_kind: "authorized_pcap",
    pcap_source_id: `mediafile/${pcap}`,
    sdp_source_id: "script/IPCRtspLib.py:1439-1463",
    compatibility: "reviewed_static",
    verified_platforms: [],
    differences,
  });
  await extractPcapMediaFile({
    input: path.join(legacyRoot, "mediafile", "mainstream.pcap"),
    outputDirectory: path.join(mediaRoot, "main"),
    id: "legacy-main",
    mediaFile: "main.h264",
    payloadType: 105,
    evidence: commonEvidence("mainstream.pcap"),
  });
  await extractPcapMediaFile({
    input: path.join(legacyRoot, "mediafile", "substream.pcap"),
    outputDirectory: path.join(mediaRoot, "sub"),
    id: "legacy-sub",
    mediaFile: "sub.h264",
    payloadType: 105,
    evidence: commonEvidence("substream.pcap"),
  });
  await extractPcapMediaFile({
    input: path.join(legacyRoot, "mediafile", "thirdstream.pcap"),
    outputDirectory: path.join(mediaRoot, "third"),
    id: "legacy-third",
    mediaFile: "third.h264",
    payloadType: 105,
    evidence: commonEvidence("thirdstream.pcap", [{
      field: "rtsp_control_path",
      pcap_value: "/media/video2",
      sdp_value: "/media/video2",
      selected_value: "/media/video3",
      resolution: "user_approved",
    }]),
  });
}

async function pack(releaseRoot, workRoot, id, kind, dependencies = []) {
  const definitionPath = path.join(workRoot, `${id}.pack.json`);
  await writeJson(definitionPath, {
    schema_version: 1,
    id,
    version: PACK_VERSION,
    engine_api: 1,
    source_dir: path.join(workRoot, id),
    usage: NON_COMMERCIAL_USAGE,
  });
  const result = await buildPack({ definitionPath, releaseRoot });
  return {
    id,
    version: PACK_VERSION,
    kind,
    dependencies,
    min_app_version: APP_VERSION,
    result,
  };
}

async function ensureExternalKeyPair(privateKey, publicKey) {
  try {
    await readFile(privateKey);
    await readFile(publicKey);
    return;
  } catch {
    const pair = generateKeyPairSync("ed25519");
    await mkdir(path.dirname(privateKey), { recursive: true });
    await mkdir(path.dirname(publicKey), { recursive: true });
    await writeFile(privateKey, pair.privateKey.export({ format: "pem", type: "pkcs8" }), { flag: "wx" });
    await writeFile(publicKey, pair.publicKey.export({ format: "pem", type: "spki" }), { flag: "wx" });
  }
}

export async function buildApprovedRelease(options) {
  await ensureExternalKeyPair(options.privateKey, options.publicKey);
  const workRoot = await mkdtemp(path.join(os.tmpdir(), "fst-approved-assets-"));
  try {
    await buildSourcePacks(workRoot, options.legacyRoot);
    const protocol = await pack(options.releaseRoot, workRoot, "protocol-core", "protocol-core");
    const media = await pack(options.releaseRoot, workRoot, "media-h264-live", "media");
    const profilePacks = [];
    for (const profileId of Object.keys(PROFILE_DEFINITIONS)) {
      profilePacks.push(await pack(
        options.releaseRoot,
        workRoot,
        profileId,
        "device-profile",
        [`protocol-core@${PACK_VERSION}`, `media-h264-live@${PACK_VERSION}`],
      ));
    }
    const packs = [protocol, media, ...profilePacks];
    const catalogDefinition = path.join(workRoot, "catalog-source.json");
    await writeJson(catalogDefinition, {
      schema_version: 1,
      generated_at: options.generatedAt,
      engine_api: 1,
      packs: packs.map(({ id, version, kind, dependencies, min_app_version }) => ({
        id,
        version,
        kind,
        dependencies,
        min_app_version,
      })),
      profiles: Object.entries(PROFILE_DEFINITIONS).map(([id, definition]) => ({
        id,
        device_kind: definition.deviceKind,
        required_packs: [`${id}@${PACK_VERSION}`],
      })),
    });
    const catalog = await buildCatalog({
      definitionPath: catalogDefinition,
      releaseRoot: options.releaseRoot,
    });
    const signature = await signCatalog({
      catalogPath: catalog.output,
      privateKeyPath: options.privateKey,
      keyId: options.keyId,
    });
    await validateRelease({
      releaseRoot: options.releaseRoot,
      catalogPath: catalog.output,
      signaturePath: signature.output,
      publicKeyPath: options.publicKey,
      expectedKeyId: options.keyId,
    });
    await publishCatalog({
      releaseRoot: options.releaseRoot,
      candidateCatalogPath: catalog.output,
      candidateSignaturePath: signature.output,
      publicKeyPath: options.publicKey,
      expectedKeyId: options.keyId,
    });
    return {
      releaseRoot: options.releaseRoot,
      keyId: options.keyId,
      trustedPublicKeyBase64: signature.publicKeyRawBase64,
      packs: packs.map(({ id, result }) => ({ id, sha256: result.sha256, archive: result.archive })),
    };
  } finally {
    await rm(workRoot, { recursive: true, force: true });
  }
}

async function main() {
  const result = await buildApprovedRelease(parseArgs(process.argv.slice(2)));
  process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error) => {
    process.stderr.write(`[approved_asset_release_failed] ${error.message}\n`);
    process.exitCode = 1;
  });
}
