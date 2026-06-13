# Task Memory: task_05.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Add three live/dynamic tab builders in `src/tui/mod.rs`: `getting_started_lines`,
`commands_tab_lines`, `skills_tab_lines`. Consumed by task_06; staged `#[allow(dead_code)]`.

## Important Decisions
- `getting_started_lines -> Vec<Line>` but `agent_roster_items -> Vec<ListItem>`; ratatui 0.29
  ListItem content is not publicly readable, so a `Vec<Line>` builder cannot consume
  `agent_roster_items` output. Resolved by extracting `agent_compact_line(index, &AgentView,
  theme) -> Line` as the single compact-row definition; `agent_roster_items` Compact arm now
  wraps it in a `ListItem`, and Getting Started calls it directly. Single data path preserved
  (the requirement's intent); literal call to `agent_roster_items` with `Compact` is not made
  because of the type mismatch. Note in PR.
- `commands_tab_lines(_filter, _theme)`: filter is a no-op underscore param in MVP (Phase-2
  filter lands in task_09); rows come straight from `slash_commands::help_command_lines()` and
  stay plain `Line::from(String)` to match today's render. `_theme` unused (plain rows).
- Two Getting Started example prompts (Open Question in PRD): chosen as runnable, read-first
  prompts; marked with a `> ` prefix so tests can count them.

## Learnings
- `SkillSuggestion` fields used: `alias`, `source_tag` (`.label()` → "Project"/"Personal").
- Compact roster row was `name · runtime/model · availability` with `theme.accent_for(index)`.

## Files / Surfaces
- `src/tui/mod.rs`: new `agent_compact_line`, `getting_started_lines`, `commands_tab_lines`,
  `skills_tab_lines`; Compact arm of `agent_roster_items` refactored; new unit tests.

## Errors / Corrections
- Raw `cargo clippy` (not `rtk`) surfaced a pre-existing dead-code warning on the `impl HelpTab`
  block (`ALL`/`title`/`next`/`prev`): task_01's `#[allow(dead_code)]` sat on the enum only, not
  the impl. Added `#[allow(dead_code)]` to the impl block (same feature, in-scope). `rtk cargo
  clippy` had been filtering this warning, so earlier tasks reported "clean".
- Pre-existing fmt failure on `HelpTab::next`/`prev` (long `.position()` line, Open Risk in
  shared MEMORY) fixed opportunistically — I was already editing that exact impl block.

## Ready for Next Run
- task_06 wires all three builders + the static task_04 builders into the tabbed
  `render_help_modal`. `getting_started_lines`/`commands_tab_lines`/`skills_tab_lines` are pure
  and `#[allow(dead_code)]`-staged; remove the allow when task_06 consumes them.
- Verified: fmt clean, clippy clean (raw, not just rtk), 710 lib tests pass.
