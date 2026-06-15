# Task Memory: task_09.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Phase 2: type-to-filter on the Commands tab via a dedicated `help_filter` buffer
(NOT `state.input`). `.contains()` over catalog usage/label; render filter line +
empty-result indicator; reset on tab change/close.

## Important Decisions
- `commands_tab_lines(filter, theme)` owns both the filter logic AND the rendered
  filter prompt line + empty-result indicator (it already receives the filter; keeps
  it a pure, testable builder). `render_help_modal` passes `&ui_state.help_filter`.
- Filter line always rendered: `Filter: <text>` (non-empty) / `Filter: (type to
  narrow commands)` hint (empty). Empty-result line: `No commands match ...`.
- Padding width computed from the FULL catalog so alignment is stable while filtered.
- Filter chars captured in the help-visible key branch ONLY when active tab ==
  Commands; Char(no-CONTROL) -> HelpFilterCharacter, Backspace -> HelpFilterBackspace.
  Nav keys (arrows/Tab) still navigate (they are not Char codes, no conflict).
- Task's integration example says type "doc" — but no catalog command matches "doc"
  (those are CLI flags). Using "goal" (matches /goal, /goal clear; excludes
  /workflow) for the matching case and a non-matching needle for the empty indicator.

## Learnings
- rustfmt collapses/expands the `lines.extend(matches.into_iter().map(...))` closure
  to a specific multi-line shape — let `cargo fmt` decide rather than guessing.
- The old `commands_tab_lines` delegated to `slash_commands::help_command_lines()`;
  filtering needs per-spec access so it now iterates `catalog()` directly and
  reproduces the same `{:usage_width$}  {desc}` format. `help_command_lines()` stays
  (pub, still covered by its own + the slash_command_catalog integration tests).
- Full gate green: fmt clean, raw clippy clean, 738 tests pass / 0 fail.

## Files / Surfaces
- `src/tui/mod.rs`: TuiUiState struct :267 / Default :316; ToggleHelp + nav executor
  arms :577-594; help-visible key branch :888; commands_tab_lines :3520;
  render_help_modal dispatch :3369; TuiCommand enum :164.

## Errors / Corrections

## Ready for Next Run
