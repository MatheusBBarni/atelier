---
status: completed
title: "Project Queue Lifecycle Events Into Chat"
type: backend
complexity: medium
dependencies:
  - task_01
  - task_02
---

# Task 03: Project Queue Lifecycle Events Into Chat

## Overview
Make queued follow-up lifecycle events visible in the existing Chat presentation model. This task maps queue history events into concise Chat items so users can understand queued, cancelled, replaying, paused, and resumed state.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST project `follow_up_queued` into visible Chat state.
- MUST project `follow_up_cancelled` into visible Chat state.
- MUST project `follow_up_replay_started` into visible Chat state.
- MUST project `follow_up_replay_paused` with the pause reason visible.
- MUST project `follow_up_replay_resumed` into visible Chat state.
- SHOULD reuse existing Chat item kinds unless a dedicated queue kind is necessary for clear rendering.
</requirements>

## Subtasks
- [x] 3.1 Add queue event handling to Chat projection.
- [x] 3.2 Choose existing Chat item kind/status/severity mappings for queue states.
- [x] 3.3 Include prompt summaries and pause reasons in Chat item body or summary.
- [x] 3.4 Preserve existing prompt and run summary projection behavior.
- [x] 3.5 Add Chat projection tests for all queue lifecycle events.

## Implementation Details
Modify Chat projection only after task 02 has introduced queue lifecycle events. Reference the TechSpec "History And Chat Events" section. Avoid introducing a new `ChatItemKind` unless existing diagnostics or user-prompt-adjacent items cannot represent the states clearly.

### Relevant Files
- `src/app/chat/projection.rs` — Central event-to-Chat projection logic.
- `src/app/chat/mod.rs` — Defines Chat item kinds, statuses, severities, and line styles.
- `src/history/mod.rs` — Defines `HistoryEvent` shape used by projection tests.
- `src/app/mod.rs` — Emits queue lifecycle event kinds created in task 02.
- `.compozy/tasks/queued-follow-up-inputs/_techspec.md` — Defines the required event kinds and Chat projection boundary.

### Dependent Files
- `src/tui/mod.rs` — Renders `state.chat_items`; visible Chat output depends on projection quality.
- Existing Chat projection tests — Must remain stable for run summaries, prompts, approvals, diagnostics, and command results.

### Related ADRs
- [ADR-003: App-Owned Queue State And Replay](adrs/adr-003.md) — Requires lightweight history events projected into Chat.

## Deliverables
- Queue event projection in `ChatProjection`.
- Visible Chat summaries for queued, cancelled, replay started, paused, and resumed states.
- Projection tests for queue lifecycle events.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for projected queue history **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `follow_up_queued` creates a visible Chat item with the queued prompt summary.
  - [x] `follow_up_cancelled` updates or creates a visible cancelled state.
  - [x] `follow_up_replay_started` shows the queued item is replaying.
  - [x] `follow_up_replay_paused` shows the pause reason.
  - [x] `follow_up_replay_resumed` shows the item is eligible again.
- Integration tests:
  - [x] Rebuilding `ChatProjection` from history containing queue events produces stable item ordering.
  - [x] Existing run summary and user prompt projection tests still pass with queue events present.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Queue lifecycle is understandable from Chat without reading raw history.
- Existing Chat projection behavior does not regress.
