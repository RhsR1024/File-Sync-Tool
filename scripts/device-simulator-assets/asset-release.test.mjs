import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  AssetReleaseError,
  NON_COMMERCIAL_USAGE,
  buildCatalog,
  buildPack,
  hashFile,
  inspectPackArchive,
  publishCatalog,
  signCatalog,
  validateRelease,
} from "./lib.mjs";

async function writeJson(filePath, value) {
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

async function fixture(name = "profile-test") {
  const root = await mkdtemp(path.join(os.tmpdir(), "device-assets-"));
  const definitions = path.join(root, "definitions");
  const source = path.join(definitions, "payload");
  const releaseRoot = path.join(root, "release");
  await mkdir(path.join(source, "profiles"), { recursive: true });
  await writeFile(
    path.join(source, "profiles", `${name}.json`),
    `${JSON.stringify({ id: name, test_only: true })}\n`,
    "utf8",
  );
  const packDefinition = path.join(definitions, "pack-source.json");
  await writeJson(packDefinition, {
    schema_version: 1,
    id: name,
    version: "1.0.0",
    engine_api: 1,
    source_dir: "payload",
    usage: NON_COMMERCIAL_USAGE,
  });
  return {
    root,
    definitions,
    source,
    releaseRoot,
    packDefinition,
    definitionPath: packDefinition,
    name,
  };
}

async function keys(root) {
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const privateKeyPath = path.join(root, "signing-private.pem");
  const publicKeyPath = path.join(root, "signing-public.pem");
  await writeFile(privateKeyPath, privateKey.export({ format: "pem", type: "pkcs8" }));
  await writeFile(publicKeyPath, publicKey.export({ format: "pem", type: "spki" }));
  return { privateKeyPath, publicKeyPath };
}

test("pack bytes are deterministic and an existing id/version is immutable", async () => {
  const context = await fixture();
  const first = await buildPack(context);
  const second = await buildPack(context);
  assert.equal(second.sha256, first.sha256);
  assert.equal(await hashFile(second.archive), first.sha256);

  const inspected = await inspectPackArchive(first.archive, {
    expectedId: context.name,
    expectedVersion: "1.0.0",
    expectedEngineApi: 1,
  });
  assert.deepEqual(inspected.manifest.usage, NON_COMMERCIAL_USAGE);

  await writeFile(
    path.join(context.source, "profiles", `${context.name}.json`),
    "{\"changed\":true}\n",
    "utf8",
  );
  await assert.rejects(
    () => buildPack(context),
    (error) => error instanceof AssetReleaseError && error.code === "immutable_pack_conflict",
  );
});

test("pack generation rejects executable asset types", async () => {
  const context = await fixture("forbidden-test");
  await writeFile(path.join(context.source, "payload.js"), "throw new Error('no');\n", "utf8");
  await assert.rejects(
    () => buildPack(context),
    (error) => error instanceof AssetReleaseError && error.code === "forbidden_file_type",
  );
});

test("catalog is signed over exact bytes, validated, and published catalog-last", async () => {
  const context = await fixture("signed-profile");
  await buildPack(context);
  const catalogDefinition = path.join(context.definitions, "catalog-source.json");
  await writeJson(catalogDefinition, {
    schema_version: 1,
    generated_at: "2026-07-18T12:00:00+08:00",
    engine_api: 1,
    packs: [
      {
        id: context.name,
        version: "1.0.0",
        kind: "device-profile",
        dependencies: [],
        min_app_version: "1.1.2",
      },
    ],
    profiles: [
      {
        id: context.name,
        device_kind: "ipc",
        required_packs: [`${context.name}@1.0.0`],
      },
    ],
  });
  const catalog = await buildCatalog({
    definitionPath: catalogDefinition,
    releaseRoot: context.releaseRoot,
  });
  const signingKeys = await keys(context.root);
  const signed = await signCatalog({
    catalogPath: catalog.output,
    privateKeyPath: signingKeys.privateKeyPath,
    keyId: "release-test-2026",
  });
  const validated = await validateRelease({
    releaseRoot: context.releaseRoot,
    catalogPath: catalog.output,
    signaturePath: signed.output,
    publicKeyPath: signingKeys.publicKeyPath,
    expectedKeyId: "release-test-2026",
  });
  assert.equal(validated.packCount, 1);

  const published = await publishCatalog({
    releaseRoot: context.releaseRoot,
    candidateCatalogPath: catalog.output,
    candidateSignaturePath: signed.output,
    publicKeyPath: signingKeys.publicKeyPath,
    expectedKeyId: "release-test-2026",
  });
  const publishedBytes = await readFile(published.catalog);
  assert.deepEqual(publishedBytes, await readFile(catalog.output));

  // Re-publishing a validated candidate exercises atomic replacement when
  // catalog and signature targets already exist.
  await publishCatalog({
    releaseRoot: context.releaseRoot,
    candidateCatalogPath: catalog.output,
    candidateSignaturePath: signed.output,
    publicKeyPath: signingKeys.publicKeyPath,
    expectedKeyId: "release-test-2026",
  });

  const previousSignature = await readFile(published.signature);
  await writeFile(signed.output, "{\"invalid\":true}\n", "utf8");
  await assert.rejects(() =>
    publishCatalog({
      releaseRoot: context.releaseRoot,
      candidateCatalogPath: catalog.output,
      candidateSignaturePath: signed.output,
      publicKeyPath: signingKeys.publicKeyPath,
    }),
  );
  assert.deepEqual(await readFile(published.catalog), publishedBytes);
  assert.deepEqual(await readFile(published.signature), previousSignature);
});

test("changing signed catalog bytes invalidates the detached signature", async () => {
  const context = await fixture("exact-bytes");
  await buildPack(context);
  const definitionPath = path.join(context.definitions, "catalog-source.json");
  await writeJson(definitionPath, {
    schema_version: 1,
    generated_at: "2026-07-18T12:00:00Z",
    engine_api: 1,
    packs: [{
      id: context.name,
      version: "1.0.0",
      kind: "device-profile",
      dependencies: [],
      min_app_version: "1.1.2",
    }],
    profiles: [{
      id: context.name,
      device_kind: "ipc",
      required_packs: [`${context.name}@1.0.0`],
    }],
  });
  const catalog = await buildCatalog({ definitionPath, releaseRoot: context.releaseRoot });
  const signingKeys = await keys(context.root);
  const signed = await signCatalog({
    catalogPath: catalog.output,
    privateKeyPath: signingKeys.privateKeyPath,
    keyId: "exact-byte-test",
  });
  await writeFile(catalog.output, Buffer.concat([await readFile(catalog.output), Buffer.from("\n")]));
  await assert.rejects(
    () => validateRelease({
      releaseRoot: context.releaseRoot,
      catalogPath: catalog.output,
      signaturePath: signed.output,
      publicKeyPath: signingKeys.publicKeyPath,
    }),
    (error) => error instanceof AssetReleaseError && error.code === "signature_hash_mismatch",
  );
});
