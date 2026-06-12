---
status: completed
title: File-mention dropdown interaction and insertion
type: frontend
complexity: medium
dependencies:
  - task_06
---

# Task 07: File-mention dropdown interaction and insertion

## Overview
Add the keyboard interaction and acceptance for the file-mention dropdown: a command enum and `TuiCommand` variant, key mapping with selection wraparound and Esc-dismiss, and the insertion that consumes the `@` and writes a bare path (folders get a trailing `/`) followed by a space. Acceptance inserts text only — it never submits the prompt.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `FileMentionDropdownCommand { Previous, Next, Accept, Dismiss }` and a `TuiCommand::FileMentionDropdown(..)` variant.
- MUST map Up/Down to Previous/Next, Tab and Enter both to Accept, and Esc to Dismiss when there are matches; selection MUST wrap at the ends.
- MUST NOT trap Enter in the no-match state (it falls through to normal submission); Esc MUST still dismiss.
- MUST implement `apply_file_mention_suggestion` that replaces the token INCLUDING the leading `@` with the bare path, appends `/` for folders, adds a trailing space if none follows, and places the cursor after the inserted text.
- MUST set the dismissal field on Esc and MUST NOT dispatch an `AppEvent` on accept.
</requirements>

## Subtasks
- [x] 7.1 Add the command enum and the `TuiCommand` variant.
- [x] 7.2 Add the key-command mapping (Up/Down/Tab/Enter/Esc, no-match fallthrough).
- [x] 7.3 Add Previous/Next selection wraparound dispatch.
- [x] 7.4 Implement `apply_file_mention_suggestion` (consume `@`, folder `/`, trailing space, cursor).
- [x] 7.5 Implement Esc dismissal.
- [x] 7.6 Add unit tests for key mapping, wraparound, and insertion.

> Note: the `TuiCommand::FileMentionDropdown` executor arm is added here (not in
> task_09) because match exhaustiveness requires it the moment the variant
> exists; task_09's "dispatch in the executor" is therefore satisfied early.
> The key-routing branch (deciding when to call the key-command mapper) remains
> task_09's responsibility.

## Implementation Details
Modify `src/tui/mod.rs`, mirroring `agent_dropdown_key_command` (Up/Down/Enter), the command dropdown's Esc handling, and `apply_skill_suggestion` for the token-replacement — but extend the replaced range back over the `@` so the result is a bare path. See TechSpec "Core Interfaces" (`FileMentionDropdownCommand`, `apply_file_mention_suggestion`) and ADR-005.

### Relevant Files
- `src/tui/mod.rs` — `DropdownCommand`/`CommandDropdownCommand`, `agent_dropdown_key_command`, `command_dropdown_key_command`, `apply_skill_suggestion`, and the `TuiCommand` enum.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Core Interfaces".

### Dependent Files
- `src/tui/mod.rs` — task_09 wires the activation branch and dispatches these commands.

### Related ADRs
- [ADR-005: Component Placement and Dropdown Integration](../adrs/adr-005.md) — `@`-consuming insertion and the command shape.

## Deliverables
- Command enum, `TuiCommand` variant, key mapping, wraparound, Esc dismissal, and `apply_file_mention_suggestion`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for the accept-and-continue flow **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Up/Down map to Previous/Next; Tab and Enter both map to Accept; Esc maps to Dismiss.
  - [ ] Selection wraps from last to first and first to last.
  - [ ] Accept on `see @run` yields `see src/runtime/claude.rs ` — the `@` is gone, a trailing space is added, the cursor follows, and surrounding text is intact.
  - [ ] Accepting a folder yields a trailing slash (e.g. `src/tui/ `).
  - [ ] Accept dispatches no `AppEvent`.
  - [ ] In the no-match state, Enter is not trapped (returns `None` / falls through).
  - [ ] Esc records the current input in the dismissal field.
- Integration tests:
  - [ ] Typing `@mod`, navigating, and accepting rewrites the buffer to the bare path; typing a second `@` afterward opens the dropdown again with the cursor correctly positioned.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Keys, wraparound, Esc-dismiss, and `@`-consuming insertion (with folder slash, trailing space, multi-reference) behave per the TechSpec and ADR-005
