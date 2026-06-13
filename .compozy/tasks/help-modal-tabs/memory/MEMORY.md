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

## Open Risks
- `cargo fmt --check` currently fails on committed task_01 code (`HelpTab::next`/`prev` in
  `src/tui/mod.rs`, commit c00d737) — long single-line `.position()` call rustfmt wants wrapped.
  Not caused by later tasks. Fix opportunistically in a task that already touches that region, or
  as a standalone fmt commit; don't bundle it into unrelated task commits.

## Handoffs
