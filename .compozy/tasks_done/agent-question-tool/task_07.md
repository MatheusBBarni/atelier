---
status: completed
title: "Add TUI Clarification Selection Key Handling"
type: frontend
complexity: medium
dependencies:
  - task_05
---

# Task 07: Add TUI Clarification Selection Key Handling

## Overview
Add the keyboard interaction model for answering pending clarifying questions in the TUI. This task gives the Input Composer a dedicated clarification mode that can select recommended options, edit custom text, submit answers, and preserve interrupt behavior.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add TUI-local selection state for pending clarification options.
- MUST make Up/Down cycle clarification options when `state.pending_clarification` is active.
- MUST keep custom text editable while the custom answer field is visible.
- MUST dispatch `AppEvent::ClarificationAnswered` on Enter with selected option or custom text metadata.
- MUST preserve Ctrl-C run interrupt behavior while clarification is pending.
- MUST ensure selection movement does not enqueue app worker events.
- MUST ensure pending approval handling continues to take the existing approval path.
</requirements>

## Subtasks
- [x] 7.1 Add TUI state for clarification option selection and custom answer input.
- [x] 7.2 Add TUI command variants for clarification navigation and submission.
- [x] 7.3 Route key events to clarification mode before normal prompt submission.
- [x] 7.4 Dispatch structured clarification answers to the app worker.
- [x] 7.5 Preserve Ctrl-C interrupt and pending approval behavior.
- [x] 7.6 Add key-handling and local-state tests.

## Implementation Details
Use TechSpec sections "TUI Composer Mode" and "Development Sequencing". Rendering belongs to task 08; this task may add minimal state helpers needed for rendering, but should focus on behavior.

### Relevant Files
- `src/tui/mod.rs` — Owns `TuiUiState`, `TuiCommand`, key routing, command execution, input editing, approval key behavior, and TUI tests.
- `src/app/mod.rs` — Provides `AppEvent::ClarificationAnswered` and `PendingClarificationView` from task 05.
- `src/app/chat/mod.rs` — Clarification status from task 06 may influence visible state labels but is not required for key routing.

### Dependent Files
- `src/tui/mod.rs` — Task 08 depends on the selection/custom-text state and command behavior for rendering.
- `src/app/mod.rs` — App event handling from task 05 is required before Enter can submit.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires app-owned state with TUI projection/controller behavior.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — Requires answer or interrupt only, with no skip command.

## Deliverables
- TUI key handling for clarification option selection.
- TUI custom text editing state for pending clarification.
- Enter submission of structured clarification answers.
- Preserved interrupt and pending approval behavior.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration-style TUI command tests for app event dispatch **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Up key selects the previous clarification option and does not move the input cursor.
  - [x] Down key selects the next clarification option and does not move the input cursor.
  - [x] Enter on a recommended option dispatches `ClarificationAnswered` with selected option metadata.
  - [x] Character input updates custom answer text while clarification is pending.
  - [x] Backspace edits custom answer text while clarification is pending.
  - [x] Enter with custom text dispatches `ClarificationAnswered` with `answer_source = "custom"`.
  - [x] Ctrl-C still dispatches run interrupt while clarification is pending.
  - [x] Pending approval Enter still dispatches `ApprovalAnswered`, not clarification answer.
- Integration tests:
  - [x] Clarification selection movement emits no app worker event until Enter is pressed.
- Test coverage target: >=80% ✓
- All tests must pass ✓

## Success Criteria
- All tests passing
- Test coverage >=80%
- Keyboard interaction can answer pending clarification without normal prompt submission.
- Interrupt remains available.
- Pending approval behavior is unchanged.
