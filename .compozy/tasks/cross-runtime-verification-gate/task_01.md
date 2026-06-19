---
status: pending
title: Model-family resolver
type: backend
complexity: low
dependencies: []
---

# Task 01: Model-family resolver

## Overview
Add the single helper that resolves an agent's model family from its configured runtime — the one signal every diversity decision in `/review` depends on. No model-string family parser exists today, so family is derived at provider granularity from `RuntimeKind → ProviderId` (ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a resolver (e.g. `agent_family(config, agent) -> Option<ProviderId>`) in `src/runtime/status.rs` that maps the agent's `runtime` id → `config.runtimes[id].kind` (`RuntimeKind`) → `ProviderId` via the existing `From<RuntimeKind>` impl.
- MUST return `None` when the runtime id is unknown or unconfigured — never guess a family.
- MUST NOT parse the model string; family is provider-level in V1.
- MUST be a pure function unit-testable without constructing an `App`.
</requirements>

## Subtasks
- [ ] 01.1 Add the family resolver next to `ProviderId` in `src/runtime/status.rs`.
- [ ] 01.2 Resolve the agent's runtime id through `EffectiveConfig` to a `RuntimeKind`, then to `ProviderId`.
- [ ] 01.3 Return `None` for unknown/unconfigured runtime ids.
- [ ] 01.4 Cover every `RuntimeKind` mapping and the unknown case with unit tests.

## Implementation Details
Modify only `src/runtime/status.rs`. Reuse the existing `From<RuntimeKind> for ProviderId` impl and `RuntimeKind`. The agent stores a runtime *id* string (`AgentProfile.runtime`); resolve it through `config.runtimes`. See TechSpec "Implementation Design → Core Interfaces" (`agent_family` signature) and the "Family resolver" component.

### Relevant Files
- `src/runtime/status.rs` — `ProviderId` enum (`:42`) and `From<RuntimeKind>` (`:66`); the new helper lives here.
- `src/config/mod.rs` — `RuntimeKind` (`:352`), `AgentProfile.runtime` (`:480`), and the `runtimes` map used for id→kind resolution.

### Dependent Files
- `src/review/mod.rs` (task_05) — reviewer selection consumes this resolver.
- `src/app/mod.rs` (task_04) — per-step provenance recording derives family from this helper.

### Related ADRs
- [ADR-005: Family from RuntimeKind→ProviderId; session-level producer set](../adrs/adr-005.md) — fixes provider-level family derivation, no model-string parsing.

## Deliverables
- The family resolver function in `src/runtime/status.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `Codex` runtime kind resolves to `ProviderId::Codex` (and Claude→Claude, Cursor→Cursor, HttpApi→HttpApi, Fake→Fake).
  - [ ] An agent whose `runtime` id is not present in `config.runtimes` returns `None`.
  - [ ] An agent referencing a configured runtime id resolves to that runtime's `ProviderId`.
- Integration tests:
  - [ ] Covered transitively by task_05's end-to-end review test (no standalone integration needed for a pure helper).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Resolver returns the correct `ProviderId` for every configured `RuntimeKind`
- Resolver returns `None` (never a guessed family) for unknown/unconfigured runtimes
