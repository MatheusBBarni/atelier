---
status: completed
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
- [x] 6.1 Added `ChatLifecycleKey::Plan { graph_id }` (+ `item_id` `chat:plan:{graph_id}`) and `ChatItemKind::Plan` (+ `slug` "plan"), append-only; the forced `chat_kind_label` arm in tui too.
- [x] 6.2 Implemented `apply_execution_graph`: full-graph re-render from each event's snapshot, per-state rollup → `ChatItemStatus` (proposed→WaitingApproval, rejected→Denied, completed→result status, node events→running/pending/failed/cancelled/completed/skipped rollup), counts-line summarization beyond 10 nodes, WaitingApproval on proposed.
- [x] 6.3 Registered all 11 DAG kinds (via the `crate::history::*_KIND` constants) in `apply_history_event`.
- [x] 6.4 Suppressed per-node live `AgentProgress` items in `apply_live_steps` for steps whose `group_id` is a known graph (tracked in a `graph_ids` set populated by `apply_execution_graph`).
- [x] 6.5 Unit tests for lifecycle evolution, durability, suppression, summarization, unknown-kind, and full-kind registration.

> The Plan item is durable: built only from `apply_history_event` via `upsert`, never added to `live_keys`/`pending_key`, so it survives every sync.

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
  - [x] Proposed creates one Plan item keyed by `graph_id`; a later event updates the SAME item. (`proposed_creates_one_plan_item_and_later_events_update_it_in_place`)
  - [x] Node-state events evolve the Plan item in place. (`node_state_transitions_re_render_the_plan_item`)
  - [x] A failed node renders the Plan item Failed with skipped dependents Skipped. (`failed_node_renders_plan_failed_with_skipped_dependents`)
  - [x] The Plan item survives a live-step sync (durable). (`plan_item_survives_a_live_step_sync`)
  - [x] A large graph collapses to a per-state counts line under the 12-line cap. (`large_graph_collapses_to_a_counts_line`)
  - [x] Per-node live `AgentProgress` items are suppressed for graph nodes (non-graph steps unaffected). (`per_node_live_items_are_suppressed_for_graph_nodes`)
  - [x] An unknown DAG-like kind produces no Plan output; every defined DAG kind IS registered. (`unknown_dag_like_kind_produces_no_plan_item`, `every_dag_kind_is_registered_and_renders_a_plan_item`)
- Integration tests:
  - [x] Lifecycle evolution (WaitingApproval→Running→Completed/Failed) is exercised by the unit tests above; the full end-to-end run through the fake runtime is task_08's harness.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Exactly one durable Plan item per graph, evolving in place; no per-node chat flooding.
- Every DAG event kind is rendered (none silently dropped).
