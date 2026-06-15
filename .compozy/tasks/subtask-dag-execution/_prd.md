# PRD: Sub-task DAG Execution

## Overview

atelier runs a user's prompt through an orchestrator that today advances mostly one agent step at a time. Its only concurrency primitive — flat parallel groups — is off by default, capped at 2, and cannot express dependencies: every child runs at once and none can build on another's work. **Sub-task DAG execution** lets the orchestrator propose a dependency graph of sub-tasks, the user approve that whole plan once (in `normal` mode), and a scheduler run independent sub-tasks concurrently while an enforced write-fence guarantees no two of them touch the same file. It is for developers running multi-file, multi-step work in atelier who want it done **faster without losing safety or oversight**. It is valuable because it converts work the harness currently serializes into safe parallel execution, and because it replaces per-action approval fatigue with a single, informed, plan-level consent — the speed of parallel agents with the trust model of plan-mode and a file-conflict guarantee neither competitor offers.

## Goals

- Cut wall-clock time on decomposable tasks by running independent sub-tasks concurrently, with a measurable speedup vs serial execution.
- Guarantee **zero file-conflict regressions**: concurrent sub-tasks can never write the same file, enforced at runtime, not by advisory instruction.
- Replace per-action approval drip with **one informed, plan-level consent** in `normal` mode, while keeping `yolo` fully fast.
- Give users **legible, real-time visibility** into what is running, what is blocked, and what is done across a concurrent run.
- Establish a clean, durable plan/graph model that becomes the foundation for later autonomy (retry/resume, dynamic re-planning) without a rewrite.

## User Stories

**Primary — the power-driver (multi-file tasks):**
- As a developer giving atelier a task with independent parts, I want the harness to run those parts concurrently so the whole task finishes faster.
- As a developer, I want to know that two concurrent sub-tasks can never clobber the same file, so I can trust parallel execution without babysitting it.

**Primary — the reviewer-in-the-loop (`normal` mode):**
- As a cautious user, I want to review the *entire* proposed plan once — what each sub-task will do and which files it may touch — and approve or reject it in one decision, instead of answering a stream of per-action prompts.
- As a reviewer, I want to reject a plan with a short reason and have the orchestrator propose a better one, so I'm not forced to accept a plan I don't like.

**Primary — the walk-away user (`yolo`, default):**
- As a user who wants speed, I want approved-by-default runs to start immediately and run to completion, so I can step away without the agent stalling on a prompt.

**Secondary — watching a run:**
- As any user, I want a single live view of the plan that shows each sub-task's status (waiting / ready / running / done / failed) and what a blocked task is waiting on, so I don't lose track of a concurrent run.
- As any user, when a sub-task fails, I want to see clearly which work completed and which was skipped and why, so I know exactly where to pick up.

**Secondary — discoverability:**
- As a new user, I want to see whether DAG planning is enabled and learn how the plan-approval gate works, so the new behavior isn't surprising.

## Core Features

| # | Feature | Priority | What it does |
| --- | --- | --- | --- |
| F1 | Automatic plan proposal + visible enable state | Critical | The orchestrator proposes a dependency graph when a prompt benefits from one; no per-prompt gesture. Whether DAG planning is enabled is visible and controllable in-app. |
| F2 | Whole-plan consent gate (`normal` mode) | Critical | In `normal` mode, the run pauses and the user accepts or rejects the entire proposed plan in one decision; a rejection can carry a short reason that returns the plan for re-proposal. `yolo` auto-approves and runs immediately. |
| F3 | Legible plan presentation | Critical | The plan shows each sub-task's intent and agent, the ordering between sub-tasks, and the exact files each sub-task may touch — so consent is informed, not reflexive. |
| F4 | Enforced write-fence guarantee | Critical | The product promises and enforces that two concurrently-running sub-tasks can never write the same file; a sub-task attempting to write outside its declared files is stopped. Presented as a user-facing trust guarantee. |
| F5 | Live evolving plan view | High | A single persistent view of the whole plan updates in place during execution, showing per-sub-task status (waiting / ready / running / done / failed), the dependency a waiting task is blocked on, and its file scope. |
| F6 | Concurrency that respects ordering | High | Independent sub-tasks run at the same time; dependent sub-tasks wait until what they depend on has finished. This is the source of the wall-clock speedup. |
| F7 | Fail-closed reporting | High | If a sub-task fails, its dependents are skipped (never run on incomplete state), and the run ends with a clear report of what completed vs what was skipped and why; the user re-prompts to continue. |

## User Experience

**Personas & goals:** the *power-driver* wants speed with trust; the *reviewer-in-the-loop* wants one informed consent instead of prompt fatigue; the *walk-away user* wants approve-once-then-run; all want to not lose track of a concurrent run.

**Primary flow — reviewer (`normal` mode):**
1. User submits a prompt. The orchestrator determines the task decomposes and proposes a plan.
2. The run pauses on a **plan-review panel** (modeled on the existing clarification surface): the proposed sub-tasks, the ordering between them, each sub-task's agent, and the files each may touch, with *approve* as the recommended default.
3. The user accepts (the plan runs) or rejects with an optional reason (the orchestrator re-proposes).
4. During execution, the user watches the **evolving plan view** — sub-tasks move waiting → ready → running → done as the scheduler releases them; blocked sub-tasks show what they're waiting on.
5. On completion, a closing summary; on a failure, the fail-closed report.

**Primary flow — walk-away (`yolo`, default):** identical, except the plan is auto-approved and execution begins immediately with no gate; the user still gets the live plan view and the same write-fence guarantee.

**Onboarding & discoverability:** DAG planning ships **off by default** and exposes a **visible enable state** in-app (the parallel/approval settings are invisible today). The help overlay teaches the plan-approval gesture and states the write-fence guarantee. A one-line first-time explainer introduces the plan gate, mirroring the existing first-approval explainer.

**Legibility guardrails:** the plan view uses the familiar node + dependency + per-status mental model; per-sub-task file scope is always shown so large plans remain reviewable; a sane concurrency ceiling keeps the number of simultaneously-running sub-tasks within what a person can supervise.

## High-Level Technical Constraints

- **Gated, off by default.** The capability is opt-in until validated; its enabled/disabled state is user-visible.
- **Mode-consistent.** The plan gate appears in `normal` mode; `yolo` remains fully automatic. Existing capability and workspace write-root limits continue to apply per sub-task.
- **Enforced isolation, single workspace.** Safety comes from per-sub-task file scopes + the runtime write-fence within one workspace — not from spawning separate repository copies.
- **Bounded speed expectation.** Speedup is real but bounded by how independently a task decomposes and by a concurrency ceiling; the product should present speed honestly, not as unbounded.
- **Frozen command surface.** The visible slash-command set is fixed; V1 introduces no new command (the feature is automatic), and any future command is a separate scope decision.

## Non-Goals (Out of Scope)

- **A general workflow / CI / pipeline-authoring tool.** The graph is orchestrator-proposed for a single prompt, not a user-built, reusable, scheduled pipeline.
- **Git-worktree / branch-per-agent isolation.** Safety is per-sub-task file scopes + write-fence in one workspace, not branch merging.
- **A graphical DAG editor.** Terminal-legible node + dependency + status is the bar; no visual graph IDE.
- **Editing or pruning the plan before approval.** V1 is binary accept/reject; node-level pruning and instruction editing are deferred.
- **Mid-run per-action approvals or per-node clarifications.** Consent is at plan altitude; the run does not drip prompts during execution.
- **Retry/resume of a failed plan and dynamic re-planning.** Failure is report-and-stop in V1; recovery and re-planning are V2+.
- **Cross-provider node placement and cross-prompt (`/queue`) concurrency.** Complementary throughput ideas deferred so V1 focuses on the intra-prompt plan.
- **Removing human consent.** `yolo` remains, but the feature's value is the *option* of informed single-approval, not eliminating it.

## Phased Rollout Plan

### MVP (Phase 1) — the full static DAG
- F1–F7: automatic plan proposal + visible enable state; `normal`-mode binary plan consent (with reject-reason re-propose); legible plan with per-node scope; enforced write-fence guarantee; live evolving plan view; ordering-respecting concurrency; fail-closed reporting. Ships behind the default-off flag.
- **Proceed to Phase 2 when:** DAG runs clear the wall-clock speedup target on decomposable tasks, zero file-conflict regressions are observed in real use, and users approve proposed plans at a healthy rate without wholesale rejection.

### Phase 2 — recovery & control
- Retry/resume of the failed sub-graph without re-running completed work; node-level pruning/editing at the approval gate; cross-provider node placement for additional concurrency.
- **Proceed to Phase 3 when:** retry/resume is used and trusted, and plan-editing reduces wholesale rejections.

### Phase 3 — toward autonomy
- Dynamic re-planning (a verify/grade step that re-plans a failed sub-graph instead of stopping) and longer unattended ("run overnight") execution.
- **Long-term success:** users routinely hand atelier large, decomposable work and trust it to run safely to completion.

## Success Metrics

| Metric | Target | From the user's perspective |
| --- | --- | --- |
| Wall-clock speedup (decomposable tasks) | ≥ 30% vs serial, tasks with ≥3 independent sub-tasks | Tasks finish meaningfully faster |
| File-conflict regressions | 0 | Concurrent work never corrupts a shared file |
| Effective concurrency | ≥ 2.0 avg simultaneously-running sub-tasks on multi-node runs | Real parallelism, not serialized-with-ceremony |
| Dependency-order compliance | 100% | Dependent work never runs on incomplete inputs |
| Plan acceptance | ≥ 70% of proposed plans approved without wholesale rejection | Proposed plans match what users actually want |
| Adoption (once enabled) | Majority of eligible multi-step runs use a DAG plan | The capability is reached, not ignored |

## Risks and Mitigations

- **Approval fatigue re-emerging.** If the run later drips re-approvals or per-node prompts, it reintroduces the fatigue this feature kills. *Mitigation:* one consent at plan altitude; fail-closed report-and-stop; no mid-run per-node prompts in V1.
- **Rubber-stamping large plans.** Oversized plans get approved without real review. *Mitigation:* show per-sub-task file scope, keep plans right-sized, and present a legible node/dependency/status view.
- **Over-parallelization inverting the benefit.** Beyond a sane ceiling, supervision and review cost eats the speed gain. *Mitigation:* a concurrency cap and honest, bounded speed messaging.
- **Trust gap if the guarantee leaks.** The write-fence is a headline promise; any perceived breach destroys trust. *Mitigation:* enforce at runtime and make the guarantee legible in the plan and help.
- **Low decomposability undercutting the value.** If real prompts rarely split into independent sub-tasks, the speedup rarely materializes. *Mitigation:* keep the feature opt-in/off-by-default, measure realized speedup, and fall back to single-agent runs when no independent work exists.
- **Discoverability.** A new gesture and a default-off capability can go unnoticed. *Mitigation:* visible enable state + help-overlay teaching + first-time explainer.
- **Competitive parity.** Competitors may add dependency-aware parallelism. *Mitigation:* lead on the enforced write-fence + single informed consent + runtime-agnostic fan-out, which single-vendor tools structurally lack.

## Architecture Decision Records

- [ADR-001: V1 architecture — one ExecutionGraph IR + ready-set scheduler, scope-derived arbitration, typed directed edges](adrs/adr-001.md) — the graph/scheduler/edge model and structural scope arbitration.
- [ADR-002: Deliver the full static DAG in V1 (no measurement gate)](adrs/adr-002.md) — ship the complete feature in one release, with council risks accepted.
- [ADR-003: V1 product shape — cohesive full-DAG MVP with a normal-mode binary plan-approval gate](adrs/adr-003.md) — automatic proposal + visible enable state, `normal`-only binary consent, evolving plan view, fail-closed report-and-stop.

## Open Questions

- **Output handoff:** how much of a finished sub-task's result flows to a dependent sub-task, and how is it bounded so a wrong/over-summarized handoff can't silently corrupt downstream work? Is a verify step needed on data-dependency links? (The council's sharpest risk.)
- **Read-dependency detection:** today read scopes are coarse; how do we reliably order "B reads what A writes" across different files without over-serializing?
- **Command sub-tasks:** sub-tasks that run commands (e.g. tests/builds) effectively read the whole workspace — how should the schedule treat them relative to writers?
- **Decomposition reality:** what fraction of real prompts actually split into ≥3 independent sub-tasks (validates the speedup goal)?
- **Concurrency ceiling:** what is the right user-visible cap, and should the user be able to change it?
- **Enable-state surface:** exactly where the user sees/sets whether DAG planning is on (today the parallel settings are invisible in-app).
- **Plan-view density:** how to keep the evolving plan view legible for larger graphs in a flat terminal transcript.
