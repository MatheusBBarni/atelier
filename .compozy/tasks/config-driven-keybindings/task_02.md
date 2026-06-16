---
status: completed
title: Composer line-editing commands and handlers
type: frontend
complexity: medium
dependencies: []
---

# Composer line-editing commands and handlers

## Overview
Add readline/emacs line-editing to the prompt composer: cursor jumps (line start/end) and kills
(to end, to start, word-back), as new command variants plus a UTF-8-safe handler. This delivers
the "native input feel" parity that every user gets, independent of any config.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extend `InputCursorCommand` with `LineStart` and `LineEnd`, handled in `move_input_cursor`
  (`src/tui/mod.rs:1564`).
- MUST add `TuiCommand::InputKill(InputKillCommand)` with `ToLineEnd`, `ToLineStart`, `WordBack`,
  and a `kill_input` handler that mutates `AppState.input` + `TuiUiState.input_cursor` using the
  existing `byte_index_for_char` helper (`src/tui/mod.rs:1516`).
- MUST implement `Ctrl-U` semantics as kill-from-cursor-to-line-start (readline
  `unix-line-discard`); kills discard text (no kill-ring/yank in V1).
- MUST be UTF-8 safe (char-indexed cursor, byte-indexed mutation) and correct at edge cursors
  (0 and end-of-input).
- MUST wire the new variants into the `TuiCommand` execution match (near `src/tui/mod.rs:669`/`717`).
- MUST NOT bind these to keys here — default key binding happens in task_04; this task adds the
  commands and handlers only (tests call the handlers/commands directly).
</requirements>

## Subtasks
- [x] 2.1 Add `LineStart`/`LineEnd` to `InputCursorCommand` and handle them in `move_input_cursor`.
- [x] 2.2 Add `TuiCommand::InputKill(InputKillCommand{ToLineEnd,ToLineStart,WordBack})`.
- [x] 2.3 Implement `kill_input` mutating input + cursor, UTF-8 safe, edge-correct.
- [x] 2.4 Wire execution arms in the `TuiCommand` match.
- [x] 2.5 Add unit tests for every operation including multi-byte and boundary cursors.

## Implementation Details
All changes are within `src/tui/mod.rs`: the `InputCursorCommand` enum (`:203`), the `TuiCommand`
enum (`:169`), the `move_input_cursor` function (`:1564`), the existing
`insert_input_character`/`remove_input_character_before_cursor` handlers (`:1528-1562`) as the
mutation pattern to mirror, the `input_char_count`/`byte_index_for_char` helpers (`:1516`), and
the command execution match (`:669-723`). See TechSpec "Core Interfaces" (new command variants)
and "Implementation Design" for the editing semantics.

### Relevant Files
- `src/tui/mod.rs` — enums, `move_input_cursor`, new `kill_input`, execution match.

### Dependent Files
- `src/keybindings.rs` — `KeyAction` names for these actions map here via `command_for_action` (task_04).
- `src/tui/mod.rs` routing (task_04) — will default-bind these actions.

### Related ADRs
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — editing command variants, Ctrl-U semantics.
- [ADR-001: V1 Scope](adrs/adr-001.md) — composer line-editing slice (the Hybrid).

## Deliverables
- New `InputCursorCommand::LineStart/LineEnd`, `TuiCommand::InputKill(InputKillCommand)`, and `kill_input`.
- Execution arms wired in the command match.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test exercising the commands through the execution path **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `ToLineEnd` from a mid-string cursor deletes the suffix and leaves the cursor in place.
  - [x] `ToLineStart` deletes the prefix before the cursor and moves the cursor to 0.
  - [x] `WordBack` deletes the word before the cursor (including a trailing run of spaces).
  - [x] `LineStart`/`LineEnd` set the cursor to 0 / char-count without altering the text.
  - [x] Multi-byte input ("héllo🚀 wörld") keeps cursor/byte indices correct after a kill.
  - [x] Empty input is a no-op for all kills; `ToLineEnd` at end and `ToLineStart` at 0 are no-ops.
- Integration tests:
  - [x] Executing `MoveInputCursor(LineStart)` then `InputKill(ToLineEnd)` through the command handler clears the line and sets cursor 0.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Editing operations are correct for ASCII and multi-byte input with no panics at boundaries.
- No regression to existing `InputCharacter`/`InputBackspace`/arrow behavior.

## Implementation Notes
- The new `InputCursorCommand::LineStart/LineEnd`, `TuiCommand::InputKill`, and the
  `InputKillCommand` variants carry `#[allow(dead_code)]` (house pattern, mirrors
  `RosterRowStyle`) because this task deliberately does **not** bind them to keys —
  task_04 constructs them in production via `command_for_action` and should remove
  the allows then. They are exercised now only by tests calling the handlers directly.
- `try_recall_history` gained the new cursor variants in its non-recall arm to keep
  the match exhaustive (LineStart/LineEnd never trigger history recall).
- Verified `2026-06-16`: 7 new tests pass; `cargo clippy --all-targets` clean;
  `cargo fmt --check` clean on `src/tui/mod.rs`. Full `cargo test --lib` shows only
  pre-existing environmental failures (skill-discovery against the user's real
  `~/.claude/skills` + codex CLI shell-outs) — the passing count rose by exactly the
  7 new tests, confirming no regression.
