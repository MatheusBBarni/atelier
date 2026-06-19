---
status: completed
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
- [x] 8.1 Add a `hooks` `DoctorCheck` builder that reads configured handlers.
- [x] 8.2 Include the dropped-event count and last-fired status when available.
- [x] 8.3 Push the check into `run_doctor`'s check list.
- [x] 8.4 Add unit tests for the configured and not-configured cases.

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
  - [x] With two handlers configured, the check reports a count of 2 and an `Ok` status. — `hooks_check_reports_configured_count_and_dropped_field`
  - [x] With no hooks configured, the check reports a `Skipped`/neutral status (not a failure). — `hooks_check_is_skipped_when_none_configured`
  - [x] The check includes the dropped-event count field. — `hooks_check_reports_configured_count_and_dropped_field` (asserts `dropped_events`) (+ `last_hook_fired_picks_most_recent_completed`)
- Integration tests:
  - [x] `atelier --doctor --json` output contains a `hooks` check entry with the expected configured count. — `doctor_json_includes_hooks_check_with_configured_count`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `--doctor` reports configured handler count, last-fired status, and dropped count
- The check is neutral (not failing) when no hooks are configured

## As-built notes
- `hooks_check(config)` (pushed onto `run_doctor`'s list) reports `configured_handlers` from `config.hooks.handlers.len()` as the primary signal: `Ok` when ≥1, `Skipped`/`Info` when 0 (neutral, never an error). Auto-rendered in human + `--json` via the standard `DoctorCheck` shape; remediation points at `--init-config` and `--events follow`.
- **last-fired** is read best-effort from the local event log (`read_all_session_events` → most recent `hook_completed`, as `{time, event, status}`), reusing the doctor module's existing history reader. `None` → "none fired yet".
- **dropped-event count** is reported as a field (`dropped_events`), but is `0` in a standalone `--doctor` invocation: the counter is process-local to the running dispatcher (task_04). `App::dropped_hook_count()` (added in task_05) is available for a future in-session doctor view; wiring the live counter into the CLI path is out of scope here. Documented in the check context (`dropped_events_note`).
- Unit tests call `hooks_check`/`last_hook_fired` directly (fast, no runtime CLI shelling); the `--json` integration test uses `run_doctor`.
