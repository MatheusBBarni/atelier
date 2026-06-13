---
status: completed
title: File-mention dropdown model and activation
type: frontend
complexity: medium
dependencies:
  - task_03
  - task_05
---

# Task 06: File-mention dropdown model and activation

## Overview
Add the `FileMentionDropdown` model and its activation logic: detect an `@` token at the cursor (reusing the existing token detector), build ranked suggestions from the cached index, show recents on a bare `@`, and model the no-match state. This is the non-rendering, non-keyboard model layer that later tasks consume.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `FILE_MENTION_PREFIX = "@"` and a `FileMentionDropdown { token, suggestions, selected, empty }` (token-based like the skill dropdown, with a command-style `empty` no-match flag).
- MUST add `file_mention_dropdown(input, ui_state)` that activates via `active_prompt_token(input, ui_state.input_cursor, "@")`.
- MUST show top-N recents (most-recently-modified) for an empty query.
- MUST set `empty = true` (no-match) when the query is non-empty and zero entries match.
- MUST suppress activation during `pending_approval`, `pending_clarification`, and `WaitingForUser`, and while the input matches the dismissal field.
- MUST NOT render or handle keystrokes in this task (later tasks).
</requirements>

## Subtasks
- [x] 6.1 Add the `@` prefix constant and the `FileMentionDropdown` struct.
- [x] 6.2 Implement activation via the shared token detector.
- [x] 6.3 Build ranked suggestions from the cached entries via the query.
- [x] 6.4 Show recents for an empty query; model the no-match state otherwise.
- [x] 6.5 Apply state-aware suppression and the dismissal gate.
- [x] 6.6 Add model-level tests for activation, recents, no-match, and gating.

## Implementation Details
Modify `src/tui/mod.rs`, mirroring `skill_dropdown` / `agent_dropdown` detection and the command dropdown's gating + `empty` modeling. Build suggestions by calling the task_03 query over `ui_state.file_mention_entries`. See TechSpec "Core Interfaces" (`FileMentionDropdown`, `file_mention_dropdown`) and ADR-005. Do not wire rendering or keys here; stage with `#[allow(dead_code)]` as the slash-command tasks did.

### Relevant Files
- `src/tui/mod.rs` — `active_prompt_token`, `skill_dropdown`/`agent_dropdown` detection, command-dropdown gating + `empty` state to mirror.
- `src/file_index.rs` — the query that produces ranked `FileSuggestion`s.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Core Interfaces".

### Dependent Files
- `src/tui/mod.rs` — task_07 (keys/insertion) and task_08 (render) consume this model.

### Related ADRs
- [ADR-005: Component Placement and Dropdown Integration](../adrs/adr-005.md) — token-based model with an `empty` flag.
- [ADR-001: Scope @-Mention File Dropdown V1](../adrs/adr-001.md) — recents and no-match behavior.

## Deliverables
- `FileMentionDropdown` model and `file_mention_dropdown` activation in `src/tui/mod.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for activation against a seeded index **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Input `see @run` with the cursor in the token activates and lists matches.
  - [ ] A bare `@` at the cursor lists recents (most-recent first) with the first row selected.
  - [ ] `@zzzz` with no matches yields `empty = true` and no selectable row.
  - [ ] A cursor positioned outside the `@` token returns `None`.
  - [ ] `pending_approval`, `pending_clarification`, and `WaitingForUser` each suppress activation.
  - [ ] An input equal to the dismissal value returns `None` until the text is edited.
  - [ ] A second `@` token mid-prompt (e.g. `a @one b @tw`) activates for the token at the cursor.
- Integration tests:
  - [ ] With a seeded `file_mention_entries`, `@mod` produces the expected ranked suggestion list in the model.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Activation, recents, no-match modeling, and state-aware/dismissal gating match the TechSpec and ADR-005
