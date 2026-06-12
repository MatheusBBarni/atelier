---
status: pending
title: TUI file-index state and consumer
type: frontend
complexity: medium
dependencies:
  - task_04
---

# Task 05: TUI file-index state and consumer

## Overview
Hold the file index and the dropdown's UI state on `TuiUiState`, and consume the worker's snapshot channel inside the render loop. This also adds the selection-index and Esc-dismissal fields the dropdown needs, plus the reset wiring that re-anchors selection on edit.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add to `TuiUiState`: `file_mention_entries: Vec<FileEntry>`, `file_mention_selection_index: usize`, and `file_mention_dropdown_dismissed: Option<String>` (with matching `Default` values empty / `0` / `None`).
- MUST consume the worker's snapshot channel in `run_loop` non-blockingly (like the existing worker-state sync) and store it on `file_mention_entries`.
- MUST extend `reset_dropdown_selections` to reset `file_mention_selection_index` to `0`.
- MUST clear `file_mention_dropdown_dismissed` on content edits (insert/backspace/clear), mirroring `clear_command_dropdown_dismissal`, and NOT on cursor moves.
</requirements>

## Subtasks
- [ ] 5.1 Add the three `TuiUiState` fields and their `Default` initializers.
- [ ] 5.2 Receive the index snapshot in `run_loop` and store it.
- [ ] 5.3 Extend `reset_dropdown_selections` for the file-mention selection.
- [ ] 5.4 Clear the dismissal on content edits only.
- [ ] 5.5 Add unit tests for defaults, snapshot update, reset, and dismissal-clear.

## Implementation Details
Modify `src/tui/mod.rs` around `TuiUiState` (+ its `Default`), `run_loop`, `reset_dropdown_selections`, and the character insert/remove helpers. See TechSpec "Data Models" (TuiUiState additions). Reads of the new state may be staged with `#[allow(dead_code)]` until task_06 consumes them.

### Relevant Files
- `src/tui/mod.rs` — `TuiUiState`, `run_loop`, `reset_dropdown_selections`, `insert_input_character` / `remove_input_character_before_cursor`, and the existing `command_dropdown_dismissed` pattern to mirror.
- `src/file_index.rs` — `FileEntry` type held in the new field.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Data Models".

### Dependent Files
- `src/tui/mod.rs` — task_06 reads `file_mention_entries`, `file_mention_selection_index`, and the dismissal field.

### Related ADRs
- [ADR-003: File-Index Acquisition via Background Worker Walk](../adrs/adr-003.md) — the consumer side of the channel.
- [ADR-005: Component Placement and Dropdown Integration](../adrs/adr-005.md) — the dropdown's UI-state shape.

## Deliverables
- Three new `TuiUiState` fields, channel consumption in `run_loop`, and reset/dismissal wiring.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for snapshot consumption **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `TuiUiState::default` initializes `file_mention_entries` empty, `file_mention_selection_index` to `0`, and `file_mention_dropdown_dismissed` to `None`.
  - [ ] `reset_dropdown_selections` sets `file_mention_selection_index` to `0`.
  - [ ] Inserting a character clears `file_mention_dropdown_dismissed`; a cursor move does not.
  - [ ] Receiving a snapshot replaces `file_mention_entries`.
- Integration tests:
  - [ ] A snapshot delivered on the channel is reflected in `file_mention_entries` after a `run_loop` iteration (or the extracted receive helper).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The TUI holds and refreshes the index, and resets selection/dismissal correctly on edits
