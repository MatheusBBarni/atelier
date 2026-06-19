---
status: pending
title: "Deterministic winner selection"
type: backend
complexity: low
dependencies:
  - task_02
---

# Task 06: Deterministic winner selection

## Overview
Select the race winner deterministically from the attempts' oracle verdicts, signalling when the oracle could not decide so the judge (Task 08) can break the tie. Keeping selection on objective evidence is what neutralizes the LLM-judge bias the design warns about.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST implement `select_winner(&[RaceAttempt]) -> Option<(winner_id, ordered_survivors, decisive)>` as a pure function over `GraderVerdict`.
- Attempts whose oracle outcome is `Fail` MUST be excluded before ranking.
- Survivors MUST be ordered by (1) outcome (`Pass` over `Skip`), (2) an objective tiebreak (fewer warnings, then smaller diff), (3) roster order.
- `decisive` MUST be `false` when the ordering cannot separate the top survivors (e.g. all `Skip`, or an objective tie) so the judge decides.
- It MUST return `None` when no attempt passes (all-fail), feeding the Task 13 all-fail path.
</requirements>

## Subtasks
- [ ] 6.1 Implement disqualification of `Fail` attempts.
- [ ] 6.2 Implement the deterministic ordering with the objective tiebreak.
- [ ] 6.3 Compute the `decisive` flag for the no-oracle/tie case.
- [ ] 6.4 Add tests across all-pass, one-pass, all-fail, all-skip, and tie inputs.

## Implementation Details
Pure function over the Task 02 types (see TechSpec "Core Interfaces" and ADR-008). The objective tiebreak source (warnings/diff size) must be a defined field on `RaceAttempt`/`GraderVerdict`; if absent, fall back to roster order and mark `decisive=false`.

### Relevant Files
- `src/orchestrator/mod.rs:369` — `GraderVerdict`/`GradeOutcome` consumed for ranking.
- `src/orchestrator/mod.rs` (Task 02) — `RaceAttempt`.

### Dependent Files
- Task 07 — calls `select_winner` after grading.
- Task 08 — invoked only when `decisive == false`.
- Task 13 — handles the `None` (all-fail) result.

### Related ADRs
- [ADR-008: Deterministic Oracle-Selection with the Judge as Narrator/Tie-Breaker](../adrs/adr-008.md) — the selection contract.
- [ADR-001: Oracle-Selected Pick-One](../adrs/adr-001.md) — oracle picks, judge narrates.

## Deliverables
- A pure `select_winner` returning winner, ordered survivors, and the `decisive` flag.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests exercising selection within the runner path **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] One `Pass` among `Fail`s selects that attempt with `decisive=true`.
  - [ ] All-`Pass` with equal objective metrics returns `decisive=false`.
  - [ ] All attempts `Fail` returns `None`.
  - [ ] All attempts `Skip` (no oracle) returns top-by-roster-order with `decisive=false`.
  - [ ] Objective tiebreak orders fewer-warnings ahead of more-warnings among `Pass`es.
- Integration tests:
  - [ ] Within a runner test, a one-pass race promotes the passing attempt deterministically.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Selection is deterministic and excludes failures; ties are flagged for the judge.
- All-fail returns `None`.
