import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  rmSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  archivesDir,
  cargoPackageVersion,
  distDir,
  ensureDir,
  npmRoot,
  repoRoot,
  run,
} from "./common.mjs";
import { archiveFileName, currentTargetKey, targetForKey } from "./targets.mjs";

function createLocalArchive(version, target) {
  const binaryPath = resolve(repoRoot, "target", "release", target.exe);
  if (!existsSync(binaryPath)) {
    throw new Error(`missing release binary ${binaryPath}`);
  }

  const stagingRoot = resolve(distDir(), "local-stage", target.key);
  rmSync(stagingRoot, { recursive: true, force: true });
  mkdirSync(stagingRoot, { recursive: true });
  const stagedBinary = resolve(stagingRoot, target.exe);
  copyFileSync(binaryPath, stagedBinary);
  if (target.os !== "win32") {
    chmodSync(stagedBinary, 0o755);
  }
  copyFileSync(resolve(repoRoot, "README.md"), resolve(stagingRoot, "README.md"));
  copyFileSync(resolve(repoRoot, "LICENSE"), resolve(stagingRoot, "LICENSE"));

  const outputRoot = archivesDir();
  ensureDir(outputRoot);
  const archivePath = resolve(outputRoot, archiveFileName(version, target));
  rmSync(archivePath, { force: true });
  if (target.archive === "zip") {
    run("powershell", [
      "-NoProfile",
      "-Command",
      `Compress-Archive -Path '${join(stagingRoot, "*")}' -DestinationPath '${archivePath}' -Force`,
    ]);
  } else {
    run("tar", ["-czf", archivePath, "-C", stagingRoot, "."]);
  }
  return archivePath;
}

export function dryRunLocal() {
  const version = cargoPackageVersion();
  const targetKey = currentTargetKey();
  if (!targetKey) {
    throw new Error(`current platform is not supported: ${process.platform}/${process.arch}`);
  }
  const target = targetForKey(targetKey);

  rmSync(distDir(), { recursive: true, force: true });
  run("cargo", ["test", "--locked"], { cwd: repoRoot });
  run("cargo", ["build", "--locked", "--release", "--bin", "atelier"], { cwd: repoRoot });
  const archivePath = createLocalArchive(version, target);
  console.log(`created local archive ${archivePath}`);

  const env = {
    ...process.env,
    ATELIER_VERSION: version,
    ATELIER_ALLOW_MISSING_TARGETS: "1",
  };
  run("node", ["scripts/check-versions.mjs"], { cwd: npmRoot, env });
  run("node", ["scripts/check-targets.mjs"], { cwd: npmRoot, env });
  run("npm", ["test"], { cwd: npmRoot, env });
  run("node", ["scripts/assemble.mjs"], { cwd: npmRoot, env });
  run("node", ["scripts/check-metadata.mjs"], { cwd: npmRoot, env });
  run("node", ["scripts/pack.mjs"], { cwd: npmRoot, env });
  run("node", ["scripts/checksum.mjs"], { cwd: npmRoot, env });
  run("node", ["scripts/verify-installed.mjs"], { cwd: npmRoot, env });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  dryRunLocal();
}
