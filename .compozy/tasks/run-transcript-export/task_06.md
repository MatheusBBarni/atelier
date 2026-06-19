---
status: pending
title: "CLI --export-session wiring and end-to-end suite"
type: backend
complexity: medium
dependencies:
  - task_04
  - task_05
---

# Task 06: CLI --export-session wiring and end-to-end suite

## Overview
Wire the `--export-session` command into the CLI — flags, relaxed `--yes`/`--update` guards, the `run_cli_with` dispatch arm, and a `confirm_export` helper — update every `Cli` struct-literal test site, and add the `tests/export_session.rs` end-to-end suite that drives the real binary. This makes the feature invocable and proves it end to end.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `--export-session <ID|latest>`, `--run <RUN_ID>`, `--out <PATH|->`, and `--allow-flagged` to the `Cli` struct in `src/cli.rs`.
- MUST relax the `--yes` guard (currently `--clean-sessions`-only) and add the new flags to the `--update` mega-exclusion; `--allow-flagged` and `--run` MUST require `--export-session`.
- MUST add a `run_cli_with` dispatch arm that builds `ExportOptions` and calls `export_session`, plus a `confirm_export` helper mirroring `confirm_cleanup` (stderr prompt, stdin read).
- MUST update every `Cli` struct-literal construction in tests to include the new fields, or the crate will not compile.
- MUST add `tests/export_session.rs` with the assert_cmd cases from TechSpec "Testing Approach".
- Fail-closed MUST surface the stderr nudge and exit non-zero; the success path prints the written path to stdout.
</requirements>

## Subtasks
- [ ] 6.1 Add the flags and validation guards (relax `--yes`/`--update`).
- [ ] 6.2 Add the `run_cli_with` dispatch arm and the `confirm_export` helper.
- [ ] 6.3 Update all `Cli` struct-literal test sites.
- [ ] 6.4 Add `tests/export_session.rs` e2e suite (clean, interactive, fail-closed, allow-flagged, egress warning).
- [ ] 6.5 Verify `--help` lists the new flag with a house-tone description.

## Implementation Details
Modify `src/cli.rs` (flags, validation, dispatch, `confirm_export`) and add the `tests/export_session.rs` suite. The dispatch arm calls `export_session` (task_04); the egress-warning e2e case exercises task_05. See TechSpec "CLI Surface" and "Development Sequencing". Use the `tests/cli.rs` patterns (`Command::cargo_bin`, inline-TOML `write_config`, the `fake` runtime) and drive the interactive gate with `.write_stdin(...)`.

### Relevant Files
- `src/cli.rs` — `Cli` struct (~13-66), validation block (~74-129), `run_cli_with` (~73), `confirm_cleanup` (~235), `Cli` struct-literal tests (~260-400).
- `tests/cli.rs` — `assert_cmd`/`predicates` patterns, `write_config` fixture, `fake`-runtime config consts.
- `src/export.rs` — `export_session`/`ExportOptions` (task_04).

### Dependent Files
- All `Cli` struct-literal test sites in `src/cli.rs` — must add the new fields.
- `tests/export_session.rs` — new integration suite.

### Related ADRs
- [ADR-002: CLI-first surface and gate vocab](../adrs/adr-002.md) — `y`/`approve`, `--yes`, `--out`.
- [ADR-004: Fail-closed enforcement and relaxed guards](../adrs/adr-004.md) — `--allow-flagged`, `bail!` on Deterministic hits.

## Deliverables
- CLI flags + validation + dispatch arm + `confirm_export` in `src/cli.rs`.
- Updated `Cli` struct-literal test sites (crate compiles).
- `tests/export_session.rs` integration suite.
- Unit + integration tests with 80%+ coverage **(REQUIRED)**.

## Tests
- Unit tests (in `src/cli.rs`):
  - [ ] `--allow-flagged` without `--export-session` → validation error.
  - [ ] `--run` without `--export-session` → validation error.
  - [ ] `--yes` with `--export-session` is accepted (guard relaxed); `--yes` alone still rejected.
- Integration tests (`tests/export_session.rs`, assert_cmd):
  - [ ] Clean session: `--export-session <id> --out f.md --yes` → success; `f.md` exists with mode `0600`, contains the prompt, contains no seeded secret, and a `session_exported` event is appended.
  - [ ] Flagged + interactive: `.write_stdin("approve\n")` → success; `.write_stdin("n\n")` → no file written.
  - [ ] Flagged + `--yes` without `--allow-flagged` → `.failure()` + stderr nudge + no file.
  - [ ] Flagged + `--yes --allow-flagged` → `.success()`, override recorded in the event.
  - [ ] `--out` to a non-ignored path → stderr egress warning present.
  - [ ] `--export-session latest` resolves the newest seeded session.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `atelier --export-session` works end to end; fail-closed exits non-zero with the nudge
- `Cli` struct-literal churn complete (crate compiles); `--help` shows the flag
