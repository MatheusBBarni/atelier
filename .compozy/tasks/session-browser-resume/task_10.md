---
status: pending
title: "LoadedSession + App::adopt_session() + exhaustiveness test"
type: backend
complexity: high
dependencies:
  - task_01
  - task_02
  - task_04
---

# Task 10: LoadedSession + App::adopt_session() + exhaustiveness test

## Overview
Introduce the single, audited swap point that lets a running `App` adopt a different session: a `LoadedSession` value holding every session-scoped field (opened store, folded projection, goal, reconciled run state, …) and `App::adopt_session(loaded)` that reassigns them all at once and broadcasts. A test enforces that no session-scoped field is forgotten. This is the highest-risk structural change and the core of resume.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define `LoadedSession` carrying every session-scoped field needed to fully replace the current session (history via `open()`, projection via `rebuild`, session goal, reconciled run state, and the pending-modal/queue/active-step state reset).
- MUST implement `App::adopt_session(loaded)` as the SINGLE place that reassigns all session-scoped fields and triggers exactly one state broadcast — no piecemeal field mutation elsewhere.
- MUST reconcile a dangling (non-terminal) run during adoption: emit a terminal `run_interrupted` event and land in `Idle` (using `RunState::is_terminal()` to detect dangling).
- MUST add an exhaustiveness test that fails if a session-scoped field is added to `App` without being reset by `adopt_session`.
</requirements>

## Subtasks
- [ ] 10.1 Define `LoadedSession` enumerating all session-scoped fields.
- [ ] 10.2 Implement `adopt_session` (single reassignment + one broadcast).
- [ ] 10.3 Reconcile dangling run → `run_interrupted` event → `Idle`.
- [ ] 10.4 Add the exhaustiveness test (sentinel-then-adopt-then-assert-all-replaced).
- [ ] 10.5 Add unit tests for projection/goal/run-state replacement and modal/queue reset.

## Implementation Details
Work in `src/app/mod.rs`. Session-scoped fields (per the architecture scan): `history`, the session subset of `state` (`session_id`/`run_state`/`active_run_id`/`session_goal`/`chat_items`/`pending_*`), `chat_projection`, `pending_approval`, `pending_clarification`, `active_step`/`active_steps`, `follow_up_queue`. The heavy disk read happens off-thread (task_11 wires that); `adopt_session` itself runs on the worker. Use `open()` (task_02), `rebuild` (`src/app/chat/projection.rs:50`), the new event kinds (task_04), and `is_terminal()` (task_01). See TechSpec "Core Interfaces" and ADR-006.

### Relevant Files
- `src/app/mod.rs` — `App` (`:316`), `AppState` (`:110`), `record_event` (`:4164`), `new_with_debug` (`:839`) as the construction reference.
- `src/app/chat/projection.rs` — `rebuild` (`:50`).
- `src/history/mod.rs` — `open()` (task_02); `src/orchestrator/mod.rs` — `is_terminal()` (task_01).

### Dependent Files
- `src/app/mod.rs` — task_11 calls `adopt_session` from the resume flow; task_12 reads the reconciled run/approval state.

### Related ADRs
- [ADR-006: Session adoption via a single adopt_session() swap + exhaustiveness test](adrs/adr-006.md) — the exact mechanism this task implements.
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — dangling run → `run_interrupted`, reconcile to Idle.
- [ADR-003: Production replay fold as a maintained schema-compatibility contract](adrs/adr-003.md) — atomic swap, no partial reset.

## Deliverables
- `LoadedSession` type + `App::adopt_session()`.
- Dangling-run reconciliation (`run_interrupted` + `Idle`).
- Exhaustiveness test guarding session-scoped field coverage.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: adopt a recorded session and verify full state replacement **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] After `adopt_session`, `session_id`, `session_goal`, and `chat_items` reflect the adopted session, not the previous one.
  - [ ] Adopting a session whose last run is non-terminal appends a `run_interrupted` event and leaves `run_state == Idle`.
  - [ ] Adopting a session whose last run is terminal does NOT append `run_interrupted`.
  - [ ] `pending_approval`, `pending_clarification`, and `follow_up_queue` are cleared/reset by adoption (no stale modal from the prior session).
  - [ ] Exhaustiveness test: a session-scoped field left out of `adopt_session` causes the test to fail.
- Integration tests:
  - [ ] Construct an `App`, record a partial run, then `adopt_session` a different recorded session and assert the projected transcript + run state match the adopted log.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A session can be swapped in atomically with no leaked prior-session state; the exhaustiveness test guards future fields.
