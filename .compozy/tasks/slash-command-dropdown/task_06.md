---
status: completed
title: "Add Command Dropdown Keyboard Handling And Text Insertion"
type: frontend
complexity: medium
dependencies:
  - task_04
  - task_05
---

# Task 06: Add Command Dropdown Keyboard Handling And Text Insertion

## Overview
Add the interactive behavior for the command dropdown: selection movement, `Tab`/`Enter` acceptance, Escape dismissal, text-only insertion, and no-match `Enter` trapping. This task makes the rendered dropdown usable without changing app command execution.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST route Up/Down to command dropdown selection while the command dropdown has selectable suggestions.
- MUST accept selected suggestions with both `Tab` and `Enter`.
- MUST insert or complete command text only; acceptance MUST NOT dispatch an app event.
- MUST trap `Enter` while the no-match empty state is visible.
- MUST support Escape dismissal without mutating raw input.
- MUST preserve normal prompt submission when no command dropdown is active.
- MUST preserve existing agent and skill dropdown key behavior.
</requirements>

## Subtasks
- [x] 6.1 Add command dropdown key routing after agent and skill dropdown routing.
- [x] 6.2 Add selection cycling for command suggestions.
- [x] 6.3 Add `Tab` and `Enter` acceptance for selected command suggestions.
- [x] 6.4 Add text-only insertion and cursor update behavior.
- [x] 6.5 Add Escape dismissal and no-match `Enter` trapping.
- [x] 6.6 Add interaction tests proving no app events are dispatched during dropdown acceptance.

## Implementation Details
Modify key handling and dropdown command execution paths in `src/tui/mod.rs`. Use existing input cursor and byte/char index helpers for insertion safety. Keep app submission behavior untouched; accepted suggestions only edit the local input.

### Relevant Files
- `src/tui/mod.rs` — Contains `key_event_to_tui_command_with_ui`, dropdown key helpers, `execute_tui_command`, insertion helpers, and TUI interaction tests.
- `src/slash_commands.rs` — Provides `insert_text` for accepted suggestions.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines text-only acceptance, no-match trapping, and keyboard behavior.

### Dependent Files
- `src/app/mod.rs` — Should not receive app events during dropdown acceptance.
- `README.md` — May need later review for `Tab` acceptance mention.

### Related ADRs
- [ADR-004: Scope Slash Dropdown Activation And Keyboard Semantics](adrs/adr-004.md) — Defines keyboard semantics, text-only insertion, and no-match trapping.
- [ADR-002: Choose Error-Reduction Product Approach](adrs/adr-002.md) — Prioritizes preventing invalid submissions before they reach the app.

## Deliverables
- Command dropdown keyboard handling in `src/tui/mod.rs`.
- Text-only insertion with correct cursor placement.
- Escape dismissal and no-match `Enter` trapping.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for no app-event dispatch during acceptance **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Up selects the previous command suggestion and wraps as expected.
  - [x] Down selects the next command suggestion and wraps as expected.
  - [x] `Tab` accepts `/config` and leaves input as `/config` without submitting.
  - [x] `Enter` accepts `/help` and leaves input as `/help` without toggling help.
  - [x] Accepting `/goal` leaves the cursor ready for additional goal text.
  - [x] Escape dismisses the dropdown and preserves raw input.
  - [x] `Enter` on `No commands found` does not dispatch `PromptSubmitted`.
  - [x] Normal Enter still submits input when no command dropdown is active.
- Integration tests:
  - [x] Channel receiver remains empty after command dropdown acceptance.
  - [x] Existing agent and skill dropdown key tests still pass.
- Test coverage target: >=80%
- All tests must pass

## Follow-up Notes (recorded during execution, 2026-06-12)
- **No trailing space on accept**: command acceptance replaces the whole
  `/`-token with `insert_text` exactly (no trailing space). Prompt prefixes
  (`/agent:`, `/skill:`) must keep no trailing text so the specialized
  dropdowns' token detection takes over; argument commands let the user type
  their own space, which then releases the dropdown.
- **Re-accept guard**: accepting sets `command_dropdown_dismissed` to the
  inserted text so a second `Enter` submits (or toggles `/help`) instead of
  re-accepting the same row; the agent/skill dropdowns ignore this dismissal,
  preserving handoff.

## Success Criteria
- All tests passing
- Test coverage >=80%
- Command dropdown keyboard behavior matches the PRD and TechSpec.
- Dropdown acceptance never submits or executes commands.
