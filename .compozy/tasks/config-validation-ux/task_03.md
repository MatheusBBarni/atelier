---
status: completed
title: "Add EffectiveConfig required_runtime_ids with guardrail test"
type: backend
complexity: low
dependencies: []
---

# Task 03: Add EffectiveConfig required_runtime_ids with guardrail test

## Overview
Add an owned derivation on `EffectiveConfig` that returns the set of runtime ids whose unavailability must be a hard error — the runtimes guaranteed to run on every prompt-driven run. In V1 this is exactly the orchestrator agent's primary runtime. A guardrail test pins the set so it cannot broaden silently.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `pub fn required_runtime_ids(&self) -> BTreeSet<&str>` to `EffectiveConfig`.
- MUST return the orchestrator agent's primary runtime id (agent id `"orchestrator"`) and nothing else in V1.
- MUST exclude council, inactive-preset, and model-fallback runtimes from the set.
- MUST be a pure function of the already-merged `EffectiveConfig` (no I/O, no availability probing).
- SHOULD document that the set may broaden in V2 (model-fallback / selected-preset coverage).
</requirements>

## Subtasks
- [x] 3.1 Implement `required_runtime_ids()` reading the orchestrator agent's runtime.
- [x] 3.2 Handle the orchestrator-agent-absent case gracefully (empty set, no panic).
- [x] 3.3 Add a guardrail unit test pinning the default-config result.

## Implementation Details
See TechSpec "Implementation Design → Core Interfaces" (the `required_runtime_ids` block) and ADR-003. The orchestrator is identified by the well-known id `"orchestrator"`; `AgentProfile.runtime` is the primary runtime id. Place the method on the `EffectiveConfig` impl near its definition.

### Relevant Files
- `src/config/mod.rs` — `EffectiveConfig` (~402), `AgentProfile` (~315, has `runtime: String`), `MergedConfig::builtin` (default orchestrator runtime is `zai`).

### Dependent Files
- `src/doctor/mod.rs` — task_04 consumes `required_runtime_ids()` to elevate the orchestrator runtime.

### Related ADRs
- [ADR-003: Orchestrator-only runtime elevation, decoupled from --strict](adrs/adr-003.md) — Defines the in-use set as the orchestrator's runtime only.

## Deliverables
- `EffectiveConfig::required_runtime_ids()` returning the orchestrator runtime set.
- A guardrail unit test pinning the default-config result.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration check that a loaded `EffectiveConfig` exposes the method **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] On the default config, `required_runtime_ids()` equals exactly `{"zai"}` (the default orchestrator runtime) — the guardrail.
  - [x] With the orchestrator agent's runtime overridden to `codex`, the set equals `{"codex"}`.
  - [x] A config whose `orchestrator` agent is absent yields an empty set (no panic).
- Integration tests:
  - [x] A config loaded via `load_effective_config` returns the orchestrator runtime from `required_runtime_ids()`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The in-use set is exactly the orchestrator's runtime; council/preset/fallback runtimes are excluded.
- The guardrail test fails if the set ever broadens unintentionally.
