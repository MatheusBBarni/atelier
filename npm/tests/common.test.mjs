import assert from "node:assert/strict";
import { test } from "node:test";
import { defaultShellForCommand } from "../scripts/common.mjs";

test("defaultShellForCommand uses a shell for npm command shims on Windows", () => {
  assert.equal(defaultShellForCommand("npm", "win32"), true);
  assert.equal(defaultShellForCommand("npx", "win32"), true);
});

test("defaultShellForCommand does not force a shell for normal commands", () => {
  assert.equal(defaultShellForCommand("node", "win32"), false);
  assert.equal(defaultShellForCommand("npm", "darwin"), false);
  assert.equal(defaultShellForCommand("npm", "linux"), false);
});
