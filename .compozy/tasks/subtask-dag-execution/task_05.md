---
status: pending
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
- [ ] 5.1 Add the `pending_plan_approval` state field and its publish path (distinct from per-action approval).
- [ ] 5.2 Insert the gate in `run_execution_graph` before admission; branch on `approval_mode`.
- [ ] 5.3 Wire accept/reject resolution through the clarification/approval channel with `question_id` from `graph_id`.
- [ ] 5.4 Implement reject-with-reason → orchestrator re-propose; emit approved/rejected events.
- [ ] 5.5 Add unit + integration tests for accept, reject-re-propose, and yolo auto-accept.

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
  - [ ] In `normal` mode, no node is spawned until an accept signal is received.
  - [ ] In `yolo` mode, the plan is auto-accepted and admission begins with no gate.
  - [ ] A reject with a reason emits `execution_graph_rejected` carrying the reason and does not spawn nodes.
  - [ ] The plan gate uses `pending_plan_approval`, leaving `state.pending_approval` (per-action) untouched (no collision).
  - [ ] The clarification `question_id` is derived deterministically from `graph_id` (accept event re-keys identically).
- Integration tests:
  - [ ] (with task_08 harness) normal-mode accept runs the graph; reject-with-reason triggers an orchestrator re-proposal; yolo runs without a prompt.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A proposed plan blocks all execution in normal mode until accepted; yolo runs immediately.
- Reject-with-reason routes back to the orchestrator; the per-action approval path is unaffected.
