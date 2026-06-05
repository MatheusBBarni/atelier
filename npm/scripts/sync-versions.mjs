import { resolve } from "node:path";
import {
  cargoPackageVersion,
  packageManifestPath,
  readJson,
  sourcePackageDir,
  sourcePlatformDir,
  writeJson,
} from "./common.mjs";
import { TARGETS, platformPackageName } from "./targets.mjs";

const version = process.env.ATELIER_VERSION ?? cargoPackageVersion();
const topManifestPath = packageManifestPath(sourcePackageDir);
const topManifest = readJson(topManifestPath);
topManifest.version = version;
topManifest.optionalDependencies = topManifest.optionalDependencies ?? {};

for (const target of TARGETS) {
  const packageName = platformPackageName(target.key);
  topManifest.optionalDependencies[packageName] = version;

  const manifestPath = packageManifestPath(resolve(sourcePlatformDir, target.key));
  const manifest = readJson(manifestPath);
  manifest.version = version;
  writeJson(manifestPath, manifest);
}

writeJson(topManifestPath, topManifest);
console.log(`synced npm package versions to ${version}`);
