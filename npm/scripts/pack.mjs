import { readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { distDir, ensureDir, readJson, run } from "./common.mjs";
import { TARGETS, TOP_LEVEL_PACKAGE, platformPackageName } from "./targets.mjs";

function packPackage(packageDir, outputRoot) {
  const result = run("npm", ["pack", "--json", "--pack-destination", outputRoot], {
    cwd: packageDir,
    stdio: "pipe",
    encoding: "utf8",
  });
  const [pack] = JSON.parse(result.stdout);
  return {
    package: pack.name,
    version: pack.version,
    filename: basename(pack.filename),
    shasum: pack.shasum,
    integrity: pack.integrity,
  };
}

export function pack({ outputRoot = distDir() } = {}) {
  ensureDir(outputRoot);
  for (const file of readdirSync(outputRoot)) {
    if (file.endsWith(".tgz")) {
      rmSync(resolve(outputRoot, file), { force: true });
    }
  }

  const entries = [];
  const platformRoot = resolve(outputRoot, "platform");
  for (const target of TARGETS) {
    const packageDir = resolve(platformRoot, target.key);
    try {
      readJson(resolve(packageDir, "package.json"));
    } catch {
      continue;
    }
    entries.push(packPackage(packageDir, outputRoot));
  }

  const topPackageDir = resolve(outputRoot, "package");
  const topManifest = readJson(resolve(topPackageDir, "package.json"));
  if (topManifest.name !== TOP_LEVEL_PACKAGE) {
    throw new Error(`unexpected top-level package name ${topManifest.name}`);
  }
  entries.push(packPackage(topPackageDir, outputRoot));

  const publishedNames = entries.map((entry) => entry.package);
  for (const target of TARGETS) {
    const packageName = platformPackageName(target.key);
    if (!publishedNames.includes(packageName) && process.env.ATELIER_ALLOW_MISSING_TARGETS !== "1") {
      throw new Error(`missing packed platform package ${packageName}`);
    }
  }

  writeFileSync(resolve(outputRoot, "npm-packages.json"), `${JSON.stringify(entries, null, 2)}\n`);
  console.log(`packed ${entries.length} npm tarball(s) into ${outputRoot}`);
  return entries;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  pack();
}
