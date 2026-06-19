---
status: pending
title: "Race command and slash catalog entry"
type: backend
complexity: medium
dependencies:
  - task_07
---

# Task 11: Race command and slash catalog entry

## Overview
Expose the feature to users: dispatch `/race <instruction>` from the prompt path, register it in the frozen-V1 slash catalog (with the required ADR amendment), announce the cost at start, and guard the unsupported cases (feature disabled, fewer than two runtimes).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST dispatch `/race <instruction>` in the prompt-handling path (like `/workflow`/`/subtask`), opening a race run via `run_race_workflow`.
- MUST add a `SlashCommandSpec` for `/race` AND amend the frozen-V1 catalog: update the CATALOG docstring with a dated scope-change note, add `"/race"` to `FIXED_V1_LABELS`, and update its length so the drift test passes.
- MUST reject empty input and, when `features.ensemble == false` or fewer than two runtimes are configured, return a clear message instead of silently degrading.
- MUST print a start line announcing N, the roster runtimes, and an estimated cost/latency multiplier before the race runs.
- MUST surface the command in the help overlay/`--doctor` consistently with the catalog.
</requirements>

## Subtasks
- [ ] 11.1 Add the `/race` dispatch branch in the prompt path.
- [ ] 11.2 Register the `SlashCommandSpec` + amend the frozen-V1 ADR note and `FIXED_V1_LABELS`.
- [ ] 11.3 Implement the empty-input, disabled-feature, and thin-fleet guards.
- [ ] 11.4 Emit the cost-announcing start line.
- [ ] 11.5 Add tests for dispatch, catalog drift, and the guards.

## Implementation Details
Follow the `/workflow` user-invoked command pattern (see TechSpec "Command & Config Surface"). The catalog is governed by a freeze ADR — adding `/race` requires the documented amendment, not just a new row (the `catalog_labels_are_exactly_the_fixed_v1_set` test enforces exact match).

### Relevant Files
- `src/slash_commands.rs:37` — CATALOG const + freeze docstring to amend.
- `src/slash_commands.rs:172` — `FIXED_V1_LABELS` + `catalog_labels_are_exactly_the_fixed_v1_set` test.
- `src/app/mod.rs` — `submit_prompt` dispatch (where `/workflow`/`/subtask` are handled).
- `src/config/mod.rs` (Task 01) — `features.ensemble`; runtime count for the thin-fleet guard.

### Dependent Files
- `src/app/mod.rs` (Task 07) — `run_race_workflow` invoked by the dispatch.
- Help overlay / `--doctor` rendering — reflect the new command.

### Related ADRs
- [ADR-003: Opt-in /race, announce-don't-block cost](../adrs/adr-003.md) — invocation + cost UX.
- [ADR-005: Race runner](../adrs/adr-005.md) — what the command drives.

## Deliverables
- `/race` dispatch + catalog entry with the freeze-ADR amendment.
- Cost-announcing start line + unsupported-case guards.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for end-to-end command dispatch **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `/race` with empty instruction returns a clear usage error.
  - [ ] With `features.ensemble=false`, `/race` returns a disabled message and runs no attempts.
  - [ ] With one configured runtime, `/race` refuses with a thin-fleet message.
  - [ ] `catalog_labels_are_exactly_the_fixed_v1_set` passes with `/race` added (length + array updated).
- Integration tests:
  - [ ] `submit_prompt("/race <instruction>")` with the feature enabled starts a race run and prints the cost-announcing start line.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/race` dispatches a race, announces cost, and guards unsupported cases.
- The frozen-V1 catalog test passes with the documented amendment.
