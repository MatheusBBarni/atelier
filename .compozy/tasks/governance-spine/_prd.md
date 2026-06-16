# PRD: Governance Spine — Shared Decision Contract + Single-Agent Early-Abort

## Overview

atelier is growing three separate governance surfaces: per-action approval and a hard irreversible-action floor (`approval-trust-list`), and whole-plan consent over a dependency graph (`subtask-dag-execution`). Both are already fully specified — and both independently invented their own decision card and plan model. Left alone, atelier ends up with three subtly different "the agent paused to ask you something" experiences, and one autonomy gap: a **single-agent run** (no DAG, no per-action stop) can start editing on a *misread goal* with no chance to catch it.

This feature defines a **shared governance-decision contract** — one decision card and one way of presenting "what's about to happen" — that all three surfaces conform to, and ships the one genuinely-new capability that gap implies: a **turn-1 early-abort** for single-agent runs that shows the orchestrator's interpreted goal and lets the user abort before any write. It is for developers running ambitious autonomous work who want a coherent, trustworthy way to be asked — and a single place to catch a misunderstanding cheaply. The siblings migrate onto the shared contract on their own schedule; nothing finished is reopened in V1.

## Goals

- **One coherent governance experience:** every pause (per-action, whole-plan, early-abort) looks, reads, and behaves the same.
- **Close the single-agent autonomy gap:** let a user catch a misread goal *before any work* on runs that neither sibling covers.
- **Measure honestly:** judge governance by downstream outcomes (kept vs reverted), not by "ran without pausing."
- **No disruption to finished work:** set the contract; let the two specced siblings adopt it incrementally.

## User Stories

**Primary — "Ambitious Andy" (single-agent autonomous runs):**
- As a developer kicking off a non-trivial single-agent run, I want to see what the agent *understood* I asked and where it may write, so I can abort or redirect a misinterpretation before it edits anything.
- As a developer, I want that pause to appear only when the work is genuinely non-trivial, so trivial/read-only runs never interrupt me.

**Any user (consistency):**
- As a user, I want every governance pause — approving a command, approving a plan, or reviewing an intent — to use the same layout, keys, and risk labeling, so I never relearn the surface.

**Harness maintainer (the spine's other consumer):**
- As a maintainer, I want one shared decision contract so the three governance surfaces can't silently diverge as each evolves.

## Core Features

**Critical**
- **Shared governance-decision contract (CF1).** One decision-card model every pause renders through: the action in plain language, the fully-qualified target, blast-radius/reversibility, the plain-language reason it paused, an explicit risk label, and clearly-separated decision affordances with consistent keys. Risk is conveyed in words, not color alone.
- **Shared plan/intent legibility model (CF2).** One way to present "what's about to happen," spanning a single-agent intent echo and a multi-node graph, so plans are reviewable the same way regardless of surface.
- **Single-agent turn-1 early-abort (CF3).** Complexity-gated and non-binding. Before any write, it shows the orchestrator's *interpreted goal*, its intended approach, the chosen agent, and the run's write-scope. Accept proceeds; Reject aborts with an optional redirect. Trivial/read-only runs never trigger it.

**High**
- **Outcome-based governance metrics (CF4).** Derived locally: the kept-vs-reverted rate of governed runs, plus an intervention-rate calibration that alarms on **both** extremes (too high = fatigue; near-zero alongside rising reverts = missing guardrails). No "completed without pausing" success signal.

**Medium**
- **Sibling adoption contract (CF5).** A defined interface so `approval-trust-list`'s decision card and `subtask-dag-execution`'s plan view conform to the shared model — adopted on each sibling's schedule, not retrofitted in V1.

## User Experience

**The early-abort flow (single-agent run):**
1. The user submits a non-trivial prompt. The orchestrator interprets it and selects an agent.
2. Because the run is complexity-gated in (write-capable, non-trivial), it pauses on a **governance-decision card** — the same card the other surfaces use — showing: *"Understood goal: …"*, the intended approach, the agent, and the write-scope it may touch, with a recommended-but-not-default Accept.
3. The user **Accepts** (the run proceeds), or **Rejects** with an optional redirect (the orchestrator re-interprets). Nothing has been written yet.
4. Read-only or trivial runs skip this entirely.

**Consistency:** approving a command (`approval-trust-list`), approving a plan (`subtask-dag-execution`), and this intent echo all render through the same card — same fields, same keys, same risk labeling — so the user learns one surface.

**Discoverability:** the Help overlay's Approvals section documents the early-abort and the unified card; a one-time first-run explainer (reusing the existing pattern) introduces it the first time it fires.

## High-Level Technical Constraints

- **Local-only, no telemetry.** All behavior and metrics derive from atelier's existing local activity record; nothing leaves the machine.
- **Reuse the existing pause surface.** The early-abort rides the established pause/resume and chat-projection patterns, not a new modal.
- **Clear ownership boundary.** The spine references but does **not** reimplement: the hard irreversible floor stays in `approval-trust-list`; the dependency-graph IR and scheduler stay in `subtask-dag-execution`.
- **Color-capability legibility.** Risk signaling must hold under truecolor, 256-color, and monochrome (`NO_COLOR`).
- **Complexity-gating is mode-independent;** it does not change what `yolo` permits (the irreversible floor that fires even in `yolo` is the sibling's, unchanged).

## Non-Goals (Out of Scope)

- **Rebuilding the hard irreversible-action gate** — owned by `approval-trust-list`; the spine references it.
- **Rebuilding the dependency-graph IR / scheduler / whole-plan gate** — owned by `subtask-dag-execution`.
- **Retrofitting the siblings' shipped specs in V1** — migration is a later phase on their schedule.
- **In-place plan/intent editing** — Accept/Reject (+ redirect) only; structured editing deferred.
- **Reliable multi-step upfront plan generation** — the early-abort echoes an interpreted goal, not a synthesized multi-step plan.
- **A governance metrics dashboard UI** — metrics are computed/queryable, not a new visual surface in V1.
- **Per-project trust profiles / policy layer** — deferred to a later phase.

## Phased Rollout Plan

### MVP (Phase 1)
- CF1–CF4: the shared decision contract + plan-legibility model, the single-agent early-abort, and outcome-based metrics, behind an opt-in flag.
- **Success criteria to proceed:** the early-abort catches misread goals at a healthy rate without inflating intervention rate into fatigue; outcome metrics are live; the shared contract is defined and documented for the siblings.

### Phase 2 — sibling migration
- `approval-trust-list`'s card and `subtask-dag-execution`'s plan view adopt the shared contract (de-duplication), each on its own schedule.
- **Success criteria:** all three surfaces render through the shared model with no divergence; no regression in either sibling's behavior.

### Phase 3 — policy layer
- Per-project/task-type trust profiles that tune *when* any governance pause fires, built on the spine.
- **Long-term success:** one governance experience, calibrated per context, that users trust to run ambitious work.

## Success Metrics

| Metric | Target | Lens |
|---|---|---|
| **Trusted Outcome Rate** *(North Star)* | ≥ 70% of governed runs end in a result the user **keeps** (no revert / corrective re-prompt / abandon) | Outcome |
| Governed-run revert/rework rate | Trends down vs ungoverned baseline; no rise alongside falling intervention | Outcome (dual-alarm) |
| Intervention-rate calibration | Within a healthy band — alarms if too high (fatigue) **or** near-zero with rising reverts (missing guardrails) | Calibration |
| Early-abort catch rate | 5–25% of fired early-aborts are rejected/redirected (proves it catches misreads, isn't theater) | New-capability value |
| Gate precision | Early-abort fires on ≤ ~25% of runs; ~0% on trivial/read-only | Anti-friction |
| Time-to-decide | Median < 20s on the decision card | Legibility |

## Risks and Mitigations

- **Coordination with two finished siblings.** *Mitigation:* contract-first; migrate on each sibling's own schedule; reference, don't reopen.
- **The early-abort doesn't earn its keep** (a preview of an interpreted goal may be low-value). *Mitigation:* complexity-gate it and make its catch rate a Phase-1 proceed-gate; cut it if it's pure ceremony.
- **Rubber-stamping** (the documented failure of always-on previews). *Mitigation:* complexity-gating + outcome-based dual-alarm metrics that refuse to reward "ran without pausing."
- **The spine's core value (consistency) is invisible.** *Mitigation:* lead with the new capability as the visible win; treat unification as the durable-but-quiet benefit.
- **Overlap confusion across three packets.** *Mitigation:* explicit ownership boundaries in this PRD and the ADRs.

## Architecture Decision Records

- [ADR-001: Reframe as a governance spine consumed by the sibling packets](adrs/adr-001.md) — own the shared contract/IR; siblings consume.
- [ADR-002: V1 product shape — shared contract + early-abort, phased sibling migration](adrs/adr-002.md) — ship the contract + the net-new capability; migrate siblings later; reference, don't reimplement, their assets.

## Open Questions

- **Exact contract surface:** the precise fields and lifecycle the shared card/legibility model standardizes (vs left to each consumer).
- **Migration coordination:** who sequences the sibling adoptions, and against which milestones?
- **Complexity-gate thresholds** for the early-abort (write-step count? write-scope size? out-of-root?).
- **Interpreted-goal reliability:** how good must the orchestrator's goal restatement be for the echo to be worth the pause?
- **Boundary with the catastrophic floor:** exact line between the spine's general decision card and `approval-trust-list`'s always-on irreversible gate.
- **Does the early-abort survive its Phase-1 catch-rate gate**, or fold into the DAG flow once real plans exist?
