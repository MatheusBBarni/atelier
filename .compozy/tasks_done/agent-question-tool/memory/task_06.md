# Task Memory: task_06.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Task was marked completed in commit 13c226b, but a re-execution audit (2026-06-11) found three requirement gaps. This run remediated them; status remains completed with all gaps closed.

## Important Decisions

- Added `ChatLifecycleKey::Clarification { run_id, question_id }` so clarification items no longer share the `Run { run_id }` slot with run_started/run_completed/blocker items. Both clarification events carry `question_id` in their payloads, so the key is derivable in projection without app changes.
- `apply_orchestrator_decision` now maps decision status `waiting_for_user` to `ChatItemStatus::WaitingForUser` (was `WaitingApproval`) — kind stays `RoutingDecision`.
- Legacy `apply_blocker` (`blocker_reported` events, replay-only path) now projects `Clarification`/`WaitingForUser` instead of `RunSummary`/`WaitingApproval`; its test was updated to guard the new semantics instead of locking in the old.
- Answered clarification body now preserves the question as a muted `Question: …` line, recovered from the pending item via the shared lifecycle key before upsert.

## Learnings

- Any chat item keyed `ChatLifecycleKey::Run` is overwritten by later run lifecycle events (`upsert` fully replaces the item at a colliding key). Pre-fix, every answered clarification was erased by `run_completed` — unit tests masked this by ending event streams at `clarification_answered`. Items that must persist need their own key variant.
- `runtime::codex::tests::codex_availability_reports_login_status` (and a few runtime timeout tests) are timing-flaky under heavy parallel system load; they pass in isolation and on quiet re-runs.
- task_07's commit (9533b32) left 5 clippy warnings and unformatted code in the tree; cleaned up in this run (unused `ClarificationAnswer` import, `field_reassign_with_default` in tests, `or_else`→`or`, repo-wide `cargo fmt`).

## Files / Surfaces

- `src/app/chat/mod.rs` — new lifecycle key variant + `item_id` arm (`chat:clarification:{run_id}:{question_id}`).
- `src/app/chat/projection.rs` — clarification key helper, both clarification appliers, orchestrator-decision status, blocker projection; tests strengthened/added (`answered_clarification_survives_run_lifecycle`, `multiple_clarifications_in_one_run_project_distinct_items`, `orchestrator_decision_waiting_for_user_does_not_use_approval_status`).
- `src/app/mod.rs` — new fake-runtime integration test `clarification_flow_chat_items_never_use_approval_kind`.
- `src/tui/mod.rs` — label distinctness test `clarification_chat_kind_label_is_distinct_from_approval`; clippy cleanups.

## Errors / Corrections

- Original execution marked test-plan items 6 (label distinctness) and 7 (fake-runtime integration) as done without the tests existing. Both now exist and pass.

## Ready for Next Run

- Task_08 (composer layout) can rely on: clarification chat items persist across the full run lifecycle, keyed per question; pending item status is `WaitingForUser`; `WaitingApproval` never appears in clarification flows.
