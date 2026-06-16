---
status: pending
title: "Add doctor strict flag, exit gate, and discovery nudge"
type: backend
complexity: medium
dependencies:
  - task_04
---

# Task 05: Add doctor strict flag, exit gate, and discovery nudge

## Overview
Add an opt-in `--strict` flag that makes `atelier --doctor` exit non-zero when the report has any error, while leaving plain `--doctor` exiting 0. When plain `--doctor` finds errors, print a one-line stderr nudge pointing to `--strict`. Standard output and `--json` stay clean for scripts.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `--strict` (`strict: bool`) to the `Cli` struct, mirroring the `--doctor`/`--json` `#[arg(long)]` attributes.
- MUST reject `--strict` without `--doctor` with `bail!("--strict is only valid with --doctor")`, alongside the existing flag-combination validations.
- MUST, under `--strict` and `report.has_errors()`, return an `Err` so the process exits non-zero via `main.rs`.
- MUST, under plain `--doctor` with errors, print a one-line nudge to stderr and still exit 0.
- MUST keep `--json` and stdout output uncorrupted — nudge and error annotation go to stderr only.
- MUST NOT change the exit behavior of plain `--doctor` (stays 0).
</requirements>

## Subtasks
- [ ] 5.1 Add the `strict` flag to `Cli`.
- [ ] 5.2 Add the `--strict requires --doctor` validation bail.
- [ ] 5.3 Gate the doctor dispatch: `--strict` + errors → `Err`; else errors → stderr nudge.
- [ ] 5.4 Add integration tests for every exit-code path.

## Implementation Details
See TechSpec "Implementation Design → Core Interfaces" (the CLI gate + nudge block) and "API Endpoints" (the exit-code table). The dispatch is cli.rs ~156-164; validation bails are at ~62; `main.rs` maps any `Err`→`exit(1)`. Emitting a distinct exit code `2` is explicitly out of scope (ADR-001) — V1 returns `0` or a generic non-zero. For the healthy-path test, configure the orchestrator on an always-available runtime (e.g. `fake`); for the failure path, point it at a runtime whose command is missing.

### Relevant Files
- `src/cli.rs` — `Cli` struct (~12-54), flag-combination validation (~62), doctor dispatch (~156-164).
- `tests/cli.rs` — `assert_cmd` integration tests (`.success()`/`.failure()`).
- `src/doctor/mod.rs` — `has_errors()` / `error_count()` (task_04) consumed by the gate and nudge.
- `src/main.rs` — `Err`→`exit(1)` mapping (unchanged; provides the non-zero exit).

### Dependent Files
- `README.md`, `.github/workflows/release.yml` — task_06 documents and dogfoods the flag.

### Related ADRs
- [ADR-001: V1 scope and exit-code contract for scriptable config validation](adrs/adr-001.md) — Opt-in `--strict`, reserved `0/1/2` contract (emit `0/1`), discovery nudge.
- [ADR-003: Orchestrator-only runtime elevation, decoupled from --strict](adrs/adr-003.md) — `--strict` only gates the exit; severity comes from the report.

## Deliverables
- `--strict` flag with `--strict requires --doctor` validation.
- Exit gate (Err under strict+errors) and stderr discovery nudge (plain doctor+errors).
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for all exit-code paths **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `run_cli_with` rejects `--strict` without `--doctor` with the expected message.
- Integration tests (`tests/cli.rs`, `assert_cmd`):
  - [ ] `--doctor --strict` on a healthy temp config (orchestrator on `fake`) → `.success()` (exit 0).
  - [ ] `--doctor --strict` on a config whose orchestrator runtime command is missing → `.failure()` (non-zero).
  - [ ] Plain `--doctor` with errors → `.success()` and stderr contains `--strict`.
  - [ ] `--strict` without `--doctor` → `.failure()` with the bail message on stderr.
  - [ ] `--doctor --strict --json` on errors → stdout parses as JSON; exit is non-zero.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Exit codes are correct across healthy/unhealthy/strict/non-strict paths.
- Plain `--doctor` exit behavior is unchanged; stdout/JSON stay clean.
