import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  assertFileExists,
  distDir,
  packageManifestPath,
  readJson,
  sourcePackageDir,
  sourcePlatformDir,
} from "./common.mjs";
import { TARGETS, TOP_LEVEL_PACKAGE, platformPackageName } from "./targets.mjs";

function assertEqual(actual, expected, label) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${expected}, got ${actual}`);
  }
}

function assertArrayEquals(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function assertNoNativeBinaryInTopLevel(packageDir) {
  for (const file of ["bin/atelier", "bin/atelier.exe"]) {
    if (existsSync(resolve(packageDir, file))) {
      throw new Error(`top-level package must not contain native binary ${file}`);
    }
  }
}

function validateTopPackage(packageDir) {
  const manifest = readJson(packageManifestPath(packageDir));
  assertEqual(manifest.name, TOP_LEVEL_PACKAGE, "top-level package name");
  assertEqual(manifest.license, "MIT", "top-level license");
  assertEqual(manifest.bin?.atelier, "bin/atelier.js", "top-level bin entry");
  assertEqual(manifest.engines?.node, ">=20", "top-level node engine");
  assertArrayEquals(manifest.files, ["bin/", "lib/", "README.md", "LICENSE"], "top-level files");
  assertFileExists(resolve(packageDir, "README.md"), "top-level README.md");
  assertFileExists(resolve(packageDir, "LICENSE"), "top-level LICENSE");
  assertFileExists(resolve(packageDir, "bin/atelier.js"), "top-level bin/atelier.js");
  assertFileExists(resolve(packageDir, "lib/launcher.cjs"), "top-level lib/launcher.cjs");
  assertFileExists(resolve(packageDir, "lib/targets.cjs"), "top-level lib/targets.cjs");
  assertNoNativeBinaryInTopLevel(packageDir);

  for (const target of TARGETS) {
    const expectedPackage = platformPackageName(target.key);
    assertEqual(
      manifest.optionalDependencies?.[expectedPackage],
      manifest.version,
      `top-level optional dependency ${expectedPackage}`,
    );
  }
}

function validatePlatformPackage(packageDir, target, { requireBinary }) {
  const manifest = readJson(packageManifestPath(packageDir));
  assertEqual(manifest.name, platformPackageName(target.key), `${target.key} package name`);
  assertEqual(manifest.license, "MIT", `${target.key} license`);
  assertArrayEquals(manifest.os, [target.os], `${target.key} os`);
  assertArrayEquals(manifest.cpu, [target.cpu], `${target.key} cpu`);
  if (target.libc) {
    assertArrayEquals(manifest.libc, [target.libc], `${target.key} libc`);
  } else if (manifest.libc) {
    throw new Error(`${target.key} must not declare libc`);
  }
  if (manifest.bin) {
    throw new Error(`${target.key} must not expose a bin entry`);
  }
  assertArrayEquals(manifest.files, ["bin/", "README.md", "LICENSE"], `${target.key} files`);
  assertFileExists(resolve(packageDir, "README.md"), `${target.key} README.md`);
  assertFileExists(resolve(packageDir, "LICENSE"), `${target.key} LICENSE`);

  if (!requireBinary) {
    return;
  }

  const binDir = resolve(packageDir, "bin");
  const binary = resolve(binDir, target.exe);
  assertFileExists(binary, `${target.key} ${target.exe}`);
  const binFiles = readdirSync(binDir);
  assertArrayEquals(binFiles, [target.exe], `${target.key} bin files`);
  if (target.os !== "win32") {
    const mode = statSync(binary).mode;
    if ((mode & 0o111) === 0) {
      throw new Error(`${target.key} binary is not executable`);
    }
  }
}

function npmPackDryRunFiles(packageDir) {
  const result = spawnSync("npm", ["pack", "--dry-run", "--json"], {
    cwd: packageDir,
    encoding: "utf8",
    stdio: "pipe",
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`npm pack --dry-run failed in ${packageDir}:\n${result.stderr}`);
  }
  const [pack] = JSON.parse(result.stdout);
  return pack.files.map((file) => file.path).sort();
}

function validatePackOutput(packageDir, target) {
  const files = npmPackDryRunFiles(packageDir);
  for (const required of ["package.json", "README.md", "LICENSE"]) {
    if (!files.includes(required)) {
      throw new Error(`${packageDir} pack output missing ${required}`);
    }
  }
  if (!target) {
    for (const required of ["bin/atelier.js", "lib/launcher.cjs", "lib/targets.cjs"]) {
      if (!files.includes(required)) {
        throw new Error(`${packageDir} pack output missing ${required}`);
      }
    }
    if (files.includes("bin/atelier") || files.includes("bin/atelier.exe")) {
      throw new Error(`${packageDir} top-level pack output includes native binary`);
    }
    return;
  }
  if (!files.includes(`bin/${target.exe}`)) {
    throw new Error(`${packageDir} pack output missing bin/${target.exe}`);
  }
}

export function validateSourceMetadata() {
  validateTopPackage(sourcePackageDir);
  for (const target of TARGETS) {
    validatePlatformPackage(resolve(sourcePlatformDir, target.key), target, { requireBinary: false });
  }
}

export function validateAssembledMetadata({ allowMissingTargets = process.env.ATELIER_ALLOW_MISSING_TARGETS === "1" } = {}) {
  const root = distDir();
  const packageDir = resolve(root, "package");
  const platformRoot = resolve(root, "platform");
  if (!existsSync(packageDir)) {
    return false;
  }

  validateTopPackage(packageDir);
  validatePackOutput(packageDir, null);

  for (const target of TARGETS) {
    const targetDir = resolve(platformRoot, target.key);
    if (!existsSync(targetDir)) {
      if (allowMissingTargets) {
        continue;
      }
      throw new Error(`assembled package missing ${target.key}`);
    }
    validatePlatformPackage(targetDir, target, { requireBinary: true });
    validatePackOutput(targetDir, target);
  }
  return true;
}

export function checkMetadata() {
  validateSourceMetadata();
  const checkedAssembled = validateAssembledMetadata();
  return { checkedAssembled };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { checkedAssembled } = checkMetadata();
  console.log(
    checkedAssembled
      ? "source and assembled npm package metadata are valid"
      : "source npm package metadata is valid; no assembled package tree found",
  );
}
