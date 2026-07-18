#!/usr/bin/env node

import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  AssetReleaseError,
  buildCatalog,
  buildPack,
  isPathInside,
  publishCatalog,
  signCatalog,
  validateRelease,
} from "./lib.mjs";

function usage() {
  return `Device simulator immutable asset release tool

Commands:
  pack --definition <pack-source.json> --release-root <directory>
  catalog --definition <catalog-source.json> --release-root <directory> [--output <catalog.json>]
  sign --catalog <catalog.json> --private-key <external PKCS#8 PEM> --key-id <id> [--output <catalog.sig>]
  validate --release-root <directory> --public-key <SPKI PEM> [--catalog <catalog.json>] [--signature <sig>] [--key-id <id>]
  publish --release-root <directory> --catalog <candidate.json> --signature <candidate.sig> --public-key <SPKI PEM> [--key-id <id>]
`;
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  if (!command || command === "--help" || command === "-h") return { command: "help", values: {} };
  const values = {};
  for (let index = 0; index < rest.length; index += 2) {
    const option = rest[index];
    const value = rest[index + 1];
    if (!option?.startsWith("--") || value === undefined || value.startsWith("--")) {
      throw new AssetReleaseError("invalid_arguments", `invalid option near ${option ?? "end"}`);
    }
    values[option.slice(2)] = value;
  }
  return { command, values };
}

function required(values, name) {
  if (!values[name]) throw new AssetReleaseError("invalid_arguments", `--${name} is required`);
  return path.resolve(values[name]);
}

function optional(values, name) {
  return values[name] ? path.resolve(values[name]) : undefined;
}

async function main() {
  const { command, values } = parseArgs(process.argv.slice(2));
  if (command === "help") {
    process.stdout.write(usage());
    return;
  }

  if (command === "pack") {
    const result = await buildPack({
      definitionPath: required(values, "definition"),
      releaseRoot: required(values, "release-root"),
    });
    process.stdout.write(`Built immutable pack ${result.id}@${result.version}\n${result.archive}\nsha256=${result.sha256}\n`);
    return;
  }
  if (command === "catalog") {
    const result = await buildCatalog({
      definitionPath: required(values, "definition"),
      releaseRoot: required(values, "release-root"),
      outputPath: optional(values, "output"),
    });
    process.stdout.write(`Built unsigned staging catalog\n${result.output}\n`);
    return;
  }
  if (command === "sign") {
    const privateKeyPath = required(values, "private-key");
    const repositoryRoot = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
    if (isPathInside(repositoryRoot, privateKeyPath)) {
      throw new AssetReleaseError(
        "private_key_in_repository",
        "the signing private key must be stored outside the repository",
      );
    }
    if (!values["key-id"]) throw new AssetReleaseError("invalid_arguments", "--key-id is required");
    const result = await signCatalog({
      catalogPath: required(values, "catalog"),
      privateKeyPath,
      keyId: values["key-id"],
      outputPath: optional(values, "output"),
    });
    process.stdout.write(
      `Signed catalog with key ${result.envelope.key_id}\n${result.output}\ntrusted_public_key_base64=${result.publicKeyRawBase64}\n`,
    );
    return;
  }
  if (command === "validate") {
    const releaseRoot = required(values, "release-root");
    const catalogPath = optional(values, "catalog") ?? path.join(releaseRoot, "catalog-v1.json");
    const result = await validateRelease({
      releaseRoot,
      catalogPath,
      signaturePath: optional(values, "signature") ?? `${catalogPath}.sig`,
      publicKeyPath: required(values, "public-key"),
      expectedKeyId: values["key-id"],
    });
    process.stdout.write(
      `Validated signed catalog and ${result.packCount} immutable pack(s)\ntrusted_public_key_base64=${result.publicKeyRawBase64}\n`,
    );
    return;
  }
  if (command === "publish") {
    const result = await publishCatalog({
      releaseRoot: required(values, "release-root"),
      candidateCatalogPath: required(values, "catalog"),
      candidateSignaturePath: required(values, "signature"),
      publicKeyPath: required(values, "public-key"),
      expectedKeyId: values["key-id"],
    });
    process.stdout.write(`Published signature, then atomically committed catalog last\n${result.catalog}\n`);
    return;
  }
  throw new AssetReleaseError("invalid_arguments", `unknown command ${command}\n${usage()}`);
}

main().catch((error) => {
  const code = error instanceof AssetReleaseError ? error.code : "unexpected_error";
  process.stderr.write(`[${code}] ${error.message}\n`);
  process.exitCode = 1;
});
