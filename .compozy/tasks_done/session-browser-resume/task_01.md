---
status: completed
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
- [x] 1.1 Add the `is_terminal()` method on `RunState`.
- [x] 1.2 Find and replace the inline `matches!` terminal checks that express run-finished semantics. — **Find done; no replacements applicable.** An exhaustive scan (every `matches!` in `src/`) found **zero** `matches!(.., Completed | Failed | Interrupted | LimitReached)` (the exact terminal set). The nearby run-state checks are deliberately *narrower* and do **not** express "is finished": `can_replay_now` matches `Completed` only (clean-completion semantics), and the queue-pause check matches `Failed | Interrupted | LimitReached | WaitingForUser` (ended-badly-or-waiting — includes a non-terminal state, excludes `Completed`). Substituting `is_terminal()` into either would change behavior, which this task forbids. See As-built notes.
- [x] 1.3 Add a unit test covering the truth table for all eight variants. — `run_state_is_terminal_truth_table`

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
  - [x] `is_terminal()` returns true for `Completed`, `Failed`, `Interrupted`, `LimitReached`. — `run_state_is_terminal_truth_table`
  - [x] `is_terminal()` returns false for `Idle`, `Planning`, `Running`, `WaitingForUser`. — same test (all 8 variants)
- Integration tests:
  - [x] Existing run-lifecycle tests (FakeRuntime completed/failed/interrupted runs) pass after the call-site refactor. — orchestrator suite 34/0; the app FakeRuntime completed/failed/limit lifecycle tests pass unchanged (no behavior change — pure additive method).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `is_terminal()` is the single expression of run-finished semantics at the refactored call sites.
- No change to which states are terminal or to run-state transitions.

## As-built notes
- Added `RunState::is_terminal()` (pure, `&self`) returning true for exactly `Completed | Failed | Interrupted | LimitReached`. No behavior change of any kind.
- **No call sites refactored — by design, verified.** A scripted scan of every `matches!` in `src/` found no expression containing all four terminal states; ADR-008's premise of "scattered `matches!(.., Completed|Failed|Interrupted|LimitReached)`" does not match the current code. The two closest checks are intentionally narrower (`can_replay_now` → `Completed` only; `react_to_run_end_for_queue` → `Failed|Interrupted|LimitReached|WaitingForUser`), and swapping `is_terminal()` into either would alter the matched state set — a behavior change the task explicitly prohibits, and (for the queue site) would also reduce readability. The helper is therefore added for its stated downstream consumers — task_03 (session-outcome derivation) and task_10 (dangling-run detection on open), per ADR-008 §4 — which is where the first true "is this run finished" call sites appear.
