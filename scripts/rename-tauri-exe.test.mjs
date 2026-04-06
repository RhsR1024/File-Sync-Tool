import { mkdtemp, mkdir, readdir, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";

const scriptPath = fileURLToPath(new URL("./rename-tauri-exe.mjs", import.meta.url));
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
  const root = await mkdtemp(path.join(os.tmpdir(), "rename-tauri-exe-"));
  tempRoots.push(root);

  const srcTauriDir = path.join(root, "src-tauri");
  await mkdir(srcTauriDir, { recursive: true });

  await writeFile(
    path.join(srcTauriDir, "tauri.conf.json"),
    JSON.stringify(
      {
        productName: "File Sync Tool",
        version: "1.0.5",
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
      'version = "1.0.5"',
      "",
    ].join("\n")
  );

  return { root };
}

async function createSourceExe(releaseDir) {
  await mkdir(releaseDir, { recursive: true });
  await writeFile(path.join(releaseDir, "app.exe"), "fake exe");
}

function runRenameScript(cwd, env = {}) {
  const childEnv = { ...process.env, ...env };
  if (!Object.prototype.hasOwnProperty.call(env, "CARGO_TARGET_DIR")) {
    delete childEnv.CARGO_TARGET_DIR;
  }

  return new Promise((resolve) => {
    const child = spawn(process.execPath, [scriptPath], {
      cwd,
      env: childEnv,
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
      resolve({ code, stderr, stdout });
    });
  });
}

async function findRenamedExe(releaseDir) {
  const entries = await readdir(releaseDir);
  return entries.find((name) =>
    /^File-Sync-Tool-1\.0\.5-\d{12}\.exe$/.test(name)
  );
}

test("renames exe in default tauri release directory", async () => {
  const { root } = await createProjectFixture();
  const releaseDir = path.join(root, "src-tauri", "target", "release");
  await createSourceExe(releaseDir);

  const result = await runRenameScript(root);

  assert.equal(result.code, 0, result.stderr);
  const renamedExe = await findRenamedExe(releaseDir);
  assert.ok(renamedExe, "expected renamed exe in default release directory");
});

test("renames exe in CARGO_TARGET_DIR release directory", async () => {
  const { root } = await createProjectFixture();
  const cargoTargetDir = path.join(root, ".cargo-target");
  const releaseDir = path.join(cargoTargetDir, "release");
  await createSourceExe(releaseDir);

  const result = await runRenameScript(root, {
    CARGO_TARGET_DIR: cargoTargetDir,
  });

  assert.equal(result.code, 0, result.stderr);
  const renamedExe = await findRenamedExe(releaseDir);
  assert.ok(renamedExe, "expected renamed exe in custom target directory");
});
