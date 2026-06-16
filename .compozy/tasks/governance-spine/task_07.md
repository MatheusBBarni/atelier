---
status: completed
title: Governance outcome proxy and calibration metrics in doctor
type: backend
complexity: medium
dependencies:
  - task_05
---

# Governance outcome proxy and calibration metrics in doctor

## Overview
Surface whether governance is working, honestly. Add a `--doctor --json` check that derives an outcome proxy (kept vs reverted) from the local event log plus the exact calibration metrics (intervention rate with a dual-alarm band, early-abort catch rate, gate precision). All local; the proxy is clearly labeled.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a governance metrics `DoctorCheck` registered in `run_doctor`, with its figures in the check's `context` JSON.
- MUST derive the Trusted Outcome Rate as a proxy from existing events — a governed run that completes and is not followed by an interrupt or corrective re-prompt counts as kept; early-abort reject and abort-after-accept count against — and MUST label it a proxy.
- MUST compute the exact calibration metrics from events: intervention rate (with a dual-alarm band: alarm if too high OR near-zero), early-abort catch rate, gate precision.
- MUST read only the local `.atelier` event log; no network, no telemetry.
- MUST behave safely on an empty/no-governance event log (zeros/None, no panic).
</requirements>

## Subtasks
- [x] 7.1 Add `governance_metrics_check` and register it in `run_doctor`.
- [x] 7.2 Derive the outcome proxy from `governance_decision_*` + run-outcome events.
- [x] 7.3 Compute intervention rate (dual-alarm band), early-abort catch rate, gate precision.
- [x] 7.4 Emit the figures in the check `context`, with the proxy clearly labeled.

## Implementation Details
Modify `src/doctor/mod.rs` (`run_doctor` ~55, new `governance_metrics_check`). Read events via the history store (`HistoryEvent.kind`/`payload`). Reference TechSpec "Monitoring and Observability" and ADR-005; do not reproduce the metric formulas verbatim.

### Relevant Files
- `src/doctor/mod.rs` — `run_doctor` (~55), `DoctorCheck` (~28), `DoctorReport` (~39).
- `src/history/mod.rs` — `HistoryEvent` (~12) for reading governance/run events.
- `src/app/mod.rs` — the `governance_decision_*` events emitted (task_03/task_05).

### Dependent Files
- None; doctor output only.

### Related ADRs
- [ADR-005: Outcome metric as an event-derived proxy](../adrs/adr-005.md) — proxy + exact calibration via `--doctor --json`.

## Deliverables
- A governance metrics `DoctorCheck` with proxy + calibration figures in `context`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An integration test of `--doctor --json` output **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] A completed governed run with no corrective re-prompt counts as kept; an early-abort reject and an abort-after-accept each count against the proxy.
  - [x] Intervention rate raises the dual-alarm when near-zero with reverts present, and when above the high band.
  - [x] Early-abort catch rate = rejects ÷ early-aborts fired; gate precision excludes trivial/read-only runs.
  - [x] An empty event log yields zeros/None without panicking.
- Integration tests:
  - [x] `--doctor --json` after a session containing governance events includes the metrics check with the labeled proxy and the calibration figures in `context`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `--doctor --json` reports the outcome proxy (labeled) + exact calibration metrics, derived locally with no telemetry.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.

## Completion Notes
- `governance.metrics` DoctorCheck registered in `run_doctor`, reading all `.atelier/sessions/*/events.jsonl` via the existing `list_session_event_paths` + `read_events_from_path` primitives (local-only, no network). Aggregation lives in the pure `governance_metrics_from_events(&[HistoryEvent])` (unit-testable; empty stream → all zeros, `None` ratios, no panic).
- **Metric definitions** (documented in code): `trusted_outcome_rate_proxy` = kept ÷ governed runs (labeled a proxy via `trusted_outcome_rate_is_proxy: true` + a note); `early_abort_catch_rate` = rejects ÷ early-aborts fired; `intervention_rate` = early-aborts fired ÷ all runs; `gate_precision` = aborts on write-intent runs ÷ all fired aborts (read-only runs that never fired are excluded from the denominator). Proxy classification: a governed run is *kept* if accepted+completed+not-reverted, *against* if rejected or accepted-then-interrupted/failed.
- **Dual-alarm** (`raises_alarm`): Warn when `intervention_rate > 0.5` (high band — gate fires on too many runs) OR when no aborts fired while reverts exist (near-zero with problems present — gate is missing them). Idle log (no aborts, no reverts) is `Ok` "no governance activity".
- "corrective re-prompt" is approximated by the abort/interrupt-after-accept signal (atelier has no first-class revert/re-prompt event — ADR-005); the figure is clearly labeled a proxy and the exact calibration metrics anchor decisions.
- Verified: `cargo test --lib doctor::tests` (16 passed: 8 new governance + 8 existing, incl. the `--doctor --json` integration test), `cargo fmt --check` clean, `cargo clippy --all-targets` clean (0 warnings). Full `cargo test --lib` = 912 passed / 12 failed; the 12 are exactly the pre-existing skill tests (proven on the clean task_01 commit). Zero failures attributable to this task.
