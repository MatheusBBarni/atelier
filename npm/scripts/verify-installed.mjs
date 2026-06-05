import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { cargoPackageVersion, distDir, readJson, run } from "./common.mjs";
import { TOP_LEVEL_PACKAGE, currentTargetKey, platformPackageName } from "./targets.mjs";

function manifestEntry(entries, packageName) {
  return entries.find((entry) => entry.package === packageName);
}

function installArgs({ outputRoot, version, registry }) {
  if (registry) {
    return [`${TOP_LEVEL_PACKAGE}@${version}`];
  }

  const manifest = readJson(resolve(outputRoot, "npm-packages.json"));
  const targetKey = currentTargetKey();
  if (!targetKey) {
    throw new Error(`current platform is not supported: ${process.platform}/${process.arch}`);
  }

  const topEntry = manifestEntry(manifest, TOP_LEVEL_PACKAGE);
  const platformEntry = manifestEntry(manifest, platformPackageName(targetKey));
  if (!topEntry) {
    throw new Error(`missing packed top-level package ${TOP_LEVEL_PACKAGE}`);
  }
  if (!platformEntry) {
    throw new Error(`missing packed platform package ${platformPackageName(targetKey)}`);
  }

  return [resolve(outputRoot, platformEntry.filename), resolve(outputRoot, topEntry.filename)];
}

function npmGlobalBin(prefix) {
  const candidates =
    process.platform === "win32"
      ? [resolve(prefix, "atelier.cmd"), resolve(prefix, "atelier")]
      : [resolve(prefix, "bin", "atelier")];
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) {
    throw new Error(`failed to find installed atelier command under ${prefix}`);
  }
  return found;
}

function runInstalled(binary, args, { cwd, env }) {
  const result = spawnSync(binary, args, {
    cwd,
    env,
    encoding: "utf8",
    stdio: "pipe",
    shell: process.platform === "win32",
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `${binary} ${args.join(" ")} failed with exit code ${result.status}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
    );
  }
  return result.stdout;
}

export function verifyInstalled({
  outputRoot = distDir(),
  version = process.env.ATELIER_VERSION ?? cargoPackageVersion(),
  registry = process.env.ATELIER_VERIFY_REGISTRY === "1",
} = {}) {
  const prefix = mkdtempSync(join(tmpdir(), "atelier-npm-prefix-"));
  const cwd = mkdtempSync(join(tmpdir(), "atelier-npm-cwd-"));
  const installTargets = installArgs({ outputRoot, version, registry });

  run(
    "npm",
    [
      "install",
      "--global",
      "--prefix",
      prefix,
      "--include=optional",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      ...installTargets,
    ],
    { cwd },
  );

  const binary = npmGlobalBin(prefix);
  const env = {
    ...process.env,
    PATH: `${resolve(prefix, process.platform === "win32" ? "" : "bin")}${delimiter}${process.env.PATH ?? ""}`,
    MULTIAGENT_CONFIG: resolve(cwd, "multiagent.toml"),
  };
  writeFileSync(env.MULTIAGENT_CONFIG, "");

  const versionOutput = runInstalled(binary, ["--version"], { cwd, env });
  if (!versionOutput.includes(version) || !versionOutput.includes("atelier")) {
    throw new Error(`unexpected atelier --version output: ${versionOutput}`);
  }

  const helpOutput = runInstalled(binary, ["--help"], { cwd, env });
  if (!helpOutput.includes("Usage: atelier")) {
    throw new Error("atelier --help output did not include Usage: atelier");
  }

  const doctorOutput = runInstalled(binary, ["--doctor", "--json"], { cwd, env });
  const doctor = JSON.parse(doctorOutput);
  if (doctor.schema_version !== 1 || !Array.isArray(doctor.checks)) {
    throw new Error(`unexpected doctor JSON shape: ${doctorOutput}`);
  }

  console.log(`verified installed ${TOP_LEVEL_PACKAGE}@${version} via ${binary}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  verifyInstalled();
}
