---
status: pending
title: Grading executor and FakeRuntime grading phrases
type: backend
complexity: high
dependencies:
  - task_01
  - task_03
  - task_04
---

# Task 05: Grading executor and FakeRuntime grading phrases

## Overview
Implement `run_grading_workflow`, the harness-driven loop that runs the grader (the built-in `reviewer`) to execute the project's checks, derives the verdict, and on FAIL re-dispatches the same editing agent with the concrete failures — bounded by `grading.max_attempts` and exempt from `max_agent_steps`. Add FakeRuntime control phrases so the loop is deterministically testable end-to-end.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `run_grading_workflow(run, producing_agent_id, changed_files) -> GradingOutcome` modeled on `run_council_workflow`, running every sub-step through the action-executing runtime path (`execute_runtime_step_with_actions`).
- MUST dispatch the grader (`reviewer`) to run the project's checks, then derive the verdict (task 03) from that sub-step's recorded command results.
- MUST, on FAIL and while attempts remain, re-dispatch the SAME `producing_agent_id` with the verdict critique appended, then re-grade; break on PASS or SKIP.
- MUST NOT increment `run.step_count` for grade/fix sub-steps (step-budget-exempt); MUST still honor the wall-clock guard and the per-command timeout.
- MUST emit a `grade_round` event per round and a `grader_verdict` event for the decisive check (task 04), and push the concluding result into `run.previous_results`.
- MUST return `GradingOutcome::Escalated` when attempts exhaust on FAIL (handled in task 07) and `Concluded` otherwise.
- MUST add FakeRuntime control phrases that drive a grade `pass` / `fail` / `skip` with a canonical command and a controlled exit code.
</requirements>

## Subtasks
- [ ] 05.1 Add the `GradingOutcome` enum and the `run_grading_workflow` skeleton from the council template.
- [ ] 05.2 Run the grader sub-step via the action-executing path and collect its command results.
- [ ] 05.3 Derive the verdict and emit the round + verdict events.
- [ ] 05.4 On FAIL with attempts remaining, re-dispatch the producing agent with the critique and loop.
- [ ] 05.5 Enforce the `max_attempts` bound and the wall-clock guard without touching `step_count`.
- [ ] 05.6 Add FakeRuntime control phrases for pass/fail/skip grade outcomes.
- [ ] 05.7 Add the end-to-end happy-path and exemption integration tests.

## Implementation Details
Clone the structure of `run_council_workflow` but use `execute_runtime_step_with_actions` (so the grader can actually run commands) and omit the `step_count += 1` / `max_agent_steps` check. See TechSpec "Core Interfaces" (`run_grading_workflow`, `GradingOutcome`) and "Build Order" step 5. The dispatch-loop findings in `_research-techspec.json` give the council template (~:3155-3323), the action-executing call (~:3071), and the step_count seams.

### Relevant Files
- `src/app/mod.rs` — `run_council_workflow` (~:3155) template; `execute_runtime_step_with_actions` (~:3071); `run.step_count` increments (~:3055, :3221); `wall_clock_limit_reached`/`stop_for_wall_clock_limit` (~:3648); `record_event` (~:4164).
- `src/runtime/fake.rs` — add grading control phrases (the fake drives behavior via prompt markers).
- `src/orchestrator/mod.rs` — `derive_grade_verdict` (task 03).
- `src/config/mod.rs` — `config.grading.max_attempts` (task 01).

### Dependent Files
- `src/app/mod.rs` — task 06 invokes this executor from the `run_agent_step` AgentResult arm; task 07 handles the `Escalated` outcome.

### Related ADRs
- [ADR-003: Harness-driven bounded grade→fix loop, step-budget-exempt](../adrs/adr-003.md) — directly implemented.
- [ADR-004: Harness-derived verdict from canonical-check exit codes](../adrs/adr-004.md) — verdict source.

## Deliverables
- `run_grading_workflow` driving grade→fix→re-grade bounded by `max_attempts`, step-budget-exempt.
- FakeRuntime grading control phrases enabling deterministic e2e tests.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the end-to-end loop **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] The loop stops after `max_attempts` FAIL rounds and returns `Escalated`.
  - [ ] A PASS on the first grade returns `Concluded` with no fix dispatch.
  - [ ] A SKIP (no canonical command) returns `Concluded` and records "unverified".
- Integration tests:
  - [ ] FakeRuntime: edit-with-changes → grade FAIL → SAME agent re-dispatched with the critique → grade PASS → run continues.
  - [ ] FakeRuntime: a 2-round grade loop does NOT advance `step_count`, so a run near `max_agent_steps` is not stopped by grading.
  - [ ] FakeRuntime: grader runs no canonical command → SKIP, run continues, "unverified" recorded.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The loop re-dispatches the same agent and converges or escalates within `max_attempts`
- Grade/fix sub-steps never consume the `max_agent_steps` budget
