---
status: pending
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
- [ ] 7.1 Add `governance_metrics_check` and register it in `run_doctor`.
- [ ] 7.2 Derive the outcome proxy from `governance_decision_*` + run-outcome events.
- [ ] 7.3 Compute intervention rate (dual-alarm band), early-abort catch rate, gate precision.
- [ ] 7.4 Emit the figures in the check `context`, with the proxy clearly labeled.

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
  - [ ] A completed governed run with no corrective re-prompt counts as kept; an early-abort reject and an abort-after-accept each count against the proxy.
  - [ ] Intervention rate raises the dual-alarm when near-zero with reverts present, and when above the high band.
  - [ ] Early-abort catch rate = rejects ÷ early-aborts fired; gate precision excludes trivial/read-only runs.
  - [ ] An empty event log yields zeros/None without panicking.
- Integration tests:
  - [ ] `--doctor --json` after a session containing governance events includes the metrics check with the labeled proxy and the calibration figures in `context`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `--doctor --json` reports the outcome proxy (labeled) + exact calibration metrics, derived locally with no telemetry.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
