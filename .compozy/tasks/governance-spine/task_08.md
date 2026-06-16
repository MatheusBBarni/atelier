---
status: completed
title: Sibling conformance contract and test
type: docs
complexity: low
dependencies:
  - task_01
---

# Sibling conformance contract and test

## Overview
Prove the spine's central claim: that the shared `GovernanceDecisionView` can represent both siblings' decisions, so "shared contract" is verified, not asserted. Document the conformance interface and add a test that maps a `RiskNote`-shaped approval and an `ExecutionGraph`-shaped plan into the shared view without loss.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST document the conformance contract: how a consumer populates `GovernanceDecisionView` (and the optional `GovernancePlanView`), as the interface the siblings adopt in Phase 2.
- MUST add a conformance test mapping a synthetic `approval-trust-list`-style approval (action/target/risk-tier/reason) into the view without losing those fields.
- MUST add a conformance test mapping a synthetic `subtask-dag-execution`-style `ExecutionGraph` (multiple nodes + edges) into `GovernancePlanView` preserving steps and edges.
- MUST NOT modify either sibling packet's code — this is V1's contract definition only.
</requirements>

## Subtasks
- [x] 8.1 Write the conformance contract notes (how siblings populate the view).
- [x] 8.2 Add the approval-shaped → `GovernanceDecisionView` mapping test.
- [x] 8.3 Add the graph-shaped → `GovernancePlanView` mapping test.
- [x] 8.4 Confirm no sibling code is touched.

## Implementation Details
Add `tests/governance_conformance.rs` (or a `#[cfg(test)]` module in `src/governance.rs`) with synthetic fixtures mirroring the sibling techspec types (`RiskNote`, `ExecutionGraph`) — defined locally in the test, not imported, since V1 must not couple to sibling code. Document the contract inline or in the packet. Reference TechSpec "Integration Points" and the sibling techspecs.

### Relevant Files
- `src/governance.rs` — the shared view/plan types under test (task_01).
- `.compozy/tasks/approval-trust-list/_techspec.md` — `RiskNote` shape to mirror as a fixture.
- `.compozy/tasks/subtask-dag-execution/_techspec.md` — `ExecutionGraph` shape to mirror as a fixture.

### Dependent Files
- None.

### Related ADRs
- [ADR-001: Reframe as a governance spine consumed by the sibling packets](../adrs/adr-001.md) — the spine the siblings consume.
- [ADR-002: V1 product shape — shared contract + early-abort, phased sibling migration](../adrs/adr-002.md) — reference, don't reimplement.
- [ADR-003: Unified GovernanceDecision data model + single pending_governance_decision state](../adrs/adr-003.md) — the view must be a superset of both siblings.

## Deliverables
- A documented sibling-conformance contract.
- Conformance tests for both sibling shapes.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An integration conformance test in the suite **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] A synthetic approval (action="force-push", target=branch, tier=High, reason set) maps into `GovernanceDecisionView` with intent, risk label, and target preserved.
  - [x] A synthetic `ExecutionGraph` of 3 nodes + 2 edges maps into `GovernancePlanView` preserving all 3 steps and both edges.
  - [x] A read-only single-step intent maps into a one-step `GovernancePlanView` with no edges.
- Integration tests:
  - [x] The conformance test file builds and runs as part of `cargo test`, demonstrating both sibling shapes are representable.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The shared view is proven to represent both siblings; no sibling code is modified.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.

## Completion Notes
- Added `tests/governance_conformance.rs` (5 tests). The sibling shapes (`RiskTier`/`RiskNote`/`TrustTarget`, `ExecutionGraph`/`ExecutionNode`/`ExecutionEdge`) are mirrored as **local synthetic fixtures**, not imported — V1 must not couple to sibling code (verified: the only changed file is the new test; no sibling packet code touched).
- The conformance **contract is documented inline** as the module doc, and *realized* by the two `map_*` functions (the interface siblings adopt in Phase 2). Mapping: an action approval → intent (`action + target`), `write_scope` = the trust target, `risk_label` = `tier + reason`; an `ExecutionGraph` → `GovernancePlanView` (nodes→steps with per-node write scope, edges→`(from,to)` pairs).
- The `kind` discriminator stays `EarlyAbort` in the fixtures because V1's `GovernanceKind` only has that variant; the contract notes that Phase-2 migration adds `ActionApproval`/`PlanApproval` variants and is additive — the envelope fields are already a superset of both siblings (ADR-003 risk: "design it as an envelope … validated against both sibling techspecs").
- Verified: `cargo test --test governance_conformance` (5 passed), `cargo fmt --check` clean, `cargo clippy --all-targets` clean (0 warnings). No `src` changes, so the full `cargo test --lib` baseline is unchanged (912 passed / 12 pre-existing skill failures). Zero failures attributable to this task.
