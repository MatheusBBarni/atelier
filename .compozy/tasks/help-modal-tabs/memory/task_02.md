# Task Memory: task_02.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Extracted the inline Ctrl-L roster loop into a shared `agent_roster_items(agents, style, theme)`
builder with `Full` (3 lines/agent) and `Compact` (1 line) arms. Ctrl-L roster now calls it with
`Full`; output byte-for-byte unchanged. `Compact` awaits its task_05 (Getting Started) consumer.

## Important Decisions
- Builder returns `Vec<ListItem<'static>>`; the `Full` status label needs `.to_string()` because
  `agent_status_label` borrows from `agent.status` (not `'static`). `availability_label` is already
  `&'static str` and `format!` strings are owned, so the rest is `'static` for free.
- Kept `#[allow(dead_code)]` on `RosterRowStyle` — `Compact` is only matched (not constructed) in
  production until task_05, so per-variant dead_code would warn otherwise. Doc comment updated.

## Learnings
- Real line numbers drift from the task spec (loop was at ~`:2196`, not `:2120`; helpers ~`:3442+`).
  Locate by symbol, not line.
- Tests assert on `ListItem` content via `item.height()` for line count, and a render-to-buffer
  helper (`roster_items_to_text`) for visible text — `ListItem` content is not publicly readable.

## Files / Surfaces
- `src/tui/mod.rs`: `agent_roster_items` added just before `work_indicator_active`; roster loop in
  `render` replaced with one `Full` call; tests added after `roster_row_style_variants_are_distinct`.

## Errors / Corrections
- `rustfmt --edition 2021 --check` flags task_01's `HelpTab::next/prev` (pre-existing, committed);
  not my lines — left untouched per scope-to-own-changes. My additions format clean.

## Ready for Next Run
- task_05 consumes `agent_roster_items(.., Compact, ..)` for Getting Started; drop the
  `allow(dead_code)` on `RosterRowStyle` when that lands.
