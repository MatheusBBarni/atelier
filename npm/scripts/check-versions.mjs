import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  capture,
  cargoPackageVersion,
  npmRoot,
  packageManifestPath,
  readJson,
  repoRoot,
  sourcePackageDir,
  sourcePlatformDir,
} from "./common.mjs";
import { TARGETS, TOP_LEVEL_PACKAGE, platformPackageName } from "./targets.mjs";

export function normalizeVersion(version) {
  return version.startsWith("v") ? version.slice(1) : version;
}

export function collectPackageVersions() {
  const packages = [
    {
      name: TOP_LEVEL_PACKAGE,
      manifest: readJson(packageManifestPath(sourcePackageDir)),
    },
  ];
  for (const target of TARGETS) {
    const packageDir = resolve(sourcePlatformDir, target.key);
    packages.push({
      name: platformPackageName(target.key),
      manifest: readJson(packageManifestPath(packageDir)),
    });
  }
  return packages;
}

export function checkPackageVersions(expectedVersion) {
  const failures = [];
  for (const { name, manifest } of collectPackageVersions()) {
    if (manifest.version !== expectedVersion) {
      failures.push(`${name} version ${manifest.version} != ${expectedVersion}`);
    }
  }

  const topManifest = readJson(packageManifestPath(sourcePackageDir));
  for (const target of TARGETS) {
    const name = platformPackageName(target.key);
    if (topManifest.optionalDependencies?.[name] !== expectedVersion) {
      failures.push(`${TOP_LEVEL_PACKAGE} optional dependency ${name} is not ${expectedVersion}`);
    }
  }
  return failures;
}

export function checkBinaryVersion(expectedVersion) {
  const binaryVersion = process.env.ATELIER_BINARY_PATH
    ? capture(process.env.ATELIER_BINARY_PATH, ["--version"], { cwd: repoRoot })
    : capture("cargo", ["run", "--locked", "--quiet", "--bin", "atelier", "--", "--version"], {
        cwd: repoRoot,
      });
  if (!binaryVersion.includes(expectedVersion) || !binaryVersion.includes("atelier")) {
    throw new Error(`atelier --version output did not include atelier ${expectedVersion}: ${binaryVersion}`);
  }
}

export function checkVersions({
  expectedVersion = normalizeVersion(process.env.ATELIER_VERSION ?? cargoPackageVersion()),
  checkBinary = process.env.ATELIER_SKIP_BINARY_VERSION_CHECK !== "1",
} = {}) {
  if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(expectedVersion)) {
    throw new Error(`expected a stable semver version, got ${expectedVersion}`);
  }

  const cargoVersion = cargoPackageVersion();
  const failures = [];
  if (cargoVersion !== expectedVersion) {
    failures.push(`Cargo version ${cargoVersion} != ${expectedVersion}`);
  }
  failures.push(...checkPackageVersions(expectedVersion));

  if (failures.length > 0) {
    throw new Error(failures.join("\n"));
  }
  if (checkBinary) {
    checkBinaryVersion(expectedVersion);
  }
  return expectedVersion;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const version = checkVersions();
  console.log(`versions match ${version} in Cargo, npm manifests, optional dependencies, and atelier --version`);
}
