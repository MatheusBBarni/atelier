# Task Memory: task_03.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Projected the five queue lifecycle history events into visible Chat items so the queue is understandable without reading raw history. Completed 2026-06-11. Builds on [[task_01]] and [[task_02]].

## Important Decisions

- **One evolving Chat item per follow-up id**, not one item per event. Added `ChatLifecycleKey::FollowUp { follow_up_id }` (mod.rs) + its `item_id()` arm (`chat:follow_up:{id}`). `apply_follow_up_lifecycle` upserts under that key, so the 5 events for one follow-up collapse into a single item that transitions Queued→Paused→Resumed→Replaying/Cancelled (last event wins). `upsert` already creates-or-updates, so a standalone event (e.g. only a paused event in a unit test) still produces a visible item.
- **Reused `ChatItemKind::Diagnostic`** (no new ChatItemKind) per the task's SHOULD and the techspec. The `FollowUp` lifecycle key is an internal projection key, not a new item kind.
- Status/severity mapping: queued→Pending/Info; replay_started→Running/Info; replay_paused→Pending/**Warning** (reason in a `ChatLineView::warning("Paused: {reason}")` body line); replay_resumed→Pending/Info ("eligible for replay"); cancelled→**Skipped**/Info. Prompt goes in `summary` (via `concise`).
- Single handler `apply_follow_up_lifecycle` matches all 5 kinds (one `match` on `event.kind`) rather than five tiny handlers.

## Learnings

- The 5 events previously fell through `apply_history_event`'s `_ => {}` no-op (that's why task_01/02 could record them without rendering). This task only ADDED match arms — no existing handler changed, so no projection regressions.
- Because App's `record_event_with_group` does `apply_history_event` → `sync_chat_items`, the queue items now automatically appear in `AppState.chat_items` (live, not just on rebuild). No app-layer change was needed for that.
- Projection test harness: `event(kind, run_id, step_id, payload)` builds a `HistoryEvent` with `event_id = "event-{kind}"`; `item_text(item)` concatenates title+summary+body; assert on `ChatProjection::rebuild(&events).items()`. `concise` collapses whitespace to one line + ellipsizes at MAX_SUMMARY_CHARS.
- `upsert` source-merge: updating an existing keyed item prepends the new event id then merges prior ids, so the final item's `source.event_ids` accumulates all contributing events.

## Files / Surfaces

- `src/app/chat/mod.rs` — `ChatLifecycleKey::FollowUp { follow_up_id }` variant + `item_id()` arm.
- `src/app/chat/projection.rs` — 5-kind arm in `apply_history_event`; `apply_follow_up_lifecycle`; 7 tests (5 unit + 2 integration) at end of `mod tests`.

## Errors / Corrections

- None of substance. fmt reformatted long `json!` lines; clippy clean first try. Full serial suite 480 passed / 0 failed.

## Ready for Next Run

- **task_04 (TUI)**: `AppState.chat_items` now includes Diagnostic-kind queue items (id `chat:follow_up:{id}`, titles "Queued/Replaying/Paused/Resumed/Cancelled follow-up"), so the existing chat renderer already shows them. task_04 should additionally render the dedicated `AppState.queued_follow_ups` list/count and dispatch `AppEvent::FollowUpCancelled(id)` / `FollowUpResumeRequested(id)`, plus help/slash entries for `/queue` + `/q` (task_05). If task_04 wants distinct queue styling vs generic diagnostics, it can branch on the `chat:follow_up:` id prefix or the title, or a dedicated `ChatItemKind::QueuedFollowUp` could be introduced then (not needed yet).
- Not committed by this run (auto-commit not requested); diff staged-ready.
