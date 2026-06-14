# Task Memory: task_01.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Done. Defined `ActivityState` enum + `RosterRow` struct in `src/app/mod.rs`, added `roster_rows: Vec<RosterRow>` to `AppState`, initialized in all constructors/fixtures, with serde + watch round-trip tests.

## Important Decisions

- Placed `ActivityState`/`RosterRow` immediately above `AppState` (after `AgentView`); Rust item order doesn't matter, kept them next to identity/liveness view-models per ADR-003.
- `roster_rows` field marked `#[serde(default)]` so durable history records deserialized before this field land empty (matches `show_first_approval_explainer` convention).
- Integration test reads `receiver.borrow_and_update()` after `publish_state()` (latest watch value) — deterministic, no `changed()` timing dependency.

## Learnings

- task_02 (StepTiming map + lifecycle stamping) is ALREADY implemented in `src/app/mod.rs` (struct at ~234, `step_timings` field + stamping at set_active_step/push_live_stream_content/set_live_step_status/clear), despite `_tasks.md` marking it pending. Verify before starting task_02.
- `AppState` literal fixtures needing the new field: 8 literals in `src/tui/mod.rs` (4673,4716,4744,4966,5060,5094,7176,7283) + the `new_with_debug` constructor. Others (`state_with_agent_roster`, `state_with_queue`, worker_state @6694) use `..state_with_input(..)` spread and are covered transitively.
- `tui/mod.rs` already has an unrelated `RosterRowStyle` enum — do not confuse with the new `RosterRow`.

## Errors / Corrections

- 2 pre-existing failures in `src/actions/mod.rs` (`unrestricted_reads_flag_*`) are environment-sensitive (host FS path resolution), unrelated to this change. app:: 221 pass, tui:: 255 pass.

## Ready for Next Run

task_03 (build_roster_rows) can consume `ActivityState`/`RosterRow` now. task_02 likely a no-op/verify-only — confirm StepTiming wiring already present.
