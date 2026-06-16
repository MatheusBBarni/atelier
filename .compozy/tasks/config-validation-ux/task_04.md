---
status: pending
title: "Elevate unavailable orchestrator runtime to Error in run_doctor"
type: backend
complexity: medium
dependencies:
  - task_03
---

# Task 04: Elevate unavailable orchestrator runtime to Error in run_doctor

## Overview
In the doctor's runtime-availability loop, elevate a runtime that is `Unavailable` to `Error` status/severity when it is in the `required_runtime_ids()` set (V1: the orchestrator's runtime). Add `DoctorReport::error_count()`. Elevation is independent of `--strict` — the report always reflects true severity, so the exit code (task_05) can be a pure function of `has_errors()`.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST elevate an `Unavailable` required runtime to `DoctorStatus::Error` and `DoctorSeverity::Error` in `run_doctor`.
- MUST keep `Unknown` availability at `Warn` even for required runtimes.
- MUST keep non-required runtimes at `Warn` when `Unavailable`/`Unknown` (unchanged behavior).
- MUST add `DoctorReport::error_count(&self) -> usize` counting `Error`-status checks.
- MUST NOT consult any `--strict` flag in `run_doctor` — elevation is decoupled from the exit gate.
- MUST update any existing doctor tests whose fixtures leave the orchestrator runtime unavailable.
</requirements>

## Subtasks
- [ ] 4.1 Compute `required_runtime_ids()` once before the runtime-availability loop.
- [ ] 4.2 Elevate the required + `Unavailable` case to `Error` in the status/severity match.
- [ ] 4.3 Add `error_count()` to `DoctorReport` alongside `has_errors()`.
- [ ] 4.4 Add unit tests covering the elevation matrix.
- [ ] 4.5 Update existing doctor tests affected by the new severity.

## Implementation Details
See TechSpec "Implementation Design → Core Interfaces" (the doctor elevation block) and ADR-003. The availability loop is doctor/mod.rs ~65-99; `DoctorReport::has_errors()` already exists (~48) and is the basis for `error_count()`. Only the `Unavailable` arm for a required runtime changes; everything else keeps current mappings.

### Relevant Files
- `src/doctor/mod.rs` — Runtime-availability loop (~65-99), `DoctorReport`/`has_errors` (~39-53), `DoctorStatus`/`DoctorSeverity` (~11-26), existing doctor tests (~410-711).
- `src/config/mod.rs` — `required_runtime_ids()` (task_03).
- `src/runtime/mod.rs` — `RuntimeAvailabilityStatus` variants (`Available`/`Unavailable`/`Unknown`).

### Dependent Files
- `src/cli.rs` — task_05 gates the process exit on `has_errors()` / `error_count()`.

### Related ADRs
- [ADR-003: Orchestrator-only runtime elevation, decoupled from --strict](adrs/adr-003.md) — Only `Unavailable` required runtimes elevate; report reflects severity regardless of `--strict`.

## Deliverables
- Orchestrator-runtime Warn→Error elevation in `run_doctor`.
- `DoctorReport::error_count()`.
- Updated existing doctor test assertions.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration check that `run_doctor` yields `has_errors() == true` for an unavailable orchestrator runtime **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Orchestrator runtime `Unavailable` → its check is `DoctorStatus::Error` / `DoctorSeverity::Error`.
  - [ ] A non-orchestrator runtime `Unavailable` → its check stays `Warn`.
  - [ ] Orchestrator runtime `Unknown` → stays `Warn`.
  - [ ] `error_count()` returns the exact number of `Error` checks in a report.
- Integration tests:
  - [ ] `run_doctor` on a config whose orchestrator runtime command is missing yields `has_errors() == true`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The elevation matrix (required+Unavailable=Error; Unknown/non-required=Warn) holds.
- `error_count()` is available for the CLI nudge/gate; existing doctor tests pass.
