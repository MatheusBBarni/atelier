# Task Memory: task_02.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Added the App-owned queue lifecycle: FIFO replay after clean completed Runs, pause after non-clean endings, cancel before replay, and resume of paused items. New `AppEvent::FollowUpCancelled` / `FollowUpResumeRequested`. Replay drives queued prompts through the normal run-creation path. Completed 2026-06-11. Builds on [[task_01]].

## Important Decisions

- **Single drain loop, called only after a real run.** `drive_and_replay(run, resume)` = `drive_run(...)` then `react_to_run_end_for_queue()`. The 4 run-driving sites (`submit_prompt` normal path, `start_subtask`, `resolve_pending_approval` main path, `resolve_pending_clarification`) now call `drive_and_replay`. `/queue`, cancel, and the "no pending approval" early-return do NOT drain, so queueing/cancelling never triggers a spurious replay.
- **`react_to_run_end_for_queue` is one loop**: while `can_replay_now()` (run_state==Completed && active_run_id==None && no pending approval/clarification) → pop oldest Pending, build a normal run, `drive_run`; if the run ended Failed/Interrupted/LimitReached/WaitingForUser → `pause_oldest_pending_for_queue()` and break. Chaining across completions happens inside this loop (each replay's clean completion replays the next), but only ONE item per completion — preserving the one-active-Run invariant (each `drive_run` is awaited sequentially).
- **Replay = pop, not mark-Replaying.** `pop_oldest_pending_for_replay` removes the item from the `VecDeque` (it becomes a real Run) and records `follow_up_replay_started`. The `QueuedFollowUpStatus::Replaying` variant is therefore currently unconstructed (reserved for task_04 if it wants to surface an in-flight replay; pub enum, so no dead-code warning). This avoided fragile mark-then-remove bookkeeping across WaitingForUser→resume cycles.
- **Resume safety**: `resume_follow_up` only flips Paused→Pending + records `follow_up_replay_resumed`. handle_event then calls `react_to_run_end_for_queue` ONLY when `can_replay_now()` is already true — so resume can replay against an already-completed state but can never hit the pause branch and re-pause the item it just resumed.
- **Pause is synchronous where there is no async run boundary**: `interrupt()` (sync) and the 3 limit early-returns in `resolve_pending_approval` call `pause_oldest_pending_for_queue()` directly (it only records an event, no await).
- **Approval/clarification-waiting pauses the queue and it stays paused even after the run later completes cleanly** (paused items are not Pending, so the post-completion replay loop skips them). Per ADR-001/ADR-003, paused items require explicit resume/cancel. This is intended UX, not a bug.
- **`first_pending_index` = first item with status Pending**, scanning from the front. Paused/Cancelled items do NOT block Pending items behind them (replay/pause select the oldest *pending*). Cancelled items are kept in the queue with `Cancelled` status (not removed) so the view can show them; this is bounded by user actions per session.

## Learnings

- `run_app_worker` (`src/tui/mod.rs:467`) processes `handle_event` strictly serially, and `drive_run` runs synchronously to completion. So in the real TUI a `/queue` typed *during* a run is buffered and only enters the queue AFTER that run completes-and-drains — i.e. it replays after the NEXT completion, not the current one. Realistic/tested usage is queue-then-run. Surfacing "queue during active run → replay after THIS run" is a TUI-dispatch concern, not app-layer (out of scope here).
- Existing clarification/approval/limit/interrupt tests call `submit_prompt` directly and pass unchanged because the drain no-ops on an empty queue.
- Fake-runtime triggers used by tests: clean = `"create a feature"`; clarification = `"needs clarification create a feature"` (WaitingForUser); approval = `"approval action create a feature"` + `approval_mode="normal"` (agent `fixer`); Failed = `"always parse error create a feature"`; LimitReached = `max_agent_steps = 1` + any prompt. Replayed prompts must avoid trigger substrings ("parse error", "needs clarification", "approval action", "parallel", "design", "typo", etc.).

## Files / Surfaces

- `src/app/mod.rs` — `AppEvent::{FollowUpCancelled, FollowUpResumeRequested}` + handle_event arms; `drive_and_replay`, `can_replay_now`, `react_to_run_end_for_queue`, `first_pending_index`, `pop_oldest_pending_for_replay`, `pause_oldest_pending_for_queue`, `queue_pause_reason`, `build_follow_up_run`, `cancel_follow_up`, `resume_follow_up`; replaced `drive_run`→`drive_and_replay` at 4 sites; sync pause in `interrupt` + 3 approval limit early-returns; 10 new tests + `queue_via_event`/`replay_started_prompts`/`approval_mode_config`/`single_agent_step_limit_config` test helpers.

## Errors / Corrections

- fmt reformatted two test assertions (multi-line vec / `.any` chain) — fixed with `cargo fmt`. clippy clean first try; full serial suite 473 passed / 0 failed.

## Ready for Next Run

- **task_03 (Chat projection)**: add handlers in `src/app/chat/projection.rs::apply_history_event` for `follow_up_queued`, `follow_up_replay_started`, `follow_up_replay_paused`, `follow_up_replay_resumed`, `follow_up_cancelled` (all currently hit the `_ => {}` no-op). Payload fields available: `id`, `prompt`, `status`, and (paused) `pause_reason`. Decide diagnostic vs prompt-adjacent Chat item kind.
- **task_04 (TUI)**: render `AppState.queued_follow_ups` (id/prompt/created_at/status/pause_reason already published via the watch sender); dispatch `AppEvent::FollowUpCancelled(id)` / `FollowUpResumeRequested(id)`; add `/queue`+`/q` to help/slash suggestions (task_05). The `Replaying` status is currently never set — if task_04 wants an in-flight badge, derive it from `active_run_id` + the last `follow_up_replay_started` event, or switch `pop_oldest_pending_for_replay` to mark-Replaying + add removal on completion.
- Not committed (auto-commit not requested); diff staged-ready.
