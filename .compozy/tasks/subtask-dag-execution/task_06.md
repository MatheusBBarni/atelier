---
status: pending
title: "Single evolving Plan chat projection"
type: backend
complexity: high
dependencies:
  - task_03
---

# Task 06: Single evolving Plan chat projection

## Overview
Project a DAG run into one durable, in-place evolving "Plan" chat item that shows every node's status, dependency, and file scope, and that carries the whole-plan WaitingApproval state — while suppressing the per-node live chat items so the chat shows a single legible plan (per-node detail stays in the Agent Roster). This delivers the PRD's "approve once, watch it run" surface.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ChatLifecycleKey::Plan { graph_id }` (with its `item_id` arm) and `ChatItemKind::Plan` (with its `slug` arm), keeping the existing snake_case `tag`/`rename_all` shape and append-only ordering so replay/identity is preserved.
- MUST add a single DURABLE handler (`apply_execution_graph`) dispatched from `apply_history_event` for every new DAG kind; the handler MUST re-render the WHOLE graph snapshot on each event (the upsert path replaces the item body wholesale), mapping node states to `ChatItemStatus` (waiting/ready→Pending, running→Running, done→Completed, failed→Failed, skipped→Skipped).
- MUST register EVERY new event kind in the `apply_history_event` match (the `_ => {}` catch-all silently drops unregistered kinds).
- MUST keep the Plan item out of `live_keys`/`pending_key` (it is durable; transient tracking would delete it on the next sync).
- MUST collapse large graphs to a per-state counts line to respect the 12-line body cap.
- MUST render the WaitingApproval state on the Plan item (the approval decision flow lives in task_05) and the fail-closed Skipped set faithfully (no automatic cascade — render the states the scheduler emits).
- MUST suppress the per-node live `AgentProgress` chat items for nodes belonging to a graph (detail remains in the Agent Roster).
</requirements>

## Subtasks
- [ ] 6.1 Add the `Plan` lifecycle key and `Plan` item kind with their exhaustive-match arms.
- [ ] 6.2 Implement `apply_execution_graph` (full-graph re-render, node-state→status, counts summarization, WaitingApproval state).
- [ ] 6.3 Register every new event kind in `apply_history_event`.
- [ ] 6.4 Suppress per-node live `AgentProgress` items for graph nodes in `apply_live_steps`.
- [ ] 6.5 Add unit tests for lifecycle evolution, durability, suppression, and summarization.

## Implementation Details
Changes span `src/app/chat/mod.rs` (the `ChatLifecycleKey`/`ChatItemKind` enums + `item_id`/`slug`) and `src/app/chat/projection.rs` (the new handler + dispatch + live-step suppression). Model the handler on `apply_parallel_group_joined` (the counts-line pattern) and reuse `live_scope_summary` for per-node scope text. The emitter (task_04) must carry the full graph snapshot in each event, since the projection keeps no per-node memory. See TechSpec "Plan projection" and ADR-005; do not duplicate code.

### Relevant Files
- `src/app/chat/mod.rs` — `ChatLifecycleKey` (`:115`), `item_id` (`:205`), `ChatItemKind` (`:25`), `slug` (`:232`), `ChatItemStatus` (`:45`).
- `src/app/chat/projection.rs` — `apply_history_event` dispatch (`:58`), `apply_parallel_group_joined` (`:935`), `apply_live_steps`/`upsert_live_step` (`:121`), `apply_clarification_requested` (`:1214`), `upsert` (`:1339`), `live_scope_summary` (`:1844`), bounds (`:17`).
- `src/app/mod.rs` — `LiveStepView.group_id`/`file_scope` (`:140`), `sync_chat_items` (`:4267`).

### Dependent Files
- `src/tui/mod.rs` — renders the resulting `ChatItemView`s; the Agent Roster (Ctrl-L) is where suppressed per-node detail surfaces.

### Related ADRs
- [ADR-005: DAG user-surface integration](../adrs/adr-005.md) — single evolving Plan item, suppress per-node live items, register every kind.
- [ADR-003: V1 product shape](../adrs/adr-003.md) — "one plan I watch" run visibility.

## Deliverables
- `ChatLifecycleKey::Plan` + `ChatItemKind::Plan` + durable `apply_execution_graph` handler.
- Per-node live-item suppression for graph nodes.
- Large-graph counts summarization under the 12-line cap.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the projected plan lifecycle **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A `execution_graph_proposed` event creates one Plan item keyed by `graph_id`; a second graph event updates the SAME item (no duplicate).
  - [ ] Node-state events evolve the Plan item: a node moving running→succeeded re-renders that node as Completed while others stay correct.
  - [ ] A failed node renders the Plan item Failed and its skipped dependents as Skipped.
  - [ ] The Plan item survives a `sync_chat_items` cycle (durable; never removed as transient).
  - [ ] A graph with >12 nodes collapses to a per-state counts line (no silent body overflow).
  - [ ] Per-node live `AgentProgress` items are NOT emitted for nodes whose live step maps to a graph.
  - [ ] An unregistered/unknown event kind produces no Plan output (guards the catch-all) — and every defined DAG kind IS registered.
- Integration tests:
  - [ ] (with task_08 harness) a full run shows one Plan item transitioning WaitingApproval → Running → Completed.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Exactly one durable Plan item per graph, evolving in place; no per-node chat flooding.
- Every DAG event kind is rendered (none silently dropped).
