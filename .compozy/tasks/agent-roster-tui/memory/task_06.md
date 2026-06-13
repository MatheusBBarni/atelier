# Task Memory: task_06.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Rewrote the Ctrl-L roster render to consume `state.roster_rows` (not `state.agents`): summary-header census line, per-row activity glyph + label + accented name + weight, current-step/elapsed on active rows, animated active glyph, terminal-status preservation. Accent now follows canonical identity (`row.accent_index`).

## Important Decisions

- **Only the roster needed accent repointing.** Chat (`item_agent_accent`) and the `/agent:` dropdown already resolve accent by canonical identity — they look the agent up by `id`/`title` in `state.agents` (canonical order) and use `accent_for(that index)`. So the `agent_dropdown_ids_carry_same_accents_as_roster` contract test was left unchanged; only `roster_names_carry_same_accents_as_chat` was updated to a pinned-reorder fixture (NeedsInput agent at canonical index 1 pinned to row 0 → still `accent_for(1)`).
- **Idle names keep their accent (with `Modifier::DIM`), never `text_dim`.** ADR-005 requires the name to carry the identity color so the roster links to the chat transcript; the contract test asserts `title_cell_fg(name) == accent_for(index)`. "Recede" for idle is the DIM modifier + no glyph/bold, not a color swap.
- **Availability dropped from the roster.** `RosterRow` carries no availability field (ADR-003 view-model), so the rewrite shows activity instead. The old availability/`down` assertions were removed from the render tests. FOLLOW-UP: if availability still needs surfacing, add it to the view-model or another panel.
- **Animated active glyph:** `ROSTER_ACTIVE_SPINNER = ["◐","◓","◑","◒"]` indexed by `ui_state.work_spinner_frame`; frame 0 is `◐` so a single static render matches `activity_glyph(Active, _)` and stays deterministic.
- **`ascii` mode is unwired in render** (no `TerminalCaps` flag for it) — render always uses unicode; NO_COLOR uses unicode glyphs (color is separate from glyph). The helper's ascii path stays test-only.

## Learnings

- The renderer reads `roster_rows`, so any full-frame render test must populate it. Added test helper `populate_roster_rows(&mut state)` calling the real `build_roster_rows`; **made `build_roster_rows` and `StepTiming` `pub(crate)`** in `src/app/mod.rs` so tui tests can call it.
- Summary-header strings are matched literally by snapshot tests — keep `▶ N working · ◔ N waiting · ○ N stalled` and `● N agents idle` exact.

## Files / Surfaces

- `src/tui/mod.rs`: `agent_roster_items` (rewritten to take `&[RosterRow]`, `spinner_frame`), new `roster_summary_header_item` + `roster_row_item` + `ROSTER_ACTIVE_SPINNER`; call site at the roster panel; removed `#[allow(dead_code)]` from `activity_glyph`/`activity_label`; replaced 3 old `AgentView`-based roster tests with `roster_rows`-based + snapshot tests; updated contract test.
- `src/app/mod.rs`: `build_roster_rows` + `StepTiming` now `pub(crate)`.

## Errors / Corrections

- First pass dimmed idle names with `text_dim` → broke the accent contract. Fixed to `accent_for(index)` + `Modifier::DIM`.

## Ready for Next Run

- task_07 (accent-by-identity consistency + strengthened contract tests). Most of the accent-by-identity work already landed here; task_07 should strengthen/extend the cross-surface (roster/chat/dropdown) contract tests. **Open item it may want:** explicit per-field ellipsis truncation of long model/step labels in the sidebar — currently the List widget clips gracefully (no breakage) but without an explicit `…`.
