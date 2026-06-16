---
status: pending
title: Cycle-exhaustion escalation (accept/retry/abort)
type: backend
complexity: medium
dependencies:
  - task_05
  - task_06
---

# Task 07: Cycle-exhaustion escalation (accept/retry/abort)

## Overview
When the grade loop exhausts `max_attempts` and still FAILs, pause the run and ask the user to accept / retry / abort — reusing the existing clarification pause transport (already a multi-option picker) rather than hard-stopping. This is the human checkpoint that guarantees unverified work is never shipped silently.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST, on `GradingOutcome::Escalated`, pause the run as `WaitingForUser` with three options carrying stable ids `accept` / `retry` / `abort` and non-empty labels, reusing the clarification pause transport.
- MUST add a `grade_escalation` marker on the paused clarification carrying the context needed to retry (producing agent id, changed files) so a normal clarification with an option id `retry` is NOT hijacked.
- MUST branch in `resolve_pending_clarification` on `selected_option_id` BEFORE the existing prompt-append, only when the `grade_escalation` marker is present: `accept` → continue the run (re-drive the orchestrator); `abort` → fail the run; `retry` → re-run the grade loop with a fresh attempt budget.
- MUST leave ordinary clarifications (no marker) on the existing append-and-re-drive path, unchanged.
- MUST surface the escalation prompt with the last failing check so the user can decide informed.
</requirements>

## Subtasks
- [ ] 07.1 Add the `grade_escalation` marker to the pending-clarification capture.
- [ ] 07.2 On exhaustion, build the accept/retry/abort pause from the executor.
- [ ] 07.3 Add the marker-gated tri-state branch in `resolve_pending_clarification` before the prompt-append.
- [ ] 07.4 Implement accept (continue), abort (fail), and retry (re-grade) outcomes.
- [ ] 07.5 Ensure normal clarifications are unaffected.
- [ ] 07.6 Cover all three options plus the non-hijack case with integration tests.

## Implementation Details
Mirror the `WaitingForUser` arm to pause and the `Complete`/`Failed` arms for the accept/abort outcomes; the TUI already maps a focused option to `selected_option_id`, so no TUI change is needed. See TechSpec "System Architecture" (Escalation) and "Build Order" step 7. The escalation findings in `_research-techspec.json` give the pause arm (~:1895), `resolve_pending_clarification` (~:1689), and the `selected_option_id` plumbing.

### Relevant Files
- `src/app/mod.rs` — `handle_orchestrator_decision` WaitingForUser arm (~:1895), `resolve_pending_clarification` (~:1689), `PendingClarification` (~:366), `ClarificationAnswer.selected_option_id` (~:250), `Complete`/`Failed` arms (~:1927) as templates.
- `src/runtime/fake.rs` — control phrases (task 05) to drive an exhausting FAIL loop.

### Dependent Files
- `src/app/mod.rs` — depends on task 05's `Escalated` outcome and task 06's `Paused` mapping.

### Related ADRs
- [ADR-001: Externally-grounded auto-verification loop](../adrs/adr-001.md) — escalate-on-exhaustion (never silent-ship).
- [ADR-003: Harness-driven bounded grade→fix loop](../adrs/adr-003.md) — exhaustion is where escalation fires.

## Deliverables
- An accept/retry/abort escalation on cycle exhaustion, reusing the clarification transport with zero TUI changes.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for each escalation outcome **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `resolve_pending_clarification` with `grade_escalation` + `selected_option_id = "abort"` sets the run Failed.
  - [ ] With `selected_option_id = "accept"` the run continues (re-drives) rather than terminating.
  - [ ] A clarification WITHOUT the marker and option id `retry` follows the normal append-and-re-drive path (not hijacked).
- Integration tests:
  - [ ] FakeRuntime: a loop that never passes exhausts `max_attempts`, pauses with accept/retry/abort, and `retry` re-runs the grade loop with a fresh budget.
  - [ ] FakeRuntime: `accept` on exhaustion continues the run with the work recorded as unverified.
  - [ ] FakeRuntime: `abort` on exhaustion fails the run with the last failing check as the reason.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Exhaustion escalates to a user choice instead of a hard stop, with no TUI changes
- Ordinary clarifications are completely unaffected
