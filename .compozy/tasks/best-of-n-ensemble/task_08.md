---
status: pending
title: "Judge narration and tie-break step"
type: backend
complexity: medium
dependencies:
  - task_07
---

# Task 08: Judge narration and tie-break step

## Overview
Add the judge: a runtime-independent reviewer step that writes the human-legible "why it won" rationale on every race and breaks the tie only when the oracle could not decide. This delivers the product's shareable verdict while keeping selection authority on objective evidence.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST dispatch a reviewer-profile agent (`Read + Review`, no `Edit`) on a runtime chosen independent of every attempt's runtime; if no independent runtime exists, fall back to the deterministic order and note it.
- MUST always produce a rationale grounded in the oracle evidence for the verdict card.
- MUST break the tie only when `select_winner` returned `decisive == false`, and MUST mark such results `low_confidence`.
- MUST NOT let the judge override an oracle-decided winner.
- MUST record the judge outcome into `RaceResult` (rationale, final winner, `low_confidence`).
</requirements>

## Subtasks
- [ ] 8.1 Build the runtime-independent reviewer dispatch (reuse the council member pattern).
- [ ] 8.2 Generate the rationale grounded in oracle evidence.
- [ ] 8.3 Apply the judge's pick only when `decisive == false`; set `low_confidence`.
- [ ] 8.4 Handle the no-independent-runtime fallback.
- [ ] 8.5 Add tests for narration, tie-break, override-prevention, and fallback.

## Implementation Details
Reuse `council_member_agent`'s per-member runtime/model dispatch to obtain an independent judge (see TechSpec "System Architecture" and ADR-008). The judge consumes the runner's selection result; on `decisive==true` it only narrates, on `decisive==false` it selects among survivors.

### Relevant Files
- `src/app/mod.rs:8602` — `council_member_agent` (independent-runtime dispatch template).
- `src/config/mod.rs:61` — `Capability::{Read, Review}` for the reviewer profile.
- `src/app/mod.rs` (Task 07) — `run_race_workflow` selection result consumed here.

### Dependent Files
- Task 10 — renders the rationale + low-confidence banner on the verdict card.
- Task 13 — wires the no-oracle path UX.

### Related ADRs
- [ADR-008: Deterministic Oracle-Selection with the Judge as Narrator/Tie-Breaker](../adrs/adr-008.md) — the judge boundary.
- [ADR-003: No-oracle banner](../adrs/adr-003.md) — low-confidence disclosure.

## Deliverables
- A runtime-independent judge step producing rationale + tie-break-only winner + `low_confidence`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the judge within a race **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] On `decisive=true`, the judge writes a rationale but the winner is unchanged.
  - [ ] On `decisive=false` (all-pass tie), the judge selects a survivor and the result is `low_confidence=true`.
  - [ ] The judge runtime differs from every attempt's runtime when an independent one is configured.
  - [ ] With no independent runtime, the deterministic order stands and the fallback is recorded.
- Integration tests:
  - [ ] A no-oracle race (all attempts `Skip`) ends with a judge-selected winner flagged low-confidence in `RaceResult`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Every race carries a grounded rationale; the judge decides only genuine ties and never overrides the oracle.
- Self-vote is structurally excluded via runtime independence.
