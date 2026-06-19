---
status: pending
title: /security-review command wiring and active-run guard
type: backend
complexity: medium
dependencies:
  - task_05
---

# Task 06: /security-review command wiring and active-run guard

## Overview
Expose the review workflow to users: add the `/security-review` entry to the slash-command catalog and a dispatch branch in `submit_prompt_with_source` that awaits `run_security_review_workflow`. Guard the command so it declines cleanly when an agent run is already active, leaving the user's input otherwise free (ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `SlashCommandSpec` entry for `/security-review` (kind `AppCommand`) to the catalog in `src/slash_commands.rs`, keeping the dropdown/help/unknown-command metadata aligned.
- MUST add a dispatch branch in `submit_prompt_with_source` (`src/app/mod.rs`) that matches `/security-review` and awaits `run_security_review_workflow` (task_05).
- MUST reject malformed invocations (e.g. trailing args in V1) with a clear usage message and not silently treat them as a prompt.
- MUST guard against concurrent runs: if `RunState` is `Planning`/`Running`/`WaitingForUser`, decline with a clear message and do NOT dispatch.
- MUST record the decline as a visible diagnostic rather than failing silently.

## Subtasks
- [ ] 6.1 Add the `/security-review` catalog entry with label/usage/description.
- [ ] 6.2 Add the dispatch branch in `submit_prompt_with_source` that awaits the workflow.
- [ ] 6.3 Implement the active-run guard and the clear decline message/diagnostic.
- [ ] 6.4 Reject malformed/extra-argument invocations with a usage message.
- [ ] 6.5 Add integration tests for dispatch, the guard, and malformed input.

## Implementation Details
Add the catalog entry in `src/slash_commands.rs` (the frozen `CATALOG`, ~49-141) — a recorded exception per ADR-001. Add the dispatch branch in the async `submit_prompt_with_source` (`src/app/mod.rs:1925`); because the review is an async runtime workflow it is awaited directly there rather than via the synchronous `handle_*_command` helpers (consistent with how council is awaited). See TechSpec "Command Surface" and "System Architecture → Command surface", and ADR-005 (active-run guard, own flow). The workflow itself lives in task_05; this task only wires invocation + guard.

### Relevant Files
- `src/slash_commands.rs` — `SlashCommandSpec`/`SlashCommandKind` and the `CATALOG` (~49-141).
- `src/app/mod.rs` — `submit_prompt_with_source` (~1925) dispatch chain; `RunState` access.

### Dependent Files
- `src/tui/mod.rs` — task_07 surfaces the command in the dropdown/help.
- `src/app/mod.rs` (`run_security_review_workflow`) — invoked by this branch (task_05).

### Related ADRs
- [ADR-005: Refuse to start while a run is active; own flow](../adrs/adr-005.md)
- [ADR-001: `/security-review` catalog exception](../adrs/adr-001.md)

## Deliverables
- `/security-review` catalog entry and dropdown/help metadata.
- Dispatch branch in `submit_prompt_with_source` awaiting the workflow.
- Active-run guard with a clear decline diagnostic.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for invocation + guard **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] The catalog contains a `/security-review` `AppCommand` entry with non-empty usage/description.
  - [ ] A malformed invocation (`/security-review extra`) is rejected with a usage message, not treated as a prompt.
- Integration tests (`#[tokio::test]` through `FakeRuntime`):
  - [ ] `submit_prompt("/security-review")` while idle dispatches the workflow (a `security_review_started` event is recorded).
  - [ ] `/security-review` while a run is active (`RunState::Running`) records a decline diagnostic and emits NO `security_review_started` event.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/security-review` invokes the workflow when idle and declines cleanly when a run is active.
- Catalog, dropdown, and help metadata stay aligned.
