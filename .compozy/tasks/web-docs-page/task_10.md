---
status: pending
title: "Build the emit-docs reference generator"
type: backend
complexity: high
dependencies:
  - task_03
  - task_09
---

# Build the emit-docs reference generator

## Overview

Build the `src/docgen` module and the `atelier --emit-docs <dir>` flag (mirroring
`--codemap`) that reads the merged config, the CLI surface, and the slash catalog
in-process and writes **deterministic** Markdown reference fragments whose frontmatter
matches the docs collection schema. This is the anti-drift engine: reference can no longer
diverge from the binary (PRD F6, ADR-003/001).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `src/docgen/mod.rs` with the `emit_docs(&EffectiveConfig, &Path) -> Result<DocgenSummary>` entry from TechSpec "Core Interfaces", registered in `src/lib.rs`.
- MUST emit a Configuration reference fragment from `build_printable_config` (agents, runtimes, council, limits, ui, workspace) and a CLI & Commands fragment from `Cli::command()` (flags) plus `slash_commands::catalog()` (slash commands).
- MUST write fragments with `{title, nav_order}` frontmatter matching the docs collection schema.
- MUST be deterministic: stable ordering, no timestamps, and NO doctor/live-runtime output.
- MUST add the `--emit-docs` flag, a `bail!` exclusion guard, and dispatch in `src/cli.rs` (after `--print-config`), updating every existing `Cli { .. }` literal in the test module.
- MUST NOT add a `Serialize` derive to the slash types — read `catalog()` directly.
</requirements>

## Subtasks
- [ ] 10.1 Add the `docgen` module with `DocgenSummary` + `emit_docs`, and register it in `lib.rs`.
- [ ] 10.2 Configuration emitter: `build_printable_config` → Markdown.
- [ ] 10.3 CLI emitter: walk `Cli::command().get_arguments()` → Markdown.
- [ ] 10.4 Commands emitter: `slash_commands::catalog()` → Markdown.
- [ ] 10.5 Wire the `--emit-docs` flag + exclusion guard + dispatch; update all `Cli` test literals.
- [ ] 10.6 Write coverage + determinism unit tests.

## Implementation Details

New `src/docgen/mod.rs`; modify `src/cli.rs` (flag + guard + dispatch + test literals) and
`src/lib.rs` (register). Mirror `src/codemap` (entry fn + file writers + `render_*` Markdown
by `format!`). See TechSpec "Core Interfaces" and "Data Models". Consume
`build_printable_config` from task_09; the output dir/frontmatter contract comes from the
collection schema in task_03.

### Relevant Files
- `src/codemap/mod.rs:111,351,391` — the precedent: entry fn, file writer, Markdown render idiom.
- `src/cli.rs:57-163` (dispatch), `:58-84` (exclusion guards), `:117-121` (`--codemap` precedent), `:190-202`/`:210-222` (`Cli` test literals to update).
- `src/config/mod.rs` — `build_printable_config` (task_09).
- `src/slash_commands.rs:119` — `catalog()`; `Cli::command()` (clap `CommandFactory`, precedent at `src/cli.rs:179`).

### Dependent Files
- `web/src/content/docs/_generated/*.md` (task_11) — consumes the emitted fragments.
- `src/cli.rs` test module — every `Cli` literal gains the new field.

### Related ADRs
- [ADR-003: Reference generator — a Rust --emit-docs subcommand](../adrs/adr-003.md) — the primary decision.
- [ADR-001: Derive reference from source, ship alongside the README](../adrs/adr-001.md) — the anti-drift goal.

## Deliverables
- `src/docgen` module + `emit_docs`; the `--emit-docs` flag wired and guarded.
- Deterministic Configuration and CLI & Commands Markdown fragments with valid frontmatter.
- Unit tests (coverage + determinism) with 80%+ coverage **(REQUIRED)**.
- Integration test driving the flag end-to-end **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Over a default `EffectiveConfig`, the output contains every slash command from `catalog()` (assert each `SlashCommandSpec.label` appears).
  - [ ] The output contains every CLI long flag from `Cli::command().get_arguments()`.
  - [ ] The output contains each config section header (`[agents]`, `[runtimes]`, `[council]`, `[limits]`, `[ui]`, `[workspace]`).
  - [ ] Two consecutive `emit_docs` runs produce byte-identical files (determinism); no timestamp or live-runtime text appears.
  - [ ] Each fragment begins with valid `{title, nav_order}` frontmatter parseable by the collection schema.
- Integration tests:
  - [ ] `Cli { emit_docs: Some(tmp), .. }` via `run_cli_with` writes the fragments into `tmp` (mirroring the `--codemap` test).
  - [ ] `--emit-docs` combined with `--print-config` (or `--update`) fails with a `bail!` exclusion error.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The generator emits complete, deterministic reference; the coverage test fails CI on any undocumented flag/command/config section.
- `--print-config` behavior is unaffected.
