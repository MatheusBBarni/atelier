---
status: completed
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
- [x] 10.1 Define `LoadedSession` enumerating all session-scoped fields.
- [x] 10.2 Implement `adopt_session` (single reassignment + one broadcast).
- [x] 10.3 Reconcile dangling run → `run_interrupted` event → `Idle`.
- [x] 10.4 Add the exhaustiveness test (sentinel-then-adopt-then-assert-all-replaced).
- [x] 10.5 Add unit tests for projection/goal/run-state replacement and modal/queue reset.

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
  - [x] After `adopt_session`, `session_id`, `session_goal`, and `chat_items` reflect the adopted session, not the previous one. — `adopt_session_swaps_identity_goal_and_transcript`
  - [x] Adopting a session whose last run is non-terminal appends a `run_interrupted` event and leaves `run_state == Idle`. — `adopt_session_reconciles_a_dangling_run_to_idle`
  - [x] Adopting a session whose last run is terminal does NOT append `run_interrupted`. — `adopt_session_does_not_reconcile_a_terminal_run`
  - [x] `pending_approval`, `pending_clarification`, and `follow_up_queue` are cleared/reset by adoption (no stale modal from the prior session). — `adopt_session_resets_every_session_scoped_field`
  - [x] Exhaustiveness test: a session-scoped field left out of `adopt_session` causes the test to fail. — `adopt_session_resets_every_session_scoped_field` (runtime) + the full `AppState` struct literal in `adopt_session` (compile-time guard)
  - [x] Dangling-run fold cases (crash mid-run, graceful quit, clean between-runs quit, terminal, multi-run). — `history::tests::dangling_run_*` (7 tests)
- Integration tests:
  - [x] Construct an `App`, record a partial run, then `adopt_session` a different recorded session and assert the projected transcript + run state match the adopted log. — `adopt_session_swaps_identity_goal_and_transcript` (records goal+prompt+completed run, adopts, asserts transcript/run state)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A session can be swapped in atomically with no leaked prior-session state; the exhaustiveness test guards future fields.

## As-built notes
- **`LoadedSession`** (`src/app/mod.rs`) carries only what the swap needs: the opened `history`, the folded `projection`, `session_id`, `session_goal`, and `dangling_run: Option<(String, RunState)>` (pre-filtered to non-terminal). `load(root, session_id)` does open→read→fold; `from_history_and_events(history, &events)` is the `Send`-friendly fold so task_11 can lift the heavy read off-thread and fold on the worker. Both are `pub` (matching the task_05 `detect_drift`/`WorkspaceDrift` convention — public lib items are exempt from the lib-target dead-code lint until task_11's caller lands).
- **`adopt_session`** is the single atomic swap (ADR-006), kept **private** per the ADR. Order: swap `history`+`projection` → reset every transient `App` field (mirrors `interrupt()` teardown) → rebuild the `AppState` session subset via a **full struct literal** (no `..` spread) so a future `AppState` field is a *compile error* here (compile-time exhaustiveness) → reconcile a dangling run via a no-publish `append_event_with_group(run_interrupted)` → `sync_chat_items` → exactly one `publish_state`. Process-scoped fields (`config_status`, `git_context`) carry across; live agent activity rebuilds to a fresh idle roster. Carries `#[allow(dead_code)]` with a task_11 pointer (removed when the resume flow wires it).
- **Single-broadcast invariant** required extracting `append_event_with_group` (durable append + fold, no `publish_state`) out of `record_event_with_group` (= append + publish); the reconciliation event uses the no-publish path so the swap emits one broadcast, not two.
- **Dangling detection** lives in `history::dangling_run_from_events` (sibling to `derive_outcome`): folds the most-recently-*started* run, closes it on a terminal run event or a `session_ended` whose `active_run_id` is set, and returns `Some` only when `!is_terminal()`. A clean between-runs quit (`active_run_id: null`) does not resurrect the closed run. 7 fold-case unit tests.
- **Follow-up (task_11):** thread `WorkspaceDrift` through `LoadedSession`/the resume flow and remove the `adopt_session` `#[allow(dead_code)]` once `AppEvent::ResumeSession` calls it. The exhaustiveness test will need extending when task_12 adds `resume_approval_mode`/`pending_drift_ack`.
