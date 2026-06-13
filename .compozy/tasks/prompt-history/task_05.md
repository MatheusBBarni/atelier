---
status: pending
title: "Up/Down recall interaction with collision gate and draft preservation"
type: frontend
complexity: high
dependencies:
  - task_04
---

# Up/Down recall interaction with collision gate and draft preservation

## Overview

The core interaction and the highest-risk step. Handle ↑/↓ so that, at the input's
top/bottom visual-row boundary with an empty or at-edge draft, the keys walk the
recall ring instead of moving the cursor — while wrapped/multi-line editing keeps
ordinary cursor navigation (the #1 competitor bug) and an in-progress draft is saved
and restored (ADR-001, ADR-003).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `try_recall_history(ui_state, state, dir)` invoked from the `MoveInputCursor` handler in `execute_tui_command`; it returns whether it consumed the key, otherwise the existing `move_input_cursor` runs.
- MUST recall only when the cursor is at the top (↑) or bottom (↓) visual-row boundary — reusing the `current_line` / `input_len <= width` logic from `move_input_cursor_vertically`; wrapped multi-line input MUST keep cursor navigation.
- MUST save the live draft into `prompt_history_draft` when entering history (cursor `0 → 1`) and restore it when ↓ steps past the newest entry (cursor `1 → 0`).
- MUST update `state.input` and `prompt_history_cursor` on each recall step and place the input cursor at the end of the recalled text.
- MUST NOT fire when `prompt_history` is empty, when the feature is disabled, or when a dropdown / queue / clarification owns ↑/↓ (guaranteed by the existing precedence chain; verify by test).
</requirements>

## Subtasks
- [ ] 5.1 Implement `try_recall_history` with boundary and emptiness gating.
- [ ] 5.2 Wire it into the `MoveInputCursor` branch of `execute_tui_command`, falling back to `move_input_cursor`.
- [ ] 5.3 Save the draft on entry and restore it on exit past the newest entry.
- [ ] 5.4 Place the input cursor at the end of recalled text on each step.
- [ ] 5.5 Add the full key-handling matrix (recall, no-collision, draft, yields).

## Implementation Details

Edit `src/tui/mod.rs`: the `TuiCommand::MoveInputCursor` arm in
`execute_tui_command(_with_interrupt)` (~`:507`) and a new `try_recall_history`;
reuse the boundary math in `move_input_cursor_vertically` (`:1259`). Routing already
gives dropdowns/queue/clarification priority via `key_event_to_tui_command_with_ui`
and `queue_control_active`, so recall is reached only in normal input. See TechSpec
"System Architecture" (data flow: recall) and "Testing Approach" (key-handling matrix).

### Relevant Files
- `src/tui/mod.rs` — `execute_tui_command` MoveInputCursor arm, `move_input_cursor`/`move_input_cursor_vertically`, `key_event_to_tui_command_with_ui` (precedence), `queue_control_active`, the `#[cfg(test)]` module and render/command helpers.

### Dependent Files
- `src/tui/mod.rs` (task_06) — reads `prompt_history_cursor` to tag submissions.
- `src/tui/mod.rs` (task_07) — the hint reflects active recall.

### Related ADRs
- [ADR-001: V1 Prompt History as Per-Project ↑/↓ Recall Projected from the Event Log](../adrs/adr-001.md) — collision gate, draft preservation, newest-first.
- [ADR-003: Recall State in TuiUiState; Tag Submissions via Extended AppEvent](../adrs/adr-003.md) — cursor semantics (0 = live draft; N = Nth-newest entry).

## Deliverables
- `try_recall_history` plus the gated `MoveInputCursor` branch and draft save/restore.
- The full key-handling test matrix **(REQUIRED)**.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test exercising recall→submit through the TUI command flow **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Empty input + ring `["b","a"]` + no queue/dropdown → ↑ sets input to "b" (cursor at end), ↑ again → "a", ↓ → "b".
  - [ ] After ↑ to the oldest entry, an extra ↑ is a no-op (input stays "a").
  - [ ] Type "draft", ↑ (saves the draft, shows "b"), ↓ past newest → input restored to exactly "draft".
  - [ ] Wrapped two-visual-row draft with the cursor on row 0 → ↑ moves the cursor up a row (NO recall); only at the top row does a further ↑ recall.
  - [ ] Empty input but a queued follow-up present → ↑ drives the queue, not recall.
  - [ ] Open command dropdown → ↑ drives the dropdown, not recall.
  - [ ] Empty ring (feature disabled) → ↑ is plain cursor navigation.
- Integration tests:
  - [ ] Through `execute_tui_command`: ↑ then Enter submits the recalled text to the worker.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Zero multi-line collision (verified by the matrix); the draft is never lost
- Recall walks the ring newest-first and yields to dropdowns/queue/clarification
