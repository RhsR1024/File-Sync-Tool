import { access, readFile, rename, unlink } from "node:fs/promises";
import path from "node:path";

function getCargoPackageName(cargoTomlContent) {
  const packageBlock = cargoTomlContent.match(
    /\[package\][\s\S]*?(?=\r?\n\[|$)/
  );
  if (!packageBlock) {
    throw new Error("Cannot find [package] section in Cargo.toml");
  }

  const nameMatch = packageBlock[0].match(/^\s*name\s*=\s*"([^"]+)"/m);
  if (!nameMatch) {
    throw new Error('Cannot find package name in Cargo.toml');
  }

  return nameMatch[1];
}

function toSafeFileName(value) {
  return value.trim().replace(/\s+/g, "-");
}

function getReleaseDirCandidates(root) {
  const cargoTargetDir = process.env.CARGO_TARGET_DIR;
  if (!cargoTargetDir) {
    return [path.join(root, "src-tauri", "target", "release")];
  }

  if (path.isAbsolute(cargoTargetDir)) {
    return [path.join(cargoTargetDir, "release")];
  }

  return [
    path.join(root, cargoTargetDir, "release"),
    path.join(root, "src-tauri", cargoTargetDir, "release"),
  ];
}

async function findSourceExe(releaseDirs, binaryName) {
  for (const releaseDir of releaseDirs) {
    const sourceExe = path.join(releaseDir, `${binaryName}.exe`);

    try {
      await access(sourceExe);
      return { releaseDir, sourceExe };
    } catch (error) {
      if (error && error.code !== "ENOENT") {
        throw error;
      }
    }
  }

  throw new Error(
    `Cannot find ${binaryName}.exe in any release directory: ${releaseDirs.join(
      ", "
    )}`
  );
}

async function main() {
  const root = process.cwd();
  const tauriConfigPath = path.join(root, "src-tauri", "tauri.conf.json");
  const cargoTomlPath = path.join(root, "src-tauri", "Cargo.toml");

  const tauriConfigContent = await readFile(tauriConfigPath, "utf8");
  const tauriConfig = JSON.parse(tauriConfigContent);
  const productName = tauriConfig.productName;
  const version = tauriConfig.version;

  if (!productName || !version) {
    throw new Error("tauri.conf.json must include productName and version");
  }

  const cargoTomlContent = await readFile(cargoTomlPath, "utf8");
  const binaryName = getCargoPackageName(cargoTomlContent);

  const { releaseDir, sourceExe } = await findSourceExe(
    getReleaseDirCandidates(root),
    binaryName
  );

  const now = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const timestamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}${pad(now.getHours())}${pad(now.getMinutes())}`;
  const outputFileName = `${toSafeFileName(productName)}-${version}-${timestamp}.exe`;
  const outputExe = path.join(releaseDir, outputFileName);

  try {
    await unlink(outputExe);
  } catch (error) {
    if (error && error.code !== "ENOENT") {
      throw error;
    }
  }

  await rename(sourceExe, outputExe);
  console.log(`Renamed to ${outputFileName}`);
  console.log(`Source: ${sourceExe}`);
  console.log(`Output: ${outputExe}`);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
