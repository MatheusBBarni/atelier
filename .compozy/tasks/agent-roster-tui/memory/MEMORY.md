# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State

- task_01 complete: `ActivityState`, `RosterRow`, and `AppState.roster_rows` exist in `src/app/mod.rs`.
- task_02 complete: `StepTiming` map + lifecycle stamping was already implemented; tests added this run (8 tests in `src/app/mod.rs`). `started_at`/`last_activity` are ready.
- task_03 complete: `build_roster_rows` (+ `classify_step`, `step_display_label`, `format_coarse_elapsed`) and `STALL_THRESHOLD` const now exist in `src/app/mod.rs`. `rebuild_roster_rows` is wired into `publish_state`, which is now `&mut self` and rebuilds `roster_rows` on every publish (unconditionally — task_04 adds the change-gate).
- task_04 complete: `refresh_roster_tick(&mut self) -> bool` (active-run gate + `roster_rows == rebuilt` change-gate via new `send_state()`), a 4th 1 Hz `select!` arm in the `tui` app-worker loop (`ROSTER_REFRESH_INTERVAL`, Skip missed ticks → `app.refresh_roster_tick()`), and a terminal-status `step_timings` clear added to `set_live_step_status`. Render-snapshot integration tests deferred to task_06 (renderer still reads `state.agents`).

## Shared Decisions

- `RosterRow.roster_rows` uses `#[serde(default)]` so older durable history records deserialize with an empty roster (no migration needed).
- `publish_state` is now `&mut self` (it rebuilds `roster_rows`). All callers were already `&mut self`, so no blast radius.
- Activity classification (task_03): `WaitingForApproval`/`WaitingForAction` → `NeedsInput`; `Starting`/`Running`/`Streaming`/`Cancelling` → `Active` (or `Stalled` if `now - last_activity >= 30s`); `Completed`/`Failed`/`Interrupted`/no-step → `Idle`. `RosterRow.status` always carries `agent.status`; activity-driven label is via `activity`. `accent_index` is canonical-order, assigned before the NeedsInput pin-sort.
- Sub-minute elapsed renders as `"8s"` (then `"1m 20s"`, `"1h 5m"`).
- **Renderer still reads `state.agents` (tui/mod.rs:2473), NOT `roster_rows`** until task_06 rewrites the render block. Roster render-snapshot integration tests belong to task_06/07, not earlier.

## Shared Learnings

- task_02 (StepTiming map + lifecycle stamping) is ALREADY implemented in `src/app/mod.rs` even though `_tasks.md` marks it pending: `struct StepTiming` (~L234), `step_timings: BTreeMap<String, StepTiming>` on `App`, stamped/bumped/cleared in `set_active_step_with_metadata`, `push_live_stream_content`, `set_live_step_status`, and step-clear. Verify rather than re-implement before starting task_02.
- `src/tui/mod.rs` already has an unrelated `RosterRowStyle` enum — distinct from the new `RosterRow` view-model.
- FIXED in task_04 run: the 2 `actions::tests::unrestricted_reads_*` failures were NOT a sandbox quirk — `fixture_agent` loads the user's home config, which now has `[workspace] allow_unrestricted_reads = true`, so the "without flag" denial assertions failed. Made hermetic by pinning `config.workspace.allow_unrestricted_reads = false` in both tests. `cargo test --lib` is now fully green (771 passed). (Committed separately from task_04 — out of task scope.)
- `src/tui/mod.rs` carries unrelated uncommitted formatting WIP, so crate-wide `cargo fmt --check` fails on it. Scope fmt to your own file with `rustfmt --edition 2021 --check <file>`.

## Open Risks

## Handoffs
