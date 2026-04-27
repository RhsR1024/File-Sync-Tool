import { access, readFile, readdir } from "node:fs/promises";
import path from "node:path";

export function getCargoPackageName(cargoTomlContent) {
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

export function toSafeFileName(value) {
  return value.trim().replace(/\s+/g, "-");
}

export function getReleaseDirCandidates(root) {
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

export async function loadTauriBuildContext(root) {
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

  return {
    binaryName,
    productName,
    version,
  };
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function formatVersionedTimestamp(date = new Date()) {
  const pad = (value) => String(value).padStart(2, "0");
  return `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(
    date.getDate()
  )}${pad(date.getHours())}${pad(date.getMinutes())}`;
}

export function buildVersionedExeFileName(productName, version, date = new Date()) {
  return `${toSafeFileName(productName)}-${version}-${formatVersionedTimestamp(
    date
  )}.exe`;
}

export function createVersionedExePattern(productName, version) {
  return new RegExp(
    `^${escapeRegExp(toSafeFileName(productName))}-${escapeRegExp(
      version
    )}-(\\d{12})\\.exe$`
  );
}

export async function findSourceExe(releaseDirs, binaryName) {
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

export async function findLatestVersionedExe(releaseDirs, productName, version) {
  const pattern = createVersionedExePattern(productName, version);
  const candidates = [];

  for (const releaseDir of releaseDirs) {
    try {
      const entries = await readdir(releaseDir);
      for (const entry of entries) {
        const match = entry.match(pattern);
        if (!match) {
          continue;
        }

        candidates.push({
          fileName: entry,
          filePath: path.join(releaseDir, entry),
          releaseDir,
          timestamp: match[1],
        });
      }
    } catch (error) {
      if (error && error.code === "ENOENT") {
        continue;
      }
      throw error;
    }
  }

  if (candidates.length === 0) {
    throw new Error(
      `Cannot find versioned ${toSafeFileName(
        productName
      )}-${version}-*.exe in any release directory: ${releaseDirs.join(", ")}`
    );
  }

  candidates.sort((left, right) =>
    right.timestamp.localeCompare(left.timestamp)
  );

  return candidates[0];
}
