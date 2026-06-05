export const PACKAGE_SCOPE = "@matheusbbarni";
export const TOP_LEVEL_PACKAGE = `${PACKAGE_SCOPE}/atelier`;

export const TARGETS = Object.freeze([
  Object.freeze({ key: "darwin-arm64", os: "darwin", cpu: "arm64", exe: "atelier", archive: "tar.gz" }),
  Object.freeze({ key: "darwin-x64", os: "darwin", cpu: "x64", exe: "atelier", archive: "tar.gz" }),
  Object.freeze({ key: "linux-arm64", os: "linux", cpu: "arm64", libc: "glibc", exe: "atelier", archive: "tar.gz" }),
  Object.freeze({ key: "linux-x64", os: "linux", cpu: "x64", libc: "glibc", exe: "atelier", archive: "tar.gz" }),
  Object.freeze({ key: "win32-arm64", os: "win32", cpu: "arm64", exe: "atelier.exe", archive: "zip" }),
  Object.freeze({ key: "win32-x64", os: "win32", cpu: "x64", exe: "atelier.exe", archive: "zip" }),
]);

export function platformPackageName(key) {
  return `${PACKAGE_SCOPE}/atelier-${key}`;
}

export function targetForKey(key) {
  return TARGETS.find((target) => target.key === key) ?? null;
}

export function supportedTargetKeys() {
  return TARGETS.map((target) => target.key);
}

export function detectLibc(proc = process) {
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

export function targetKeyFor(platform = process.platform, arch = process.arch, libc) {
  const runtimeLibc = platform === "linux" ? libc ?? detectLibc() : undefined;
  const target = TARGETS.find((candidate) => {
    if (candidate.os !== platform || candidate.cpu !== arch) {
      return false;
    }
    return !candidate.libc || candidate.libc === runtimeLibc;
  });
  return target?.key ?? null;
}

export function currentTargetKey() {
  return targetKeyFor(process.platform, process.arch);
}

export function archiveFileName(version, target) {
  const extension = target.archive === "zip" ? "zip" : "tar.gz";
  return `atelier-v${version}-${target.key}.${extension}`;
}
