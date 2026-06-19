---
status: pending
title: "Core race types and RunStepResult Race variant"
type: backend
complexity: medium
dependencies: []
---

# Task 02: Core race types and RunStepResult Race variant

## Overview
Define the shared data types the rest of the feature depends on: `RaceResult`, `RaceAttempt`, `RaceStatus`, `TaskSignature`/`ChangeKind`, and the new `ActionScope::AttemptScope` variant — plus the `RunStepResult::Race` variant and the two exhaustive-match fixups it forces. Extracting the types here keeps tasks 03/06/10 free of dependency cycles.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `RunStepResult::Race { result: RaceResult }` and update every exhaustive match over `RunStepResult`.
- MUST define `RaceResult`, `RaceAttempt`, `RaceStatus`, `TaskSignature`, `ChangeKind` per TechSpec "Core Interfaces"/"Data Models" with serde derives and a stable `schema_version` where the type is persisted.
- MUST add `ActionScope::AttemptScope(AttemptScope)` with `attempt_id`, `scratch_dir`, `read_roots` (behavior lands in Task 03; this task adds the variant + any non-overlay match arms so the crate compiles).
- Types MUST round-trip through serde (the run result is persisted/replayed).
- This task MUST NOT implement overlay resolution, the runner, or projection — only the type surface and match fixups they share.
</requirements>

## Subtasks
- [ ] 2.1 Add `RaceResult`/`RaceAttempt`/`RaceStatus` to the orchestrator result types.
- [ ] 2.2 Add `TaskSignature` + `ChangeKind` with deterministic-derivation seam (derivation impl in Task 04).
- [ ] 2.3 Add `ActionScope::AttemptScope` variant and a placeholder/deny arm in `validate_action_scope` (real overlay behavior in Task 03).
- [ ] 2.4 Add `RunStepResult::Race` and fix the exhaustive match sites.
- [ ] 2.5 Add serde round-trip tests for the new persisted types.

## Implementation Details
See TechSpec "Core Interfaces" for the exact struct/enum shapes. Keep `RaceResult` aligned with the sibling `ExecutionGraphResult`/`ParallelGroupResult` style (status enum + summary + per-unit refs + changed_files). The two `RunStepResult` match sites are the compile gate for this task.

### Relevant Files
- `src/orchestrator/mod.rs:227` — `RunStepResult` enum; add `Race`.
- `src/orchestrator/mod.rs:294` — `ExecutionGraphResult` as the result-shape template.
- `src/orchestrator/mod.rs:507` — `agent_results()` exhaustive match; add the `Race` arm.
- `src/runtime/fake.rs:233` — `fake_decision()` last-agent match; add the `Race` arm.
- `src/actions/mod.rs:229` — `ActionScope` enum; add `AttemptScope`.

### Dependent Files
- `src/actions/mod.rs:455` — `validate_action_scope` gains an `AttemptScope` arm (full behavior in Task 03).
- Tasks 03, 06, 07, 09, 10 — consume these types.

### Related ADRs
- [ADR-005: Dedicated run_race_workflow Runner](../adrs/adr-005.md) — `RunStepResult::Race`.
- [ADR-006: Writes-Redirect + Diff-Replay](../adrs/adr-006.md) — `AttemptScope`.
- [ADR-007: Verdict Ledger + Task-Type Signature](../adrs/adr-007.md) — `TaskSignature` shape.

## Deliverables
- New race result/attempt/status types, `TaskSignature`/`ChangeKind`, and `ActionScope::AttemptScope`.
- `RunStepResult::Race` with both match sites updated; crate compiles.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for serde round-trip of persisted types **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `RaceResult` with two attempts serializes and deserializes to an equal value.
  - [ ] `RunStepResult::Race` round-trips through the `kind`-tagged serde representation.
  - [ ] `ActionScope::AttemptScope` constructs and pattern-matches without affecting `ParallelFileScope` arms.
  - [ ] `TaskSignature` with `sig_version` round-trips and is `Eq`/hashable for bucketing.
- Integration tests:
  - [ ] A run whose `previous_results` contains a `Race` variant replays from history without a deserialization error.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The crate compiles with the new variant; both exhaustive match sites handle `Race`.
- Persisted types round-trip through serde.
