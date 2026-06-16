---
status: pending
title: Doctor hooks check
type: backend
complexity: low
dependencies:
  - task_02
  - task_04
---

# Task 8: Doctor hooks check

## Overview
Add a "Lifecycle hooks" check to `atelier --doctor` reporting how many handlers are configured, when a hook last fired, and the dropped-event count. This is the PRD's local adoption signal — measured on the machine, with nothing phoned home.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `DoctorCheck` reporting the number of configured handlers (from `EffectiveConfig.hooks`) and, when available, the last-fired status and the dropped-event count (from task_04's counter).
- MUST report a `Skipped`/neutral status when no hooks are configured (not a failure).
- MUST appear in both the human and `--json` doctor outputs via the existing rendering.
- SHOULD include a short remediation/help string pointing at `--init-config` and `--events follow`.
</requirements>

## Subtasks
- [ ] 8.1 Add a `hooks` `DoctorCheck` builder that reads configured handlers.
- [ ] 8.2 Include the dropped-event count and last-fired status when available.
- [ ] 8.3 Push the check into `run_doctor`'s check list.
- [ ] 8.4 Add unit tests for the configured and not-configured cases.

## Implementation Details
Edit `src/doctor/mod.rs`: add a check builder mirroring the existing checks (`run_doctor`, `:55-100`) and push it onto the `checks` vector; reuse the `DoctorCheck { id, title, status, severity, message, remediation, context }` shape so both `render_human` and `render_json` pick it up automatically. Read `config.hooks` (task_02); read the dropped counter exposed by task_04 (a process-local counter — last-fired/dropped may be zero in a fresh non-TUI `--doctor` invocation, so report config-derived facts as the primary signal). See TechSpec "Monitoring and Observability".

### Relevant Files
- `src/doctor/mod.rs:55` — `run_doctor` + `DoctorCheck` shape; add and push the hooks check.
- `src/config/mod.rs` — `EffectiveConfig.hooks` (task_02) read for handler count.

### Dependent Files
- `src/hooks/dispatch.rs` — exposes the dropped-event counter (task_04).
- `README.md` — task_09 documents the doctor signal.

### Related ADRs
- [ADR-002: Thin dispatcher plus one built-in battery](../adrs/adr-002.md) — local-only adoption measurement via `--doctor`.

## Deliverables
- A "Lifecycle hooks" doctor check in human and JSON output.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test: `atelier --doctor --json` includes the hooks check with the configured count **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] With two handlers configured, the check reports a count of 2 and an `Ok` status.
  - [ ] With no hooks configured, the check reports a `Skipped`/neutral status (not a failure).
  - [ ] The check includes the dropped-event count field.
- Integration tests:
  - [ ] `atelier --doctor --json` output contains a `hooks` check entry with the expected configured count.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `--doctor` reports configured handler count, last-fired status, and dropped count
- The check is neutral (not failing) when no hooks are configured
