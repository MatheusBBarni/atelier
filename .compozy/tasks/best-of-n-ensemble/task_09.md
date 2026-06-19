---
status: pending
title: "Winner promotion via diff-replay"
type: backend
complexity: high
dependencies:
  - task_03
  - task_07
---

# Task 09: Winner promotion via diff-replay

## Overview
Land the winning attempt on the real tree safely: compute its scratch-vs-real diff, re-derive a write scope from the actual changed files, and replay it as a fresh `ApplyPatch` through the existing approval gate — fail-closed. This is the medium-risk seam where an isolated patch becomes a real edit with intact provenance.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST compute the winning attempt's changed-file set from its `scratch_dir` (including tombstoned deletes/renames) and build a unified diff against the real tree.
- MUST re-derive a `ParallelFileScope` from the actual write-set and replay the diff as a fresh `ApplyPatch` through `validate_action_request_with_scope` + the approval gate.
- Promotion MUST be union-bounded and fail-closed: any change outside the re-derived scope is rejected, never auto-widened.
- MUST re-validate against the current real tree at promotion time and surface a clear failure if a hunk no longer applies (drift).
- MUST emit a `race_promoted` event and set `RaceResult.status`/`changed_files` accordingly.
</requirements>

## Subtasks
- [ ] 9.1 Build the winner's changed-file set + unified diff from scratch.
- [ ] 9.2 Re-derive the `ParallelFileScope` from the actual write-set.
- [ ] 9.3 Replay as an `ApplyPatch` through the approval gate, union-bounded fail-closed.
- [ ] 9.4 Handle hunk-apply drift with a clear surfaced failure.
- [ ] 9.5 Add tests for promotion, scope re-derivation, escape rejection, and drift.

## Implementation Details
Reuse the existing patcher and fence (see TechSpec "Integration Points" and ADR-006): the promoted patch is "just another `ApplyPatch`". The approval modal is the standard one — no custom race approval UI. Tombstones from Task 03 must be reflected in the diff so deletes promote correctly.

### Relevant Files
- `src/actions/mod.rs:1950` — `apply_unified_diff` (the patcher to replay through).
- `src/actions/mod.rs:272`,`455` — `validate_action_request_with_scope` / `validate_action_scope` (the gate).
- `src/actions/mod.rs` (Task 03) — scratch write-set + tombstones as the diff source.
- `src/app/mod.rs` (Task 07) — `RaceResult` updated with promotion outcome; approval flow.

### Dependent Files
- Task 13 — the all-fail path bypasses promotion; depends on this task's contract.
- Task 10 — the verdict card reflects promotion status.

### Related ADRs
- [ADR-006: Writes-Redirect + Diff-Replay Isolation and Promotion](../adrs/adr-006.md) — the promotion mechanism.
- [ADR-001: Fail-closed promotion](../adrs/adr-001.md) — re-derive scope, fail-closed.

## Deliverables
- Diff-replay promotion of the winner through the existing approval gate.
- `race_promoted` event + `RaceResult` status/changed_files.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for end-to-end promotion through approval **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] The re-derived `ParallelFileScope` equals the winner's actual changed-file set.
  - [ ] A synthesized change touching a file outside the write-set is rejected (fail-closed).
  - [ ] A tombstoned deletion promotes as a real file removal.
  - [ ] A hunk that no longer applies to the real tree surfaces a clear drift failure (no partial write).
- Integration tests:
  - [ ] In `normal` mode, the winning patch surfaces the standard approval modal; on approve, the real tree reflects the change; on deny, the tree is unchanged.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The winner promotes through the existing fence + approval gate with zero scope escapes.
- Drift is surfaced, never silently partially applied.
