---
status: pending
title: "Add RunState::is_terminal() helper"
type: refactor
complexity: low
dependencies: []
---

# Task 01: Add RunState::is_terminal() helper

## Overview
Add a `RunState::is_terminal()` helper and replace the inline `matches!(.., Completed | Failed | Interrupted | LimitReached)` checks scattered through the app. This is the small enabling refactor that downstream session-outcome derivation (task_03) and dangling-run detection on resume (task_10) build on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `RunState::is_terminal()` returning true for exactly `Completed`, `Failed`, `Interrupted`, and `LimitReached`, and false for `Idle`, `Planning`, `Running`, `WaitingForUser`.
- MUST replace existing inline terminal-state `matches!` duplications with the helper where they express "is this run finished" (e.g. queue replay / run-end handling).
- MUST NOT change any run-state transition behavior or the set of states considered terminal.
</requirements>

## Subtasks
- [ ] 1.1 Add the `is_terminal()` method on `RunState`.
- [ ] 1.2 Find and replace the inline `matches!` terminal checks that express run-finished semantics.
- [ ] 1.3 Add a unit test covering the truth table for all eight variants.

## Implementation Details
Add the method to the `RunState` enum in `src/orchestrator/mod.rs` (enum at `:14`). See TechSpec "Core Interfaces" for the helper shape. Replace duplicated checks in `src/app/mod.rs` (e.g. around the queue `can_replay_now` / run-end paths). Do not introduce a new file.

### Relevant Files
- `src/orchestrator/mod.rs` — defines `RunState` (`:14`); add the method here.
- `src/app/mod.rs` — has the inline `matches!` terminal checks to replace (run-end / queue logic).

### Dependent Files
- `src/app/mod.rs` — task_03 (outcome derivation) and task_10 (dangling-run detection) will call `is_terminal()`.

### Related ADRs
- [ADR-008: Lifecycle events as additive string-kinds + self-healing metadata cache](adrs/adr-008.md) — names `is_terminal()` as a supporting helper.

## Deliverables
- `RunState::is_terminal()` method.
- Refactored call sites using the helper (no behavior change).
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration coverage: existing orchestrator/app run-lifecycle tests still pass unchanged **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `is_terminal()` returns true for `Completed`, `Failed`, `Interrupted`, `LimitReached`.
  - [ ] `is_terminal()` returns false for `Idle`, `Planning`, `Running`, `WaitingForUser`.
- Integration tests:
  - [ ] Existing run-lifecycle tests (FakeRuntime completed/failed/interrupted runs) pass after the call-site refactor.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `is_terminal()` is the single expression of run-finished semantics at the refactored call sites.
- No change to which states are terminal or to run-state transitions.
