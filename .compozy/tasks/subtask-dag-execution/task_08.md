---
status: pending
title: "Fake-runtime DAG harness and integration suite"
type: test
complexity: high
dependencies:
  - task_04
  - task_05
  - task_06
---

# Task 08: Fake-runtime DAG harness and integration suite

## Overview
Extend the deterministic `fake` runtime so tests can drive a full DAG end-to-end — emitting a `Dag` decision and scripting per-node outcomes via control phrases — then add the cross-feature integration suite that exercises concurrency, fail-closed skipping, the normal-mode approval gate, and the write-fence together. The fake-runtime harness is the enabling implementation; the integration tests prove the scheduler, approval, and projection compose correctly.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extend `src/runtime/fake.rs` so a prompt can trigger a `Dag` orchestrator decision with a specified node/edge shape, and so each node's outcome (succeed/fail/blocked) is scriptable via control phrases (consistent with the existing fake-runtime control-phrase mechanism).
- MUST audit/handle the refutable `let Some(DecisionNextStep::ParallelGroup(..))` cast in `fake.rs` (`:705`) so a `Dag` decision is not mis-handled.
- MUST add an end-to-end integration suite (under `tests/`) covering the cross-feature flows that cannot live in a single feature task.
- Integration tests MUST assert observable behavior (event log / projected chat / timestamps), not internal scheduler state.
- MUST keep the harness deterministic (no wall-clock flakiness); concurrency assertions rely on event ordering / overlap, not sleeps.
</requirements>

## Subtasks
- [ ] 8.1 Add DAG-decision emission + per-node outcome scripting to the `fake` runtime.
- [ ] 8.2 Handle the `Dag` case in the `fake.rs` `let-else` cast.
- [ ] 8.3 Add the diamond-concurrency + dependency-ordering integration test.
- [ ] 8.4 Add the fail-closed skip integration test.
- [ ] 8.5 Add the normal-mode approval (accept / reject-re-propose) and yolo auto-accept integration tests.
- [ ] 8.6 Add the write-fence + scheduler-disjointness integration test.

## Implementation Details
The harness lives in `src/runtime/fake.rs`; integration tests live under `tests/` (follow the `runtime_integration.rs` / app-test conventions and the control-phrase pattern). Reuse the existing fake-runtime trigger style (control phrases embedded in the prompt) to script the graph and node outcomes. See TechSpec "Testing Approach → Integration Tests" and CLAUDE.md (the `fake` runtime drives deterministic end-to-end tests). Do not duplicate scheduler logic — exercise it.

### Relevant Files
- `src/runtime/fake.rs` — control-phrase mechanism and the `let-else` decision cast (`:705`); `/workflow parallel ...` precedent (`:727`).
- `tests/runtime_integration.rs`, `tests/cli.rs` — integration-suite conventions.
- `src/app/mod.rs` — `run_execution_graph`/approval (task_04/05) under test.
- `src/app/chat/projection.rs` — the Plan projection (task_06) asserted via projected chat items.

### Dependent Files
- None (terminal task in the chain).

### Related ADRs
- [ADR-004: DAG decision schema + ready-set scheduler](../adrs/adr-004.md) — the concurrency/fail-closed behaviors under test.
- [ADR-003: V1 product shape](../adrs/adr-003.md) — the approval and run-visibility flows under test.

## Deliverables
- A `fake`-runtime DAG harness (decision emission + per-node outcome scripting).
- An end-to-end integration suite for the DAG feature.
- Unit tests for the harness's decision/outcome scripting **(REQUIRED)**
- Integration tests for the cross-feature flows **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] The fake runtime emits a valid `Dag` decision for a configured graph shape and validates via task_02's validator.
  - [ ] A control phrase scripts a specific node to fail (drives the fail-closed path).
- Integration tests:
  - [ ] Diamond A→{B,C}→D: B and C run concurrently (overlapping start/finish in the event log) and D starts only after both succeed.
  - [ ] Fail-closed: when B fails, D is `Skipped` and the run ends with a report distinguishing completed vs skipped nodes.
  - [ ] Normal-mode accept: the run blocks until accepted, then completes; reject-with-reason triggers an orchestrator re-proposal; yolo runs with no gate.
  - [ ] Write-fence: a node scripted to write outside its scope is denied by the runtime fence, and the scheduler never co-runs two intersecting-write nodes.
  - [ ] Projection: the run surfaces exactly one evolving Plan item (WaitingApproval → Running → Completed) with no per-node chat flooding.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The fake runtime can drive any small DAG deterministically.
- The integration suite proves concurrency, fail-closed, approval, write-fence, and the single Plan view compose correctly.
