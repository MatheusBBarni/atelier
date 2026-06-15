# TechSpec: Sub-task DAG Execution

## Executive Summary

This feature extends atelier's existing flat parallel-group execution into a dependency graph. The orchestrator emits a new `DecisionNextStep::Dag(ExecutionGraph)` variant (a `kind="dag"` arm under a new decision `schema_version 3`); the app lowers it onto **one ready-set scheduler** that reuses the current parallel executor (`run_parallel_group`'s tokio/mpsc machinery, approval queue, limits, and the per-node runtime write-fence) but replaces the *spawn-all-at-once* loop with lazy admission. A node is admitted when its predecessors have succeeded **and** its write scope doesn't intersect any running node (command nodes run in isolation). Conflict-freedom is two-layered: the scheduler guarantees no two intersecting-write nodes co-run, and the unchanged `validate_action_scope` fence enforces each node's scope at runtime. The whole plan surfaces as a single durable, evolving "Plan" chat item that, in `normal` mode, gates execution on a binary accept/reject resolved through the clarification channel; `yolo` auto-accepts. Everything is additive at history `schema_version 1` (new event kinds + a serde-default `graph_id`).

**Primary trade-off:** we reuse the proven executor and fence rather than building a new engine — minimizing new surface and guaranteeing backward-compatible replay — at the cost of a genuinely new correctness burden in the scheduler: a fail-closed ready-set that must proactively terminalize never-admittable nodes to avoid deadlocking the join loop, plus a durable Plan projection that must re-render the full graph snapshot on every node transition.

## System Architecture

### Component Overview

| Component | Responsibility | Boundary |
| --- | --- | --- |
| **Decision schema** (`src/orchestrator/mod.rs`) | New `Dag(ExecutionGraph)` variant, `ExecutionNode`/`ExecutionEdge` types, `validate_execution_graph` (acyclicity, edge endpoints, node-id uniqueness, concurrency-aware disjointness), `schema_version 3` normalization, prompt guidance. | Pure data + validation; no execution. |
| **Ready-set scheduler** (`src/app/mod.rs`) | `run_execution_graph` + `admit_ready_nodes`: lowers the graph onto the reused executor, admits nodes by predecessor-success + write-disjointness + command-isolation, drives fail-closed skips, re-runs admission on every terminal transition. | Owns concurrency/ordering; delegates per-node run to the existing `spawn_parallel_runtime_task`. |
| **Write-fence** (`src/actions/mod.rs`) | Per-node runtime enforcement of `ActionScope::ParallelFileScope`. **Unchanged.** | Second line of defense; node-agnostic. |
| **Plan approval** (`src/app/mod.rs`) | A new `pending_plan_approval` gate before scheduling, in `normal` mode, resolved via the clarification answer channel. | Distinct from the per-action approval queue. |
| **Events** (`src/history/mod.rs`, `src/app/mod.rs`) | `graph_id` field + new graph/node event kinds, additive at `schema_version 1`. | Durable source of truth for replay/projection. |
| **Plan projection** (`src/app/chat/`) | `ChatLifecycleKey::Plan` + `ChatItemKind::Plan` + `apply_execution_graph`: the single evolving Plan item; suppresses per-node live chat items for graph nodes. | Read-only derivation from events. |
| **Config** (`src/config/mod.rs`) | `features.execution_graph` flag; reuse `max_parallel_agent_steps` ceiling; surface enable-state in `/config`. | Gating + visibility. |

**Data flow:** prompt → orchestrator emits `Dag` decision → `validate_execution_graph` → (`normal`) Plan item `WaitingApproval` → accept → `run_execution_graph` admits ready nodes → each node runs via the reused executor under its `ParallelFileScope` → `record_parallel_child_result` triggers re-admission → `execution_graph_completed` → Plan item terminal.

## Implementation Design

### Core Interfaces

The new orchestrator plan types (the contract every other component depends on):

```rust
// src/orchestrator/mod.rs — new Dag arm on the existing internally-tagged enum
pub enum DecisionNextStep {
    SingleAgent(SingleAgentStepPlan),   // unchanged (kind="single_agent")
    ParallelGroup(ParallelGroupPlan),   // unchanged (kind="parallel_group")
    Dag(ExecutionGraph),                // new (kind="dag")
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionGraph {
    pub graph_id: String,
    pub reason: String,
    pub nodes: Vec<ExecutionNode>,
    pub edges: Vec<ExecutionEdge>,
}
```

```rust
// A node mirrors ParallelChildStepPlan + a durable node_id; command-capability
// is derived from required_capabilities (no new field).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionNode {
    pub node_id: String,
    pub step_label: String,
    pub agent: String,
    pub instruction: String,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    pub file_scope: ParallelFileScope,   // reused verbatim by the fence
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionEdge { pub from: String, pub to: String, pub kind: ExecutionEdgeKind }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEdgeKind { DataDependency, SemanticGate } // both gate readiness identically
```

The scheduler's per-node runtime state gains an explicit run-state (today's two-state `terminal_result` cannot distinguish *planned* from *running*):

```rust
// src/app/mod.rs — added to ParallelChildRuntimeState
enum NodeRunState { Pending, Running, Terminal }  // Pending = planned, not yet admitted

// Admission re-runs after the approval gate and after every record_parallel_child_result.
// Admits `node` iff: all predecessors Terminal+succeeded (else mark dependent Skipped),
// AND node.write_files ∩ (union of running nodes' write_files) == ∅,
// AND (node runs commands ⇒ no other node Running).
fn admit_ready_nodes(&mut self, graph: &ExecutionGraph, children: &mut BTreeMap<String, ParallelChildRuntimeState>, /* … */);
```

### Data Models

- **`ExecutionGraph` / `ExecutionNode` / `ExecutionEdge` / `ExecutionEdgeKind`** — above; serialized into the `execution_graph_proposed` event and the `Dag` decision. `ParallelFileScope` (`write_files`, `read_roots`) is reused unchanged.
- **`ExecutionGraphResult`** — terminal aggregate modeled on `ParallelGroupResult` (`src/orchestrator/mod.rs:170`): `{ graph_id, status, node_results: Vec<NodeResultRef>, counts, changed_files, skipped, failed }`; added as `RunStepResult::Dag(ExecutionGraphResult)` and serialized into `execution_graph_completed`.
- **`HistoryEvent`** — gains `#[serde(default)] graph_id: Option<String>` (mirrors `group_id`; threaded through `new_with_group` and the hand-built `append_debug_event`).
- **`ChatLifecycleKey::Plan { graph_id }`** + **`ChatItemKind::Plan`** — the single evolving Plan item's identity.
- **`Features { …, execution_graph: bool }`** (+ `RawFeatures.execution_graph: Option<bool>`) — the default-off gate.

### Orchestrator Decision & Event Contract

(No external/HTTP API — this is a single binary; the "surface" is the orchestrator JSON contract and the durable event log.)

- **Decision contract:** `schema_version: 3`, `status: "continue"`, `next_step: { "kind": "dag", "graph_id", "reason", "nodes": [...], "edges": [{ "from", "to", "kind": "data_dependency" | "semantic_gate" }] }`. Gated in `build_orchestrator_prompt` on `features.execution_graph && max_parallel_agent_steps > 0`.
- **New event kinds** (all `schema_version 1`, registered in `apply_history_event`): `execution_graph_proposed`, `execution_graph_approved`, `execution_graph_rejected`, `node_pending|node_ready|node_running|node_succeeded|node_failed|node_skipped|node_cancelled`, `execution_graph_completed`.

## Integration Points

No external services. Internal boundaries only: the scheduler ↔ the runtime layer (`execute_runtime_step_streaming`, reused), the scheduler ↔ the action/fence layer (reused), and the projection ↔ the watch-channel `AppState` (additive). Each node may run on a different `RuntimeKind` exactly as parallel children do today.

## Impact Analysis

| Component | Impact | Description & Risk | Required Action |
| --- | --- | --- | --- |
| `src/orchestrator/mod.rs` | modified | New `Dag` variant + types + `validate_execution_graph` + `schema_version 3`. Low risk (additive enum arm; compiler-forced match). | Add variant, types, validator, prompt guidance; keep v1/v2 + error strings frozen. |
| `src/app/mod.rs` (executor) | modified | Ready-set scheduler + `NodeRunState`; **deadlock risk** if admission isn't re-run on every terminal. Highest-risk change. | Add `run_execution_graph`/`admit_ready_nodes`; audit every `terminal_result` reader; add the proactive-terminalize invariant. |
| `src/app/mod.rs` (`let-else` casts) | modified | `let Some(DecisionNextStep::ParallelGroup(..))` at `:4249` (and `fake.rs:705`) silently skip `Dag`. | Audit and add `Dag` handling. |
| `src/app/mod.rs` (approval) | new | `pending_plan_approval` gate. Risk: collision with per-action `state.pending_approval`. | Add a distinct state field + projection path. |
| `src/actions/mod.rs` | unchanged | Fence reused as-is per node. | None. |
| `src/history/mod.rs` | modified | `graph_id` field (serde-default) + `append_debug_event` lockstep. Low risk if defaulted. | Add field; thread through both serializers. |
| `src/app/chat/*` | modified | `Plan` key/kind + `apply_execution_graph`; suppress per-node live items for graph nodes. Risk: durable-vs-transient, 12-line cap. | Add handler; register every event kind; summarize large graphs. |
| `src/config/mod.rs` | modified | `execution_graph` flag + `ConfigStatusView` extension. Low risk; golden tests churn. | Add flag, raw field, merge arm, `/config` fields. |
| `src/slash_commands.rs` | unchanged | No new command (automatic trigger). | None. |

## Testing Approach

### Unit Tests
- **`validate_execution_graph`**: rejects cycles, dangling edge endpoints, duplicate `node_id`, and concurrent write-scope overlap; **accepts** sequential nodes sharing a write file (the relaxed-disjointness case); enforces edit-needs-write_files per node. Distinct new error messages (don't reword existing).
- **Admission logic** (pure function over node states + edges): correct ready-set given predecessor success/failure; fail-closed marks dependents `Skipped`; command node admitted only in isolation; write-overlap blocks co-running.
- **Deadlock/forward-progress**: a graph whose tail is all downstream of a failed predecessor drains to all-terminal (no hang).
- **Schema**: `schema_version 3` round-trips `Dag`; v1/v2 decisions and legacy events still deserialize; `kind="dag"` rejected under v2.
- **Projection**: `apply_execution_graph` renders one Plan item evolving through approval→running→completed/failed; skipped nodes render `Skipped`; large graphs collapse to counts; the Plan item is durable (survives a sync, never in `live_keys`).

### Integration Tests
- Drive a full DAG run through the **`fake` runtime** (`tests/runtime_integration.rs` style): a 3-node diamond (A → {B, C} → D) verifies B/C run concurrently, D waits, and timestamps show overlap. Reuse the fake runtime's control-phrase mechanism to script node outcomes (incl. a forced node failure → fail-closed skip of D).
- `normal`-mode approval: plan proposed → Plan item `WaitingApproval` → accept runs; reject-with-reason re-proposes; `yolo` auto-runs.
- Write-fence end-to-end: a node attempting an out-of-scope write is denied (existing fence) while the scheduler independently prevents two intersecting-write nodes from co-running.

## Development Sequencing

### Build Order
1. **Decision schema + types + validation** (`ExecutionGraph`/`ExecutionNode`/`ExecutionEdge`, `Dag` variant, `validate_execution_graph`, `schema_version 3`, prompt guidance). No dependencies.
2. **Config flag** (`features.execution_graph` + raw/merge + ceiling reuse). No dependencies (parallel with step 1).
3. **Event kinds + `graph_id`** (history field, new kinds, recorder plumbing). Depends on **1** (serializes the graph types).
4. **Ready-set scheduler** (`run_execution_graph`, `admit_ready_nodes`, `NodeRunState`, fail-closed, deadlock invariant; `Dag` arm in `handle_orchestrator_decision`; audit `let-else` casts). Depends on **1, 3** (consumes the graph + emits node events).
5. **Plan approval gate** (`pending_plan_approval`, clarification-channel resolution, `yolo` auto-accept). Depends on **3, 4** (gates before scheduling; emits approve/reject events).
6. **Plan projection** (`ChatLifecycleKey::Plan`, `ChatItemKind::Plan`, `apply_execution_graph`; suppress per-node live items for graph nodes). Depends on **3** (renders the new event kinds).
7. **`/config` visibility** (`ConfigStatusView` extension). Depends on **2**.
8. **Integration tests + fake-runtime DAG scripting**. Depends on **4, 5, 6**.

### Technical Dependencies
- The **`fake` runtime** must learn to emit a `Dag` decision and per-node outcomes (extend its control-phrase handling) — required before step 8.
- No infrastructure or external-service dependencies.

## Monitoring and Observability

- **Events (already the durable log):** `execution_graph_proposed/approved/rejected`, `node_*` lifecycle, `execution_graph_completed` (carrying counts, changed files, skipped/failed sets) — replayable and projected.
- **KPI-bearing fields:** node `started_at`/`finished_at` timestamps (wall-clock speedup + effective concurrency), the count of write-fence denials keyed by `graph_id`/`node_id` (conflict-regression signal — target 0), and a topological-order assertion over the event log (dependency-order compliance — target 100%). These directly back the PRD success metrics.
- **Structured fields:** every event carries `graph_id` (+ `node_id` for node events) for grouping.

## Technical Considerations

### Key Decisions
- **Decision:** new `schema_version 3` + `Dag` variant on the existing tagged enum. **Rationale:** keeps v1/v2 frozen for replay; compiler-forced validation arm. **Trade-off:** a third schema version to maintain. **Rejected:** widening v2 (mutates its meaning).
- **Decision:** reuse the parallel executor; replace only the spawn loop with `admit_ready_nodes`. **Rationale:** inherits tokio/mpsc, approval queue, limits, fence. **Trade-off:** the ready-set + deadlock invariant is new and subtle. **Rejected:** a separate DAG engine (duplication).
- **Decision:** concurrency-aware write-disjointness (disjoint only among nodes with no path between them). **Rationale:** sequential dependents legitimately share files. **Trade-off:** validator and runtime admission must agree on the concurrency definition. **Rejected:** global-across-group disjointness (rejects valid graphs).
- **Decision:** command nodes run in isolation. **Rationale:** their unbounded reads/effects can't be fenced. **Trade-off:** less concurrency for command-heavy graphs. **Rejected:** widening the command allow-list (weakens the guarantee).
- **Decision:** one durable Plan item; approval on it via the clarification channel; suppress per-node live chat items. **Rationale:** "one plan I watch"; avoids the per-action approval-slot collision and chat flooding. **Trade-off:** full-graph snapshot per event; 12-line cap → summarize.

### Known Risks
- **Scheduler deadlock** (medium likelihood, high impact): the join loop exits only when all nodes are terminal. *Mitigation:* proactively terminalize never-admittable nodes; re-admit on every terminal transition; a forward-progress test.
- **`terminal_result` overload** (medium): existing readers assume `None == running`. *Mitigation:* introduce `NodeRunState`; audit all readers (loop-exit, `drop_terminal_parallel_approvals`, unrecorded sweep, synthesize).
- **Silent event-kind drop** (low/medium): the projection's `_ => {}` ignores unregistered kinds. *Mitigation:* register every new kind; assert presence in tests.
- **Read-after-write across nodes** (medium): the fence does not order producer-before-consumer; correctness rests on `data_dependency` edges the orchestrator must emit. *Mitigation:* prompt guidance + validation that consumers declare edges; this is the PRD's tracked output-handoff open question — prototype before relying on it.
- **Large-graph legibility** (low): 12-line body cap truncates. *Mitigation:* per-state counts summarization.

## Architecture Decision Records

- [ADR-001: V1 architecture — one ExecutionGraph IR + ready-set scheduler, scope-derived arbitration, typed directed edges](adrs/adr-001.md) — the graph/scheduler/edge model and structural scope arbitration.
- [ADR-002: Deliver the full static DAG in V1 (no measurement gate)](adrs/adr-002.md) — ship the complete feature in one release, council risks accepted.
- [ADR-003: V1 product shape — cohesive full-DAG MVP with a normal-mode binary plan-approval gate](adrs/adr-003.md) — automatic proposal, visible enable state, evolving plan view, fail-closed report-and-stop.
- [ADR-004: DAG decision schema + ready-set scheduler on the reused parallel executor](adrs/adr-004.md) — `Dag` variant @ schema_version 3, `admit_ready_nodes`, concurrency-aware disjointness, command-node isolation, two-layer fence.
- [ADR-005: DAG user-surface integration — enable flag, single evolving Plan item, plan-item approval](adrs/adr-005.md) — `features.execution_graph`, `ChatLifecycleKey::Plan`, approval via the clarification channel, additive event kinds.
