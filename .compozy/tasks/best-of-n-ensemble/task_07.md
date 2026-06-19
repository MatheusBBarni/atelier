---
status: pending
title: "Race workflow runner"
type: backend
complexity: critical
dependencies:
  - task_01
  - task_03
  - task_05
  - task_06
---

# Task 07: Race workflow runner

## Overview
The orchestrating runner: compose the roster, spawn N attempts concurrently into isolated `AttemptScope`s, grade each with the existing oracle over its overlay, select the winner deterministically, and append a `RunStepResult::Race` — without yet landing the patch (promotion is Task 09). This is the feature's spine.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `run_race_workflow` modeled on `run_council_workflow`'s lifecycle and `run_parallel_group`'s concurrent spawn/await/cancel.
- MUST compose the roster via `route_roster` (Task 05) capped at `ensemble.max_attempts`.
- MUST spawn each attempt on a distinct runtime in its own `AttemptScope` (Task 03), grade each via `derive_grade_verdict` over the overlay, and call `select_winner` (Task 06).
- MUST populate `RaceResult`, append `RunStepResult::Race`, and emit `race_started` / `ensemble_attempt_verdict` / `race_selected` events plus a ledger record per attempt (Task 04).
- This task MUST stop at winner selection — it MUST NOT apply the winner to the real tree (Task 09) and MUST NOT invoke the judge narration (Task 08).
- MUST advance run state and respect interrupts/limits exactly like council/DAG runners.
</requirements>

## Subtasks
- [ ] 7.1 Add `RaceRunContext` and the runner skeleton wired into the run lifecycle.
- [ ] 7.2 Compose the roster (route_roster) and create per-attempt `AttemptScope`s.
- [ ] 7.3 Spawn attempts concurrently, reusing the parallel-group spawn/await/cancel plumbing.
- [ ] 7.4 Grade each attempt via the oracle over its overlay; collect `RaceAttempt`s.
- [ ] 7.5 Select the winner, build `RaceResult`, append `RunStepResult::Race`, emit events + ledger records.
- [ ] 7.6 Add a fake-runtime control phrase and an end-to-end runner test.

## Implementation Details
Follow the council workflow lifecycle (return-to-`drive_and_replay`, append to `run.previous_results`) and borrow the concurrent spawn from `run_parallel_group`/`prepare_parallel_children` (see TechSpec "System Architecture" and ADR-005). If spawn logic duplicates the parallel path materially, extract a shared helper. Grading reuses the shipped oracle path; do not re-implement check execution.

### Relevant Files
- `src/app/mod.rs:5544` — `run_council_workflow` (lifecycle template, result append at :5710).
- `src/app/mod.rs:3391`,`4698` — `run_parallel_group` / `prepare_parallel_children` (spawn/await/cancel).
- `src/app/mod.rs:6947` — `record_event` for the race events.
- `src/app/mod.rs:714`,`2306` — `RunDriveContext` / `drive_and_replay` integration.
- `src/orchestrator/mod.rs:369` — `derive_grade_verdict` (oracle).
- `src/runtime/fake.rs:228` — add a `"race"` control phrase.

### Dependent Files
- Task 08 (judge), Task 09 (promotion), Task 10 (projection), Task 11 (command), Task 13 (edge UX) — all build on the runner.

### Related ADRs
- [ADR-005: Dedicated run_race_workflow Runner](../adrs/adr-005.md) — runner architecture.
- [ADR-001: Oracle-Selected Pick-One](../adrs/adr-001.md) — grade-then-select flow.

## Deliverables
- `run_race_workflow` + `RaceRunContext`, producing a `RunStepResult::Race` (winner selected, not promoted).
- Race events + per-attempt ledger records.
- A fake-runtime `"race"` marker.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests driving a race end-to-end via the fake runtime **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Roster composition caps at `max_attempts` and uses `route_roster` output.
  - [ ] Each attempt receives a distinct `AttemptScope` (distinct scratch dirs).
  - [ ] A graded attempt produces a `RaceAttempt` carrying its `GraderVerdict`.
  - [ ] An attempt runtime error is recorded as a failed attempt without aborting the race.
- Integration tests:
  - [ ] `submit_prompt("... race ...")` with one passing / one failing fake attempt yields a `RunStepResult::Race` whose winner is the passing attempt; assert on the event log (`race_started`, two `ensemble_attempt_verdict`, `race_selected`) by position.
  - [ ] Two attempts run concurrently (overlapping `node_running`-style markers) before selection.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A `/race`-driven run grades N isolated attempts and selects a winner, emitting events + ledger records.
- The winner is selected but not yet applied to the real tree.
