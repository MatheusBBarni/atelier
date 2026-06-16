---
status: pending
title: "Session browser modal + off-thread session list"
type: frontend
complexity: high
dependencies:
  - task_03
---

# Task 07: Session browser modal + off-thread session list

## Overview
Build the picker itself: a modal opened with `Ctrl-R`, holding its state in `TuiUiState`, slotted into the key-routing precedence cascade, with arrow navigation, type-to-filter (substring narrow), and a newest-first list rendered from `SessionSummary` rows loaded off-thread (mirroring the file-index watch-channel pattern). This is the Phase 1 browse surface.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add session-browser state to `TuiUiState` (visible flag, summaries, selection index, filter buffer, mode) and slot the modal into `key_event_to_tui_command_with_ui` precedence (consuming keys while visible; `Esc` closes).
- MUST open the modal via `Ctrl-R` (hardcoded; no keybinding config exists yet) and support `↑/↓` navigation, type-to-filter (case-insensitive substring narrow), and `Esc` to close.
- MUST load the session list OFF-THREAD via a watch side-channel (mirroring `spawn_file_index_refresh`), never blocking the render loop, and render rows with label + timestamp + outcome (outcome reusing the existing run-state color tokens, with a text label for `NO_COLOR`).
- MUST route all color through `theme.rs` tokens (no inline `Color::` literals; the `colors_live_only_in_theme_module` test must still pass).
</requirements>

## Subtasks
- [ ] 7.1 Add `SessionBrowserState` to `TuiUiState` and the open/close + nav/filter `TuiCommand`s.
- [ ] 7.2 Slot the modal into the key-routing precedence cascade.
- [ ] 7.3 Add the off-thread session-list loader + watch side-channel and sync into `TuiUiState`.
- [ ] 7.4 Render the list (label/timestamp/outcome badge) using theme tokens.
- [ ] 7.5 Add key-routing precedence + render/state unit tests.

## Implementation Details
Extend `TuiUiState` (`src/tui/mod.rs:272`) and the cascade in `key_event_to_tui_command_with_ui` (`:1056`) — place the browser at modal precedence (alongside/below help). Add `TuiCommand` variants (`:169`) and bind `Ctrl-R` in `key_event_to_tui_command` (`:1388`). Mirror `spawn_file_index_refresh` (`:947`) + `sync_file_index` (`:812`) for the off-thread `watch::Sender<Vec<SessionSummary>>`. Render near the help/clarification modal renderers; use run-state colors from `theme.rs` (`:122`). See TechSpec "API Endpoints"/"System Architecture" and ADR-001.

### Relevant Files
- `src/tui/mod.rs` — `TuiUiState` (`:272`), cascade (`:1056`), `TuiCommand` (`:169`), `AppWorkerCommand` (`:267`), `spawn_file_index_refresh` (`:947`), `sync_file_index` (`:812`), `render` (`:2522`).
- `src/tui/theme.rs` — run-state/semantic color tokens (`:122`).
- `src/history/mod.rs` — `list_session_summaries` (task_03).

### Dependent Files
- `src/tui/mod.rs` — task_08 adds the preview pane on top of this modal; task_09 adds `/sessions` dispatching the same open command.

### Related ADRs
- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — substring narrow, not fuzzy; list columns.
- [ADR-005: Product approach — recovery-first, phased delivery](adrs/adr-005.md) — this is the Phase 1 browse surface.

## Deliverables
- Session browser modal (state + cascade slot + `Ctrl-R` + nav/filter) rendering an off-thread-loaded newest-first list.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: open → list populated off-thread → navigate/filter **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] With the browser visible, a printable key narrows the filter and `↑/↓` move the selection index; `Esc` returns a close command.
  - [ ] Key-routing precedence: when the browser is visible it takes precedence over the command/queue branches; help still wins if both are somehow set.
  - [ ] The filter narrows the rendered rows case-insensitively by label.
  - [ ] `colors_live_only_in_theme_module` passes (no inline color literals added).
- Integration tests:
  - [ ] `Ctrl-R` opens the modal, the off-thread loader publishes summaries via the watch channel, and the list renders newest-first.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can open the browser, see their sessions newest-first, narrow by typing, and navigate — without the UI stalling on load.
