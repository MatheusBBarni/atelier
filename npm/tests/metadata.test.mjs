import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";
import { npmRoot, readJson, sourcePackageDir, sourcePlatformDir } from "../scripts/common.mjs";
import { validateSourceMetadata } from "../scripts/check-metadata.mjs";
import { TARGETS, TOP_LEVEL_PACKAGE, platformPackageName } from "../scripts/targets.mjs";

test("source package metadata is valid", () => {
  assert.doesNotThrow(() => validateSourceMetadata());
});

test("top-level package exposes exactly the atelier command", () => {
  const manifest = readJson(resolve(sourcePackageDir, "package.json"));
  assert.equal(manifest.name, TOP_LEVEL_PACKAGE);
  assert.deepEqual(manifest.bin, { atelier: "bin/atelier.js" });
  assert.equal(Object.keys(manifest.bin).length, 1);
  assert.equal(existsSync(resolve(sourcePackageDir, "bin/atelier")), false);
  assert.equal(existsSync(resolve(sourcePackageDir, "bin/atelier.exe")), false);
});

test("top-level optional dependencies cover every platform package at the same version", () => {
  const manifest = readJson(resolve(sourcePackageDir, "package.json"));
  const expected = Object.fromEntries(
    TARGETS.map((target) => [platformPackageName(target.key), manifest.version]),
  );
  assert.deepEqual(manifest.optionalDependencies, expected);
});

test("platform packages declare os and cpu and do not expose bin entries", () => {
  for (const target of TARGETS) {
    const manifest = readJson(resolve(sourcePlatformDir, target.key, "package.json"));
    assert.equal(manifest.name, platformPackageName(target.key));
    assert.deepEqual(manifest.os, [target.os]);
    assert.deepEqual(manifest.cpu, [target.cpu]);
    assert.equal("bin" in manifest, false);
    assert.equal(existsSync(resolve(sourcePlatformDir, target.key, "README.md")), true);
    assert.equal(existsSync(resolve(sourcePlatformDir, target.key, "LICENSE")), true);
  }
});

test("npm workspace root is private", () => {
  const manifest = readJson(resolve(npmRoot, "package.json"));
  assert.equal(manifest.private, true);
});
