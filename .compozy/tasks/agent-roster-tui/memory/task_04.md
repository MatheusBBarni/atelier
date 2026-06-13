# Task Memory: task_04.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Wire the 1 Hz gated roster refresh. `rebuild_roster_rows`/`publish_state` integration was already done by task_03, so the real work was `refresh_roster_tick`, the 4th `select!` arm, the terminal-status timing clear, and unit tests.

## Important Decisions

- `refresh_roster_tick` change-gate reuses `RosterRow`'s derived `PartialEq`: `self.state.roster_rows == rows` already compares activity + coarse-elapsed bucket (elapsed is pre-formatted into the row), satisfying req #4 without a bespoke comparator.
- Split `publish_state` into `rebuild_roster_rows` + new `send_state()` so `refresh_roster_tick` can publish its already-built rows without a redundant second rebuild.
- The active-run gate is inlined as `matches!(run_state, Planning | Running)` in `refresh_roster_tick` (kept in sync with `tui::work_indicator_active`) — app must not depend on tui, so it can't call that fn directly.

## Learnings

- `set_live_step_status` did NOT clear `step_timings` on terminal status (only bumped activity on Running/Streaming). Subtask 4.5 required adding the terminal clear — it was real work, not just verification.
- Render-integration snapshot tests (task_04 spec §"Integration Tests", subtask 4.7) are DEFERRED to task_06: the renderer still reads `state.agents`, not `roster_rows`, until task_06 rewrites the render block. Confirmed against the standing decision in shared MEMORY.md.

## Files / Surfaces

- `src/app/mod.rs`: `publish_state` (+ `send_state`), `refresh_roster_tick`, `set_live_step_status` (terminal clear), 5 new unit tests.
- `src/tui/mod.rs`: `ROSTER_REFRESH_INTERVAL` const + `roster_poll` (1 Hz, Skip missed ticks) + 4th `select!` arm calling `app.refresh_roster_tick()`.

## Errors / Corrections

- The full `cargo test --lib` was red on two PRE-EXISTING `actions::tests::unrestricted_reads_*` tests: `fixture_agent` loads the user's home config, which now has `[workspace] allow_unrestricted_reads = true`, so the "without flag" denial assertions failed. Fixed by pinning `config.workspace.allow_unrestricted_reads = false` in those two tests (hermetic baseline). Committed separately from task_04 since `src/actions/mod.rs` is out of task scope.

## Ready for Next Run

- task_05 (`activity_glyph`/`activity_label` helpers) is independent (depends only on task_01) and unblocked. task_06 (render rewrite) is what finally makes the roster render-snapshot tests possible.
