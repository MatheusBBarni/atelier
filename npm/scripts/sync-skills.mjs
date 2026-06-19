// Regenerates atelier's skill-discovery mirrors from the canonical skill source
// and (with --check) asserts they have not drifted (ADR-004).
//
// Source of truth:  skills/atelier-config-setup/   (the only hand-edited tree)
// Mirrors:
//   .agents/skills/atelier-config-setup/   real byte-copy (atelier's own root)
//   .claude/skills/atelier-config-setup    symlink -> ../../.agents/skills/...
//     (matches the repo's existing .claude/skills/<name> symlink convention)
//
// The drift contract is **content equality, following symlinks**: every file
// under the canonical tree must exist with identical bytes under each mirror
// path (reading through the .claude symlink yields the .agents copy). This is
// the same rule the task_06 mirror-equality test and `--check` both apply, so a
// real copy or a symlink both satisfy it as long as the bytes match.
//
// Usage:
//   node scripts/sync-skills.mjs            regenerate the mirrors
//   node scripts/sync-skills.mjs --check    exit non-zero if any mirror drifted

import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { repoRoot } from "./common.mjs";

export const SKILL_NAME = "atelier-config-setup";
export const canonicalDir = resolve(repoRoot, "skills", SKILL_NAME);

// Mirror order matters for sync: the real copy must exist before the symlink
// that points at it.
export const mirrors = [
  { kind: "copy", path: resolve(repoRoot, ".agents", "skills", SKILL_NAME) },
  {
    kind: "symlink",
    path: resolve(repoRoot, ".claude", "skills", SKILL_NAME),
    linkTarget: `../../.agents/skills/${SKILL_NAME}`,
  },
];

// Recursively list file paths under `root` relative to it, sorted. `statSync`
// follows symlinks, so passing a symlinked directory lists the target's files.
export function listRelFiles(root) {
  const files = [];
  const walk = (absDir) => {
    for (const name of readdirSync(absDir).sort()) {
      const abs = join(absDir, name);
      if (statSync(abs).isDirectory()) {
        walk(abs);
      } else {
        files.push(relative(root, abs));
      }
    }
  };
  walk(root);
  return files.sort();
}

// Returns an array of human-readable drift descriptions ([] means in sync).
export function compareTrees(srcDir, dstDir) {
  const failures = [];
  if (!existsSync(dstDir)) {
    return [`missing mirror ${dstDir}`];
  }
  const srcFiles = listRelFiles(srcDir);
  const dstFiles = listRelFiles(dstDir);
  const srcSet = new Set(srcFiles);
  const dstSet = new Set(dstFiles);
  for (const f of srcFiles) {
    if (!dstSet.has(f)) {
      failures.push(`${dstDir}: missing ${f}`);
      continue;
    }
    if (!readFileSync(join(srcDir, f)).equals(readFileSync(join(dstDir, f)))) {
      failures.push(`${dstDir}: ${f} differs from canonical`);
    }
  }
  for (const f of dstFiles) {
    if (!srcSet.has(f)) {
      failures.push(`${dstDir}: stray ${f} not in canonical`);
    }
  }
  return failures;
}

export function syncCopy(srcDir, dstDir) {
  rmSync(dstDir, { recursive: true, force: true });
  mkdirSync(dirname(dstDir), { recursive: true });
  cpSync(srcDir, dstDir, { recursive: true });
}

export function syncSymlink(linkPath, linkTarget) {
  rmSync(linkPath, { recursive: true, force: true });
  mkdirSync(dirname(linkPath), { recursive: true });
  symlinkSync(linkTarget, linkPath);
}

export function syncSkills() {
  for (const mirror of mirrors) {
    if (mirror.kind === "symlink") {
      syncSymlink(mirror.path, mirror.linkTarget);
    } else {
      syncCopy(canonicalDir, mirror.path);
    }
  }
}

export function checkSkills() {
  const failures = [];
  for (const mirror of mirrors) {
    failures.push(...compareTrees(canonicalDir, mirror.path));
  }
  return failures;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (!existsSync(canonicalDir)) {
    throw new Error(`canonical skill source not found: ${canonicalDir}`);
  }
  if (process.argv.includes("--check")) {
    const failures = checkSkills();
    if (failures.length > 0) {
      console.error("skill mirrors are out of sync with the canonical source:");
      for (const failure of failures) {
        console.error(`  - ${failure}`);
      }
      console.error("run `npm --prefix npm run sync:skills` to regenerate them");
      process.exitCode = 1;
    } else {
      console.log(`skill mirrors match canonical ${SKILL_NAME}`);
    }
  } else {
    syncSkills();
    console.log(`synced ${SKILL_NAME} mirrors from canonical skills/ source`);
  }
}
