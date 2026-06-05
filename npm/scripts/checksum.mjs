import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { archivesDir, distDir, repoRoot } from "./common.mjs";

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function collectFiles(outputRoot, archivesRoot) {
  const files = [];
  if (existsSync(archivesRoot)) {
    for (const file of readdirSync(archivesRoot)) {
      if (file.endsWith(".tar.gz") || file.endsWith(".zip")) {
        files.push(resolve(archivesRoot, file));
      }
    }
  }
  for (const file of readdirSync(outputRoot)) {
    if (file.endsWith(".tgz")) {
      files.push(resolve(outputRoot, file));
    }
  }
  return files.sort();
}

export function checksum({ outputRoot = distDir(), archivesRoot = archivesDir() } = {}) {
  const files = collectFiles(outputRoot, archivesRoot);
  if (files.length === 0) {
    throw new Error("no archives or npm tarballs found for checksum generation");
  }
  const lines = files.map((file) => `${sha256(file)}  ${relative(repoRoot, file)}`);
  const checksumPath = resolve(outputRoot, "SHA256SUMS");
  writeFileSync(checksumPath, `${lines.join("\n")}\n`);
  console.log(`wrote ${checksumPath}`);
  return checksumPath;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  checksum();
}
