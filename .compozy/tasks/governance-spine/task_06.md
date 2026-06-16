---
status: pending
title: Orchestrator prompt nudge for turn-one intent
type: backend
complexity: low
dependencies:
  - task_05
---

# Orchestrator prompt nudge for turn-one intent

## Overview
Make the early-abort echo legible by nudging the orchestrator to emit a clear interpreted-goal restatement and concrete approach bullets on the first turn. A small, additive instruction in the orchestrator prompt — no schema change — so the `reason`/`plan` the echo renders are reliably useful.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add an instruction to the orchestrator prompt so that, on the first turn, `reason` carries a one-line interpreted-goal restatement and `plan` carries concrete approach bullets.
- MUST be additive and prompt-only — no new decision/schema fields.
- MUST degrade gracefully: the early-abort never blocks on echo quality; if `reason`/`plan` are thin, the card still renders what exists.
- SHOULD keep the instruction from bloating or destabilizing existing orchestrator behavior on later turns.
</requirements>

## Subtasks
- [ ] 6.1 Add the turn-one interpreted-goal/approach instruction to the orchestrator prompt.
- [ ] 6.2 Confirm it does not regress existing orchestrator tests.
- [ ] 6.3 Verify the early-abort card shows a non-empty intent + at least one approach bullet.

## Implementation Details
Modify `src/orchestrator/mod.rs` `build_orchestrator_prompt` (~672) to append the instruction. Keep it concise and additive. Reference TechSpec "Known Risks" (echo quality is best-effort) and ADR-004.

### Relevant Files
- `src/orchestrator/mod.rs` — `build_orchestrator_prompt` (~672).
- `src/app/mod.rs` — the early-abort gate (task_05) that renders `reason`/`plan` into the card.
- `src/runtime/fake.rs` — deterministic decision content for the assertion.

### Dependent Files
- None.

### Related ADRs
- [ADR-004: Single-agent turn-1 early-abort mechanism](../adrs/adr-004.md) — prompt-nudged echo over a schema change.

## Deliverables
- An additive orchestrator-prompt instruction for turn-one intent/approach.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An integration test that the early-abort card carries a legible intent **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `build_orchestrator_prompt` output contains the turn-one interpreted-goal/approach instruction.
  - [ ] Existing orchestrator-prompt tests still pass (no regression to other sections).
- Integration tests:
  - [ ] An early-abort run (via `FakeRuntime` emitting a `reason`+`plan`) renders a card with a non-empty intent line and at least one approach bullet.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The early-abort echo is reliably legible without any schema change; existing orchestrator behavior is unregressed.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
