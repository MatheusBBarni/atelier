---
status: pending
title: "Wire generated reference into the docs site"
type: frontend
complexity: medium
dependencies:
  - task_07
  - task_10
---

# Wire generated reference into the docs site

## Overview

Connect the generator's output to the Astro build: a `generate`/`prebuild` script that runs
`atelier --emit-docs` into a git-ignored `_generated/` area of the docs collection, the
hand-written `[presets.*]` reference (absent from the generated config), and verification
that the generated Configuration and CLI & Commands pages render through `DocsLayout` and
flow into the machine-readable surfaces (PRD F7/F8, ADR-003/004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `web/package.json` `generate` script that runs the `atelier` binary's `--emit-docs` into `web/src/content/docs/_generated`, plus a `prebuild`/`predev` hook so local build and dev produce the fragments first.
- MUST add `_generated/` to `web/.gitignore` — generated content MUST NOT be committed.
- MUST add the hand-written `[presets.*]` reference (sourced from the `--init-config` template), since presets are absent from the generated config output.
- MUST ensure the generated Configuration and CLI & Commands pages render through `DocsLayout` with correct `nav_order` and appear in `llms.txt`, `llms-full.txt`, and the `.md` twins.
- Generated Markdown tables MUST render with the themed doc styles from task_03.
</requirements>

## Subtasks
- [ ] 11.1 Add the `generate` + `prebuild`/`predev` scripts to `web/package.json`.
- [ ] 11.2 Add `_generated/` to `web/.gitignore`.
- [ ] 11.3 Author the hand-written `[presets.*]` reference into the Configuration source.
- [ ] 11.4 Verify the generated pages render through `DocsLayout` and are themed.
- [ ] 11.5 Verify the generated pages appear in `llms-full.txt` and have `.md` twins.

## Implementation Details

Modify `web/package.json` (scripts) and `web/.gitignore`; add the committed Configuration
wrapper / presets section; generated fragments land in the git-ignored
`web/src/content/docs/_generated/`. See TechSpec "Build Order" step 8 and "Data Models" (the
`[presets.*]` exception). Local dev runs the binary via `cargo run`; CI builds it (task_12).

### Relevant Files
- `web/package.json` — the `dev`/`build` scripts to chain a `generate` step onto.
- `web/.gitignore` — currently `node_modules/`, `dist/`, `.astro/`, `.verification/`; add `_generated/`.
- `src/config/mod.rs:2417` — the `--init-config` presets example to hand-write the `[presets.*]` reference from.
- `web/src/content.config.ts` (task_03) — the schema the generated frontmatter must satisfy.

### Dependent Files
- `.github/workflows/*` (task_12) — runs the same generate flow in CI.
- `web/src/pages/llms*.ts` (task_07) — pick up the generated entries automatically.

### Related ADRs
- [ADR-003: Reference generator — a Rust --emit-docs subcommand](../adrs/adr-003.md) — build-time generation, no committed artifacts.
- [ADR-004: Docs site as all-Markdown content collections](../adrs/adr-004.md) — generated entries consumed by the collection.

## Deliverables
- `generate`/`prebuild` scripts and a git-ignored `_generated/` dir.
- The hand-written `[presets.*]` reference section.
- Generated Configuration + CLI pages rendering and flowing into the machine-readable surfaces.
- Build + integration verification **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Not applicable (build wiring); covered by the integration checks below.
- Integration tests:
  - [ ] `npm run generate` produces `_generated/configuration.md` and `_generated/cli.md`.
  - [ ] `npm run build` renders those pages through `DocsLayout` with their `nav_order`, and Markdown tables use the themed doc styles.
  - [ ] The Configuration page includes the hand-written `[presets.*]` section.
  - [ ] The generated pages appear in `llms-full.txt` and each has a `/docs/<slug>.md` twin.
  - [ ] After `npm run generate`, `git status` is clean (the `_generated/` dir is ignored).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Generated reference renders, is themed, and flows into the machine-readable surfaces.
- Presets are documented; nothing generated is committed.
