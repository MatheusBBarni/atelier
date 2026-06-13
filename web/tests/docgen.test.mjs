import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { appendPresetsSection } from "../scripts/lib/docgen.mjs";

const scriptsDir = join(dirname(fileURLToPath(import.meta.url)), "..", "scripts");

test("appendPresetsSection appends the presets section onto the config page", () => {
  const config = '---\ntitle: "Configuration"\nnav_order: 30\n---\n\n# Configuration\n\nBody.\n';
  const presets = "## Presets — `[presets.*]`\n\nNamed agent overrides.\n";
  const combined = appendPresetsSection(config, presets);

  assert.ok(combined.startsWith(config.trimEnd()), "keeps the generated config body first");
  assert.ok(combined.includes("## Presets — `[presets.*]`"), "includes the presets heading");
  assert.ok(combined.endsWith("\n"), "ends with a single trailing newline");
  assert.ok(!combined.endsWith("\n\n"), "does not leave a trailing blank line");
});

test("appendPresetsSection is idempotent over re-generated input", () => {
  // The generator rewrites configuration.md from scratch each run, so feeding the
  // same fresh body twice must yield identical output (no double-append drift).
  const config = "# Configuration\n\nBody.\n";
  const presets = "## Presets\n\nText.\n";
  assert.equal(
    appendPresetsSection(config, presets),
    appendPresetsSection(config, presets),
  );
});

test("appendPresetsSection normalizes irregular whitespace between sections", () => {
  const combined = appendPresetsSection("# Configuration\n\n\n", "\n\n## Presets\n\nText.\n\n\n");
  assert.equal(combined, "# Configuration\n\n## Presets\n\nText.\n");
});

test("the committed presets reference documents the [presets.*] schema", () => {
  const presets = readFileSync(join(scriptsDir, "presets-reference.md"), "utf8");
  assert.ok(presets.includes("[presets."), "names the [presets.*] table");
  assert.ok(/^preset = /m.test(presets), "shows the top-level preset selector key");
  assert.ok(
    presets.includes("[presets.fast.agents.fixer]"),
    "shows a concrete per-agent preset override",
  );
});
