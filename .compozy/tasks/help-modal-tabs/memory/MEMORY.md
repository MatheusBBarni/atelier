# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State
- task_01 complete: `HelpTab` + `RosterRowStyle` enums live in `src/tui/mod.rs` (just before
  the `TuiCommand` enum, ~`:91`). `HelpTab` still `#[allow(dead_code)]`.
- task_02 complete: shared `fn agent_roster_items(agents, style, theme) -> Vec<ListItem<'static>>`
  (just before `work_indicator_active`) backs the Ctrl-L roster in `Full`. `RosterRowStyle::Full`
  is live; `Compact` (1 line) is ready but unconstructed in prod until task_05, so `RosterRowStyle`
  keeps its `#[allow(dead_code)]`.

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

## Handoffs
