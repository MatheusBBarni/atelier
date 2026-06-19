---
status: pending
title: ReviewFinding types and review module
type: backend
complexity: low
dependencies: []
---

# Task 03: ReviewFinding types and review module

## Overview
Define the structured finding type the PRD's finding anatomy needs (severity, file:line, claim, rationale, confidence) and create the `src/review` module that houses review logic. `AgentResult.findings` is only `Vec<String>` today, which cannot carry confidence or location for gating and metrics (ADR-006).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define `ReviewFinding` with `severity` (`FindingSeverity::{Important,Nit}`), `file`, `line: Option<u32>`, `claim`, `rationale`, and `confidence` (`FindingConfidence::{High,Medium,Low}`).
- MUST serialize the enums as snake_case and make `ReviewFinding` serde round-trippable.
- MUST create `src/review/mod.rs` and register the module in the crate.
- MUST keep `ReviewFinding` separate from `AgentResult.findings` — no overload of the `Vec<String>` field.
</requirements>

## Subtasks
- [ ] 03.1 Define the `FindingSeverity` and `FindingConfidence` enums.
- [ ] 03.2 Define the `ReviewFinding` struct.
- [ ] 03.3 Create `src/review/mod.rs` and register it in the crate root.
- [ ] 03.4 Unit-test serde round-trip and field handling.

## Implementation Details
Create `src/review/mod.rs` and register the module at the crate root. Mirror the serde style of existing orchestrator types. Do not reproduce the struct definition here — see TechSpec "Implementation Design → Core Interfaces".

### Relevant Files
- `src/orchestrator/mod.rs` — `AgentResult.findings: Vec<String>` (`:216`) for contrast; `ArtifactReference` (`:200`) for serde conventions.
- crate root module declarations (`src/lib.rs` / `src/main.rs`) — register the new `review` module.

### Dependent Files
- `src/review/mod.rs` engine (task_05), chat projection (task_07), and feedback (task_08) all consume `ReviewFinding`.

### Related ADRs
- [ADR-006: Structured ReviewFinding carried in review events](../adrs/adr-006.md) — defines this type and why it is separate from `AgentResult.findings`.
- [ADR-002: Single independent reviewer with research-backed finding anatomy](../adrs/adr-002.md) — the required finding fields.

## Deliverables
- `FindingSeverity`, `FindingConfidence`, `ReviewFinding` in `src/review/mod.rs`, module registered.
- Unit tests with 80%+ coverage **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A `ReviewFinding` with `line = Some(42)` serde round-trips unchanged.
  - [ ] A `ReviewFinding` with `line = None` serde round-trips unchanged.
  - [ ] `FindingSeverity::Important` and `FindingConfidence::High` serialize to snake_case strings.
- Integration tests:
  - [ ] Type is exercised end-to-end by task_05 (engine) and task_07 (projection); no standalone integration here.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `ReviewFinding` carries severity, file, optional line, claim, rationale, and confidence
- The `review` module compiles and is registered without touching `AgentResult.findings`
