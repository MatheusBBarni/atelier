// Build-time reference generator (`npm run generate`, also `prebuild`/`predev`).
//
// Runs the `atelier` binary's `--emit-docs` subcommand to write the Configuration
// and CLI & Commands reference fragments into the git-ignored `_generated/` area
// of the docs content collection (ADR-003/004), then stitches the hand-written
// `[presets.*]` reference onto the generated Configuration page (presets are
// absent from the merged config). Astro's `glob()` loader then merges these with
// the committed prose pages at build time.
//
// Locally the binary is run via `cargo run` (a build dependency of the site). CI
// (task_12) builds the binary once and points `ATELIER_BIN` at it to skip the
// rebuild. Failure is fail-closed: a non-zero exit aborts the build.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { appendPresetsSection } from "./lib/docgen.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const webDir = resolve(scriptDir, "..");
const repoRoot = resolve(webDir, "..");

const generatedDir = join(webDir, "src", "content", "docs", "_generated");
const presetsFile = join(scriptDir, "presets-reference.md");

mkdirSync(generatedDir, { recursive: true });

// Prefer a prebuilt binary (CI) when ATELIER_BIN is set; otherwise build+run via
// cargo from the repo root (local dev).
const atelierBin = process.env.ATELIER_BIN;
const [command, baseArgs, cwd] = atelierBin
  ? [atelierBin, [], webDir]
  : ["cargo", ["run", "--quiet", "--bin", "atelier", "--"], repoRoot];

const args = [...baseArgs, "--emit-docs", generatedDir];
console.log(`generate: ${command} ${args.join(" ")}`);

const result = spawnSync(command, args, { cwd, stdio: "inherit" });
if (result.error) {
  console.error(`generate: failed to run ${command}: ${result.error.message}`);
  process.exit(1);
}
if (result.status !== 0) {
  console.error(`generate: ${command} exited with status ${result.status}`);
  process.exit(result.status ?? 1);
}

// Stitch the hand-written presets reference onto the generated Configuration page.
const configPath = join(generatedDir, "configuration.md");
const configMarkdown = readFileSync(configPath, "utf8");
const presetsMarkdown = readFileSync(presetsFile, "utf8");
writeFileSync(configPath, appendPresetsSection(configMarkdown, presetsMarkdown));

console.log(`generate: wrote reference fragments + presets into ${generatedDir}`);
