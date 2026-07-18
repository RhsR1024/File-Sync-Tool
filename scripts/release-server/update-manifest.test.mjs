import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(
  new URL("./update-manifest.mjs", import.meta.url)
);
const tempRoots = [];

afterEach(async () => {
  while (tempRoots.length > 0) {
    const root = tempRoots.pop();
    await import("node:fs/promises").then(({ rm }) =>
      rm(root, { force: true, recursive: true })
    );
  }
});

async function createProjectFixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "update-manifest-"));
  tempRoots.push(root);

  const srcTauriDir = path.join(root, "src-tauri");
  const releaseDir = path.join(srcTauriDir, "target", "release");
  const releaseServerDir = path.join(root, "scripts", "release-server");

  await mkdir(srcTauriDir, { recursive: true });
  await mkdir(releaseDir, { recursive: true });
  await mkdir(releaseServerDir, { recursive: true });

  await writeFile(
    path.join(srcTauriDir, "tauri.conf.json"),
    JSON.stringify(
      {
        productName: "file-sync-tool",
        version: "1.0.7",
      },
      null,
      2
    )
  );

  await writeFile(
    path.join(srcTauriDir, "Cargo.toml"),
    [
      "[package]",
      'name = "app"',
      'version = "1.0.7"',
      "",
    ].join("\n")
  );

  return { root, releaseDir, releaseServerDir };
}

async function createVersionedExe(releaseDir, fileName, contents) {
  const exePath = path.join(releaseDir, fileName);
  await writeFile(exePath, contents);
  return exePath;
}

function sha256Hex(contents) {
  return createHash("sha256").update(contents).digest("hex");
}

function todayDateString() {
  const now = new Date();
  const pad = (value) => String(value).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(
    now.getDate()
  )}`;
}

function runUpdateManifestScript(cwd) {
  return new Promise((resolve) => {
    const env = { ...process.env };
    delete env.CARGO_TARGET_DIR;
    const child = spawn(process.execPath, [scriptPath], {
      cwd,
      env,
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });

    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    child.on("close", (code) => {
      resolve({ code, stdout, stderr });
    });
  });
}

async function readManifest(releaseServerDir) {
  const manifestPath = path.join(releaseServerDir, "manifest.json");
  const content = await readFile(manifestPath, "utf8");
  return JSON.parse(content);
}

test("creates manifest.json from the latest versioned exe when missing", async () => {
  const { root, releaseDir, releaseServerDir } = await createProjectFixture();
  const fileName = "file-sync-tool-1.0.7-202604271200.exe";
  const contents = "fake exe payload for first manifest";

  await createVersionedExe(releaseDir, fileName, contents);

  const result = await runUpdateManifestScript(root);

  assert.equal(result.code, 0, result.stderr);

  const manifest = await readManifest(releaseServerDir);
  assert.equal(manifest.latest, "1.0.7");
  assert.equal(manifest.versions.length, 1);
  assert.deepEqual(manifest.versions[0], {
    version: "1.0.7",
    url: fileName,
    sha256: sha256Hex(contents),
    released_at: todayDateString(),
    changelog: [],
  });
});

test("updates an existing same-version entry incrementally and preserves changelog", async () => {
  const { root, releaseDir, releaseServerDir } = await createProjectFixture();
  const fileName = "file-sync-tool-1.0.7-202604271215.exe";
  const contents = "newer exe payload for same version";

  await createVersionedExe(releaseDir, fileName, contents);

  await writeFile(
    path.join(releaseServerDir, "manifest.json"),
    JSON.stringify(
      {
        latest: "1.0.6",
        versions: [
          {
            version: "1.0.6",
            url: "file-sync-tool-1.0.6-202604201020.exe",
            sha256: "aa".repeat(32),
            released_at: "2026-04-20",
            changelog: ["older release"],
          },
          {
            version: "1.0.7",
            url: "file-sync-tool-1.0.7-202604261000.exe",
            sha256: "bb".repeat(32),
            released_at: "2026-04-26",
            changelog: ["keep this note"],
          },
        ],
      },
      null,
      2
    )
  );

  const result = await runUpdateManifestScript(root);

  assert.equal(result.code, 0, result.stderr);

  const manifest = await readManifest(releaseServerDir);
  assert.equal(manifest.latest, "1.0.7");
  assert.equal(manifest.versions.length, 2);
  assert.deepEqual(manifest.versions[0], {
    version: "1.0.7",
    url: fileName,
    sha256: sha256Hex(contents),
    released_at: todayDateString(),
    changelog: ["keep this note"],
  });
  assert.deepEqual(manifest.versions[1], {
    version: "1.0.6",
    url: "file-sync-tool-1.0.6-202604201020.exe",
    sha256: "aa".repeat(32),
    released_at: "2026-04-20",
    changelog: ["older release"],
  });
});

test("prepends a new version entry and keeps older history intact", async () => {
  const { root, releaseDir, releaseServerDir } = await createProjectFixture();
  const fileName = "file-sync-tool-1.0.7-202604271240.exe";
  const contents = "payload for new version insertion";

  await createVersionedExe(releaseDir, fileName, contents);

  await writeFile(
    path.join(releaseServerDir, "manifest.json"),
    JSON.stringify(
      {
        latest: "1.0.6",
        versions: [
          {
            version: "1.0.6",
            url: "file-sync-tool-1.0.6-202604201020.exe",
            sha256: "aa".repeat(32),
            released_at: "2026-04-20",
            changelog: ["older release"],
          },
        ],
      },
      null,
      2
    )
  );

  const result = await runUpdateManifestScript(root);

  assert.equal(result.code, 0, result.stderr);

  const manifest = await readManifest(releaseServerDir);
  assert.equal(manifest.latest, "1.0.7");
  assert.equal(manifest.versions.length, 2);
  assert.deepEqual(manifest.versions[0], {
    version: "1.0.7",
    url: fileName,
    sha256: sha256Hex(contents),
    released_at: todayDateString(),
    changelog: [],
  });
  assert.deepEqual(manifest.versions[1], {
    version: "1.0.6",
    url: "file-sync-tool-1.0.6-202604201020.exe",
    sha256: "aa".repeat(32),
    released_at: "2026-04-20",
    changelog: ["older release"],
  });
});

test("fails instead of overwriting malformed manifest history", async () => {
  const { root, releaseDir, releaseServerDir } = await createProjectFixture();
  const fileName = "file-sync-tool-1.0.7-202604271230.exe";

  await createVersionedExe(releaseDir, fileName, "payload");

  const manifestPath = path.join(releaseServerDir, "manifest.json");
  await writeFile(manifestPath, "{ broken json");

  const before = await readFile(manifestPath, "utf8");
  const result = await runUpdateManifestScript(root);
  const after = await readFile(manifestPath, "utf8");

  assert.notEqual(result.code, 0);
  assert.equal(after, before);
  assert.match(result.stderr, /manifest/i);
});
