import assert from "node:assert/strict";
import { test } from "node:test";
import { cargoPackageVersion } from "../scripts/common.mjs";
import {
  checkPackageVersions,
  collectPackageVersions,
  normalizeVersion,
} from "../scripts/check-versions.mjs";

test("normalizeVersion accepts tags with or without leading v", () => {
  assert.equal(normalizeVersion("v1.2.3"), "1.2.3");
  assert.equal(normalizeVersion("1.2.3"), "1.2.3");
});

test("npm package versions match Cargo package version", () => {
  const version = cargoPackageVersion();
  const packages = collectPackageVersions();
  assert.equal(packages.length, 7);
  assert.deepEqual(checkPackageVersions(version), []);
});
