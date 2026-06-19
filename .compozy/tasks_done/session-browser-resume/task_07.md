---
status: completed
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
- [x] 7.1 Add `SessionBrowserState` to `TuiUiState` and the open/close + nav/filter `TuiCommand`s.
- [x] 7.2 Slot the modal into the key-routing precedence cascade.
- [x] 7.3 Add the off-thread session-list loader + watch side-channel and sync into `TuiUiState`.
- [x] 7.4 Render the list (label/timestamp/outcome badge) using theme tokens.
- [x] 7.5 Add key-routing precedence + render/state unit tests.

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
  - [x] With the browser visible, a printable key narrows the filter and `↑/↓` move the selection index; `Esc` returns a close command. — `session_browser_keys_route_to_filter_nav_and_close` + `browser_filter_narrows_rows_case_insensitively`
  - [x] Key-routing precedence: when the browser is visible it takes precedence over the command/queue branches; help still wins if both are somehow set. — `browser_takes_precedence_over_normal_but_help_wins`
  - [x] The filter narrows the rendered rows case-insensitively by label. — `browser_filter_narrows_rows_case_insensitively`
  - [x] `colors_live_only_in_theme_module` passes (no inline color literals added). — verified passing.
- Integration tests:
  - [x] `Ctrl-R` opens the modal, the off-thread loader publishes summaries via the watch channel, and the list renders newest-first. — `ctrl_r_opens_browser_from_normal_context` + `sync_session_summaries_adopts_published_snapshot` + `browser_renders_summaries_newest_first`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can open the browser, see their sessions newest-first, narrow by typing, and navigate — without the UI stalling on load.

## As-built notes
- `SessionBrowserState { visible, mode: BrowserMode, summaries, selection_index, filter }` on `TuiUiState` (derives align with `TuiUiState`'s Clone/PartialEq; the watch *sender* stays in `run_loop`, not the state). `BrowserMode::List` only (Preview arrives task_08). `TuiCommand::SessionBrowser(SessionBrowserCommand{ Open, Close, Up, Down, FilterChar, FilterBackspace })`.
- Cascade: browser slotted **below help, above every other context** — when visible it consumes nav/filter/close and swallows other keys. `session_browser_key_command` maps them.
- **Ctrl-R conflict resolved:** Ctrl-R was already bound to *resume a PAUSED queue item* (`queue_control_key_command`). The new browser-open binding lives in the base handler (the queue path's `.or_else` fallback), so paused-resume keeps precedence; only the previously-inert *pending* / no-queue case now opens the browser. The existing `ctrl_r_does_not_resume_pending_item` test was updated to assert it opens the browser (still no resume). Guarded off while approval/clarification/governance is pending.
- Off-thread load mirrors `spawn_file_index_refresh`: `run_loop` owns a `watch::channel<Vec<SessionSummary>>`, spawns `spawn_session_summaries_load` (→ `spawn_blocking(list_session_summaries)`) when the browser opens, and `sync_session_summaries` adopts the snapshot each frame (clamping selection). Render never blocks on load.
- `render_session_browser` is a full-takeover modal (early-return in `render`, so it draws correctly over any composer/clarification context). Rows are `▶ label · timestamp · outcome`, outcome via the existing `run_state_style` theme token (no inline `Color::`; `colors_live_only_in_theme_module` passes); transcript-derived label/timestamp are `sanitize_transcript_text`'d (task_06).
