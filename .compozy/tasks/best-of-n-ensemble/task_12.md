---
status: pending
title: "Win-rate read-back in provider status"
type: backend
complexity: medium
dependencies:
  - task_04
  - task_05
---

# Task 12: Win-rate read-back in provider status

## Overview
Make the learning visible: surface accumulated per-runtime win-rates by task type in `/provider:status`, with a "still learning" state below the routing threshold. This is the V1 payoff of the router-led framing — the user sees the fleet's track record in their own codebase.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a win-rate-by-task-type section to `/provider:status` sourced from the verdict ledger aggregate (Task 04).
- Below `min_route_samples` for a signature, it MUST show a "still learning" state rather than a misleadingly precise rate.
- Win-rates MUST be labeled as historical, workspace-local, and sample-sized (not guarantees).
- MUST handle the empty-ledger (no races yet) case with a clear "no data yet" message.
- MUST NOT perform routing here — it only displays the same aggregate the router (Task 05) consumes.
</requirements>

## Subtasks
- [ ] 12.1 Read the ledger aggregate and group by task signature + runtime.
- [ ] 12.2 Render the win-rate section in `/provider:status`.
- [ ] 12.3 Show the "still learning" state below threshold and "no data yet" when empty.
- [ ] 12.4 Add tests for populated, below-threshold, and empty cases.

## Implementation Details
Reuse the Task 04 aggregate API (see TechSpec "Command & Config Surface"); render alongside the existing provider runway/usage output. Keep the display read-only — no side effects on the ledger.

### Relevant Files
- `src/app/mod.rs:2235` — `/provider:status` command handler to extend.
- `src/race/` (Task 04) — ledger aggregate API.
- `src/config/mod.rs` (Task 01) — `min_route_samples` for the threshold label.

### Dependent Files
- None downstream; this is a leaf display task.

### Related ADRs
- [ADR-004: Router read-back / cold-start "learning" state](../adrs/adr-004.md) — the display semantics.
- [ADR-007: Verdict ledger as the source](../adrs/adr-007.md) — where the data comes from.

## Deliverables
- A win-rate-by-task-type section in `/provider:status` with learning/empty states.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the rendered read-back **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A signature with samples above threshold renders its per-runtime win-rate.
  - [ ] A signature below `min_route_samples` renders the "still learning" state.
  - [ ] An empty ledger renders the "no data yet" message.
  - [ ] Win-rates are labeled historical/workspace-local (the disclaimer text is present).
- Integration tests:
  - [ ] After a race writes verdicts, `/provider:status` shows the corresponding win-rate row.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The read-back reflects ledger data with honest learning/empty states.
- No routing or ledger mutation occurs in the display path.
