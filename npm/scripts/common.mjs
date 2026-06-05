import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const scriptsDir = dirname(fileURLToPath(import.meta.url));
export const npmRoot = resolve(scriptsDir, "..");
export const repoRoot = resolve(npmRoot, "..");
export const sourcePackageDir = resolve(npmRoot, "package");
export const sourcePlatformDir = resolve(npmRoot, "platform");

export function resolveFromRepo(value, fallback) {
  const raw = process.env[value] ?? fallback;
  return isAbsolute(raw) ? raw : resolve(repoRoot, raw);
}

export function distDir() {
  return resolveFromRepo("ATELIER_NPM_DIST_DIR", "target/npm-dist");
}

export function archivesDir() {
  return resolveFromRepo("ATELIER_RELEASE_ARCHIVES_DIR", "target/npm-dist/archives");
}

export function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

export function writeJson(path, data) {
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`);
}

export function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? repoRoot,
    env: options.env ?? process.env,
    stdio: options.stdio ?? "inherit",
    encoding: options.encoding ?? "utf8",
    shell: options.shell ?? false,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed with exit code ${result.status}`);
  }
  return result;
}

export function capture(command, args, options = {}) {
  const result = run(command, args, {
    ...options,
    stdio: "pipe",
    encoding: "utf8",
  });
  return result.stdout.trim();
}

export function ensureDir(path) {
  mkdirSync(path, { recursive: true });
}

export function cargoPackageVersion() {
  const output = capture("cargo", ["metadata", "--locked", "--format-version", "1", "--no-deps"], {
    cwd: repoRoot,
  });
  const metadata = JSON.parse(output);
  const rootPackageId =
    metadata.resolve?.root ?? metadata.workspace_default_members?.[0] ?? metadata.workspace_members?.[0];
  const rootPackage = metadata.packages.find((pkg) => pkg.id === rootPackageId);
  if (!rootPackage?.version) {
    throw new Error("failed to read Cargo package version from cargo metadata");
  }
  return rootPackage.version;
}

export function packageManifestPath(packageDir) {
  return resolve(packageDir, "package.json");
}

export function assertFileExists(path, label = path) {
  if (!existsSync(path)) {
    throw new Error(`missing ${label}`);
  }
}
