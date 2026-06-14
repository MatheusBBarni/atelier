# Task Memory: task_11.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Wire `atelier --emit-docs` output into the Astro build (git-ignored `_generated/`), hand-write the `[presets.*]` reference, and verify the generated Configuration + CLI pages render themed and flow into the machine-readable surfaces. DONE.

## Important Decisions
- **Slug flattening was the crux.** The glob loader turned `_generated/configuration.md` into id `_generated/configuration`, so pages served at `/docs/_generated/configuration/` and the prose forward-links `../configuration/`, `../cli/` 404'd. Fixed with `generateId: ({entry}) => entry.replace(/^_generated\//,"").replace(/\.md$/,"")` in `web/src/content.config.ts` → clean slugs `configuration`/`cli`. This is what actually "wires" the generated reference to the site.
- **Presets stitched onto the SAME Configuration page** (not a separate page): the task requires "the Configuration page includes the `[presets.*]` section". The generate script appends the committed `web/scripts/presets-reference.md` onto the generated `_generated/configuration.md` via the pure `appendPresetsSection` helper. Idempotent because emit-docs rewrites configuration.md from scratch each run.
- `generate` runs the binary via `cargo run` locally; honors `ATELIER_BIN` env to use a prebuilt binary (task_12/CI path). Fail-closed on non-zero exit.

## Learnings
- npm `prebuild`/`predev` lifecycle hooks auto-run `generate` before `build`/`dev` — no manual chaining in the `dev`/`build` scripts themselves.
- The committed presets partial MUST live outside `src/content/docs/` (it's at `web/scripts/presets-reference.md`) — the collection glob `**/*.md` would otherwise pick it up as a broken frontmatter-less page.
- Remaining link-check errors after this task are ONLY `/docs/governance` (task_06). All `configuration`/`cli` link errors are resolved by task_11.

## Files / Surfaces
- `web/package.json` — added `generate`, `predev`, `prebuild` scripts.
- `web/.gitignore` — added `src/content/docs/_generated/`.
- `web/scripts/generate-docs.mjs` — emit-docs runner + presets stitch.
- `web/scripts/lib/docgen.mjs` — pure `appendPresetsSection`.
- `web/scripts/presets-reference.md` — hand-written `[presets.*]` reference.
- `web/src/content.config.ts` — `generateId` to flatten `_generated/` from slugs.
- `web/src/pages/docs/[...slug].md.ts` — comment updated (ids now flat).
- `web/tests/docgen.test.mjs` — 4 tests for the append helper + presets content.

## Errors / Corrections
- First build served generated pages at `/docs/_generated/...`; caught by `check:links` (7 errors). Added `generateId` → down to 3 (governance only).

## Ready for Next Run
- task_12 (CI): the same `generate` flow; set `ATELIER_BIN` to the built binary to skip `cargo run`, and run `npm run generate` before `astro build` in the Pages workflow. Landing task_06's governance page turns the link gate fully green.
