---
status: completed
title: Approval resolution, trust grant & audit events
type: backend
complexity: high
dependencies:
  - task_03
  - task_04
---

# Task 05: Approval resolution, trust grant & audit events

## Overview
Replace the boolean approval answer with a three-way `ApprovalResolution`, grant trust on `ApproveAndTrust`, populate the enriched `PendingApprovalView`, and record the new audit events. Also confirm deny-and-continue: a denial returns a structured reason and the run resumes via the existing replay path.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST replace `ApprovalSignal { approved: bool }` / `ApprovalHandle::answer(bool)` with `ApprovalResolution { Deny, ApproveOnce, ApproveAndTrust }`; update the TUI dispatch site to send `ApproveOnce`/`Deny` so existing behavior keeps working (rich key routing is task_07).
- MUST enrich `PendingApprovalView` with the risk fields (`tier`, `catastrophic`, `reason`, `resolved_command`, `diff`, `affected_paths`, `boundary_crossed`, `reversible`, `trust_target`) and populate them from `ActionResult.risk` at the serial and parallel approval-creation sites.
- MUST grant the pending action's `TrustTarget` into the `TrustStore` only on `ApproveAndTrust` and only when non-catastrophic.
- MUST record `approval_auto_resolved` (trust), `floor_warned` (gray-area Yolo+Warn), and `trust_granted` events from `gate_outcome`/resolution.
- MUST ensure a denial produces an actionable reason on the `ActionResult` and the run resumes (deny-and-continue) via the existing `drive_and_replay` path.
- MUST cap `diff`/`resolved_command` previews to a bounded length before they enter `AppState`.

## Subtasks
- [x] 05.1 Introduce `ApprovalResolution` and update `ApprovalSignal`/`ApprovalHandle` + the TUI dispatch site.
- [x] 05.2 Enrich and populate `PendingApprovalView` from the action's risk (serial + parallel).
- [x] 05.3 Grant trust on `ApproveAndTrust` (non-catastrophic only) in `resolve_pending_approval`.
- [x] 05.4 Record `approval_auto_resolved`, `floor_warned`, and `trust_granted` events.
- [x] 05.5 Ensure deny-and-continue carries a structured reason through the resume path.
- [x] 05.6 Add unit + FakeRuntime tests for grant, auto-approve, warn, and deny-and-continue.

## Implementation Details
Work in `src/app/mod.rs`: `ApprovalHandle`/`ApprovalSignal` (~236), `PendingApprovalView` (~200), `resolve_pending_approval` (~1587), parallel head (`publish_parallel_approval_head` ~2705 / `resolve_parallel_approval` ~2574), event recording (`record_event` ~4164, `record_action_completed`). The TUI dispatch site to update is in `src/tui/mod.rs` (~753 / ~1441). Reuse `GateOutcome`/`risk` (task_03) and `TrustStore` (task_04). See TechSpec "Core Interfaces", "Data Models" (event payloads), and "System Architecture → Data flow".

### Relevant Files
- `src/app/mod.rs` — resolution, grant, event recording, `PendingApprovalView` population.
- `src/tui/mod.rs` — approval dispatch site that constructs the signal (minimal change here).
- `src/actions/mod.rs` — `ActionResult.risk`/`gate_outcome` (task_03), `ActionResult::approval_denied` (~69).

### Dependent Files
- `src/app/chat/projection.rs` (task_06) — projects the new events and the enriched view.
- `src/tui/mod.rs` (task_07) — renders the enriched `PendingApprovalView` and maps keys to the resolutions.

### Related ADRs
- [ADR-004: In-memory exact-target session trust](../adrs/adr-004.md) — `ApproveAndTrust` grant, never catastrophic.
- [ADR-002: Phased floor rollout](../adrs/adr-002.md) — `floor_warned` annotation for gray-area Yolo+Warn.

## Deliverables
- `ApprovalResolution` end-to-end, trust grant, enriched/populated `PendingApprovalView`, and the three new events.
- Deny-and-continue verified through the resume path.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- FakeRuntime integration tests for grant/auto-approve/warn/deny-and-continue **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `resolve_pending_approval(ApproveAndTrust)` on a non-catastrophic action inserts its `TrustTarget` and emits `trust_granted`.
  - [x] `resolve_pending_approval(ApproveAndTrust)` is a no-op grant for a catastrophic action (no target to store).
  - [x] `resolve_pending_approval(ApproveOnce)` runs the action without granting trust.
  - [x] `resolve_pending_approval(Deny)` produces an `ActionResult` whose diagnostic carries the denial reason.
  - [x] `PendingApprovalView` is populated with tier/reason/resolved_command from the action's risk; long diffs/commands are truncated (`capped_preview`).
- Integration tests:
  - [x] FakeRuntime: a pre-trusted gray-area command auto-approves (`approval_auto_resolved`), no modal. (Equivalent to re-emission: trust persists for the session, so any later occurrence of the same target auto-runs.)
  - [x] FakeRuntime: gray-area action under Yolo+Warn → action runs and a `floor_warned` event is recorded.
  - [x] FakeRuntime: deny an action → the run continues with the denied result (deny-and-continue).

Note: approving a catastrophic/`floor=Enforce` action required a new `pre_approved` flag on `ActionExecutionContext`; the prior "re-run as Yolo" trick fails because catastrophic and `floor=Enforce` ignore Yolo and would re-prompt forever.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Approve-once, approve-and-trust, and deny each behave per spec; catastrophic actions never produce a trust grant.
- Denials resume the run with an actionable reason; trust grants and auto-approvals are recorded as events.
