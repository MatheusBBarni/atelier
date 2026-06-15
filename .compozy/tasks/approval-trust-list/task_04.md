---
status: pending
title: Session TrustStore & per-action context wiring
type: backend
complexity: medium
dependencies:
  - task_02
  - task_03
---

# Task 04: Session TrustStore & per-action context wiring

## Overview
Add the in-memory, session-scoped `TrustStore` to `App` and feed each action's `ActionExecutionContext` with the configured `floor` and a snapshot of trusted targets. After this task, an action whose target is already trusted auto-runs (no modal) because the enforcement matrix returns `AllowedByTrust`.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `TrustStore` on `App` holding `TrustTarget`s in memory, with `grant`, `contains`, `revoke_index`, `clear`, `list`, and `snapshot` (→ `Arc<HashSet<TrustTarget>>`).
- MUST NOT persist the trust store to disk or to `.atelier/ui_state.json` — it dies with the process (ADR-004).
- MUST populate `ActionExecutionContext.floor` from `config.approval.floor` and `trusted_targets` from `TrustStore::snapshot()` at every context construction site (serial and parallel paths).
- MUST preserve listing order for `revoke_index` (1-based, matching the `/trust` listing in task_08).
- SHOULD keep the snapshot cheap (clone into a set per action or share via `Arc`).

## Subtasks
- [ ] 04.1 Implement `TrustStore` and add it as a field on `App`.
- [ ] 04.2 Build `ActionExecutionContext` with `floor` (from config) and a trust snapshot at all construction sites.
- [ ] 04.3 Confirm trusted non-catastrophic actions auto-run via the task_03 matrix (no `pending_approval`).
- [ ] 04.4 Add `TrustStore` unit tests and a FakeRuntime test for auto-run-when-trusted.

## Implementation Details
`TrustStore` and the `App` field live in `src/app/mod.rs`; context construction happens where the serial step path (`process_step_action` ~4000) and the parallel path build `ActionExecutionContext`. Reuse `TrustTarget` (task_01) and the enriched context (task_03). See TechSpec "Core Interfaces" (`TrustStore`) and "System Architecture → Data flow".

### Relevant Files
- `src/app/mod.rs` — `App` struct, `AppState` (~110), context construction in the serial/parallel run paths.
- `src/actions/mod.rs` — `ActionExecutionContext` fields (task_03), `TrustTarget` (task_01).

### Dependent Files
- `src/app/mod.rs` (task_05) — grants into the store on `ApproveAndTrust`.
- `src/app/mod.rs` (task_08) — `/trust` lists/revokes the store.

### Related ADRs
- [ADR-004: In-memory exact-target session trust](../adrs/adr-004.md) — store shape, no persistence, never covering catastrophic.

## Deliverables
- `TrustStore` on `App` and context wiring for `floor` + trust snapshot.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- FakeRuntime integration test for auto-run-when-trusted **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `grant` then `contains` returns true for the same `TrustTarget`; a different target returns false.
  - [ ] `revoke_index(1)` removes the first-listed entry; out-of-range index returns `None` and leaves the store unchanged.
  - [ ] `clear` empties the store; `snapshot` reflects the current entries.
  - [ ] Context built from `config.approval.floor = enforce` carries `FloorPolicy::Enforce`.
- Integration tests:
  - [ ] FakeRuntime run where a `cargo test` target is pre-granted, then emitted by the agent → action completes with no `pending_approval` raised.
  - [ ] Same target NOT granted → `pending_approval` is raised (control case).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Trusted non-catastrophic targets auto-run with no modal; untrusted targets still prompt.
- Trust state is never written to disk and is empty on a fresh process.
