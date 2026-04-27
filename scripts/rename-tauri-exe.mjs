import { rename, unlink } from "node:fs/promises";
import path from "node:path";
import {
  buildVersionedExeFileName,
  findSourceExe,
  getReleaseDirCandidates,
  loadTauriBuildContext,
} from "./versioned-exe-utils.mjs";

async function main() {
  const root = process.cwd();
  const { binaryName, productName, version } = await loadTauriBuildContext(
    root
  );

  const { releaseDir, sourceExe } = await findSourceExe(
    getReleaseDirCandidates(root),
    binaryName
  );

  const outputFileName = buildVersionedExeFileName(productName, version);
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
