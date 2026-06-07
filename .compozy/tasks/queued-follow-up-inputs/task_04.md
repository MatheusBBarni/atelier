---
status: pending
title: "Render Queue State And Controls In TUI"
type: frontend
complexity: high
dependencies:
  - task_01
  - task_02
  - task_03
---

# Task 04: Render Queue State And Controls In TUI

## Overview
Expose queued follow-up state in the interactive TUI and add keyboard-dispatchable controls for cancelling and resuming items. This task makes the app-owned queue usable without moving lifecycle ownership out of `App`.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render queue count and item summaries from `AppState`.
- MUST distinguish pending, paused, replaying, and cancelled item states.
- MUST expose cancellation for queued items before replay.
- MUST expose resume for paused items.
- MUST preserve input composer stability and existing Chat rendering.
- MUST preserve existing help, agent dropdown, skill dropdown, approval, and interrupt behavior.
</requirements>

## Subtasks
- [ ] 4.1 Add compact queue rendering to the TUI.
- [ ] 4.2 Add paused queue reason rendering.
- [ ] 4.3 Add TUI command dispatch for queue cancellation.
- [ ] 4.4 Add TUI command dispatch for queue resume.
- [ ] 4.5 Preserve existing input cursor, scroll, dropdown, approval, and interrupt behavior.
- [ ] 4.6 Add render and key-dispatch tests.

## Implementation Details
Modify `src/tui/mod.rs` after queue state, lifecycle events, and Chat projection exist. Reference the TechSpec "Component Overview" and "Testing Approach" sections. Keep the first UI compact; do not introduce a large modal or full queue manager in MVP.

### Relevant Files
- `src/tui/mod.rs` — Contains rendering, input status, Chat rendering, TUI command dispatch, help modal, dropdowns, and TUI tests.
- `src/app/mod.rs` — Provides `AppState` queue view data and queue control `AppEvent` variants from earlier tasks.
- `src/app/chat/mod.rs` — Provides Chat item data rendered by TUI.
- `.compozy/tasks/queued-follow-up-inputs/_prd.md` — Defines visible queue, cancel, resume, and paused-state user experience.
- `.compozy/tasks/queued-follow-up-inputs/_techspec.md` — Defines TUI responsibilities and testing expectations.

### Dependent Files
- `README.md` — Documentation should reflect the final visible TUI commands in task 05.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — If shared command dropdown work is present, queued command rendering should not conflict with command dropdown precedence.

### Related ADRs
- [ADR-001: Scope Queued Follow-Up Inputs V1](adrs/adr-001.md) — Requires visible queue state and cancel controls.
- [ADR-003: App-Owned Queue State And Replay](adrs/adr-003.md) — Keeps TUI as rendering/control surface only.

## Deliverables
- TUI rendering for queue count/list and paused reasons.
- TUI dispatch for cancel and resume controls.
- Render tests for queue states.
- Keyboard/command dispatch tests for cancel and resume events.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for TUI queue interaction **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Rendering with one pending queue item shows the item summary and count.
  - [ ] Rendering with multiple pending items preserves FIFO display order.
  - [ ] Rendering a paused item shows the pause reason.
  - [ ] Rendering a replaying item distinguishes it from pending items.
  - [ ] Existing help, approval, and Chat render tests still pass.
- Integration tests:
  - [ ] Queue cancel control dispatches `AppEvent::FollowUpCancelled` for the selected item id.
  - [ ] Queue resume control dispatches `AppEvent::FollowUpResumeRequested` for the paused item id.
  - [ ] Agent and skill dropdown key routing remains unchanged when no queue control is active.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Users can see queued follow-ups and paused reasons in the TUI.
- Users can cancel and resume queued items through TUI controls.
- Existing TUI layout and input behavior remain stable.
