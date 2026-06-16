---
status: pending
title: Grading trigger gate at run_agent_step
type: backend
complexity: medium
dependencies:
  - task_05
---

# Task 06: Grading trigger gate at run_agent_step

## Overview
Wire the grading executor into the run loop: after a top-level single-agent Edit step reports `Completed` with changed files and grading is enabled, invoke `run_grading_workflow`. This is what makes verification automatic (not orchestrator-discretionary) while scoping it tightly to avoid grading subtasks, parallel children, or the grader/fixer sub-steps themselves.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST invoke `run_grading_workflow` from the `AgentResult` arm of `run_agent_step`, gated on ALL of: `config.grading.enabled`, the producing agent has `Capability::Edit`, `result.changed_files` is non-empty, and the step is a top-level single-agent step (not a subtask, not a parallel child).
- MUST compute the gate inputs (capability + changed files) BEFORE the result is moved into `run.previous_results`.
- MUST map `GradingOutcome::Escalated` to `AgentStepOutcome::Paused` and `Concluded` to `AgentStepOutcome::Completed` so the existing loop semantics hold.
- MUST NOT trigger grading for the grader or the re-dispatched fixer sub-steps (no recursion), nor when grading is disabled.
- MUST leave behavior unchanged when `config.grading.enabled` is false.
</requirements>

## Subtasks
- [ ] 06.1 Compute the grade gate (enabled + Edit capability + changed files + top-level single-agent) before the result push.
- [ ] 06.2 Invoke `run_grading_workflow` when the gate passes.
- [ ] 06.3 Map the executor outcome to `Completed`/`Paused`.
- [ ] 06.4 Confirm sub-steps and subtasks bypass the gate.
- [ ] 06.5 Cover trigger / no-trigger paths with integration tests.

## Implementation Details
The gate lives in the `run_agent_step` `AgentResult` arm; re-fetch the producing profile via `self.agent(next_agent_id)` (it was moved into the request) and read `result.changed_files` before the push. See TechSpec "System Architecture" (Grading trigger) and "Build Order" step 6. The dispatch-loop findings in `_research-techspec.json` flag the move-at-:3092 and profile-out-of-scope gotchas.

### Relevant Files
- `src/app/mod.rs` — `run_agent_step` `AgentResult` arm (~:3083-3092); `self.agent` profile lookup (~:4066); `AgentProfile::has_capability` (`src/config/mod.rs:368`); `AgentStepOutcome` (~:765).
- `src/config/mod.rs` — `config.grading.enabled` (task 01).

### Dependent Files
- `src/app/mod.rs` — task 07's escalation handling depends on the `Paused` mapping established here.

### Related ADRs
- [ADR-003: Harness-driven bounded grade→fix loop](../adrs/adr-003.md) — the trigger scope and no-recursion property.
- [ADR-002: Agent-discovered verification in V1](../adrs/adr-002.md) — V1 grades Edit-producing single-agent steps.

## Deliverables
- An automatic grading trigger gated to top-level single-agent Edit steps, off by default.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for trigger and no-trigger paths **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] The gate is false when `grading.enabled` is false (no executor call).
  - [ ] The gate is false when the producing agent lacks `Capability::Edit`.
  - [ ] The gate is false when `changed_files` is empty.
- Integration tests:
  - [ ] FakeRuntime: an enabled, Edit-producing single-agent step triggers a grade loop.
  - [ ] FakeRuntime: a read-only (no-Edit) step or a no-change step does NOT trigger grading.
  - [ ] FakeRuntime: grader/fixer sub-steps inside the loop do not recursively re-trigger grading.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Grading fires automatically on qualifying edits and never on disqualified steps
- Default-off behavior is fully preserved
