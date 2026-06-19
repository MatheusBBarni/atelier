---
status: completed
title: "DAG events, graph_id, and ExecutionGraphResult"
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 03: DAG events, graph_id, and ExecutionGraphResult

## Overview
Add the durable event vocabulary for DAG runs — a serde-default `graph_id` on `HistoryEvent`, the new graph/node lifecycle event kinds, and a typed `ExecutionGraphResult` (a `RunStepResult::Dag` variant) — all additive at history `schema_version 1` so existing event logs keep replaying. These events are the single source of truth the scheduler emits and the projection renders.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `#[serde(default)] graph_id: Option<String>` to `HistoryEvent`, mirroring `group_id`, and thread it through `new_with_group` (or a `new_with_graph` sibling) AND the hand-built JSON in `append_debug_event` (which omits fields it is not told about).
- MUST NOT bump `HistoryEvent.schema_version` (readers reject anything ≠ 1); the DAG is additive — new kinds + serde-default fields only.
- MUST define typed payload structs for the proposal (`ExecutionGraph`, from task_02) and the terminal aggregate `ExecutionGraphResult` (modeled on `ParallelGroupResult`), and add `RunStepResult::Dag(ExecutionGraphResult)` (tagged-enum, forever-deserialize).
- MUST add the new event KINDS and a recorder path that carries `graph_id` (extend `record_event`/`record_event_with_group` or add a `record_event_with_graph` sibling — orchestrator-altitude graph events MUST NOT be mis-keyed through the group path).
- MUST keep `graph_id` consistent between the `HistoryEvent` column and any payload duplication (the projection reads payload first).
</requirements>

## Subtasks
- [x] 3.1 Add the `graph_id` field and thread it through both serializers (`append_event` via the struct + `new_with_graph` sibling constructor, `append_debug_event` hand-built JSON).
- [x] 3.2 Add `ExecutionGraphResult` (+ `ExecutionGraphStatus`, `NodeResultRef`) and `RunStepResult::Dag`.
- [x] 3.3 Add the new event-kind constants/strings in `history/mod.rs`: `execution_graph_proposed`, `execution_graph_approved`, `execution_graph_rejected`, `node_pending|ready|running|succeeded|failed|skipped|cancelled`, `execution_graph_completed`.
- [x] 3.4 Add the `graph_id`-carrying recorder path (`record_event_with_graph` + `append_event_with_graph`, `#[allow(dead_code)]` until task_04 wires the emit sites).
- [x] 3.5 Add unit tests for legacy deserialize, round-trip, debug-event lockstep, mixed-log parse, non-v1 reject, and full-sequence replay.

> Compilation note: adding `graph_id` to `HistoryEvent` and `RunStepResult::Dag`
> compiler-forced minimal additions in shared test helpers — `graph_id: None` in
> the `HistoryEvent` struct literals (`app/chat/projection.rs`, `tui/mod.rs`,
> history `event_at`) and a `Dag` arm in `fake.rs`' result-label match and the
> orchestrator `agent_results` flat-map (both mirror the `ParallelGroup` arm).
> `record_event_with_graph` is `#[allow(dead_code)]` until task_04 emits events.

## Implementation Details
Changes span `src/history/mod.rs` (the `HistoryEvent` struct + constructors + `append_debug_event`) and `src/app/mod.rs` (the recorder funnel `record_event_with_group` and the new emit sites). `ExecutionGraphResult`/`RunStepResult::Dag` live in `src/orchestrator/mod.rs` alongside `ParallelGroupResult`. Most parallel events today use loose `json!{}` payload maps; the proposal/completed events SHOULD use the typed structs for serde-checked fields. See TechSpec "Data Models" and "Orchestrator Decision & Event Contract".

### Relevant Files
- `src/history/mod.rs` — `HistoryEvent` (`:12`), `new_with_group` (`:39`), `append_event` (`:140`), `append_debug_event` (`:152`), `read_events_from_path` schema gate (`:263`), legacy-compat test (`:457`).
- `src/app/mod.rs` — `record_event` (`:4164`), `record_event_with_group` (`:4215`), existing parallel event sites (`:2025`, `:2066`, `:2320`, `:2841`).
- `src/orchestrator/mod.rs` — `ParallelGroupResult` (`:170`), `RunStepResult` (`:162`).

### Dependent Files
- `src/app/mod.rs` (scheduler, task_04) — emits the node lifecycle + completed events.
- `src/app/chat/projection.rs` (task_06) — registers and renders every new kind.

### Related ADRs
- [ADR-005: DAG user-surface integration](../adrs/adr-005.md) — additive-at-schema_version-1 event strategy and the kind list.

## Deliverables
- `graph_id` field on `HistoryEvent` (serde-default) threaded through both serializers.
- `ExecutionGraphResult` + `RunStepResult::Dag`.
- The full set of DAG event kinds + a `graph_id`-carrying recorder path.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for event round-trip and replay backward-compat **(REQUIRED)**

## Tests
- Unit tests:
  - [x] A legacy JSONL event with no `graph_id` deserializes with `graph_id == None`. (`reads_legacy_jsonl_events_without_graph_id`)
  - [x] An event written with a `graph_id` round-trips through `append_event` and read-back. (`graph_id_round_trips_through_append_and_read`)
  - [x] `append_debug_event` includes `graph_id` in its hand-built JSON (lockstep check). (`append_debug_event_includes_graph_id_in_lockstep`)
  - [x] An `ExecutionGraphResult` serializes/deserializes via `RunStepResult::Dag`. (`execution_graph_result_round_trips_via_run_step_result`)
  - [x] `read_events_from_path` still rejects `schema_version != 1` (unchanged), and a mixed log of old `parallel_*` and new `node_*`/`execution_graph_*` events all parse. (`read_events_still_rejects_non_v1_schema`, `mixed_legacy_parallel_and_dag_events_all_parse`)
- Integration tests:
  - [x] A recorded `execution_graph_proposed` + `node_*` + `execution_graph_completed` sequence replays cleanly through history read-back. (`dag_event_sequence_replays_through_read_back`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- History `schema_version` stays 1; legacy and DAG events coexist in one `events.jsonl`.
- `graph_id` is consistent across the event column and payload.
