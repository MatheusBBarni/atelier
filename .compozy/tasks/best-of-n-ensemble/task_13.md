---
status: pending
title: "All-fail and no-oracle UX wiring"
type: backend
complexity: medium
dependencies:
  - task_08
  - task_09
  - task_10
---

# Task 13: All-fail and no-oracle UX wiring

## Overview
Wire the two edge flows end-to-end across runner, judge, promotion, and projection: when every attempt fails the oracle, promote nothing and let the user retry or abort; when no test can discriminate, auto-pick the judge's top with a low-confidence banner. These are the safety-and-honesty paths the design promised.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- When `select_winner` returns `None` (all-fail), the race MUST promote nothing, surface each attempt's failures, and offer a retry/abort choice; it MUST emit `race_all_failed`.
- When the oracle cannot discriminate (no-oracle / all-tie), the race MUST auto-pick the judge's top and render the low-confidence banner, then proceed to the standard approval gate.
- The retry path MUST re-enter the race (optionally adjusting roster/N) without leaving stale scratch dirs.
- The abort path MUST end the run cleanly with the attempts' evidence preserved in the transcript.
- These flows MUST reuse the existing runner, judge, promotion, and projection pieces — no new parallel path.
</requirements>

## Subtasks
- [ ] 13.1 Wire the all-fail branch: no promotion, surface failures, `race_all_failed`.
- [ ] 13.2 Implement the retry/abort choice and the retry re-entry (clean scratch).
- [ ] 13.3 Wire the no-oracle branch: judge auto-pick + low-confidence banner → approval gate.
- [ ] 13.4 Add end-to-end tests for both edge flows.

## Implementation Details
This task integrates Task 07 (runner result), Task 08 (judge/low-confidence), Task 09 (promotion bypass), and Task 10 (banner/failed rendering) — see TechSpec "User Experience" steps 4–7. Reuse the existing escalation/resume routing for retry/abort rather than inventing a new pause type.

### Relevant Files
- `src/app/mod.rs` (Task 07) — runner outcome branching for all-fail/no-oracle.
- `src/app/mod.rs` (Task 08) — judge low-confidence selection.
- `src/actions/mod.rs` (Task 09) — promotion bypass on all-fail.
- `src/app/chat/projection.rs` (Task 10) — banner + failed-item rendering.

### Dependent Files
- None downstream; this closes the V1 behavior set.

### Related ADRs
- [ADR-003: No-oracle auto-pick + banner; all-fail surface/retry](../adrs/adr-003.md) — the edge-flow decisions.
- [ADR-008: Judge tie-break low-confidence](../adrs/adr-008.md) — no-oracle selection.

## Deliverables
- End-to-end all-fail (surface + retry/abort) and no-oracle (auto-pick + banner) flows.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for both edge flows via the fake runtime **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] All attempts failing yields `RaceResult.status = AllFailed`, no `race_promoted`, and a `race_all_failed` event.
  - [ ] The retry path re-enters the race and leaves no stale scratch dir from the prior attempt set.
  - [ ] The abort path ends the run with attempt evidence retained.
  - [ ] A no-oracle race sets `low_confidence=true` and routes the auto-picked winner to the approval gate.
- Integration tests:
  - [ ] `submit_prompt` driving an all-fail race (fake attempts all failing) surfaces failures and the retry/abort choice; choosing abort ends cleanly.
  - [ ] A no-oracle race renders the low-confidence banner and still reaches the approval modal.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- All-fail promotes nothing and lets the user retry/abort; no-oracle auto-picks with an honest banner.
- Both flows reuse the existing pieces with no scratch leaks.
