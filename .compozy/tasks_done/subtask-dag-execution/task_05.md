---
status: completed
title: "Whole-plan approval gate (normal mode)"
type: backend
complexity: high
dependencies:
  - task_03
  - task_04
---

# Task 05: Whole-plan approval gate (normal mode)

## Overview
Add a single, binary accept/reject gate that pauses a proposed DAG before any node runs, shown only in `normal` mode and resolved through the existing clarification answer channel; `yolo` auto-accepts. A rejection carries an optional free-text reason that returns the plan to the orchestrator to re-propose, replacing the per-action approval drip with one informed plan-level consent.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST insert the gate AFTER plan validation/preparation and BEFORE any node is admitted/spawned in `run_execution_graph`.
- MUST show the gate only when `approval_mode == Normal`; under `Yolo` the plan is auto-accepted and runs immediately.
- MUST resolve accept/reject through the existing binary approval/clarification channel (`ApprovalSignal { approved }` / clarification answer with a stable `question_id` derived from `graph_id`), with accept as the recommended default.
- MUST support reject-with-reason: a rejection ends the current graph and returns the reason to the orchestrator so it can re-propose (the orchestrator re-plan loop), not a hard run failure.
- MUST use a DISTINCT state field (e.g. `pending_plan_approval`) — MUST NOT reuse the per-action `state.pending_approval` slot (collision with mid-run per-action approvals and the first-approval explainer latch).
- MUST emit `execution_graph_approved` / `execution_graph_rejected` events (task_03).
- Fail-closed node behavior (task_04) MUST hold regardless of approval mode.
</requirements>

## Subtasks
- [x] 5.1 Add the `pending_plan_approval` state field (`AppState` view + internal `App` holder) and its publish path, distinct from per-action `pending_approval`.
- [x] 5.2 Gate before admission, branching on `approval_mode`. **Design note:** implemented as return-and-resume via the clarification channel in `handle_execution_graph_decision` (before `run_execution_graph`), mirroring the governance-decision pattern — required to satisfy ADR-005 (clarification channel + free-text reject reason) and tests 5.67/5.69, which the binary approval channel cannot. The gate is after validation (upstream) and before any node is admitted; preparation is deferred to accept so rejected plans waste no work.
- [x] 5.3 Accept/reject resolution via the clarification answer channel (`AppEvent::PlanApprovalResolved` → `resolve_pending_plan_approval`); `question_id` derived deterministically from `graph_id` via `plan_question_id`.
- [x] 5.4 Reject-with-reason appends the reason to the prompt and re-drives the orchestrator (re-propose, not a hard failure); emits `execution_graph_proposed`/`approved`/`rejected` events.
- [x] 5.5 Unit/integration tests for normal-pause, accept, reject-with-reason, yolo auto-accept, distinct-field, and question_id determinism.

> The TUI wiring that sends `AppEvent::PlanApprovalResolved` and renders the Plan item's WaitingApproval/accept-reject affordance is task_06 (projection + TUI); this task owns the app-side decision flow + events.

## Implementation Details
Changes are in `src/app/mod.rs`. Reuse `ApprovalHandle::answer`/`wait_for_approval` (already a binary accept/reject primitive) or the clarification-answer path; mint a `question_id` from `graph_id`. The gate must not spawn any node before resolution. See TechSpec "Component Overview → Plan approval" and ADR-005 (approval on the Plan item via the clarification channel). The Plan item's WaitingApproval rendering is implemented in task_06; this task owns the decision flow and events.

### Relevant Files
- `src/app/mod.rs` — `run_execution_graph` (task_04), `ApprovalHandle::answer` (`:240`), `wait_for_approval` (`:5780`), `ApprovalSignal` (`:789`), `PendingApprovalView`/per-action slot (`:199`, `:2705`), `PendingClarificationView`/`ClarificationAnswer` (`:211`, `:251`), `approval_mode` read (`:2480`).
- `src/orchestrator/mod.rs` — the re-propose path and `ClarificationOption` (`:77`).

### Dependent Files
- `src/app/chat/projection.rs` (task_06) — renders the Plan item's WaitingApproval state and accept/reject prompt.
- `src/app/mod.rs` (scheduler, task_04) — admission begins only after accept.

### Related ADRs
- [ADR-003: V1 product shape](../adrs/adr-003.md) — binary accept/reject, normal-only gate, reject-reason.
- [ADR-005: DAG user-surface integration](../adrs/adr-005.md) — approval on the Plan item via the clarification channel; distinct from the per-action slot.

## Deliverables
- `pending_plan_approval` gate wired before scheduling, normal-mode only, yolo auto-accept.
- Reject-with-reason → orchestrator re-propose loop.
- `execution_graph_approved`/`execution_graph_rejected` events.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the accept/reject/yolo flows **(REQUIRED)**

## Tests
- Unit tests:
  - [x] In `normal` mode, no node is spawned until accept. (`dag_normal_mode_pauses_for_plan_approval_without_spawning`)
  - [x] In `yolo` mode, the plan is auto-accepted and runs with no gate. (`dag_yolo_mode_auto_accepts_and_runs`)
  - [x] A reject with a reason emits `execution_graph_rejected` carrying the reason and spawns no nodes. (`dag_plan_reject_emits_rejected_with_reason_and_spawns_nothing`)
  - [x] The plan gate uses `pending_plan_approval`, leaving `state.pending_approval` (per-action) untouched. (`dag_normal_mode_pauses_for_plan_approval_without_spawning`)
  - [x] The `question_id` is derived deterministically from `graph_id`. (`plan_question_id_is_deterministic_from_graph_id` + `dag_normal_mode_pauses...`)
- Integration tests:
  - [x] (task_05) normal-mode accept runs the graph end-to-end via the fake runtime (`dag_plan_accept_runs_the_graph`); reject re-drives the orchestrator; yolo runs without a prompt (`dag_yolo_mode_auto_accepts_and_runs`). task_08 adds the full fake-runtime DAG-emission harness.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A proposed plan blocks all execution in normal mode until accepted; yolo runs immediately.
- Reject-with-reason routes back to the orchestrator; the per-action approval path is unaffected.
