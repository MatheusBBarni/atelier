# Task Memory: task_02.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

StepTiming map + lifecycle stamping. Production code was ALREADY implemented in a
prior run (see shared MEMORY). This run only added the missing tests.

## Important Decisions

- Tests live in the `src/app/mod.rs` `#[cfg(test)] mod tests` block (same module),
  so they read the private `step_timings` field directly — no `#[cfg(test)]`
  accessor was needed, keeping the field strictly private per requirement 7.
- Backdating pattern: after `set_active_step`, mutate the entry to
  `Instant::now() - Duration::from_secs(10)` to make bumps observably later than
  `started_at`. Mirrors the existing offset idiom (e.g. limit tests).
- Serialization-leak test asserts `serde_json::to_string(app.state())` contains
  none of `step_timings` / `last_activity` / `started_at`.

## Learnings

- 8 tests added: stamp-on-register, bump-on-stream (started_at unchanged),
  bump-on-active-status, NO-bump-on-terminal/waiting-status, clear-on-end,
  parallel-independence, multi-step lifecycle (integration-style through app
  layer), and no-serialization-leak.
- `set_active_step(run, step, agent)` is a thin wrapper over
  `set_active_step_with_metadata`; use the metadata form with `Some(group_id)`
  for parallel-group fixtures.

## Files / Surfaces

- `src/app/mod.rs` — added timing tests at end of test module (before final `}`).

## Errors / Corrections

- `cargo fmt --check -- <file>` still checks the whole crate; the pre-existing
  unformatted `src/tui/mod.rs` (parallel WIP, not mine) trips it. Use
  `rustfmt --edition 2021 --check src/app/mod.rs` to scope fmt to my file.
- Full `cargo test --lib` shows 2 pre-existing failures in
  `actions::tests::unrestricted_reads_flag_*` (sandbox absolute-read path
  resolution, environmental). Confirmed via stash A/B — NOT caused by this task.

## Ready for Next Run

- task_02 fully done (code + tests). Next: task_03 (elapsed + stall detection)
  consumes `started_at`/`last_activity` and `STALL_THRESHOLD`.
