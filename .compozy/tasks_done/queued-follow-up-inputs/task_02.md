---
status: completed
title: "Add Queue Replay, Pause, Cancel, And Resume Lifecycle"
type: backend
complexity: high
dependencies:
  - task_01
---

# Task 02: Add Queue Replay, Pause, Cancel, And Resume Lifecycle

## Overview
Add the queue lifecycle behavior that turns pending queue items into normal Runs only when replay is safe. This task implements FIFO replay, paused replay after unsafe endings, cancellation, and resume controls at the app layer.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST replay at most one pending queued follow-up after each clean completed Run.
- MUST start replayed items through the normal prompt/run creation path.
- MUST pause replay after failed, interrupted, limit-reached, approval-waiting, and clarification-waiting outcomes.
- MUST add app events or methods for cancelling and resuming queued items.
- MUST record lightweight queue lifecycle history events for queued, cancelled, replay started, replay paused, and replay resumed states.
- MUST preserve the one-active-Run invariant.
</requirements>

## Subtasks
- [x] 2.1 Add queue lifecycle status transitions for pending, paused, replaying, and cancelled items.
- [x] 2.2 Add cancellation handling for queued items before replay.
- [x] 2.3 Add resume handling for paused queued items.
- [x] 2.4 Add safe replay gating around Run completion paths.
- [x] 2.5 Add pause handling for non-clean Run endings.
- [x] 2.6 Add queue lifecycle history events.
- [x] 2.7 Add focused app lifecycle tests.

## Implementation Details
Extend the queue state from task 01. Reference the TechSpec "Replay Rules" and "History And Chat Events" sections. Keep replay logic app-owned and avoid adding runtime adapter behavior.

### Relevant Files
- `src/app/mod.rs` — Owns `drive_run`, run completion paths, pending approval/clarification, interrupt handling, limit handling, event recording, and app tests.
- `src/orchestrator/mod.rs` — Defines `RunState`, which is used for safe replay gating.
- `src/runtime/fake.rs` — Provides fake runtime prompt patterns used by existing app tests for completion, clarification, approval, parse errors, and streaming.
- `src/history/mod.rs` — Defines `HistoryEvent` and event append behavior.
- `.compozy/tasks/queued-follow-up-inputs/_techspec.md` — Defines replay, pause, and lifecycle event requirements.

### Dependent Files
- `src/app/chat/projection.rs` — Consumes lifecycle events in task 03.
- `src/tui/mod.rs` — Dispatches cancel/resume events and renders state in task 04.
- `.multiagent` history output — Queue lifecycle events will appear in session history.

### Related ADRs
- [ADR-001: Scope Queued Follow-Up Inputs V1](adrs/adr-001.md) — Requires FIFO replay and safe replay gating.
- [ADR-003: App-Owned Queue State And Replay](adrs/adr-003.md) — Requires app-owned replay, pause, cancel, and resume behavior.

## Deliverables
- App-owned FIFO replay after clean completion.
- App-owned pause behavior after unsafe endings.
- App cancellation and resume controls for queued items.
- Queue lifecycle history events.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for replay and paused queue behavior **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Two queued prompts replay oldest-first across two clean completed Runs.
  - [x] A clean completed Run replays only one queued item.
  - [x] Cancelling a pending queued item prevents replay.
  - [x] Resuming a paused queued item makes it eligible for replay.
  - [x] Replay records `follow_up_replay_started` and normal `prompt_submitted` for the replayed Run.
- Integration tests:
  - [x] A queued prompt after a successful fake-runtime prompt starts as the next Run.
  - [x] `needs clarification create a feature` pauses the queue until clarification is resolved or the item is resumed later.
  - [x] `approval action create a feature` pauses the queue while approval is pending.
  - [x] `always parse error create a feature` does not replay queued items.
  - [x] A limit-reached Run does not replay queued items.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- FIFO replay works without overlapping Runs.
- Unsafe endings pause replay with a stored reason.
- Cancel and resume transitions are deterministic and recorded in history.
