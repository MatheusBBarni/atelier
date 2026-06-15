# Idea: Sub-task DAG Execution

## Overview

`atelier` already fans out **flat parallel groups** — independent child agents with disjoint, runtime-enforced file scopes — but children cannot depend on one another, and the group spawns all at once. **Sub-task DAG execution** turns that flat fan-out into a real dependency graph: the orchestrator proposes a directed acyclic graph of sub-task *nodes* connected by typed ordering *edges*, the user approves the whole plan once, and a single **ready-set scheduler** runs every node the moment its predecessors finish and no concurrently-running node shares its write scope. The goal is **maximum safe concurrency** — measurable wall-clock speedup and higher effective concurrency with **zero conflict regressions**. It is framed as a Strategic Bet: the node/edge/scheduler model is architected as the clean foundation for later autonomy (resumable runs, dynamic re-planning), and V1 ships the full static graph — including orchestrator-authored ordering edges and output handoff — in one release.

## Problem

atelier's orchestrator advances a run mostly one agent step at a time. The one concurrency primitive it has — flat parallel groups — is gated OFF by default (`parallel_step_groups`), capped at 2 children (`max_parallel_agent_steps`), and structurally limited: every child spawns simultaneously and **no child can consume another's output**. So any task with internal structure — "update the API type, then regenerate the client and the tests that read it," "scaffold three modules then wire them together" — collapses back to slow serial single-agent steps, even when large parts of the work are genuinely independent. The harness leaves parallelism on the table precisely when a task is big enough to benefit.

The deeper problem is **safe** concurrency. Running agents in parallel is easy; running them without clobbering each other's files is the hard part the whole industry punts on. The dominant answer (git-worktree isolation) defers conflicts to a later, error-prone merge. atelier is unusual in already enforcing per-node write scopes at runtime — `validate_action_scope` hard-denies any write outside a child's exact `write_files` (`src/actions/mod.rs:218`). What's missing is the layer above: a graph that expresses *ordering* (this node must finish before that one starts) and a scheduler that exploits independence up to that ordering while the existing fence guarantees no two concurrent nodes collide.

### Market Data

- **Enterprise coding-agent market:** ~$9.8B (2025) → ~$148.2B (2034), ~35% CAGR; the autonomous-agent subsegment grows ~38.5% CAGR and the "specialized agent" slice ~52.1% (MarketIntelo).
- **Agentic AI:** ~$7.6B (2025) → ~$10.8B (2026); Gartner projects **40% of enterprise apps will embed task-specific agents by end-2026**, with the market explicitly shifting "from single-threaded assistance to orchestrated multi-agent workflows."
- **Performance prior art:** LLMCompiler reports **~3.6× latency / ~6.7× cost** improvement from DAG-scheduled parallel tasks vs sequential execution.
- **Competitive gap:** Claude Code subagents fan out up to 10 parallel tasks but have **no file-conflict mechanism** and require manual dependency ordering; LangGraph is a Python library where concurrency safety is the developer's problem; Temporal is heavy server infra; OpenHands is a cloud platform. No competitor combines plan-time, file-scope-aware conflict prevention with a structure-approval gate in a single terminal binary.

## Summary / Differentiator

atelier can claim a seam no one else fills: **file-scope-aware conflict *prevention* at plan time + a structure-approval gate + runtime-agnostic fan-out, in one terminal-native binary.** Independence is proven structurally from declared scopes (not guessed), conflicting writes are rejected by an existing runtime fence (not merged later), the user approves the graph's shape before any write lands, and because each node carries its own agent/runtime, independent nodes can run on *different* model providers concurrently — something single-vendor SDKs structurally cannot do.

## Core Features

| # | Feature | Priority | Description |
| --- | --- | --- | --- |
| F1 | ExecutionGraph IR + ready-set scheduler | Critical | One canonical graph model; SingleAgent (1 node) and ParallelGroup (N nodes, 0 edges) become degenerate cases. A single scheduler admits a node when all predecessors complete **and** no running node shares its write scope, retiring the all-spawn-at-once path. |
| F2 | Structural scope arbitration (schedule-time) | Critical | Generalize today's decision-time disjoint-write check into schedule-time admission. The existing runtime write fence stays as both the enforcement boundary and the instrument that makes "zero conflict regressions" observable. |
| F3 | Typed directed edges (`data_dependency` / `semantic_gate`) | Critical | Orchestrator-authored ordering for cases scope cannot express (read-after-write across disjoint files; design→implement; edit→test). Cycle detection; minimal-edge expectation; never free-form mutex edges. |
| F4 | Structure-authorizing whole-plan approval | High | User approves the proposed graph — nodes, edges, concurrency, per-node touch-sets, handoff sources — once. In-scope writes are waived thereafter; `RunCommand` and out-of-scope escalations stay gated in `normal`; `yolo` stays fast. |
| F5 | Output handoff between nodes | High | An upstream node's result is passed as explicit, bounded, attributed context to its dependent nodes and recorded as events. |
| F6 | Durable `node_id` + event-sourced node lifecycle | High | Opaque node ids and a `graph_id`; lifecycle events (Pending→Ready→Running→Succeeded/Failed/Skipped/Cancelled); a `ChatLifecycleKey::Node` projection. This is the foundation that makes V2 resume/re-planning a policy relaxation. |
| F7 | Fail-closed mid-graph failure handling | Medium | On a node failure, descendants are poisoned and cancelled; writes use atomic temp+rename so cooperative cancellation cannot leave torn files. |
| F8 | Concurrency/speedup instrumentation + replay baseline | Medium | Measure effective concurrency, realized speedup, and conflict-deny events; replay real prompts from `.atelier/` history to establish the KPI baseline and decomposition-width distribution. |

## KPIs

| KPI | Target | How to Measure |
| --- | --- | --- |
| Wall-clock speedup (parallelizable tasks) | **≥ 30%** vs serial baseline, tasks with ≥3 independent nodes | DAG vs serial run duration on a fixed benchmark + history replay (event timestamps) |
| Effective concurrency | **≥ 2.0** avg in-flight nodes on multi-node runs | Mean concurrent running nodes/run from `node_started`/`node_completed` events |
| Conflict regressions | **0** escaped write collisions | Plan-time overlap rejections + runtime fence denials; assert none reach the filesystem |
| Dependency-order compliance | **100%** (no node starts before its predecessors complete) | Topological-order assertion over the event log per run |
| Plan usability | **≥ 70%** of proposed DAGs approved without whole-plan rejection | `approval_resolved` on `dag_plan_proposed` events |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | Must do |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Must do |
| **Defensibility** | Is this easy to copy or does it compound over time? | Strong |
| **Feasibility** | Can we actually build this? | Strong |

Leverage type: **Strategic Bet**

## Council Insights

- **Recommended approach:** One `ExecutionGraph` IR + a single ready-set scheduler (flat groups become a degenerate case); structural, always-on scope arbitration; typed minimal directed edges; durable `node_id` + event-sourced lifecycle; fail-closed. The product owner ratified building the **full static DAG** — including LLM-authored ordering edges, output handoff, and structure approval — in V1 (ADR-002).
- **Key trade-offs:** scope overlap is *symmetric* mutual exclusion vs *directed* ordering (edges needed for the latter); edges derived-from-scope vs authored-by-LLM; whole-plan approval vs the per-action gate; build-the-full-feature-now vs measure-first.
- **Risks identified:** (1) *missing-edge silent-wrong-result* — a forgotten ordering edge yields disjoint scopes, zero conflict alarms, and a silently wrong node; (2) *output-handoff corruption* — the sharpest surface, invisible to all three KPIs and load-bearing on the linear common case; (3) *Amdahl / low decomposition* — narrow tasks pay overhead for no speedup; (4) *approval theater*. Mitigations: independence is derived structurally (only ordering is authored); informed-consent approval enumerating scopes/edges/handoff sources; optional verify node; replay baseline + single-agent fallback; the fence enforces regardless.
- **Stretch goal (V2+):** durable resume of an interrupted graph + a verify/grade node that re-plans a failed *subgraph* instead of failing the whole run + runtime-heterogeneous placement across providers — "leave atelier running on real work overnight." The IR makes these a policy relaxation, not a rewrite.

## Integration with Existing Features

| Integration Point | How |
| --- | --- |
| Flat parallel groups (`run_parallel_group`) | Becomes the 0-edge degenerate case of the ExecutionGraph; the tokio/mpsc coordination is reused. |
| Runtime write fence (`validate_action_scope`) | Reused unchanged per node as the enforcement + conflict-detection instrument. |
| Approval queue / `ApprovalMode` | Extended with a whole-plan structure-approval; per-action prompts reused for `RunCommand`/out-of-scope in `normal`. |
| Event sourcing (`group_id`) | Add a `graph_id` and node lifecycle event kinds; replay rebuilds graph state deterministically. |
| Chat projection (`ChatLifecycleKey::Step`) | Add a `ChatLifecycleKey::Node` so each node evolves as one chat item; graph status renders like group status. |
| `WorkflowTarget` ledger | Node write scopes populate the ledger as today; node-level completion reconciles per node. |
| Config (`[features]`, `[limits]`) | Reuse `parallel_step_groups` (default OFF); revisit `max_parallel_agent_steps` / a graph-width cap. |

## Out of Scope (V1)

- **Dynamic node-spawning / mid-run re-planning** — the IR supports it later as a policy relaxation; V1 commits to a *static* graph to bound the scheduler's state machine.
- **Resumable / persisted partial DAGs across sessions** — deferred to V2; event-sourcing lays the groundwork but the resume *mechanism* is out.
- **Mid-run plan editing** — adds approval/projection complexity; V1's surface is whole-plan approve/reject.
- **Free-form LLM mutex edges** — rejected in favor of structural scope arbitration; only typed *ordering* edges are authored.
- **Observed-edge incremental scheduling** — V1 uses static prediction; observing actual writes to confirm independence is a V2 hardening that would shrink the missing-edge risk.
- **Cross-provider placement & cross-prompt (`/queue`) concurrency** — complementary throughput enhancements deferred so V1 focuses on the intra-prompt DAG.

## Architecture Decision Records

- [ADR-001: V1 architecture — one ExecutionGraph IR + ready-set scheduler, scope-derived arbitration, typed directed edges](adrs/adr-001.md) — adopts the graph/scheduler/edge model and structural scope arbitration.
- [ADR-002: Deliver the full static DAG in V1 (no measurement gate)](adrs/adr-002.md) — ratifies building the complete feature (LLM-authored edges + handoff + structure approval) in one release, with the council's risks accepted and assigned mitigations.

## Open Questions

- **Output-handoff contract:** how much of an upstream node's output flows to a dependent node, and how is it bounded/attributed to avoid silent corruption (the sharpest risk)? Is a verify node required on `data_dependency` edges?
- **Read-scope granularity:** `read_roots` are coarse directories today — how to detect read-after-write across files without over-serializing? Are explicit `data_dependency` edges the interim answer?
- **Command nodes:** `cargo test`/`RunCommand` nodes have effectively unbounded read scope — how does admission treat them (serialize after all in-scope writers)?
- **Decomposition reality:** what fraction of real prompts decompose into ≥3 genuinely independent file-scoped nodes (replay-measurement validates the speedup KPI)?
- **Approval UX:** how to render the graph + per-node touch-sets + handoff sources compactly in the TUI so consent is informed, not rubber-stamp?
- **Concurrency cap:** keep `max_parallel_agent_steps` at 2 when the DAG ships, or raise it / introduce a separate graph-width cap?
- **Validation surfacing:** how are cycle-detection and malformed-graph errors surfaced back to the orchestrator/user?
