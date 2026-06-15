---
status: pending
title: "DAG decision schema, types, validation, and prompt guidance"
type: backend
complexity: high
dependencies:
  - task_01
---

# Task 02: DAG decision schema, types, validation, and prompt guidance

## Overview
Introduce the `ExecutionGraph` plan types and a new `DecisionNextStep::Dag` variant the orchestrator can emit (a `kind="dag"` arm at decision `schema_version 3`), with a `validate_execution_graph` that enforces graph integrity and concurrency-aware write-disjointness, plus flag-gated prompt guidance. This is the data-and-contract foundation every downstream task builds on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ExecutionGraph`, `ExecutionNode`, `ExecutionEdge`, and `ExecutionEdgeKind` (`data_dependency` | `semantic_gate`, snake_case) as serde types, reusing `ParallelFileScope` verbatim for per-node scope. See TechSpec "Core Interfaces".
- MUST add `Dag(ExecutionGraph)` to the internally-tagged `DecisionNextStep` enum WITHOUT renaming/retagging/removing the `SingleAgent`/`ParallelGroup` variants or their `kind` values (durable events must keep deserializing forever).
- MUST gate the DAG behind decision `schema_version 3` in `normalized_next_step`, leaving the v1 and v2 arms byte-identical; the v1/v2 error messages MUST remain unchanged (tests assert them).
- MUST add a `Dag` arm to the exhaustive `validate_decision_next_step` dispatch calling a new `validate_execution_graph`.
- `validate_execution_graph` MUST: reuse the per-node `validate_parallel_child_step_plan` + `validate_parallel_scope_path` logic; reject cycles; reject edges whose endpoints reference unknown `node_id`s; reject duplicate `node_id`s; and enforce write-file disjointness ONLY among nodes with no dependency path between them (the possible-concurrency set) — NOT globally across the graph.
- MUST gate `kind="dag"` prompt guidance in `build_orchestrator_prompt` on `features.execution_graph && max_parallel_agent_steps > 0`, mirroring the existing flat-group gate; the disabled path MUST keep steering to v1/v2.
- New validation error messages MUST be distinct strings (do not reword existing ones).
</requirements>

## Subtasks
- [ ] 2.1 Define `ExecutionGraph`/`ExecutionNode`/`ExecutionEdge`/`ExecutionEdgeKind` and add the `Dag` variant.
- [ ] 2.2 Add the `schema_version 3` arm to `normalized_next_step`; keep v1/v2 frozen.
- [ ] 2.3 Implement `validate_execution_graph` (per-node reuse + acyclicity + edge-endpoint + node-id uniqueness + concurrency-aware disjointness) and wire the `Dag` dispatch arm.
- [ ] 2.4 Add flag-gated `kind="dag"` guidance to `build_orchestrator_prompt`.
- [ ] 2.5 Add the result-side `RunStepResult` consideration note for task_03 (the `Dag` result variant is added there).
- [ ] 2.6 Add unit tests for round-trip, schema gating, and every validation accept/reject case.

## Implementation Details
All changes are in `src/orchestrator/mod.rs`. The `Dag` arm slots into the existing `#[serde(tag="kind")]` enum at the `DecisionNextStep` definition; the dispatch and normalization sites are compiler-forced (exhaustive matches). Reuse `ParallelFileScope` (already `Ord`, so write-set intersection is cheap). See TechSpec "Core Interfaces" and "Orchestrator Decision & Event Contract"; do not duplicate the type bodies here.

### Relevant Files
- `src/orchestrator/mod.rs` — `DecisionNextStep` (`:85`), `normalized_next_step` (`:246`), `validate_decision_next_step` (`:486`), `validate_parallel_group_plan`/`validate_parallel_child_step_plan`/`validate_parallel_scope_path` (`:503`/`:545`/`:573`), `build_orchestrator_prompt` (`:672`), `ParallelFileScope` (`:118`).

### Dependent Files
- `src/app/mod.rs` — `handle_orchestrator_decision` match (`:1884`) and the `let-else` `ParallelGroup` casts (`:4249`) gain `Dag` handling in task_04.
- `src/runtime/fake.rs` — task_08 constructs a `Dag` decision; the `let-else` at `:705` must be audited.
- `src/history/mod.rs` — task_03 serializes these types into events.

### Related ADRs
- [ADR-004: DAG decision schema + ready-set scheduler](../adrs/adr-004.md) — defines the variant, schema_version 3, typed edges, and concurrency-aware disjointness.
- [ADR-001: V1 architecture](../adrs/adr-001.md) — typed ordering edges, not free-form mutex edges.

## Deliverables
- `ExecutionGraph` plan types + `Dag` decision variant at `schema_version 3`.
- `validate_execution_graph` with full integrity + concurrency-aware disjointness checks.
- Flag-gated DAG prompt guidance.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for decision parsing/validation of a `Dag` payload **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A `kind="dag"` decision at `schema_version 3` round-trips through serde; a `single_agent`/`parallel_group` decision still deserializes unchanged.
  - [ ] `kind="dag"` under `schema_version 2` is rejected; the existing v2 error string is unchanged.
  - [ ] `validate_execution_graph` rejects a graph with a cycle (A→B→A).
  - [ ] It rejects an edge whose `from`/`to` references a non-existent `node_id`.
  - [ ] It rejects duplicate `node_id`s.
  - [ ] It rejects two nodes with NO path between them that share a `write_files` entry (concurrent-write conflict).
  - [ ] It ACCEPTS two nodes on a dependency chain (A→B) that share a `write_files` entry (sequential reuse is legal).
  - [ ] It rejects an edit-capable node with empty `write_files` (per-node rule reused).
  - [ ] `build_orchestrator_prompt` emits DAG guidance only when `execution_graph && max_parallel_agent_steps > 0`.
- Integration tests:
  - [ ] A full `OrchestratorDecision` JSON carrying a `Dag` next_step parses and validates end-to-end via the existing `parse_contract` path.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `SingleAgent`/`ParallelGroup` variants and v1/v2 error strings are byte-unchanged.
- A valid diamond graph validates; cyclic, dangling-edge, duplicate-id, and concurrent-write-overlap graphs are rejected with distinct messages.
