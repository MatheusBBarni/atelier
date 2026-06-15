---
status: pending
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
- [ ] 3.1 Add the `graph_id` field and thread it through both serializers (`append_event` via the struct, `append_debug_event` hand-built JSON).
- [ ] 3.2 Add `ExecutionGraphResult` and `RunStepResult::Dag`.
- [ ] 3.3 Add the new event-kind constants/strings: `execution_graph_proposed`, `execution_graph_approved`, `execution_graph_rejected`, `node_pending|ready|running|succeeded|failed|skipped|cancelled`, `execution_graph_completed`.
- [ ] 3.4 Add the recorder path that emits these with `graph_id` (+ `node_id` in payload for node events).
- [ ] 3.5 Add unit tests for legacy deserialize, round-trip, and debug-event lockstep.

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
  - [ ] A legacy JSONL event with no `graph_id` deserializes with `graph_id == None` (extends the existing `reads_legacy_jsonl_events_without_group_id` pattern).
  - [ ] An event written with a `graph_id` round-trips through `append_event` and read-back.
  - [ ] `append_debug_event` includes `graph_id` in its hand-built JSON (lockstep check).
  - [ ] An `ExecutionGraphResult` serializes/deserializes via `RunStepResult::Dag`.
  - [ ] `read_events_from_path` still rejects `schema_version != 1` (unchanged), and a mixed log of old `parallel_*` and new `node_*`/`execution_graph_*` events all parse.
- Integration tests:
  - [ ] A recorded `execution_graph_proposed` + `node_*` + `execution_graph_completed` sequence replays cleanly through history read-back.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- History `schema_version` stays 1; legacy and DAG events coexist in one `events.jsonl`.
- `graph_id` is consistent across the event column and payload.
