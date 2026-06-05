import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdtempSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  archivesDir,
  cargoPackageVersion,
  distDir,
  ensureDir,
  run,
  sourcePackageDir,
  sourcePlatformDir,
} from "./common.mjs";
import { TARGETS, archiveFileName } from "./targets.mjs";

function findFile(root, name) {
  const entries = readdirSync(root, { withFileTypes: true });
  for (const entry of entries) {
    const fullPath = join(root, entry.name);
    if (entry.isFile() && entry.name === name) {
      return fullPath;
    }
    if (entry.isDirectory()) {
      const nested = findFile(fullPath, name);
      if (nested) {
        return nested;
      }
    }
  }
  return null;
}

function extractArchive(archivePath, target) {
  const extractDir = mkdtempSync(join(tmpdir(), `atelier-${target.key}-`));
  if (target.archive === "zip") {
    run("unzip", ["-q", archivePath, "-d", extractDir]);
  } else {
    run("tar", ["-xzf", archivePath, "-C", extractDir]);
  }
  return extractDir;
}

function assembleTarget(root, archivesRoot, version, target, { allowMissingTargets }) {
  const archivePath = resolve(archivesRoot, archiveFileName(version, target));
  if (!existsSync(archivePath)) {
    if (allowMissingTargets) {
      console.log(`skipping ${target.key}; missing ${basename(archivePath)}`);
      return false;
    }
    throw new Error(`missing release archive ${archivePath}`);
  }

  const packageDir = resolve(root, "platform", target.key);
  cpSync(resolve(sourcePlatformDir, target.key), packageDir, { recursive: true });
  const binDir = resolve(packageDir, "bin");
  ensureDir(binDir);

  const extractDir = extractArchive(archivePath, target);
  const nativeBinary = findFile(extractDir, target.exe);
  if (!nativeBinary) {
    throw new Error(`${archivePath} does not contain ${target.exe}`);
  }

  const destination = resolve(binDir, target.exe);
  copyFileSync(nativeBinary, destination);
  if (target.os !== "win32") {
    chmodSync(destination, 0o755);
  }
  console.log(`assembled ${target.key} from ${archivePath}`);
  return true;
}

export function assemble({
  version = process.env.ATELIER_VERSION ?? cargoPackageVersion(),
  outputRoot = distDir(),
  archivesRoot = archivesDir(),
  allowMissingTargets = process.env.ATELIER_ALLOW_MISSING_TARGETS === "1",
} = {}) {
  rmSync(resolve(outputRoot, "package"), { recursive: true, force: true });
  rmSync(resolve(outputRoot, "platform"), { recursive: true, force: true });
  ensureDir(outputRoot);
  ensureDir(resolve(outputRoot, "platform"));

  cpSync(sourcePackageDir, resolve(outputRoot, "package"), { recursive: true });

  let assembledTargets = 0;
  for (const target of TARGETS) {
    if (assembleTarget(outputRoot, archivesRoot, version, target, { allowMissingTargets })) {
      assembledTargets += 1;
    }
  }

  if (assembledTargets === 0) {
    throw new Error("no platform packages were assembled");
  }
  if (!allowMissingTargets && assembledTargets !== TARGETS.length) {
    throw new Error(`assembled ${assembledTargets}/${TARGETS.length} platform packages`);
  }
  console.log(`assembled npm package tree in ${outputRoot}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  assemble();
}
