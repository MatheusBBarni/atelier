---
status: pending
title: "Roster routing from win-rate history"
type: backend
complexity: medium
dependencies:
  - task_04
---

# Task 05: Roster routing from win-rate history

## Overview
Choose which runtimes enter a race for a given task type by ordering the configured roster on accumulated win-rate, falling back to the default roster on cold start. This is the active, bounded routing that makes the learning loop real in V1 without ever skipping the race.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST implement `route_roster(sig, ledger_aggregate, default_roster, min_samples, n)` as a pure function returning an ordered roster of size ≤ `n`.
- Below `min_route_samples` for a signature, it MUST return the configured default roster (cold start).
- At/above the threshold, it MUST order roster members by win-rate for that signature, breaking ties deterministically.
- It MUST only select competitors — it MUST NOT reduce `n` to skip racing or route a single model (that is V2, per ADR-004).
- It MUST be a pure function of its inputs (no I/O), so it is trivially testable and safe to fall back.
</requirements>

## Subtasks
- [ ] 5.1 Implement the pure `route_roster` selector.
- [ ] 5.2 Apply the cold-start threshold and default-roster fallback.
- [ ] 5.3 Implement deterministic tie-breaking among equal win-rates.
- [ ] 5.4 Add tests covering cold start, warm ordering, unknown signature, and tie-breaks.

## Implementation Details
Place alongside the ledger/race logic (see TechSpec "Core Interfaces" for the `route_roster` signature). Consume Task 04's aggregate API; keep this layer free of persistence so it is a deterministic function over `(signature, aggregate, default_roster, min_samples, n)`.

### Relevant Files
- `src/race/` (Task 04 module) — ledger aggregate API consumed here.
- `src/config/mod.rs` (Task 01) — `EnsembleConfig.default_preset` / `min_route_samples` / `max_attempts`.
- `src/orchestrator/mod.rs` (Task 02) — `TaskSignature`.

### Dependent Files
- Task 07 — calls `route_roster` to compose the attempt roster before spawning.
- Task 12 — shares the win-rate aggregate surfaced in the read-back.

### Related ADRs
- [ADR-004: Route the Race Roster, Never Skip the Race](../adrs/adr-004.md) — the bounded routing contract.
- [ADR-007: Verdict Ledger + Signature](../adrs/adr-007.md) — the data the router reads.

## Deliverables
- A pure `route_roster` function with cold-start fallback and deterministic ordering.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests pairing the ledger and router **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Signature with 0 samples returns the default roster unchanged (cold start).
  - [ ] Signature with samples below `min_samples` returns the default roster.
  - [ ] Signature at/above threshold orders members by descending win-rate.
  - [ ] Equal win-rates break ties deterministically (stable order).
  - [ ] Returned roster length never exceeds `n` and never drops below the race minimum.
- Integration tests:
  - [ ] Verdicts written by the ledger (Task 04) drive `route_roster` to a history-ordered roster in a later call.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Routing reorders the roster by real history above threshold and falls back safely below it.
- The function never skips the race or routes a single model.
