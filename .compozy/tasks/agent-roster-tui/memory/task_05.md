# Task Memory: task_05.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Two pure string-lookup helpers in `src/tui/mod.rs`: `activity_glyph(state, ascii)` (Set 1 BMP circles / 7-bit ASCII fallback) and `activity_label(state)` (working/waiting/stalled?/idle). `ActivityState` already existed (task_01) — subtask 05.1 was verify-only.

## Important Decisions

- Followed the spec's by-value `state: ActivityState` signature. `ActivityState` is NOT `Copy`, so task_06 callers will pass `row.activity.clone()` (or task_06 can add `Copy` if it wants — left as task_06's call, not expanded here).
- Both helpers carry `#[allow(dead_code)]` — only tests exercise them until task_06's render rewrite consumes them; without it, `clippy --lib` flags dead code.

## Learnings

- **`colors_live_only_in_theme_module` is a naive substring scan for the literal `Color` `::` token** across `src/tui/mod.rs`. A doc comment that merely *mentions* the literal trips it. Reworded to "no inline color literals" — never write that back-tick literal in tui code/comments.
- Render-snapshot NO_COLOR integration test (subtask 05.6 / §Integration) is DEFERRED to task_06: the render path doesn't call `activity_glyph`/`activity_label` until the task_06 rewrite, so "glyph appears in rendered output" can't be asserted yet. The no-color-literal requirement is already enforced statically by the invariant.

## Files / Surfaces

- `src/tui/mod.rs`: `activity_glyph` + `activity_label` (beside `agent_status_label`), `ActivityState` added to the `use crate::app::{...}` import, 5 unit tests.

## Errors / Corrections

- First full-suite run failed `colors_live_only_in_theme_module` because a doc comment wrote the color-literal token. Fixed by rewording (logic untouched).

## Ready for Next Run

- task_06 (render rewrite, high) now has both the data (`roster_rows`, task_04) and the vocabulary (`activity_glyph`/`activity_label`, this task). It will consume `roster_rows` in the render block, call these helpers, and finally enable the deferred roster render-snapshot tests from tasks 04/05.
