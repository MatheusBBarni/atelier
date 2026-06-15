---
status: pending
title: Floor + trust enforcement in the single decision point
type: backend
complexity: high
dependencies:
  - task_01
  - task_02
---

# Task 03: Floor + trust enforcement in the single decision point

## Overview
Wire the floor and trust into atelier's single enforcement point so all gating stays in one place. `validate_action_request_with_scope` consults a `FloorPolicy` and a trusted-targets snapshot (both carried on `ActionExecutionContext`) and returns an enriched `ActionDecision`; `execute_action_request` stamps the outcome onto `ActionResult` for the App to act on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extend `ActionDecision` with `AllowedByTrust(TrustTarget)` and `AllowedWithWarning(RiskNote)`, and change `RequiresApproval` to carry a `RiskNote` (was `String`).
- MUST add `floor: FloorPolicy` and `trusted_targets: Arc<HashSet<TrustTarget>>` to `ActionExecutionContext` (default `Warn` / empty), and a `GateOutcome` + `risk: Option<RiskNote>` on `ActionResult` (with `#[serde(default)]`).
- MUST apply the matrix in `validate_action_request_with_scope` AFTER the existing hard checks: catastrophic → `RequiresApproval` (any mode); trusted non-catastrophic → `AllowedByTrust`; gray-area → `RequiresApproval` in Normal/Enforce, `AllowedWithWarning` in Yolo+Warn; safe → `Allowed`.
- MUST keep the existing schema/tool/capability/path/VCS denials returning `Denied` unchanged.
- MUST translate the decision in `execute_action_request` and stamp `risk`/`gate_outcome` onto the result so the App can record events later.
- MUST update all `ActionDecision`/`ActionResult` match sites and `ActionExecutionContext` constructors to compile with the new shape.

## Subtasks
- [ ] 03.1 Enrich `ActionDecision`, `ActionResult` (`risk`, `gate_outcome`), and `ActionExecutionContext` (`floor`, `trusted_targets`).
- [ ] 03.2 Implement the floor/trust/mode matrix in the enforcement point, after the hard checks.
- [ ] 03.3 Translate the decision in `execute_action_request` and stamp the outcome onto `ActionResult`.
- [ ] 03.4 Update affected match sites/constructors across `actions` and `app`.
- [ ] 03.5 Add matrix unit tests covering every (tier × mode × floor) combination.

## Implementation Details
Core change in `src/actions/mod.rs`: `validate_action_request_with_scope` (~145), `decision_for_command` (~356), `execute_action_request` (~278), `ActionDecision` (~90), `ActionResult` (~57). `ActionExecutionContext` is defined in `src/actions/mod.rs` (constructed in `src/app/mod.rs`). Reuse `assess_risk` from task_01. See TechSpec "Implementation Design → Core Interfaces" for the enriched enums and the enforcement-matrix table in ADR-003 "Implementation Notes".

### Relevant Files
- `src/actions/mod.rs` — enforcement point, `ActionDecision`, `ActionResult`, `ActionExecutionContext`, `execute_action_request`.
- `src/app/mod.rs` — constructs `ActionExecutionContext` and matches on `ActionResult.status`/`ActionDecision`.

### Dependent Files
- `src/app/mod.rs` (task_04/05) — builds the context fields and reacts to `gate_outcome`/`risk`.
- Event-log consumers / `src/app/chat/projection.rs` (task_06) — read the new `ActionResult` fields.

### Related ADRs
- [ADR-003: Enforce floor + trust at the single enforcement point](../adrs/adr-003.md) — the enriched decision and enforcement matrix.
- [ADR-001: V1 scope — fail-closed destructive floor](../adrs/adr-001.md) — fail-closed (unknown → confirm) framing.

## Deliverables
- Enriched `ActionDecision`/`ActionResult`/`ActionExecutionContext` and the enforcement matrix in `src/actions/mod.rs`.
- Backward-compatible serde for `ActionResult` (`#[serde(default)]`).
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration coverage of the decision path via existing action tests **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Gray-area command + Yolo + `floor=Warn` → `AllowedWithWarning`.
  - [ ] Gray-area command + Yolo + `floor=Enforce` → `RequiresApproval`.
  - [ ] Gray-area command + Normal (any floor) → `RequiresApproval`.
  - [ ] Catastrophic command + Yolo → `RequiresApproval` (mode ignored).
  - [ ] Trusted non-catastrophic command in `trusted_targets` + Yolo → `AllowedByTrust`.
  - [ ] Trusted command that is ALSO catastrophic → `RequiresApproval` (trust never wins over catastrophic).
  - [ ] Safe (read-only) command → `Allowed`, no `RiskNote`.
  - [ ] Existing denial (path outside write root / unauthorized tool) still → `Denied`.
- Integration tests:
  - [ ] An old serialized `ActionResult` (without `risk`/`gate_outcome`) still deserializes (serde default).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Every (tier × mode × floor) combination returns the documented decision; catastrophic always prompts; trust never overrides catastrophic.
- Existing hard denials are unchanged and old event records still deserialize.
