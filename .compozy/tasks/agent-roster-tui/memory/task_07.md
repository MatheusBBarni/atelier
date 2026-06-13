# Task Memory: task_07.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Accent-by-identity consistency across the three surfaces + strengthened contract tests. Most of the substance landed in task_06 (roster → `accent_index`; chat/dropdown already canonical). This task documented the single-source rule at all three sites, recorded the fourth-surface risk, and strengthened the dropdown contract test to a pinned-reorder fixture.

## Important Decisions

- No code-path changes were needed for chat/dropdown — they were already accent-by-identity (canonical `id`/`title` lookup into `state.agents`). Subtasks 7.3/7.4 reduced to verify + comment.
- Picked the **comment** option for the fourth-surface risk (7.7), not a CI invariant: an `accent_for(` substring-scan would be brittle (the helper is legitimately used at multiple sites + tests). The canonical doc block lives on `item_agent_accent`; the roster and dropdown sites cross-reference it.
- Strengthened `agent_dropdown_ids_carry_same_accents_as_roster` to render **both** the pinned roster and the `/agent:` dropdown in one frame (roster_visible: true), asserting fixer = `accent_for(1)` in both the lowercase dropdown id and the capitalized roster name even though fixer is pinned to row 0. Distinguishes by case in `title_cell_fg`.

## Learnings

- The accent-persists-under-pin **unit** test already existed from task_03: `needs_input_pin_preserves_canonical_accent_index` (`src/app/mod.rs`). No new unit test needed.
- Clippy `doc_lazy_continuation`: a markdown numbered list in a doc comment needs a blank `///` line before the following paragraph, or it lints. Watch this in long doc comments.
- The full `cargo test --lib` flaked once (6 failures) under load (20.99s run vs ~12s) — timing-sensitive tests. Re-ran clean (782). Treat slow-run failures of timing tests as load flakes, not regressions.

## Files / Surfaces

- `src/tui/mod.rs` only: single-source-rule doc block on `item_agent_accent`; cross-ref comments at `roster_row_item` and the `/agent:` dropdown accent lookup; strengthened dropdown contract test.

## Errors / Corrections

- Doc comment numbered list tripped clippy `doc_lazy_continuation`; fixed by adding blank `///` separators around the list.

## Ready for Next Run

- agent-roster-tui tasks 01–07 are all complete. The feature (live-activity-first roster with stall detection, glyph+label vocabulary, summary header, accent-by-identity) is fully implemented and committed.
