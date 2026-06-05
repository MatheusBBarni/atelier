"use strict";

const PACKAGE_SCOPE = "@matheusbbarni";
const TOP_LEVEL_PACKAGE = `${PACKAGE_SCOPE}/atelier`;

const TARGETS = Object.freeze({
  "darwin-arm64": Object.freeze({
    key: "darwin-arm64",
    os: "darwin",
    cpu: "arm64",
    exe: "atelier",
    archive: "tar.gz",
    packageName: `${PACKAGE_SCOPE}/atelier-darwin-arm64`,
    binPath: "bin/atelier",
  }),
  "darwin-x64": Object.freeze({
    key: "darwin-x64",
    os: "darwin",
    cpu: "x64",
    exe: "atelier",
    archive: "tar.gz",
    packageName: `${PACKAGE_SCOPE}/atelier-darwin-x64`,
    binPath: "bin/atelier",
  }),
  "linux-arm64": Object.freeze({
    key: "linux-arm64",
    os: "linux",
    cpu: "arm64",
    libc: "glibc",
    exe: "atelier",
    archive: "tar.gz",
    packageName: `${PACKAGE_SCOPE}/atelier-linux-arm64`,
    binPath: "bin/atelier",
  }),
  "linux-x64": Object.freeze({
    key: "linux-x64",
    os: "linux",
    cpu: "x64",
    libc: "glibc",
    exe: "atelier",
    archive: "tar.gz",
    packageName: `${PACKAGE_SCOPE}/atelier-linux-x64`,
    binPath: "bin/atelier",
  }),
  "win32-arm64": Object.freeze({
    key: "win32-arm64",
    os: "win32",
    cpu: "arm64",
    exe: "atelier.exe",
    archive: "zip",
    packageName: `${PACKAGE_SCOPE}/atelier-win32-arm64`,
    binPath: "bin/atelier.exe",
  }),
  "win32-x64": Object.freeze({
    key: "win32-x64",
    os: "win32",
    cpu: "x64",
    exe: "atelier.exe",
    archive: "zip",
    packageName: `${PACKAGE_SCOPE}/atelier-win32-x64`,
    binPath: "bin/atelier.exe",
  }),
});

function supportedTargetKeys() {
  return Object.keys(TARGETS);
}

function detectLibc(proc = process) {
  if (proc.platform !== "linux") {
    return undefined;
  }
  const report = proc.report?.getReport?.();
  const header = report?.header ?? {};
  if (header.glibcVersionRuntime || header.glibcVersionCompiler) {
    return "glibc";
  }
  return "unknown";
}

function targetKeyFor(platform = process.platform, arch = process.arch, libc) {
  const runtimeLibc = platform === "linux" ? libc ?? detectLibc() : undefined;
  for (const target of Object.values(TARGETS)) {
    if (target.os !== platform || target.cpu !== arch) {
      continue;
    }
    if (target.libc && runtimeLibc !== target.libc) {
      continue;
    }
    return target.key;
  }
  return null;
}

function targetForPlatform(platform = process.platform, arch = process.arch, libc) {
  const key = targetKeyFor(platform, arch, libc);
  return key ? TARGETS[key] : null;
}

module.exports = {
  PACKAGE_SCOPE,
  TOP_LEVEL_PACKAGE,
  TARGETS,
  detectLibc,
  supportedTargetKeys,
  targetForPlatform,
  targetKeyFor,
};
