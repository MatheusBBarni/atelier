# Task Memory: task_01.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Added the foundational `HelpTab` enum (6 variants, `ALL`/`title`/`next`/`prev`) and
`RosterRowStyle { Full, Compact }` in `src/tui/mod.rs`. Pure value types, no render/state. Done.

## Important Decisions
- Both types annotated `#[allow(dead_code)]` because they have no non-test consumer until
  tasks 02–06. Documented in doc-comments as intentionally-staged. The crate has no
  `deny(warnings)`, but the lib build would otherwise warn on dead code.
- `next`/`prev` implemented via `ALL.iter().position(...)` + modular arithmetic over `ALL.len()`
  rather than hardcoded match arms — single source of order is `ALL`.

## Learnings
- Test module is at `src/tui/mod.rs:4099` (`mod tests`, `use super::*`). Types placed just
  before `TuiCommand` enum (now ~`:91`).
- `colors_live_only_in_theme_module` guard test still passes (no color literals introduced).

## Files / Surfaces
- `src/tui/mod.rs` — added `HelpTab`, `RosterRowStyle`, `impl HelpTab`, and 6 unit tests.

## Errors / Corrections
- None.

## Ready for Next Run
- Task 02 consumes `RosterRowStyle` via `agent_roster_items`; that wiring removes the
  `#[allow(dead_code)]` on `RosterRowStyle`. `HelpTab`'s allow stays until task 03+ wire it in.
