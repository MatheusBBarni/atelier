---
status: pending
title: "Add Clarification Option Schema Fields"
type: backend
complexity: medium
dependencies: []
---

# Task 01: Add Clarification Option Schema Fields

## Overview
Add the shared structured option model required by the Clarification Select UI. This task establishes the contract shape that later validation, app state, runtime prompts, Chat, and TUI tasks consume.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `ClarificationOption` model with id, label, and optional description fields.
- MUST extend `OrchestratorDecision` with `clarifying_options` and `recommended_option_id`.
- MUST preserve serde compatibility for existing decisions that omit the new fields.
- MUST update all direct `OrchestratorDecision` literals so the crate compiles after the schema change.
- MUST NOT add any-agent question request behavior or a new `RuntimeOutput` variant.
- SHOULD keep the model close to the orchestrator contract unless implementation discovers a stronger existing shared-model location.
</requirements>

## Subtasks
- [ ] 1.1 Add the structured clarification option model.
- [ ] 1.2 Add option fields to the orchestrator decision contract.
- [ ] 1.3 Update existing decision literals and fixture constructors to include the new fields or defaults.
- [ ] 1.4 Preserve parse compatibility for existing decisions without structured options.
- [ ] 1.5 Add focused schema parse/serialization coverage.

## Implementation Details
Start from TechSpec sections "Core Interfaces" and "Data Models". Keep this task to schema shape and compatibility only; validation rules and fake runtime behavior belong to task 02.

### Relevant Files
- `src/orchestrator/mod.rs` — Defines `OrchestratorDecision`, `DecisionStatus`, contract parsing, validation entry points, and direct contract tests.
- `src/runtime/fake.rs` — Builds fake `OrchestratorDecision` values and will need schema fields to compile.
- `src/runtime/codex.rs` — Contains direct `OrchestratorDecision` test fixtures affected by the new fields.
- `src/app/mod.rs` — Contains app tests and fixtures that construct or assert decision behavior.
- `.compozy/tasks/agent-question-tool/_techspec.md` — Defines the approved core interface fields and schema trade-offs.

### Dependent Files
- `src/runtime/claude.rs` — Later prompt contract task depends on the expanded schema.
- `src/runtime/cursor.rs` — Later prompt contract task depends on the expanded schema.
- `src/app/chat/projection.rs` — Later Chat projection task depends on structured clarification payloads.
- `src/tui/mod.rs` — Later TUI tasks depend on a stable option model exposed through app state.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires structured options on the orchestrator decision.
- [ADR-001: Scope Clarification Select UI](adrs/adr-001.md) — Keeps the model scoped to the existing orchestrator clarification flow.

## Deliverables
- Expanded orchestrator decision schema with structured clarification fields.
- Updated decision fixtures and constructors across the crate.
- Backward-compatible parsing for decisions without clarification option fields.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration-compatible schema assertions for downstream tasks **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Parse an existing orchestrator decision JSON payload that omits `clarifying_options` and `recommended_option_id`.
  - [ ] Parse a waiting-for-user decision containing two clarification options and a recommended option id.
  - [ ] Serialize an `OrchestratorDecision` with structured options and confirm the expected field names are present.
  - [ ] Existing non-clarification decision tests continue to pass with default option fields.
- Integration tests:
  - [ ] Fake runtime and app test fixtures compile with the expanded decision schema.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The orchestrator contract can carry structured clarification options.
- Existing decisions without structured options remain parseable.
- No any-agent question request behavior is introduced.
