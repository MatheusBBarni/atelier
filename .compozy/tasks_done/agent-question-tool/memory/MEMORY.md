# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State

- Tasks 01-08 completed: schema, validation, runtime contracts, app pending state, structured answer path, Chat semantics, TUI key handling, and clarification composer rendering. Feature complete.
- Task 08: composer transforms in place when `pending_clarification` is Some (Cyan-bordered panel: question, option rows with `>` selection + `★ recommended` suffix, always-visible `Custom:` line, Ctrl-C hint, dynamic height, cursor in custom field). Normal/approval rendering byte-identical when not pending.
- Task 05: `AppEvent::ClarificationAnswered` with full answer metadata; validates question_id, rejects empty answers, resumes run.
- Task 06: Added `ChatItemKind::Clarification` and `ChatItemStatus::WaitingForUser`. Projects `clarification_requested` as pending, `clarification_answered` as completed. Remediated 2026-06-11: clarification items now use dedicated `ChatLifecycleKey::Clarification { run_id, question_id }` (they previously shared the `Run` key and were erased by `run_completed`); orchestrator decision `waiting_for_user` and legacy `blocker_reported` no longer project `WaitingApproval`.
- Task 07: TUI state fields `clarification_option_index` and `clarification_custom_answer`; command routing checks pending clarification before dropdowns; Up/Down cycle options, Enter submits with answer_source logic, character input/backspace edit custom answer; Ctrl-C preserved. Remediated 2026-06-11 (test-only): Submit dispatch, approval-routing precedence, and movement-no-event coverage were missing/tautological and are now genuine executor-level tests.

## Shared Decisions

- Runtime prompts and orchestrator guidance state that the app always provides its own custom text answer path, so runtimes must not emit a custom/other/free-text option among the 2-4 recommended options. App/TUI tasks (04-08) must actually provide that custom path.
- `question_id` is app-generated (`new_id()`) when the waiting decision is handled; validated in task 05 answer resolution against `state.pending_clarification.question_id`.
- The pause site records `clarification_requested`; Chat projection routes it (and `clarification_answered`) through dedicated `ChatItemKind::Clarification` handlers. `apply_blocker` remains only for replayed legacy `blocker_reported` events and also projects `Clarification`/`WaitingForUser`.
- Structured answer validation happens before clearing pending state — wrong question_id or empty answer leaves `pending_clarification` intact so retry is possible.

## Shared Learnings

- Chat items keyed `ChatLifecycleKey::Run` are fully overwritten by later run lifecycle events; any item that must persist in the transcript needs its own key variant. Projection unit tests should cover the full event sequence the app actually emits (run_started … run_completed), not truncated streams.

- Z.ai has no embedded decision-schema prompt: it receives orchestrator guidance because `app/mod.rs` sets the orchestrator profile's instructions from `build_orchestrator_prompt`. Changes to shared guidance automatically cover Z.ai.
- Task 07 TUI will dispatch `ClarificationAnswered` events on Enter with selected option or custom text metadata. Test implementations already use structured event path.

## Open Risks

## Handoffs
- None pending — all eight tasks complete.
