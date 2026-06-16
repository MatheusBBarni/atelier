---
status: completed
title: Governance module with shared decision and plan types
type: backend
complexity: medium
dependencies: []
---

# Governance module with shared decision and plan types

## Overview
Create the shared contract every governance surface will render through: a new `src/governance.rs` module holding the `GovernanceDecisionView`, the minimal plan/intent legibility model, the answer type, and the two governance event-kind constants. This is the foundation the projection, app state, early-abort, doctor metrics, and the sibling-conformance test all depend on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a new single-file module `src/governance.rs` declared from `src/lib.rs`.
- MUST define `GovernanceDecisionView` (intent, approach, agent, write-scope, risk label, optional structured plan), `GovernancePlanView`/`GovernancePlanStep`, `GovernanceAnswer` (`Accept` | `Reject { redirect }`), and `GovernanceKind` — see TechSpec "Core Interfaces".
- MUST carry an optional structured plan payload so a single-agent intent echo and (later) a DAG graph render through the same type.
- MUST define the event-kind constants `governance_decision_requested` and `governance_decision_resolved`.
- All types MUST be serializable and round-trip stable (events are frozen for replay).
- MUST NOT depend on `app`, `tui`, or sibling-packet types — this module is data + pure helpers only.
</requirements>

## Subtasks
- [x] 1.1 Create `src/governance.rs` and declare `pub mod governance;` in `src/lib.rs`.
- [x] 1.2 Define `GovernanceDecisionView` and `GovernanceKind`.
- [x] 1.3 Define `GovernancePlanView` + `GovernancePlanStep` (steps + edges).
- [x] 1.4 Define `GovernanceAnswer` and the two event-kind constants.
- [x] 1.5 Derive serde + equality; confirm round-trip.

## Implementation Details
Add `src/governance.rs` and insert `pub mod governance;` in the `src/lib.rs` module list (after `orchestrator`/before `runtime` per project alphabetical grouping). Pure data types only. Reference TechSpec "Implementation Design → Core Interfaces / Data Models" for the field set; do not reproduce it here.

### Relevant Files
- `src/lib.rs` — module declaration list (~line 13) for `pub mod governance;`.
- `src/app/mod.rs` — `PendingClarificationView` (~211) as a shape reference for a serializable view type.
- `src/orchestrator/mod.rs` — `OrchestratorDecision` fields (`reason`, `plan`, `required_capabilities`) the view is populated from (consumed later in task_05).

### Dependent Files
- `src/app/chat/mod.rs`, `projection.rs` (task_02) — render the view.
- `src/app/mod.rs` (task_03) — holds the view in pending state.
- `tests/governance_conformance.rs` (task_08) — proves the view represents both siblings.

### Related ADRs
- [ADR-003: Unified GovernanceDecision data model + single pending_governance_decision state](../adrs/adr-003.md) — defines this contract.

## Deliverables
- `src/governance.rs` with the decision/plan/answer/kind types + event constants.
- `pub mod governance;` wired in `src/lib.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- A cross-module construction test **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `GovernanceDecisionView` round-trips through serde (serialize→deserialize equals original).
  - [x] `GovernanceAnswer::Reject { redirect: Some("x") }` and `Reject { redirect: None }` and `Accept` each round-trip.
  - [x] `GovernancePlanView` with one step + no edges (single-agent) and with multiple steps + edges (DAG-shaped) both round-trip.
  - [x] The event-kind constants equal `"governance_decision_requested"` and `"governance_decision_resolved"`.
- Integration tests:
  - [x] `multiagent::governance::GovernanceDecisionView` is constructible from outside the module (export smoke test).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The module compiles, is exported, and its types serialize stably.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.

## Completion Notes
- `pub mod governance;` was placed between `file_index` and `history` in `src/lib.rs`. The spec said "after `orchestrator`/before `runtime` per project alphabetical grouping" — those two directives contradict (the module list is strictly alphabetical, and `governance` sorts between `file_index` and `history`). Followed the stated *alphabetical* rationale, which matches the project's actual convention.
- `GovernanceAnswer` serializes internally-tagged as `{ "outcome": "accept" | "reject", "redirect"? }`, matching the techspec's documented `governance_decision_resolved` payload shape so task_03/task_05 can map it directly.
- Verified: `cargo test --lib governance` (10 passed), `cargo test --test governance_types` (3 passed), `cargo fmt --check` (clean), `cargo clippy --all-targets` (clean).
