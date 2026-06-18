import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { compareTrees, syncCopy, syncSymlink } from "../scripts/sync-skills.mjs";

function makeCanonical() {
  const root = mkdtempSync(join(tmpdir(), "skill-canon-"));
  writeFileSync(join(root, "SKILL.md"), "---\nname: x\n---\nbody\n");
  mkdirSync(join(root, "references"));
  writeFileSync(join(root, "references", "a.md"), "alpha\n");
  return root;
}

test("compareTrees reports no drift right after a copy sync", () => {
  const canonical = makeCanonical();
  const work = mkdtempSync(join(tmpdir(), "skill-mirror-"));
  const mirror = join(work, "atelier-config-setup");
  try {
    syncCopy(canonical, mirror);
    assert.deepEqual(compareTrees(canonical, mirror), []);
  } finally {
    rmSync(canonical, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
});

test("compareTrees detects an altered mirror file", () => {
  const canonical = makeCanonical();
  const work = mkdtempSync(join(tmpdir(), "skill-mirror-"));
  const mirror = join(work, "atelier-config-setup");
  try {
    syncCopy(canonical, mirror);
    writeFileSync(join(mirror, "references", "a.md"), "tampered\n");
    const failures = compareTrees(canonical, mirror);
    assert.equal(failures.length, 1);
    assert.match(failures[0], /references\/a\.md differs/);
  } finally {
    rmSync(canonical, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
});

test("compareTrees detects a stray mirror file", () => {
  const canonical = makeCanonical();
  const work = mkdtempSync(join(tmpdir(), "skill-mirror-"));
  const mirror = join(work, "atelier-config-setup");
  try {
    syncCopy(canonical, mirror);
    writeFileSync(join(mirror, "extra.md"), "extra\n");
    const failures = compareTrees(canonical, mirror);
    assert.equal(failures.length, 1);
    assert.match(failures[0], /stray extra\.md/);
  } finally {
    rmSync(canonical, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
});

test("compareTrees follows a symlink mirror to the copied tree", () => {
  const canonical = makeCanonical();
  const work = mkdtempSync(join(tmpdir(), "skill-mirror-"));
  const copy = join(work, "agents", "atelier-config-setup");
  const link = join(work, "claude", "atelier-config-setup");
  try {
    syncCopy(canonical, copy);
    // Relative symlink from work/claude/ to work/agents/atelier-config-setup.
    syncSymlink(link, "../agents/atelier-config-setup");
    assert.deepEqual(compareTrees(canonical, link), []);
  } finally {
    rmSync(canonical, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
});
