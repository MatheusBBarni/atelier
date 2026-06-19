---
status: completed
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
- [x] 12.1 Add a per-session approval-mode override and apply it during resumed-session action decisions.
- [x] 12.2 Track a pending drift acknowledgment on the resumed session (set on resume, cleared after first ack).
- [x] 12.3 Fold the drift context into the first mutating action's approval prompt and require acknowledgment.
- [x] 12.4 Record the acknowledgment in the audit log.
- [x] 12.5 Add unit/integration tests for the cautious default, the interlock, and the no-gate cases.

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
  - [x] A resumed session evaluates an action under `Normal` mode even when global config is `Yolo` (approval is required). — `resumed_session_forces_normal_approval_mode_over_yolo_config` + `actions::drift_ack_gate_forces_approval_on_mutating_kinds_even_when_otherwise_allowed`
  - [x] With drift present, the first `WriteFile`/`ApplyPatch`/`RunCommand` requires acknowledgment; once acknowledged, a second mutating action is not re-gated for drift. — `acknowledging_first_mutation_records_audit_and_clears_gate`
  - [x] With NO drift, resume does not add a drift acknowledgment gate (only the normal cautious approval applies). — `resume_without_drift_has_no_interlock`
  - [x] A read-only action (e.g. `ReadFile`) is never gated by drift; a dirty-but-same-HEAD/cwd resume does not trigger the interlock. — `drift_notice_is_folded_into_mutating_approval_prompts_only`, `acknowledging_first_mutation_*` (read-only branch), `resume_dirty_tree_but_same_head_is_not_drift`
  - [x] The acknowledgment is recorded in the audit log. — `acknowledging_first_mutation_records_audit_and_clears_gate` (`resume_drift_acknowledged` event)
  - [x] Drift arming on HEAD change. — `resume_with_head_drift_arms_the_interlock`
- Integration tests:
  - [x] FakeRuntime E2E: resume a session whose HEAD changed externally, submit a prompt that triggers a write → the first write prompts with drift context and proceeds only after acknowledgment. — `resume_with_drift_requires_acknowledgment_before_first_write`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A resumed session cannot silently write to a moved tree; cautious mode + the conditional interlock protect the first mutation without taxing the clean common path.

## As-built notes
- **Cautious default:** `App.resume_approval_mode: Option<ApprovalMode>` (set `Some(Normal)` by `resume_session` after adopt). `effective_approval_mode()` = override ?? `config.approval_mode`, consulted at **both** `ActionExecutionContext` construction sites (serial + parallel) and in the `session_resumed.approval_mode` payload. Prior approvals don't carry over because `adopt_session` clears the trust store.
- **Drift interlock:** `App.pending_drift_ack: Option<git::WorkspaceDrift>`, armed by `resume_session` when `detect_drift(stored_cwd, stored_head, live_cwd, live_head).any()` (stored cwd from `metadata.working_directory`, stored HEAD = new `LoadedSession.stored_head_sha` via `last_recorded_head_sha`; live from the just-refreshed `GitContext`). `ActionExecutionContext` gained `drift_ack: Option<String>` (the message); the enforcement matrix (`apply_floor_and_trust`) forces `RequiresApproval` for the **first** `is_mutating_kind` action while armed — *after* `pre_approved`/catastrophic, *before* trust, so even an otherwise-Low command prompts and trust can't bypass it. Read-only kinds are never gated.
- **Acknowledgment:** approving the first mutating action calls `acknowledge_resume_drift` (in both serial + parallel approve paths), which records a `resume_drift_acknowledged` audit event (new additive kind, ADR-008) and `take()`s the gate so later mutations run under plain cautious approval. The approval modal renders the `drift_notice` (`PendingApprovalView.drift_notice`, set in `build_pending_approval_view` for mutating kinds) as a bold warning line.
- **dirty ≠ drift:** `detect_drift` takes no dirty input, so a dirty-same-HEAD resume cannot arm the gate (verified by `resume_dirty_tree_but_same_head_is_not_drift`).
- The `adopt_session` exhaustiveness test was extended to sentinel + assert reset of the two new `App` fields (the task_10/11 follow-up). No new follow-ups; the resume safety model (ADR-004) is complete except the named at-rest redaction risk (explicitly deferred to first shared-host deployment).
