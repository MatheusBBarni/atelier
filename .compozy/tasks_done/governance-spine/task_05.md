---
status: completed
title: Early-abort gate in the drive loop with feature flag
type: backend
complexity: high
dependencies:
  - task_02
  - task_03
---

# Early-abort gate in the drive loop with feature flag

## Overview
Add the spine's net-new capability: on the first orchestrator turn of a non-trivial single-agent run, pause before any write to show the interpreted goal and let the user Accept or Reject. The gate lives in `drive_run_inner`, builds a `GovernanceDecisionView`, and pauses into the unified governance state — all behind a `features.governance_early_abort` flag.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `features.governance_early_abort` (default `false`) through `Features` / `RawFeatures` / the merge logic, mirroring `parallel_step_groups`.
- MUST gate the early-abort in `drive_run_inner` only when ALL hold: it is the first orchestrator turn (`step_count == 0`, no previous results), the run is not a subtask, the decision `status` is `Continue`, `next_step` is `SingleAgent`, and `required_capabilities` includes a write capability (`Edit` or `Command`).
- MUST build the `GovernanceDecisionView` from the decision's `reason` (intent), `plan` (approach), agent, and the run's workspace write-roots (write-scope) — no new `SingleAgentStepPlan` field.
- MUST pause into `pending_governance_decision`, record a `governance_decision_requested` event, and stop the loop until resolved — before any action runs.
- MUST be a no-op when the flag is off, when the run is read-only, or past the first turn.
- MUST add a `fake` runtime control phrase so the gate is drivable in deterministic tests.
</requirements>

## Subtasks
- [x] 5.1 Add the `governance_early_abort` flag through config + merge.
- [x] 5.2 Implement the `early_abort_triggers` predicate (first-turn ∧ single-agent ∧ write).
- [x] 5.3 Build the `GovernanceDecisionView` from the decision + workspace write-roots.
- [x] 5.4 Pause into `pending_governance_decision` + record the requested event, before any write.
- [x] 5.5 Add a `fake` runtime control phrase to drive the gate in tests.

## Implementation Details
Modify `src/app/mod.rs` (the gate in/around `drive_run_inner` ~1858, reading `RunDriveContext` + the first decision), `src/config/mod.rs` (`Features` ~197, `RawFeatures` ~449, merge ~939), and `src/runtime/fake.rs` (control phrase). Write capability = `required_capabilities` contains `Capability::Edit` or `Capability::Command` (no `is_write()` helper exists). Reference TechSpec "Core Interfaces" (the predicate) and ADR-004.

### Relevant Files
- `src/app/mod.rs` — `drive_run_inner` loop (~1858), `handle_orchestrator_decision` (~1873), `RunDriveContext` (~413), `pending_governance_decision` + resolver (task_03).
- `src/config/mod.rs` — `Features` (~197), `RawFeatures` (~449), feature merge (~939).
- `src/orchestrator/mod.rs` — `OrchestratorDecision` fields + `DecisionNextStep::SingleAgent`, `Capability` (`Edit`/`Command`).
- `src/runtime/fake.rs` — control-phrase mechanism (~622) for the E2E test.

### Dependent Files
- `src/orchestrator/mod.rs` (task_06) — prompt nudge improves the echo this gate shows.
- `src/doctor/mod.rs` (task_07) — consumes the `governance_decision_*` events this emits.

### Related ADRs
- [ADR-004: Single-agent turn-1 early-abort mechanism](../adrs/adr-004.md) — the gate, predicate, and echo sourcing.
- [ADR-002: V1 product shape — shared contract + early-abort, phased sibling migration](../adrs/adr-002.md) — complexity-gated, feature-flagged.

## Deliverables
- `features.governance_early_abort` flag (default off).
- The early-abort gate that pauses a first-turn single-agent write run before any write.
- A `fake` runtime control phrase for deterministic testing.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An end-to-end `FakeRuntime` integration test **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `early_abort_triggers` is true for a first-turn `SingleAgent` decision whose `required_capabilities` include `Edit` (also `Command`).
  - [x] It is false for a read-only decision (no write capability), false when past the first turn (`step_count > 1` and with previous results), false for a subtask, false for a `ParallelGroup` next step, and false for a non-`Continue` status.
  - [x] With the flag off, the predicate path is skipped entirely (covered by the flag-off integration test — the predicate is flag-independent by design; the caller gates on the flag).
- Integration tests:
  - [x] `FakeRuntime` first-turn single-agent write run (flag on) pauses into `pending_governance_decision` before any write event is recorded.
  - [x] After `resolve(Accept)` the write proceeds; after `resolve(Reject { redirect })` the orchestrator re-drives.
  - [x] A read-only first-turn run does not pause; with the flag off, no pause occurs.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A non-trivial single-agent run pauses for an intent check before any write, only when flagged on; trivial/read-only runs are untouched.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.

## Completion Notes
- **`step_count == 1`, not `0`.** The techspec/ADR-004 pseudocode uses `run.step_count == 0`, but `run_orchestrator_step` increments `step_count` to 1 *before* returning the decision, so at the gate hook (after the orchestrator step, before `handle_orchestrator_decision`) the first orchestrator turn is `step_count == 1`. Using `== 1` also means the gate never re-fires on the Accept / reject-redirect re-drive, because each re-drive runs another orchestrator step and the count climbs to 2+. (A turn-1 parse-retry would bump it to 2 and gracefully skip the gate — acceptable per ADR-004's "degrade gracefully; never block on echo quality".) `early_abort_triggers(run, decision)` is a flag-independent free function; the call site gates on `features.governance_early_abort`.
- **Write signal:** `Capability::Edit | Capability::Command` on the decision's top-level `required_capabilities` (no `is_write()` helper exists). Write-scope is rendered from the workspace write-roots already on the run (working directory + `extra_write_roots`) — no new `SingleAgentStepPlan`/`file_scope` field (ADR-004).
- **Pause mechanics:** `pause_for_early_abort` sets `WaitingForUser` + both pending fields, then records `governance_decision_requested` (whose payload is the serialized `GovernanceDecisionView`). `record_event` already applies the event to the chat projection and publishes state, so the task_02 decision card renders automatically. The gate `break`s the loop *before* `handle_orchestrator_decision`, so no agent/action runs and nothing is written before the user resolves.
- **Fake control phrase:** `"governance early abort"` → a first-turn single-agent (fixer) `Edit` decision. The E2E tests combine it with `"write action"` so Accept actually writes `multiagent-action-output.txt`.
- Verified: `cargo test --lib early_abort` (14 passed: 8 predicate + 5 E2E + helpers), `governance_early_abort` config test (1), full governance filter (32), `cargo fmt --check` clean, `cargo clippy --all-targets` clean (0 warnings). Full `cargo test --lib` = 894 passed / 19 failed; the 19 are 12 pre-existing skill tests (proven on the clean task_01 commit) + 7 `runtime::codex`/`cursor` CLI tests that pass in isolation and fail only under the parallel suite (flaky/env per CLAUDE.md, in untouched `src/runtime/`). Zero failures attributable to this task.
