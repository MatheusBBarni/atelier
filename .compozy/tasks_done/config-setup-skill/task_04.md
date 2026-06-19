---
status: completed
title: Add all() enum iteration for the drift test
type: refactor
complexity: low
dependencies: []
---

# Add all() enum iteration for the drift test

## Overview
The enum-coverage drift test (task_06) must enumerate every serde variant of the five config enums to assert each is documented and none is stray. `ToolName::all()` already exists; this task adds an equivalent hand-written `all()` to the four enums that lack iteration so the test can compare documentation against source without an LLM.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add an `all()` associated function (returning every variant, in declaration order) to `RuntimeKind`, `ApprovalMode`, `AgentEffort`, and `Capability` in `src/config/mod.rs`, mirroring the existing `ToolName::all()` shape.
- MUST hand-write `all()` (no new `strum` dependency) — `strum` is not currently a dependency and ADR-005 permits a small `all()`.
- MUST keep `all()` exhaustive and in sync with the enum definitions (a new variant must be reflected) — `Capability::all()` MUST include the current `McpTool` variant.
- MUST NOT change any serde representation, variant, or behavior — iteration helpers only.
- SHOULD expose `all()` with visibility usable from the integration test (`pub` or `pub(crate)` consistent with `ToolName::all()`).
</requirements>

## Subtasks
- [x] 4.1 Add `RuntimeKind::all()` (codex, claude, cursor, zai, fake).
- [x] 4.2 Add `ApprovalMode::all()` (yolo, normal).
- [x] 4.3 Add `AgentEffort::all()` (minimal, low, medium, high, xhigh).
- [x] 4.4 Add `Capability::all()` (plan, read, answer, challenge, edit, command, verify, review, mcp_tool).
- [x] 4.5 Add/extend a unit test asserting each `all()` length matches the enum's variant count.

## Implementation Details
Modify `src/config/mod.rs`. Follow the existing `impl ToolName { pub fn all() -> Vec<Self> { … } }` pattern exactly (same visibility/return type). Place each `all()` next to its enum. The serde `rename_all = "snake_case"` (and `AgentEffort::XHigh => "xhigh"`) already determines the serde names the drift test compares against — `all()` only needs the variants. See TechSpec "Core Interfaces" note ("add a small `all()` to the four enums lacking it") and ADR-005.

### Relevant Files
- `src/config/mod.rs` — `RuntimeKind` (~307), `ApprovalMode` (~27), `AgentEffort` (~324), `Capability` (~54, includes `McpTool`); existing `ToolName::all()` (~85) as the template.

### Dependent Files
- `tests/atelier_config_skill.rs` (task_06) — calls each `all()` for the enum-coverage drift test.

### Related ADRs
- [ADR-005: Skill correctness via an enum/TOML drift guard (hand-written `all()` permitted)](../adrs/adr-005.md)

## Deliverables
- `all()` on `RuntimeKind`, `ApprovalMode`, `AgentEffort`, `Capability` (exhaustive, declaration order), matching `ToolName::all()`'s shape.
- A unit test asserting each `all()` is exhaustive.
- Unit tests **(REQUIRED)** with 80%+ coverage of the new functions.

## Tests
- Unit tests (`src/config/mod.rs`):
  - [ ] `RuntimeKind::all().len() == 5` and contains each variant.
  - [ ] `ApprovalMode::all().len() == 2`.
  - [ ] `AgentEffort::all().len() == 5`.
  - [ ] `Capability::all().len() == 9` and includes `Capability::McpTool` (guards against forgetting a variant).
- Integration tests:
  - [ ] (Covered transitively in task_06's enum-coverage test, which iterates these `all()` results.)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- The four enums expose an exhaustive `all()` usable by the drift test; no serde/behavior change.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
