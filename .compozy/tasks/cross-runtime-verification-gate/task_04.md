---
status: pending
title: Producer provenance and session family-set
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 04: Producer provenance and session family-set

## Overview
Record each producing step's runtime and model family on the `agent_step_started` event, and accumulate the session-level producer-family set that `/review` must diversify against. Today the event carries only `{agent}` and the set has no source; recording lineage as an immutable fact is the load-bearing primitive (ADR-001/003/005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extend the `agent_step_started` payload from `{agent}` to `{agent, runtime, family}` at every emit site, deriving family via the task_01 resolver.
- MUST accumulate a session-level producer-family set: the union of families of Edit-capable steps in the current (or resumed) session.
- MUST reconstruct the producer-family set from history on session resume by reading the persisted `family` field.
- MUST treat a step lacking a recorded family (legacy history or hand-edit) as `unknown` and NOT add a concrete family for it (permissive, per ADR-003).
- MUST update existing tests that assert the `agent_step_started` payload shape.
</requirements>

## Subtasks
- [ ] 04.1 Derive runtime/family at each `agent_step_started` emit and add them to the payload.
- [ ] 04.2 Add a session producer-family-set accumulator updated when an Edit-capable step completes.
- [ ] 04.3 Reconstruct the set from history on session resume.
- [ ] 04.4 Treat missing family as `unknown` / permissive (do not exclude a guessed family).
- [ ] 04.5 Update payload-asserting tests and add set-computation tests.

## Implementation Details
Modify `src/app/mod.rs` at the `agent_step_started` emit sites (`:3466, :5272, :5420, :5611, :5841, :13434`), deriving family via `agent_family` (task_01). Hold the producer-family set on the run/session state. See TechSpec "System Architecture → Producer-set tracker" and "Data Models → Provenance fields".

### Relevant Files
- `src/app/mod.rs` — `agent_step_started` emit sites, `record_event`, run/session state.
- `src/runtime/status.rs` — `agent_family` (task_01).
- `src/history/mod.rs` — `HistoryEvent` payload and the resume read path.

### Dependent Files
- `src/review/mod.rs` (task_05) — reviewer selection reads the producer-family set.
- Existing `src/app/mod.rs` tests asserting `agent_step_started` payloads.

### Related ADRs
- [ADR-001: Record lineage as an immutable fact](../adrs/adr-001.md) — provenance on the event, not inferred from config.
- [ADR-003: Independence over the producer-family set; `unknown` is permissive](../adrs/adr-003.md).
- [ADR-005: Session-level producer set; provenance recorded on the step event](../adrs/adr-005.md).

## Deliverables
- Extended `agent_step_started` payload (`runtime`, `family`) at all emit sites.
- A session producer-family-set accumulator, reconstructable on resume.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for provenance recording and resume reconstruction **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] The producer-family set equals the union of families of Edit-capable steps; a non-Edit step does not contribute.
  - [ ] A step with no recorded family leaves the set unchanged (permissive `unknown`).
  - [ ] Family on the payload is derived from the producing agent's runtime via `agent_family`.
- Integration tests:
  - [ ] A run with producing steps on two different `RuntimeKind`s records both `family` values on `agent_step_started`.
  - [ ] A session resumed from history reconstructs the producer-family set from the persisted `family` fields.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `agent_step_started` carries `{agent, runtime, family}` at every emit site
- The session producer-family set is correct live and after resume, with missing family treated as permissive `unknown`
