import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);
const {
  inferNpmPrefix,
  isUpdateCommand,
  main,
  resolveBinary,
  runUpdate,
  spawnBinary,
  updateArgs,
  unsupportedPlatformMessage,
} = require("../package/lib/launcher.cjs");
const { targetKeyFor } = require("../package/lib/targets.cjs");

test("targetKeyFor maps supported platform and architecture pairs", () => {
  assert.equal(targetKeyFor("darwin", "arm64"), "darwin-arm64");
  assert.equal(targetKeyFor("darwin", "x64"), "darwin-x64");
  assert.equal(targetKeyFor("linux", "arm64", "glibc"), "linux-arm64");
  assert.equal(targetKeyFor("linux", "x64", "glibc"), "linux-x64");
  assert.equal(targetKeyFor("win32", "arm64"), "win32-arm64");
  assert.equal(targetKeyFor("win32", "x64"), "win32-x64");
});

test("resolveBinary returns ATELIER_BINARY_PATH without platform resolution", () => {
  const resolved = resolveBinary({
    env: { ATELIER_BINARY_PATH: "/tmp/custom-atelier" },
    platform: "freebsd",
    arch: "riscv64",
    requireResolve() {
      throw new Error("requireResolve should not be called");
    },
  });

  assert.equal(resolved.binaryPath, "/tmp/custom-atelier");
  assert.equal(resolved.overridden, true);
});

test("resolveBinary resolves the matching optional dependency binary", () => {
  const resolved = resolveBinary({
    env: {},
    platform: "darwin",
    arch: "arm64",
    requireResolve(request) {
      assert.equal(request, "@matheusbbarni/atelier-darwin-arm64/bin/atelier");
      return "/native/atelier";
    },
  });

  assert.equal(resolved.binaryPath, "/native/atelier");
  assert.equal(resolved.target.key, "darwin-arm64");
});

test("resolveBinary reports unsupported platforms with supported targets", () => {
  assert.throws(
    () =>
      resolveBinary({
        env: {},
        platform: "freebsd",
        arch: "x64",
      }),
    /unsupported platform: platform=freebsd arch=x64/,
  );
  assert.match(unsupportedPlatformMessage("linux", "x64", "unknown"), /supported targets:/);
});

test("resolveBinary reports omitted optional dependency with reinstall command", () => {
  assert.throws(
    () =>
      resolveBinary({
        env: {},
        platform: "linux",
        arch: "x64",
        libc: "glibc",
        requireResolve() {
          const error = new Error("Cannot find module");
          error.code = "MODULE_NOT_FOUND";
          throw error;
        },
      }),
    /npm install -g @matheusbbarni\/atelier --include=optional/,
  );
});

test("update command is only the standalone --update flag", () => {
  assert.equal(isUpdateCommand(["--update"]), true);
  assert.equal(isUpdateCommand(["--update", "--doctor"]), false);
  assert.equal(isUpdateCommand(["--doctor"]), false);
});

test("inferNpmPrefix maps npm package roots back to global prefixes", () => {
  assert.equal(
    inferNpmPrefix("/opt/npm/lib/node_modules/@matheusbbarni/atelier", "linux"),
    "/opt/npm",
  );
  assert.equal(
    inferNpmPrefix(
      "C:\\Users\\me\\AppData\\Roaming\\npm\\node_modules\\@matheusbbarni\\atelier",
      "win32",
    ),
    "C:\\Users\\me\\AppData\\Roaming\\npm",
  );
  assert.equal(inferNpmPrefix("/workspace/npm/package", "linux"), null);
});

test("updateArgs installs the latest top-level package with optional deps", () => {
  assert.deepEqual(updateArgs({ prefix: "/opt/npm" }), [
    "install",
    "--global",
    "--prefix",
    "/opt/npm",
    "--include=optional",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    "@matheusbbarni/atelier@latest",
  ]);
});

test("runUpdate spawns npm install and exits with child status", () => {
  const child = new EventEmitter();
  const exits = [];
  let stderr = "";
  let spawnCall;

  runUpdate({
    cwd: "/workspace",
    env: { PATH: "/bin" },
    platform: "linux",
    packageRoot: "/opt/npm/lib/node_modules/@matheusbbarni/atelier",
    spawnImpl(command, args, options) {
      spawnCall = { command, args, options };
      return child;
    },
    exit(code) {
      exits.push(code);
    },
    stderr: {
      write(chunk) {
        stderr += chunk;
      },
    },
  });

  child.emit("exit", 0, null);

  assert.equal(spawnCall.command, "npm");
  assert.deepEqual(spawnCall.args, updateArgs({ prefix: "/opt/npm" }));
  assert.equal(spawnCall.options.cwd, "/workspace");
  assert.equal(spawnCall.options.env.PATH, "/bin");
  assert.equal(spawnCall.options.stdio, "inherit");
  assert.match(stderr, /Updating atelier with npm in \/opt\/npm/);
  assert.deepEqual(exits, [0]);
});

test("main handles --update before resolving the native binary", () => {
  const child = new EventEmitter();
  let spawnCall;

  main({
    argv: ["--update"],
    platform: "linux",
    packageRoot: "/opt/npm/lib/node_modules/@matheusbbarni/atelier",
    requireResolve() {
      throw new Error("native binary should not be resolved for --update");
    },
    spawnImpl(command, args, options) {
      spawnCall = { command, args, options };
      return child;
    },
    exit() {},
    stderr: { write() {} },
  });

  assert.equal(spawnCall.command, "npm");
  assert.deepEqual(spawnCall.args, updateArgs({ prefix: "/opt/npm" }));
});

test("spawnBinary passes argv env cwd and exits with child status", () => {
  const child = new EventEmitter();
  const exits = [];
  let spawnCall;

  spawnBinary({
    binaryPath: "/native/atelier",
    argv: ["--version"],
    env: { TEST_ENV: "1" },
    cwd: "/workspace",
    spawnImpl(command, args, options) {
      spawnCall = { command, args, options };
      return child;
    },
    exit(code) {
      exits.push(code);
    },
    stderr: { write() {} },
  });

  child.emit("exit", 7, null);

  assert.equal(spawnCall.command, "/native/atelier");
  assert.deepEqual(spawnCall.args, ["--version"]);
  assert.equal(spawnCall.options.cwd, "/workspace");
  assert.equal(spawnCall.options.env.TEST_ENV, "1");
  assert.equal(spawnCall.options.stdio, "inherit");
  assert.deepEqual(exits, [7]);
});

test("spawnBinary exits one when the native binary cannot start", () => {
  const child = new EventEmitter();
  const exits = [];
  let stderr = "";

  spawnBinary({
    binaryPath: "/missing/atelier",
    spawnImpl() {
      return child;
    },
    exit(code) {
      exits.push(code);
    },
    stderr: {
      write(chunk) {
        stderr += chunk;
      },
    },
  });

  child.emit("error", new Error("ENOENT"));

  assert.deepEqual(exits, [1]);
  assert.match(stderr, /failed to start native binary/);
});
