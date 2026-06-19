---
status: completed
title: "Append near-miss did-you-mean hints at config-load error sites"
type: backend
complexity: low
dependencies:
  - task_01
---

# Task 02: Append near-miss did-you-mean hints at config-load error sites

## Overview
Enrich the three existing config-load errors that fire on a typo'd table name with a "did you mean `codex`?" suggestion, using the shared `suggest_nearby_name` helper from task_01. The hint is strictly additive — it never introduces a new failure or warning for a config that loads successfully, including unconventional custom names.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST append a near-miss suggestion to the runtime "missing required field type" error (config/mod.rs ~1313), comparing the runtime id against the sibling runtime keys.
- MUST append a suggestion to the agent "points at undefined runtime" error (~1411), comparing the referenced runtime against the defined runtime keys.
- MUST append a suggestion to the agent "missing required field" errors (~1410/1421), comparing the agent id against the sibling agent keys.
- MUST use the friendly phrasing "did you mean `<name>`?" per the PRD decision.
- MUST be strictly additive: a config that loads successfully gains no new error or warning, including legitimately unconventional custom names.
- MUST emit no suggestion when no sibling is within the edit-distance threshold.
</requirements>

## Subtasks
- [x] 2.1 Add a small formatter turning a `suggest_nearby_name` result into "; did you mean `x`?" or an empty string.
- [x] 2.2 Apply it at the runtime-missing-type error site.
- [x] 2.3 Apply it at the agent-undefined-runtime error site.
- [x] 2.4 Apply it at the agent-missing-field error site(s).
- [x] 2.5 Add unit tests, including the false-positive lock.

## Implementation Details
See TechSpec "Implementation Design" and ADR-004. The "known" set is the sibling keys already present in the merged map (the four builtin runtimes / eight builtin agents are always present), so no hardcoded const is needed. All edits are in `into_effective`; the `anyhow` error types are unchanged — only the message string is enriched.

### Relevant Files
- `src/config/mod.rs` — `into_effective` error sites (~1313 runtime missing type, ~1410/1421 agent missing field, ~1411 agent→undefined runtime); `MergedConfig::builtin` for the always-present sibling names.
- `src/util.rs` — `suggest_nearby_name` (task_01).

### Dependent Files
- None — the hint terminates at the config-load error; no downstream component consumes it.

### Related ADRs
- [ADR-004: Shared util::edit_distance and additive near-miss config hints](adrs/adr-004.md) — Defines the hint sites, phrasing, and additive-only constraint.

## Deliverables
- Near-miss suggestions appended at all three config-load error sites.
- A shared formatter for the "; did you mean `x`?" fragment.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test loading a typo'd config and asserting the enriched error **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `[runtimes.codx]` with no `type` → error message contains "did you mean `codex`?".
  - [x] An agent with `runtime = "codx"` → error contains "did you mean `codex`?".
  - [x] `[agents.fixr]` missing required fields → error contains "did you mean `fixer`?".
  - [x] False-positive lock: a valid `[runtimes.my_custom_thing]` with all required fields loads successfully with no hint and no new warning.
  - [x] A wild typo `[runtimes.zzzzzz]` missing `type` → base error with NO suggestion appended.
- Integration tests:
  - [x] `load_effective_config` on a temp config containing a typo'd runtime returns an error whose message includes the suggestion.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- All three error sites carry a near-miss suggestion when one applies.
- Configs that load successfully are byte-for-byte unaffected (no new hints/warnings).
