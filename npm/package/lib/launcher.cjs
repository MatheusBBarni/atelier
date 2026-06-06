"use strict";

const { spawn } = require("node:child_process");
const os = require("node:os");
const path = require("node:path");
const {
  TARGETS,
  supportedTargetKeys,
  targetForPlatform,
} = require("./targets.cjs");

const TOP_LEVEL_PACKAGE = "@matheusbbarni/atelier";

function unsupportedPlatformMessage(platform, arch, libc) {
  const libcSuffix = libc ? ` libc=${libc}` : "";
  return [
    `unsupported platform: platform=${platform} arch=${arch}${libcSuffix}`,
    `supported targets: ${supportedTargetKeys().join(", ")}`,
    "Download a native archive from GitHub Releases or build from source with Cargo.",
  ].join("\n");
}

function missingOptionalDependencyMessage(target) {
  return [
    `missing optional dependency for ${target.key}`,
    `expected package: ${target.packageName}`,
    "Optional dependencies may have been omitted by npm configuration.",
    "Reinstall with: npm install -g @matheusbbarni/atelier --include=optional",
  ].join("\n");
}

function resolveBinary({
  env = process.env,
  platform = process.platform,
  arch = process.arch,
  libc,
  requireResolve = require.resolve,
} = {}) {
  if (env.ATELIER_BINARY_PATH && env.ATELIER_BINARY_PATH.trim() !== "") {
    return {
      binaryPath: env.ATELIER_BINARY_PATH,
      target: null,
      overridden: true,
    };
  }

  const target = targetForPlatform(platform, arch, libc);
  if (!target) {
    throw new Error(unsupportedPlatformMessage(platform, arch, libc));
  }

  try {
    return {
      binaryPath: requireResolve(`${target.packageName}/${target.binPath}`),
      target,
      overridden: false,
    };
  } catch (error) {
    if (error && error.code === "MODULE_NOT_FOUND") {
      throw new Error(missingOptionalDependencyMessage(target));
    }
    throw error;
  }
}

function exitCodeForSignal(signal) {
  const signalNumber = os.constants.signals?.[signal];
  return typeof signalNumber === "number" ? 128 + signalNumber : 1;
}

function isUpdateCommand(argv) {
  return argv.length === 1 && argv[0] === "--update";
}

function pathApiForPlatform(platform) {
  return platform === "win32" ? path.win32 : path.posix;
}

function inferNpmPrefix(packageRoot, platform = process.platform) {
  const pathApi = pathApiForPlatform(platform);
  const resolved = pathApi.resolve(packageRoot);
  const parts = resolved.split(pathApi.sep);
  const nodeModulesIndex = parts.lastIndexOf("node_modules");
  if (nodeModulesIndex === -1) {
    return null;
  }

  let prefixParts = parts.slice(0, nodeModulesIndex);
  if (platform !== "win32" && prefixParts[prefixParts.length - 1] === "lib") {
    prefixParts = prefixParts.slice(0, -1);
  }

  return prefixParts.join(pathApi.sep) || pathApi.parse(resolved).root;
}

function npmCommand(platform = process.platform) {
  return platform === "win32" ? "npm.cmd" : "npm";
}

function updateArgs({ packageName = TOP_LEVEL_PACKAGE, prefix = null } = {}) {
  const args = ["install", "--global"];
  if (prefix) {
    args.push("--prefix", prefix);
  }
  args.push(
    "--include=optional",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    `${packageName}@latest`,
  );
  return args;
}

function runUpdate({
  env = process.env,
  cwd = process.cwd(),
  platform = process.platform,
  packageRoot = path.resolve(__dirname, ".."),
  spawnImpl = spawn,
  exit = process.exit,
  stderr = process.stderr,
} = {}) {
  const prefix = inferNpmPrefix(packageRoot, platform);
  const args = updateArgs({ prefix });
  stderr.write(
    prefix
      ? `Updating atelier with npm in ${prefix}\n`
      : "Updating atelier with npm\n",
  );

  const child = spawnImpl(npmCommand(platform), args, {
    cwd,
    env,
    stdio: "inherit",
  });

  child.on("error", (error) => {
    stderr.write(`atelier update failed to start npm: ${error.message}\n`);
    exit(1);
  });

  child.on("exit", (code, signal) => {
    exit(code ?? exitCodeForSignal(signal));
  });

  return child;
}

function spawnBinary({
  binaryPath,
  argv = process.argv.slice(2),
  env = process.env,
  cwd = process.cwd(),
  spawnImpl = spawn,
  exit = process.exit,
  stderr = process.stderr,
}) {
  const child = spawnImpl(binaryPath, argv, {
    cwd,
    env,
    stdio: "inherit",
  });

  child.on("error", (error) => {
    stderr.write(`atelier launcher failed to start native binary: ${error.message}\n`);
    exit(1);
  });

  child.on("exit", (code, signal) => {
    exit(code ?? exitCodeForSignal(signal));
  });

  return child;
}

function main({
  argv = process.argv.slice(2),
  env = process.env,
  cwd = process.cwd(),
  platform = process.platform,
  arch = process.arch,
  libc,
  packageRoot = path.resolve(__dirname, ".."),
  requireResolve = require.resolve,
  spawnImpl = spawn,
  exit = process.exit,
  stderr = process.stderr,
} = {}) {
  if (isUpdateCommand(argv)) {
    return runUpdate({
      env,
      cwd,
      platform,
      packageRoot,
      spawnImpl,
      exit,
      stderr,
    });
  }

  let resolved;
  try {
    resolved = resolveBinary({ env, platform, arch, libc, requireResolve });
  } catch (error) {
    stderr.write(`atelier launcher error:\n${error.message}\n`);
    exit(1);
    return null;
  }

  return spawnBinary({
    binaryPath: resolved.binaryPath,
    argv,
    env,
    cwd,
    spawnImpl,
    exit,
    stderr,
  });
}

module.exports = {
  TARGETS,
  inferNpmPrefix,
  isUpdateCommand,
  main,
  missingOptionalDependencyMessage,
  runUpdate,
  resolveBinary,
  spawnBinary,
  updateArgs,
  unsupportedPlatformMessage,
};
