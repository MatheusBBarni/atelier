---
status: pending
title: Pending governance decision state and resolver
type: backend
complexity: medium
dependencies:
  - task_01
---

# Pending governance decision state and resolver

## Overview
Add the unified governance-pause surface to `App`: a `pending_governance_decision` state and a `resolve_pending_governance_decision` handler that mirrors the clarification pause/resume. This is the shared state the early-abort uses now and the siblings migrate onto later.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add an internal `PendingGovernanceDecision { run }` and a public `PendingGovernanceDecisionView { run_id, decision_id, view }` (serializable), plus `pending_governance_decision` fields on both `App` and `AppState`.
- MUST add `resolve_pending_governance_decision(GovernanceAnswer)` mirroring `resolve_pending_clarification`: take the pending decision, record a `governance_decision_resolved` event, and resume.
- MUST handle the three outcomes: `Accept` resumes via `drive_and_replay`; `Reject { redirect: Some }` appends the redirect to the prompt and re-drives; `Reject { redirect: None }` aborts the run cleanly.
- MUST clear `pending_governance_decision` (both internal + state) on interrupt and on resolve.
- MUST error if resolve is called with no pending decision or a mismatched decision id.
- MUST initialize the new fields to `None` in all `App` constructors.
</requirements>

## Subtasks
- [ ] 3.1 Define the internal + public pending structs and add the App/AppState fields.
- [ ] 3.2 Initialize the fields to `None` in the constructors.
- [ ] 3.3 Implement `resolve_pending_governance_decision` (Accept / Reject+redirect / Reject abort).
- [ ] 3.4 Record the `governance_decision_resolved` event on resolve.
- [ ] 3.5 Clear the pending state in the interrupt handler.

## Implementation Details
Modify `src/app/mod.rs` only. Mirror `PendingClarification`/`PendingClarificationView`, the `resolve_pending_clarification` body, the constructor initializers, and the interrupt-handler clears. Reuse the `RunDriveContext` capture + `drive_and_replay` resume. Reference TechSpec "Core Interfaces" for the resolve signature; do not reproduce it.

### Relevant Files
- `src/app/mod.rs` — `AppState` field block (~124), `App` field block (~328), `PendingClarification` (~367), `PendingClarificationView` (~211), `resolve_pending_clarification` (~1689), interrupt handler (~1738), constructors (~857, ~876).
- `src/governance.rs` — `GovernanceDecisionView` / `GovernanceAnswer` (task_01).

### Dependent Files
- `src/tui/mod.rs` (task_04) — reads `pending_governance_decision` for routing/render.
- `src/app/mod.rs` drive loop (task_05) — pauses into this state.

### Related ADRs
- [ADR-003: Unified GovernanceDecision data model + single pending_governance_decision state](../adrs/adr-003.md) — one unified pending surface over the clarification transport.

## Deliverables
- `pending_governance_decision` state on `App` + `AppState` with a public view.
- `resolve_pending_governance_decision` handling all three outcomes.
- Interrupt + resolve clearing.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An integration test of the pause→resolve cycle **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `resolve` with `Accept` clears the pending state and resumes the run (drive continues).
  - [ ] `resolve` with `Reject { redirect: Some("focus on X") }` appends the redirect to the prompt and re-drives.
  - [ ] `resolve` with `Reject { redirect: None }` aborts: pending cleared, run not resumed.
  - [ ] `resolve` with no pending decision returns a descriptive error.
  - [ ] `interrupt` clears both the internal and `AppState` pending governance fields.
- Integration tests:
  - [ ] A run paused with a pending governance decision exposes it on `AppState.pending_governance_decision`; after `resolve(Accept)` the field is cleared and the run progresses.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The pause/resume cycle works for Accept, Reject-redirect, and Reject-abort; interrupt is clean.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
