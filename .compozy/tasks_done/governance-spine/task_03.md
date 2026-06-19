---
status: completed
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
- [x] 3.1 Define the internal + public pending structs and add the App/AppState fields.
- [x] 3.2 Initialize the fields to `None` in the constructors.
- [x] 3.3 Implement `resolve_pending_governance_decision` (Accept / Reject+redirect / Reject abort).
- [x] 3.4 Record the `governance_decision_resolved` event on resolve.
- [x] 3.5 Clear the pending state in the interrupt handler.

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
  - [x] `resolve` with `Accept` clears the pending state and resumes the run (drive continues).
  - [x] `resolve` with `Reject { redirect: Some("focus on X") }` appends the redirect to the prompt and re-drives.
  - [x] `resolve` with `Reject { redirect: None }` aborts: pending cleared, run not resumed.
  - [x] `resolve` with no pending decision returns a descriptive error.
  - [x] `resolve` with a mismatched decision id returns a descriptive error (extra, per the requirement).
  - [x] `interrupt` clears both the internal and `AppState` pending governance fields.
- Integration tests:
  - [x] A run paused with a pending governance decision exposes it on `AppState.pending_governance_decision`; after `resolve(Accept)` the field is cleared and the run progresses.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The pause/resume cycle works for Accept, Reject-redirect, and Reject-abort; interrupt is clean.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.

## Completion Notes
- **Resolver signature: `resolve_pending_governance_decision(&mut self, decision_id: &str, answer: GovernanceAnswer)`.** The requirement mandates an error on a *mismatched decision id*, but `GovernanceAnswer` (task_01, committed: `Accept | Reject { redirect }`) carries no id, and the techspec's illustrative signature is `(answer)` ("reference … do not reproduce"). Threading `decision_id` as a parameter is the only reconciliation that honors the MUST without modifying the shipped shared type. **task_04 (TUI) must pass the pending view's `decision_id` when calling resolve.**
- Reject-abort (`redirect: None`) concludes the run cleanly: records `governance_decision_resolved` + `run_interrupted` (reason `governance_decision_rejected`), writes the run record, clears the pending state + `active_run_id`, and sets `RunState::Interrupted`. Reject with a blank `Some` redirect re-drives without appending an empty line.
- Beyond the listed scope I also mirrored clarification parity in two spots: `can_replay_now()` now also requires no pending governance decision, and `queue_pause_reason()` reports "run is waiting for a governance decision". Both are defensive/UX parity with the clarification transport.
- `pending_governance_decision: None` was added to all `AppState` constructors: the one runtime constructor (`new_with_debug`) and the 8 `AppState { … }` test-helper literals in `src/tui/mod.rs`.
- The task-03 "integration" test lives in the in-crate `app::tests` module (FakeRuntime-driven, like the rest of the app/orchestrator suite) because the production pause path that sets `pending_governance_decision` is task_05; the full cross-crate early-abort flow belongs to task_05's integration tests.
- Verified: `cargo test --lib governance_decision` (15 passed, incl. 8 new app tests), `cargo test --test governance_chat` (1), `cargo test --test governance_types` (3), `cargo fmt --check` clean, `cargo clippy --all-targets` clean. Full `cargo test --lib` = 875 passed / 16 failed; the 16 are 12 pre-existing skill tests (proven on the clean task_01 commit) + 4 `runtime::codex`/`cursor` CLI tests that pass in isolation and fail only under the parallel suite (flaky/env-sensitive per CLAUDE.md). Zero failures attributable to this task.
