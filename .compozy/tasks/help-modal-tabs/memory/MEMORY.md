# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State
- task_01 complete: `HelpTab` + `RosterRowStyle` enums live in `src/tui/mod.rs` (just before
  the `TuiCommand` enum, ~`:91`). `HelpTab` still `#[allow(dead_code)]`.
- task_02 complete: shared `fn agent_roster_items(agents, style, theme) -> Vec<ListItem<'static>>`
  (just before `work_indicator_active`) backs the Ctrl-L roster in `Full`. `RosterRowStyle::Full`
  is live; `Compact` (1 line) is ready but unconstructed in prod until task_05, so `RosterRowStyle`
  keeps its `#[allow(dead_code)]`.
- task_03 complete: `TuiUiState.help_active_tab: HelpTab` (default `GettingStarted`, reset on
  `ToggleHelp` close) is live (`src/tui/mod.rs` struct `:257`, Default `:303`, arm `:562`).
  Ephemeral UI state, never in `AppState`. `HelpTab` keeps `#[allow(dead_code)]` (variants/
  `next`/`prev`/`title`/`ALL` still unused until task_06/07).
- task_04 complete: static-content builders `keys_tab_lines`/`cli_tab_lines`/`approvals_tab_lines`
  (`fn(&Theme) -> Vec<Line<'static>>`) live just after `render_help_modal` (~`:3360+`). Keys/CLI
  relocated verbatim from the old `render_help_modal` literals (still present there until task_06
  swaps them in); Approvals is net-new static prose (theme-token headers/body). All three carry
  `#[allow(dead_code)]` until task_06 consumes them.
- task_05 complete: live builders `getting_started_lines(&AppState,&Theme)`,
  `commands_tab_lines(&str,&Theme)` (catalog-derived; filter is a no-op `_filter` until task_09),
  `skills_tab_lines(&TuiUiState,&Theme)` live just before `centered_rect`. All `#[allow(dead_code)]`
  until task_06. Shared `agent_compact_line(index,&AgentView,&Theme) -> Line` extracted (just
  before `agent_roster_items`); the `Compact` arm now wraps it, and Getting Started reuses it —
  one compact-row definition. `RosterRowStyle::Compact` is still only constructed in tests, so
  `RosterRowStyle` keeps its `#[allow(dead_code)]`.

- task_06 complete: `render_help_modal(frame, &AppState, &TuiUiState, &Theme)` is now a tabbed
  overlay — theme-token tab strip (active = accent + BOLD|UNDERLINED, inactive = text_muted) +
  blank line + dispatch on `ui_state.help_active_tab` to the per-tab builders. Default
  GettingStarted renders on open. Both call sites (clarification branch + main render) updated.
  All six builders lost their `#[allow(dead_code)]`; `impl HelpTab` keeps it for `next`/`prev`
  (still unused until task_07). Outer block still titled " Help " + " Esc " w/ accent border.

- task_07 complete: `TuiCommand::HelpNextTab`/`HelpPrevTab` (enum after `ToggleHelp`) + executor
  arms (`ui_state.help_active_tab = .next()/.prev()`) + key bindings in the help-visible branch
  (`:879`): Right/Tab → next, Left/Shift-Tab/BackTab → prev, Esc → ToggleHelp, all else `None`.
  `impl HelpTab` lost its `#[allow(dead_code)]` (next/prev now consumed). The whole feature is
  now interactive; only Phase 2 (filter, first-approval explainer) remains deferred.

- task_09 complete (Phase 2): `TuiUiState.help_filter: String` (default `""`, cleared on
  tab change + close) backs a Commands-tab substring filter. `TuiCommand::HelpFilterCharacter`/
  `HelpFilterBackspace` are routed in the help-visible key branch ONLY when `help_active_tab ==
  Commands` (printable Char w/o CONTROL → char, Backspace → backspace; arrows/Tab still
  navigate since they aren't Char codes). `commands_tab_lines(filter,&theme)` now iterates
  `catalog()` directly (not `help_command_lines()`), case-insensitive `.contains()` over
  usage+label, renders a leading "Filter: …" line and a "No commands match …" empty indicator.
  Never touches `state.input`. First Phase-2 item done; only the first-approval explainer remains.

## Shared Decisions
- New foundational TUI types are staged with `#[allow(dead_code)]` + a doc-comment naming the
  consuming task, since the crate has no `deny(warnings)` and the lib build would warn otherwise.
  Remove the allow when the consumer lands.

## Shared Learnings
- `src/tui/mod.rs` test module is at `:4099` (`#[cfg(test)] mod tests { use super::*; ... }`).
- Spec line numbers drift; locate by symbol. Roster helpers: `status_style`/`agent_status_label`/
  `availability_style`/`availability_label` sit together (~`:3442+`). `availability_label` →
  "ok"/"down"/"?"; `theme.accent_for(index)` cycles agent colors.
- Test `ListItem`s via `item.height()` (line count) + a render-to-`TestBackend` helper for visible
  text — `ListItem` content is not publicly readable in ratatui 0.29.

## For task_07 (key navigation)
- The tab strip legitimately renders the literal "Commands" (a tab title), so
  `help_modal_suppresses_command_dropdown_rendering` now asserts the command dropdown's unique
  right-aligned hint "Up/Down Tab/Enter" is ABSENT (not `!contains("Commands")`). Keep that hint
  string distinct from any future tab-strip nav hint, or that test breaks.
- `renders_help_modal_commands` and `help_modal_command_rows_are_catalog_derived` now render
  multiple tabs (Commands + Keys + CLI) per test because each tab shows one body at a time.

## Open Risks
- RESOLVED in task_05: the pre-existing `cargo fmt --check` failure on `HelpTab::next`/`prev`
  (long `.position()` line) and the raw-`cargo clippy` dead-code warning on `impl HelpTab` (the
  task_01 `#[allow(dead_code)]` was on the enum, not the impl) are both fixed. NOTE: `rtk cargo
  clippy` filters dead-code warnings — verify with raw `cargo clippy --all-targets` for the gate.

## Handoffs
