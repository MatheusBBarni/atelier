# Task Memory: task_06.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Rewrite `render_help_modal` into a tabbed overlay: new signature
`(frame, state: &AppState, ui_state: &TuiUiState, theme: &Theme)`, theme-token tab
strip with active highlight, dispatch on `ui_state.help_active_tab` to the per-tab
builders (task_04/05). Default tab GettingStarted renders on open. Update breaking tests.

## Important Decisions
- Tab strip is a plain `Line` of `HelpTab::ALL` titles (active = accent + BOLD/UNDERLINED,
  inactive = text_muted), then a blank line, then the active builder's `Vec<Line>`.
  No `ratatui::Tabs` (ADR-003). Outer block keeps title " Help " + " Esc " + accent
  border so `help_and_clarification_borders_differ_from_input_composer_token` (box_corner_fg
  on "Help") stays green.
- `commands_tab_lines("", theme)` called with empty filter (no-op until task_09).

## Learnings
- `renders_help_modal_commands` asserts content from THREE tabs (Commands + Keys + CLI) in
  one render — tabbed render shows one tab at a time, so the test now renders once per tab
  and routes each assertion group to its tab. All original assertions preserved.
- `help_modal_command_rows_are_catalog_derived`: catalog loop runs on Commands tab; the
  "non-command rows survive" trio (Ctrl-L/Arrow keys/Mouse wheel) now asserted on Keys tab.

## Files / Surfaces
- `src/tui/mod.rs`: `render_help_modal` (rewrite), call sites (clarification branch + main),
  six builders' `#[allow(dead_code)]` removed, tests.

## Errors / Corrections
- DEVIATION 1: task names 3 breaking tests, but `readme_workflow_command_wording_matches_v1_limits`
  also asserts `/workflow <prompt>` from help_text with `help_visible:true` → also breaks.
  Updated it to select Commands tab (same contract-preserving fix).
- DEVIATION 2: `help_modal_suppresses_command_dropdown_rendering` asserted
  `!text.contains("Commands")`. The new tab strip legitimately renders the "Commands" tab
  TITLE, so the literal assertion can't stay "unchanged". Preserved the test's INTENT
  (command dropdown suppressed) by asserting the dropdown's unique right-aligned hint
  ("Up/Down Tab/Enter") is absent instead. Dropdown suppression is still verified.

## Ready for Next Run
- task_07 adds key nav (Arrows/Tab → HelpNextTab/HelpPrevTab) that drives `help_active_tab`
  this render reads. `HelpTab::next/prev` still `#[allow(dead_code)]` (impl-level) until then.
