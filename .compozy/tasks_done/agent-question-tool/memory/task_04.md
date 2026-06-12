# Task Memory: task_04.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Completed: public `PendingClarificationView` on `AppState`, `clarification_requested` lifecycle event, and clearing on interrupt / leave-WaitingForUser.

## Important Decisions

- `question_id` is generated app-side with `new_id()` when the waiting decision is handled (the orchestrator decision has no question id field). Task 05 must match answers against `state.pending_clarification.question_id`.
- The old `blocker_reported` event at the clarification pause site was **replaced** by `clarification_requested` (payload: `question_id`, `question`, `options`, `recommended_option_id`; event-level `run_id`). Chat projection routes `clarification_requested` through the existing `apply_blocker` as a temporary bridge so the "Clarification needed" chat item still renders; task 06 replaces this with dedicated `ChatItemKind::Clarification` semantics. `blocker_reported` routing kept for replay of old sessions.
- Private `PendingClarification { run }` was left unchanged; question-id matching state lives only in the public view.

## Learnings

- `record_event` publishes state, so `state.pending_clarification` is set *before* `record_event` in the WaitingForUser branch.
- `state.pending_clarification` is cleared in three places: `interrupt()` (app/mod.rs ~1191), the runtime-interrupt path (~3814), and the free-text clarification answer path in `submit_prompt` (~837). Task 05's structured answer path must also clear it.
- TUI has six `AppState` test literals needing the new field (src/tui/mod.rs).

## Files / Surfaces

- `src/app/mod.rs` — AppState field, `PendingClarificationView` struct, WaitingForUser decision branch, two interrupt paths, answer path, tests.
- `src/app/chat/projection.rs` — one-line event-routing bridge (line ~77).
- `src/tui/mod.rs` — test fixture literals only.

## Errors / Corrections

None.

## Ready for Next Run

Task 05 can rely on `state.pending_clarification` (question_id, options, recommended_option_id) and must clear it + take private `pending_clarification` on a valid answer.
