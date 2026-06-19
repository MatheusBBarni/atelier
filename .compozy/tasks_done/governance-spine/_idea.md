# Governance Spine: a Shared Plan IR and Reusable Checkpoint Primitive

## Overview

- **Problem:** atelier's autonomous runs offer governance only at the per-action level (approve this command) or not at all (`yolo`). There's no run-level steering, no checkpoint before irreversible steps, and the `OrchestratorDecision.plan` field is dead code. Meanwhile three separate packets — this one, `approval-trust-list`, and `subtask-dag-execution` — are each about to invent their own plan model and approval surface.
- **Who:** Power users running ambitious autonomous multi-step work ("Ambitious Andy"), and the harness itself — the spine is consumed by the two sibling features, not just end users.
- **Why valuable:** One event-sourced governance primitive + one plan IR across the harness, with the *editorial* intelligence (deterministic risk classification + complexity-gating) that the market lacks — instead of three divergent approval UIs bolted on.
- **V1 ambition:** A **Strategic Bet** with compounding platform upside — a foundation layer. Its own visible feature is the smallest consumer (a turn-1 early-abort glance); its real value is the shared IR + primitive the siblings adopt.

## Summary / Differentiator

"Plan preview before execution" is table stakes (Claude Code, Cline, Devin, Copilot) — and the evidence says it may not even build trust. The white space is **when and how** an agent interrupts: auto-deciding *when* a run is risky enough to gate (no competitor does this — they make you toggle plan-vs-auto), classifying risk *deterministically from the action* (not the model's self-grade), and keeping the executor adaptive (the plan is a guide, not a frozen contract). atelier's wedge: **"plan mode you never have to turn on — it interrupts only when the work is genuinely risky, and a hard gate stops irreversible actions even in yolo."** Delivering that as a *shared spine* — not a third approval modal — is the defensible move.

## Problem

atelier runs autonomous, multi-step agents, but a user's only governance choices today are babysitting every action (`normal` mode) or flying blind (`yolo`). There is no way to steer at the run level, and no enforced pause before an irreversible step. The infrastructure half-exists and is unused: `RunState::Planning` is live but `OrchestratorDecision.plan` is a vestigial `Vec<String>` with zero readers, and routing is myopic and one-step-at-a-time.

The deeper problem is portfolio-shaped. Three in-flight packets overlap on the same primitive: this packet (plan preview), `approval-trust-list` (a fail-closed floor that re-prompts irreversible actions even in `yolo`), and `subtask-dag-execution` F4 (whole-plan structure-approval over committed nodes). Each is about to define its own plan/decision model and its own approve/reject surface. Left uncoordinated, that means three divergent IRs, three projection lifecycles, and three subtly different trust semantics to maintain.

The market makes the strategic case sharper: plan preview is commoditized, and forcing users to review plans can *backfire* — a 248-person study found it did not improve calibrated trust and sometimes hurt it, because users rubber-stamp plausible-but-flawed plans. The thing that actually helps is rationing the user's scarce attention to the *rare, genuinely risky* moments — which is exactly the "editorial" layer no competitor automates. The opportunity is to own that layer once, as a spine, and let the three features consume it.

### Market Data

- **Plan preview before execution is table stakes** — Claude Code plan mode, Cline Plan/Act, Devin interactive planning, GitHub Copilot Plan mode all ship it (2025).
- **Showing the plan does not reliably build trust** — arXiv 2502.01390 (248 participants): user involvement in planning did *not* improve calibrated trust and sometimes reduced plan quality and raised cognitive load.
- **Approval fatigue is security-degrading** — the prescribed fix is "fewer, higher-stakes gates" (~10/session, not 100); bundled approvals can inflate user error ~50%.
- **The cost of no hard gate** — a Cursor agent deleted a production database in ~9 seconds in a single call with no confirmation (2026).
- **Oversight demand** — 68% rate human-in-the-loop "essential/very important"; AI-authored PRs carry ~1.7× more issues than human ones.
- **Closest competitors:** Cline (mechanics: Plan/Act + per-step gates, but gates everything, manual toggle, no complexity gate) and Devin (concept: interactive planning + checkpoints + re-planning, but cloud-first). atelier is terminal-native + multi-runtime.

## Core Features

| #  | Feature | Priority | Description |
|----|---------|----------|-------------|
| F1 | Shared plan/decision IR | Critical | One serializable model — `Plan { nodes: [PlanNode { id, agent, instruction, write_scope, risk }], edges }` — frozen as an event at produce time. The DAG feature enriches it with real edges/parallel groups; scope reasoning consumes `write_scope`. Replaces the dead `OrchestratorDecision.plan`. |
| F2 | Reusable governance-checkpoint primitive | Critical | A `PendingGovernanceDecision` state riding the proven clarification pause/resume (`RunDriveContext` + `drive_and_replay`) and key-routing. Structured Accept/Reject, recorded as events, projected to chat. The single pause-gate all consumers share — not a new modal. |
| F3 | Deterministic risk classification + complexity-gating | Critical | Classify risk from the `ActionRequest` / write-scope / step-count / parallel-groups — **never** the model's self-graded risk. Decide *when* a governance decision surfaces (by irreversibility + complexity, independent of approval mode). The editorial "when to interrupt" the market lacks. |
| F4 | Hard irreversible-action gate contract | High | The enforcement contract: irreversible classes (delete/deploy/force-push/DB-drop/mass-overwrite/out-of-root) gate **even in `yolo`**, fail-closed. The spine defines the contract + classification; the allowlist and post-hoc exact-target trust are implemented by `approval-trust-list` as a consumer. |
| F5 | Turn-1 early-abort intent echo | High | The simplest consumer and this packet's visible feature: a complexity-gated, **non-binding** turn-1 glance with Accept/Reject that aborts before any write, for single-agent / non-decomposed runs. Framed as transparency + early-exit, explicitly **not** a safety guarantee. |
| F6 | Sibling consumer contracts | Medium | Narrow integration contracts so `subtask-dag-execution` F4 (whole-plan approval) and `approval-trust-list` (fail-closed floor) consume the IR + primitive instead of reimplementing them. |

## KPIs

| KPI | Target | How to Measure |
|-----|--------|----------------|
| **Trusted Outcome Rate** *(North Star)* | ≥ 70% of gated runs end in a result the user **keeps** (no revert, corrective re-prompt, or abandon) | Run-outcome events downstream of the decision |
| Checkpoint actionability | ≥ 30% of fired governance decisions are acted on (reject/redirect/typed-confirm), not reflexively accepted | Decision events vs outcome |
| Escaped-risk rate | → 0 irreversible actions that ran ungated and were later reverted | Action classification vs revert events |
| False-accept rate | < 10% of accepted decisions later reverted/corrected (the rubber-stamp readout) | Decision + revert events |
| Gate precision | Decisions fire on ≤ ~25% of runs; ~0% on trivial/read-only | Gate decisions vs run shape |

## Feature Assessment

| Criteria | Question | Score |
|----------|----------|-------|
| **Impact** | More valuable? | **Strong** — a foundational governance primitive unifying three features + the trust-effective gating layer |
| **Reach** | % of users affected? | **Strong** — every gated/autonomous run flows through it via the consumers |
| **Frequency** | How often encountered? | **Strong** — it underlies all governance moments (surfaced selectively, by design) |
| **Differentiation** | Set us apart? | **Strong** — complexity-gating + deterministic classification + non-binding/adaptive + event-sourced unification; "plan preview" alone is **Pass** |
| **Defensibility** | Compounds / hard to copy? | **Strong** — a foundation woven into the event-sourced multi-runtime orchestrator that three features depend on; compounds as consumers adopt |
| **Feasibility** | Can we build it? | **Maybe** — the mechanism (clarification reuse) is proven, but coordinating two in-flight siblings + defining the IR/contracts is real coordination risk |

Leverage type: **Strategic Bet** with **compounding platform** upside.

## Council Insights

- **Recommended approach:** Own the shared IR + reusable checkpoint primitive + deterministic risk classification + complexity-gating; the siblings consume it. This packet's visible contribution is the turn-1 early-abort echo (the simplest consumer). The unanimous council verdict flipped the lead away from the (table-stakes, possibly trust-negative) plan card.
- **Key trade-offs:** plan-preview-leads vs gating/checkpoints-leads; a per-run plan card vs a reusable primitive; building here vs folding into the siblings (resolved: own the spine, siblings consume).
- **Risks identified:** (1) the orchestrator can't plan reliably → the IR must serve single-agent and DAG-supplied plans, not bet on rich orchestrator planning; (2) coordination with two in-flight siblings → narrow consumer contracts, ship spine + early-abort first; (3) rubber-stamping → North Star is *Trusted Outcome Rate*, not "confident completion"; (4) the hard irreversible gate must override `yolo` (security non-negotiable).
- **Stretch goal (V2+):** trust profiles (per-project/task-type posture) as policy *on top of* the spine; richer structured plan editing once real plans exist.

## Integration with Existing Features

| Integration Point | How |
|-------------------|-----|
| Clarification pause/resume (`resolve_pending_clarification`, `drive_and_replay`) | The transport the primitive rides (new `PendingGovernanceDecision`, not an overload of clarification) |
| Key-routing cascade (`src/tui`) | New governance-decision slot in the precedence (between clarification and approval) |
| Event sourcing + projection | New governance-decision event kinds + projection arm; frozen plan/decision for replay determinism |
| `ApprovalMode` (yolo/normal) | The hard irreversible gate overrides `yolo` (fail-closed); complexity-gating is mode-independent |
| `OrchestratorDecision.plan` (dead) | Superseded by the shared Plan IR |
| `subtask-dag-execution` F4 | Consumes the IR + primitive for whole-plan structure-approval over committed nodes |
| `approval-trust-list` | Consumes the gate contract for the fail-closed irreversible-action floor |

## Out of Scope (V1)

- **In-place plan editing** — Accept/Reject only; structured editing deferred (the evidence shows editing adds load without improving trust).
- **Model-inferred risk as a gate signal** — risk may be *shown*, but gating uses deterministic `ActionRequest`/scope classification.
- **The irreversible-action allowlist + post-hoc exact-target trust** — owned by `approval-trust-list` (a consumer), not rebuilt here.
- **DAG topology / scheduler / write-scope fences** — owned by `subtask-dag-execution`; the spine supplies the IR + gate it consumes.
- **Reliable multi-step upfront plan generation** — the IR serves single-agent and DAG-supplied plans; teaching the orchestrator to plan richly is uncertain and not required for V1.
- **Trust profiles / per-project policy** — Alt 3, deferred as a V2+ follow-on.

## Architecture Decision Records

- [ADR-001: Reframe as a governance spine consumed by the sibling packets](adrs/adr-001.md) — own the shared IR + reusable checkpoint primitive + classification; `approval-trust-list` and `subtask-dag-execution` F4 consume it.

## Open Questions

- **Sequencing vs the in-flight siblings:** refactor `approval-trust-list` + `subtask-dag-execution` to consume the spine now, or ship the spine + early-abort first and migrate them? Who owns the coordination?
- **Ownership boundary:** the irreversible-action *allowlist* (approval-trust-list) vs the *classification mechanism* (spine) — exact line.
- **Complexity-gate thresholds:** what counts as "non-trivial" (write-step count, parallel group, out-of-root)?
- **Yolo override:** confirm the hard irreversible gate fires even in `yolo` by default (security says yes).
- **Early-abort value:** does the turn-1 early-abort echo earn its place, or is it pure transparency that should fold into DAG once real plans exist? (devils-advocate's residual skepticism.)
