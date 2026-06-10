# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State

- Tasks 01-06 completed: schema, validation, runtime contracts, app pending state, structured answer path, and Chat semantics.
- Task 05: `AppEvent::ClarificationAnswered` with full answer metadata; validates question_id, rejects empty answers, resumes run.
- Task 06: Added `ChatItemKind::Clarification` and `ChatItemStatus::WaitingForUser` to distinguish from Approval. Projects `clarification_requested` as pending Clarification item and `clarification_answered` as completed state. Kept `blocker_reported` using RunSummary/WaitingApproval for backward compatibility.

## Shared Decisions

- Runtime prompts and orchestrator guidance state that the app always provides its own custom text answer path, so runtimes must not emit a custom/other/free-text option among the 2-4 recommended options. App/TUI tasks (04-08) must actually provide that custom path.
- `question_id` is app-generated (`new_id()`) when the waiting decision is handled; validated in task 05 answer resolution against `state.pending_clarification.question_id`.
- The pause site records `clarification_requested`. Chat projection temporarily routes through `apply_blocker`; task 06 must replace with dedicated `ChatItemKind::Clarification` semantics.
- Structured answer validation happens before clearing pending state — wrong question_id or empty answer leaves `pending_clarification` intact so retry is possible.

## Shared Learnings

- Z.ai has no embedded decision-schema prompt: it receives orchestrator guidance because `app/mod.rs` sets the orchestrator profile's instructions from `build_orchestrator_prompt`. Changes to shared guidance automatically cover Z.ai.
- Task 07 TUI will dispatch `ClarificationAnswered` events on Enter with selected option or custom text metadata. Test implementations already use structured event path.

## Open Risks

## Handoffs
- Task 06: Chat projection must add `ChatItemKind::Clarification` and handle `clarification_answered` events distinctly from approvals.
- Task 07: TUI key handling dispatches `ClarificationAnswered` events with structured metadata on Enter and Up/Down option navigation.
