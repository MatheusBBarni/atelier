---
status: pending
title: "Ready-set scheduler with fail-closed admission"
type: backend
complexity: critical
dependencies:
  - task_02
  - task_03
---

# Task 04: Ready-set scheduler with fail-closed admission

## Overview
Lower a validated `ExecutionGraph` onto a single ready-set scheduler that reuses the existing parallel executor but replaces its spawn-all-at-once loop with lazy admission: a node runs only when its predecessors have succeeded and its write scope doesn't collide with any running node (command nodes run alone). This is the critical-path task and owns the feature's hardest invariant — fail-closed forward progress that never deadlocks the join loop.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `run_execution_graph` path and an `admit_ready_nodes` step that reuses the existing executor machinery (`spawn_parallel_runtime_task`, the mpsc/`select!` join loop, `record_parallel_child_result`, per-node limits, cancellation) — MUST NOT build a second executor.
- MUST admit a node only when: (a) every predecessor node is terminal AND succeeded; (b) the node's `write_files` do not intersect the union of `write_files` of currently-running (spawned, non-terminal) nodes; AND (c) if the node runs commands (derived from `required_capabilities`), no other node is currently running.
- MUST keep per-node enforcement by passing `ActionScope::ParallelFileScope(node.file_scope)` per node — MUST NOT modify `src/actions/mod.rs`.
- MUST add a `NodeRunState` (Pending = planned-not-admitted, Running, Terminal) so the scheduler and join-loop can distinguish "waiting" from "running"; every existing reader that treats `terminal_result == None` as "running" MUST be audited.
- MUST be fail-closed: any failed/blocked/cancelled predecessor marks its dependents terminal `Skipped` (never run them on incomplete state).
- MUST guarantee forward progress: proactively mark every never-admittable node terminal and re-run admission after the approval gate AND after every `record_parallel_child_result`, so the join loop always drains to all-terminal (no hang).
- MUST add the `Dag` arm to `handle_orchestrator_decision` and audit the refutable `let Some(DecisionNextStep::ParallelGroup(..))` casts so a `Dag` decision is not silently mishandled.
- MUST emit the node lifecycle and `execution_graph_completed` events (from task_03) and produce an `ExecutionGraphResult`.
</requirements>

## Subtasks
- [ ] 4.1 Add `NodeRunState` to the per-node runtime state and audit all `terminal_result` readers.
- [ ] 4.2 Implement `admit_ready_nodes` (predecessor-success + write-overlap + command-isolation).
- [ ] 4.3 Implement `run_execution_graph` reusing the spawn/join machinery, calling admission at start and after each terminal transition.
- [ ] 4.4 Implement fail-closed skip propagation and the proactive-terminalize forward-progress invariant.
- [ ] 4.5 Wire the `Dag` arm into `handle_orchestrator_decision` and fix the `let-else` casts.
- [ ] 4.6 Emit node lifecycle + completed events and synthesize `ExecutionGraphResult`.
- [ ] 4.7 Add unit tests for admission, fail-closed, command-isolation, and forward-progress.

## Implementation Details
Changes are concentrated in `src/app/mod.rs`. Reuse `run_parallel_group`'s structure but split the spawn-all loop (`:2060–2112`) into per-child setup + lazy `admit_ready_nodes`; re-run admission inside the `select!` loop's completion arm (after `record_parallel_child_result`, `:2812`). Clone the `ParallelRuntimeResumeHandle` per spawned node (its `sender` is moved on spawn). See TechSpec "Core Interfaces" (the `admit_ready_nodes` contract) and "Known Risks" (deadlock + `terminal_result` overload). Do not duplicate the signatures here.

### Relevant Files
- `src/app/mod.rs` — `run_parallel_group` (`:2005`), spawn loop (`:2060`), join `select!` loop (`:2124`), `prepare_parallel_children` (`:2378`), `ParallelChildRuntimeState` (`:795`), `spawn_parallel_runtime_task` (`:5795`), `record_parallel_child_result` (`:2812`), `synthesize_parallel_group_result` (`:5867`), `handle_orchestrator_decision` (`:1884`), `let-else` cast (`:4249`).
- `src/orchestrator/mod.rs` — `ExecutionGraph`/edges (task_02), `ExecutionGraphResult` (task_03).
- `src/actions/mod.rs` — `validate_action_scope` (`:219`) reused unchanged as the runtime backstop.

### Dependent Files
- `src/app/mod.rs` (approval, task_05) — gates before `run_execution_graph` is allowed to admit.
- `src/app/chat/projection.rs` (task_06) — renders the emitted node lifecycle events.
- `src/runtime/fake.rs` (task_08) — drives this scheduler in integration tests.

### Related ADRs
- [ADR-004: DAG decision schema + ready-set scheduler](../adrs/adr-004.md) — admission policy, fail-closed, deadlock invariant, command-node isolation, two-layer fence.

## Deliverables
- `run_execution_graph` + `admit_ready_nodes` + `NodeRunState` on the reused executor.
- Fail-closed skip propagation and the forward-progress (no-deadlock) invariant.
- `Dag` decision wiring + `let-else` audit + node/completed event emission.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for a multi-node graph run (added end-to-end in task_08) **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Admission: given A done-succeeded and B,C depending on A with disjoint writes, both B and C become ready in the same admission pass.
  - [ ] Write-overlap: two ready nodes sharing a `write_files` entry are not both admitted (one waits).
  - [ ] Command-isolation: a command-capable ready node is not admitted while any other node is running.
  - [ ] Fail-closed: when A fails, its dependent D is marked `Skipped` and never spawned.
  - [ ] Forward-progress: a graph whose entire tail is downstream of a failed node drains to all-terminal without hanging (bounded-iteration assertion).
  - [ ] `NodeRunState`: a planned-not-admitted node (Pending) is not counted as "running" by the write-overlap check.
  - [ ] A `Dag` decision routes through `handle_orchestrator_decision` to the scheduler (not the parallel-group or single-agent path).
- Integration tests:
  - [ ] (with task_08 harness) A 3-node diamond runs B and C concurrently and D after both, with overlapping wall-clock for B/C.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- No graph can deadlock the join loop; fail-closed always drains to all-terminal.
- No two intersecting-write nodes ever run concurrently; the unchanged fence remains the backstop.
