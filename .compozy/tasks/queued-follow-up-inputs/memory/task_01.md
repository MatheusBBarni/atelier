# Task Memory: task_01.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Added the App-owned queued follow-up data model and `/queue <message>` + `/q <message>` parsing in `App::submit_prompt`, before unknown slash-command rejection. Queue commands store a `Pending` item and do NOT start a Run. Replay/pause/cancel/resume, Chat projection, and TUI rendering are intentionally out of scope (tasks 02–04). Completed 2026-06-11.

## Important Decisions

- View type `QueuedFollowUpView` + `QueuedFollowUpStatus { Pending, Paused, Replaying, Cancelled }` are `pub` (snake_case serde) and live next to `LiveStepStatus`. Internal `QueuedFollowUp` struct (private) holds the same fields with `new()`/`to_view()`; internal storage is `VecDeque<QueuedFollowUp>` on `App` (FIFO via `push_back`). The view `Vec<QueuedFollowUpView>` on `AppState` is rebuilt by `sync_queued_follow_ups()`.
- Parser `parse_queue_command(&str) -> Result<Option<String>>` splits the trimmed prompt on first whitespace into (command, rest); only `/queue` and `/q` match; empty rest bails `usage: /queue <message> or /q <message>`; non-slash / non-queue / `/queueextra` return `Ok(None)`. This cleanly disambiguates `/q` from plain `q` and from `/queue`-prefixed tokens.
- `handle_queue_command` is wired in `submit_prompt` immediately AFTER `handle_subtask_command` and BEFORE the `WaitingForUser` bails and `reject_unknown_slash_command`, so queueing works from any run state and never starts a Run.
- Records a `follow_up_queued` history event (id, prompt, created_at, status) via the existing `record_event`. The Chat projection's `apply_history_event` has a catch-all `_ => {}`, so this is a no-op in Chat for now — task_03 will add the projection handler. This keeps observability + the `state.events` feedback line without leaking Chat-projection scope into task_01.
- Did NOT add the `AppEvent::FollowUpCancelled` / `FollowUpResumeRequested` variants from the techspec — those belong to task_02 (lifecycle). Task_01 reuses the existing `PromptSubmitted` -> `submit_prompt` path.

## Learnings

- Adding a field to `AppState` breaks 6 struct-literal initializers in `src/tui/mod.rs` test helpers (`chat_items: Vec::new(), <field>, events: ...`). `cargo check` (non-test) passes without them because they are `#[cfg(test)]`; run `cargo test`/`clippy --tests` to catch. The `worker_state` (`..state_with_input(...)`) and `state_with_agent_roster` helpers reuse `state_with_input`, so they need no change.
- `runtime::codex` / `runtime::cursor` unit tests are FLAKY under parallel `cargo test` (they shell out to real `codex`/`cursor` CLIs with timing/timeout/child-spawn assertions). Failure count is non-deterministic (saw 4 then 5). Verify with `cargo test -- --test-threads=1` (serial = 454 lib / 0 failed, 463 all-targets / 0 failed). Baseline-with-changes-stashed serial = 448/0. Not caused by app/tui changes.

## Files / Surfaces

- `src/app/mod.rs` — `QueuedFollowUpView` + `QueuedFollowUpStatus` (near `LiveStepStatus`); internal `QueuedFollowUp` struct/impl (near `RunDriveContext`); `queued_follow_ups` on `AppState` (+init); `follow_up_queue: VecDeque<QueuedFollowUp>` on `App` (+`VecDeque::new()` init); `handle_queue_command` (after `handle_config_command`); `sync_queued_follow_ups` (next to `sync_chat_items`); `parse_queue_command` (above `reject_unknown_slash_command`); 6 tests in `mod tests`.
- `src/tui/mod.rs` — added `queued_follow_ups: Vec::new()` to 6 test-only `AppState` literals.

## Errors / Corrections

- Initial `cargo test` showed `missing field queued_follow_ups` in 6 `src/tui/mod.rs` test helpers — fixed by adding the field. Then `cargo fmt --check` flagged one multi-line assertion — fixed with `cargo fmt`. clippy clean throughout.

## Ready for Next Run

- task_02 (replay/pause/cancel/resume): mutate `App::follow_up_queue` item `status`/`pause_reason`, then call `sync_queued_follow_ups()` to refresh the view; add the `follow_up_replay_started|paused|resumed|cancelled` history events; add the `AppEvent` queue-control variants. Replay must start at most one `Pending` item after a clean `RunState::Completed` and reuse the normal Run-creation path in `submit_prompt`.
- task_03 (Chat projection): add `follow_up_queued` (and the task_02 event kinds) to `apply_history_event` in `src/app/chat/projection.rs` (currently no-op via `_ => {}`).
- task_04 (TUI): render `AppState.queued_follow_ups`; the field is already published through the existing `watch::Sender<AppState>`.
- Not committed by this run (auto-commit was not requested); diff is staged-ready for manual review.
