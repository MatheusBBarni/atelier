---
status: pending
title: "Add Structured Clarification Answer Path"
type: backend
complexity: medium
dependencies:
  - task_04
---

# Task 05: Add Structured Clarification Answer Path

## Overview
Add the dedicated app event and resume path for structured clarification answers. This task replaces the implicit "next prompt is the answer" behavior for the select UI while preserving existing run resume semantics.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `AppEvent::ClarificationAnswered` with structured answer metadata.
- MUST accept recommended-option answers and custom-text answers.
- MUST record `clarification_answered` with question id, answer text, selected option metadata, and `answer_source`.
- MUST reject answers whose question id does not match the active pending clarification.
- MUST preserve slash-prefixed custom answers as valid clarification text.
- MUST clear pending clarification state and resume the paused run after a valid answer.
- MUST keep normal prompt submission blocked while a run is waiting for clarification.
- MUST NOT implement skip as a separate V1 answer outcome.
</requirements>

## Subtasks
- [ ] 5.1 Add the structured clarification answer type and app event.
- [ ] 5.2 Resolve recommended-option answers against the active pending clarification.
- [ ] 5.3 Resolve custom-text answers without slash-command interpretation.
- [ ] 5.4 Record enriched `clarification_answered` history payloads.
- [ ] 5.5 Resume the paused run and clear pending clarification state.
- [ ] 5.6 Add app tests for valid, invalid, selected, and custom answer paths.

## Implementation Details
Use TechSpec sections "App Answer Path", "AppEvent", and "History Events". Preserve the existing behavior that clarification text is appended to the run prompt as user clarification after the answer is accepted.

### Relevant Files
- `src/app/mod.rs` — Owns `AppEvent`, `handle_event`, clarification resume behavior, `submit_prompt`, and app tests.
- `src/history/mod.rs` — Persists enriched answer events through generic JSONL history.
- `src/runtime/fake.rs` — Provides deterministic full flow for app tests.

### Dependent Files
- `src/tui/mod.rs` — Task 07 dispatches `ClarificationAnswered` from key handling.
- `src/app/chat/projection.rs` — Task 06 projects enriched `clarification_answered` events.
- `src/app/chat/mod.rs` — Task 06 adds status/kind variants used by answered projection.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires a dedicated answer event and structured answer metadata.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — Requires answer or interrupt only, with no skip action.

## Deliverables
- Structured clarification answer event and app handler.
- Recommended-option answer support.
- Custom-text answer support.
- Enriched `clarification_answered` history payload.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- App integration tests for pause, answer, resume, and completion **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Recommended option answer records `answer_source = "recommended"` and selected option id/label.
  - [ ] Custom answer records `answer_source = "custom"` with no selected option id.
  - [ ] Answer with the wrong question id is rejected without clearing pending state.
  - [ ] Empty custom answer is rejected with a useful diagnostic.
  - [ ] Slash-prefixed custom answer such as `/tmp/project` is accepted as text.
- Integration tests:
  - [ ] Fake runtime `needs clarification` flow resumes and completes after selected option answer.
  - [ ] Fake runtime `needs clarification` flow resumes and completes after custom answer.
  - [ ] Pending approval still rejects normal prompts and does not route through clarification answer handling.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Structured answers resume waiting runs reliably.
- Session History can distinguish recommended-option answers from custom text.
- V1 still has no skip action.
