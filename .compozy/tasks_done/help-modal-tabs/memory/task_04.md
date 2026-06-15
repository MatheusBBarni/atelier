# Task Memory: task_04.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Three pure static-content help-tab builders added in `src/tui/mod.rs`, just after
`render_help_modal`: `keys_tab_lines`, `cli_tab_lines`, `approvals_tab_lines`. All
`fn(&Theme) -> Vec<Line<'static>>`. Keys/CLI relocated verbatim from the pre-tab
`render_help_modal` literals; Approvals is net-new static prose (ADR-001: static in V1).

## Important Decisions
- All three builders carry `#[allow(dead_code)]` + a doc/comment naming task 06 as the
  consumer (shared-memory staging convention) — they are unused in prod until the tabbed
  render lands.
- `keys_tab_lines`/`cli_tab_lines` take `_theme` (rows are plain `Line::from` text, no
  styling — matches the old literals). Only `approvals_tab_lines` uses theme tokens
  (`theme.accent` bold headers, `theme.text` body) — no inline `Color::` literals.
- `render_help_modal` was NOT modified (still the old non-tabbed path) — rewriting it is
  task 06. The old literals remain in place; task 06 deletes them when it calls the builders.

## Learnings
- Test helper to read a `Line`'s text: collect `line.spans.iter().map(|s| s.content.as_ref())`
  (pattern already used elsewhere in the file, e.g. `:9491`).
- Tests build a theme via `Theme::resolve(TerminalCaps::detect())`.

## Files / Surfaces
- `src/tui/mod.rs`: builders right after `render_help_modal` (~`:3360+`); unit tests appended
  to the end of `#[cfg(test)] mod tests`.

## Errors / Corrections
- rustfmt wanted the `body` closure on one line; fixed in-place. Did NOT run blanket
  `cargo fmt` (parallel WIP / scope-to-own-files).
- Pre-existing fmt diff at `:129/:136` and the `HelpTab` dead_code clippy warning are from
  task_01, NOT this task — left untouched (see shared Open Risks).

## Ready for Next Run
Task 06 should: rewrite `render_help_modal` to call these builders for the Keys/CLI/Approvals
tabs and remove the now-duplicated literals + the three `#[allow(dead_code)]` markers.
