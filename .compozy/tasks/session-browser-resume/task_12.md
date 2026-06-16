---
status: pending
title: "Resume safety - cautious-default approval + first-mutation drift interlock"
type: backend
complexity: high
dependencies:
  - task_05
  - task_11
---

# Task 12: Resume safety - cautious-default approval + first-mutation drift interlock

## Overview
Make resume safe against a moved workspace and forgotten permissions: a resumed session defaults to the cautious (`Normal`) approval mode regardless of global config and never replays prior approvals, and — when drift is present (cwd moved or HEAD changed) — the first state-mutating action requires an explicit, recorded acknowledgment folded into its approval prompt. Read-only browsing/preview is never gated.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST default a resumed session to `ApprovalMode::Normal` via a per-session override (NOT the global config default of `Yolo`), so an approval fires before any write/command; prior point-in-time approvals MUST NOT carry over as standing grants.
- MUST, when `WorkspaceDrift::any()` is true at resume, require positive acknowledgment on the FIRST action whose decision modifies the workspace (`WriteFile`/`ApplyPatch`/`RunCommand`), folding the drift context into that action's approval prompt; subsequent actions are not re-gated for drift.
- MUST record the acknowledgment in the `session_resumed` event (or a linked event) so it is auditable.
- MUST NOT gate read-only operations (browse, preview, reads), and MUST NOT treat a dirty working tree as drift.
</requirements>

## Subtasks
- [ ] 12.1 Add a per-session approval-mode override and apply it during resumed-session action decisions.
- [ ] 12.2 Track a pending drift acknowledgment on the resumed session (set on resume, cleared after first ack).
- [ ] 12.3 Fold the drift context into the first mutating action's approval prompt and require acknowledgment.
- [ ] 12.4 Record the acknowledgment in the audit log.
- [ ] 12.5 Add unit/integration tests for the cautious default, the interlock, and the no-gate cases.

## Implementation Details
Add a per-session `resume_approval_mode: Option<ApprovalMode>` and `pending_drift_ack` on `App` (`src/app/mod.rs:316`); consult them where the approval mode is read for action decisions (`ActionExecutionContext` construction, ~`:2480`/`:3991`) instead of always using `self.config.approval_mode`. The drift comes from task_05's `detect_drift`; the approval prompt surface is `decision_for_command` (`src/actions/mod.rs:356`) + `PendingApproval`/`resolve_pending_approval` (`src/app/mod.rs:1587`). `ApprovalMode` is at `src/config/mod.rs:20` (default `Yolo`). See TechSpec "Data Models" (per-session safety state) and ADR-004/007.

### Relevant Files
- `src/app/mod.rs` — `App` (`:316`), approval-mode usage (`:2480`/`:3991`), `resolve_pending_approval` (`:1587`), resume flow (task_11).
- `src/actions/mod.rs` — `ActionDecision` (`:91`), `decision_for_command` (`:356`), `execute_action_request` (`:278`).
- `src/config/mod.rs` — `ApprovalMode` (`:20`).
- `src/app/git.rs` — `detect_drift`/`WorkspaceDrift` (task_05).

### Dependent Files
- `src/tui/mod.rs` — the approval prompt rendering must surface the drift context message.

### Related ADRs
- [ADR-004: Resume safety model](adrs/adr-004.md) — cautious default, no replayed approvals, drift interlock at first mutation, never gate reads, dirty ≠ drift.
- [ADR-007: Drift detection model](adrs/adr-007.md) — the drift signal driving the interlock.

## Deliverables
- Per-session cautious approval default + no-replay-of-prior-approvals on resume.
- First-mutation drift interlock with recorded acknowledgment.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: resume-with-drift requires acknowledgment before the first write **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A resumed session evaluates an action under `Normal` mode even when global config is `Yolo` (approval is required).
  - [ ] With drift present, the first `WriteFile`/`ApplyPatch`/`RunCommand` requires acknowledgment; once acknowledged, a second mutating action is not re-gated for drift.
  - [ ] With NO drift, resume does not add a drift acknowledgment gate (only the normal cautious approval applies).
  - [ ] A read-only action (e.g. `ReadFile`) is never gated by drift; a dirty-but-same-HEAD/cwd resume does not trigger the interlock.
  - [ ] The acknowledgment is recorded in the audit log.
- Integration tests:
  - [ ] FakeRuntime E2E: resume a session whose HEAD changed externally, submit a prompt that triggers a write → the first write prompts with drift context and proceeds only after acknowledgment.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A resumed session cannot silently write to a moved tree; cautious mode + the conditional interlock protect the first mutation without taxing the clean common path.
