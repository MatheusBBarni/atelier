---
status: pending
title: "Validate Clarification Options And Fake Runtime Fixture"
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 02: Validate Clarification Options And Fake Runtime Fixture

## Overview
Add the validation rules that make structured clarifying questions reliable and update the fake runtime to exercise the new contract deterministically. This task turns the schema from task 01 into a safe contract for app and TUI work.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST validate that every `waiting_for_user` decision has 2-4 clarification options.
- MUST reject blank option ids, blank labels, duplicate option ids, and invalid recommended option ids.
- MUST preserve existing terminal decision validation that disallows `next_agent` or `next_step`.
- MUST update fake runtime `needs clarification` behavior to return deterministic options and a recommended option id.
- MUST NOT require options for non-`waiting_for_user` decisions.
- SHOULD keep validation errors specific enough to diagnose bad runtime output.
</requirements>

## Subtasks
- [ ] 2.1 Add option-count validation for waiting clarification decisions.
- [ ] 2.2 Add option identity, label, and uniqueness validation.
- [ ] 2.3 Add recommended-option validation.
- [ ] 2.4 Update fake runtime clarification fixture with deterministic options.
- [ ] 2.5 Expand validation and fake runtime tests.

## Implementation Details
Use the TechSpec "Data Models" validation rules. This task depends on task 01's schema fields and should not add app state or TUI behavior.

### Relevant Files
- `src/orchestrator/mod.rs` — Contains `validate_orchestrator_decision` and tests for accepted/rejected decisions.
- `src/runtime/fake.rs` — Provides deterministic fake `WaitingForUser` behavior used by app tests.
- `src/app/mod.rs` — Existing fake-runtime app tests depend on the `needs clarification` scenario.
- `.compozy/tasks/agent-question-tool/_techspec.md` — Lists the exact validation rules.

### Dependent Files
- `src/app/mod.rs` — Task 04 depends on validated options being available when a run pauses.
- `src/runtime/codex.rs` — Runtime prompt task should align examples with validation rules.
- `src/runtime/claude.rs` — Runtime prompt task should align examples with validation rules.
- `src/runtime/cursor.rs` — Runtime prompt task should align examples with validation rules.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires structured options and validation before app/TUI integration.

## Deliverables
- Validation rules for structured clarification options.
- Fake runtime clarification branch with deterministic option set.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration-oriented fake runtime coverage for downstream app tests **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Waiting decision with exactly two valid options is accepted.
  - [ ] Waiting decision with exactly four valid options is accepted.
  - [ ] Waiting decision with zero, one, or five options is rejected.
  - [ ] Waiting decision with blank option id is rejected.
  - [ ] Waiting decision with blank option label is rejected.
  - [ ] Waiting decision with duplicate option ids is rejected.
  - [ ] Waiting decision with a recommended option id not present in options is rejected.
  - [ ] Continue and complete decisions do not require clarification options.
- Integration tests:
  - [ ] Fake runtime prompt containing `needs clarification` produces `WaitingForUser` with deterministic options and a recommended id.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Invalid clarification option payloads fail before app state is updated.
- Fake runtime can drive the full V1 clarification flow in later tasks.
