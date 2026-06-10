# Workflow Memory

Keep only durable, cross-task context here. Do not duplicate facts that are obvious from the repository, PRD documents, or git history.

## Current State

- Tasks 01-05 completed: schema fields, validation + fake fixture, runtime prompt contracts, app-owned `AppState.pending_clarification` with `clarification_requested` lifecycle events, and structured `AppEvent::ClarificationAnswered` answer path.
- Task 05 implementation: `AppEvent::ClarificationAnswered(ClarificationAnswer)` with `question_id`, `answer`, `selected_option_id`, `selected_option_label`, `answer_source` metadata.
- Resolution validates question_id match, rejects empty answers, records enriched `clarification_answered` events, clears pending state, resumes run.
- Normal `submit_prompt` is blocked when clarification is pending — enforces structured answer path over free text.

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
