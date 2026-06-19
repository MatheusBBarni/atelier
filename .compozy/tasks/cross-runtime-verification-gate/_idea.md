# Cross-Runtime Verification Gate

## Overview

A high-risk diff produced by one model is, today, blessed by that same model — atelier has no way to get an *independent* opinion from a different model lineage. This feature extends atelier's existing `council` review routing so the reviewer runs on a model whose **family differs** from the diff's producer, turning the harness's heterogeneous runtimes (Codex/Claude/Cursor/Z.ai) into a genuine second opinion that doesn't inherit the producer's blind spots.

It is for power users running high-risk autonomous work who want a real review without manually re-running a second model — **Unattended Uma**, **Skeptical Sam**, and **Multi-runtime Mia**. It complements (does not replace) the shipped deterministic self-grading gate: that gate answers *"did it pass the tests?"*; this answers *"is it actually right, judged by a different mind?"* — exactly the judgment layer where there is no test oracle.

V1 is a **Strategic Bet, sequenced as a Quick Win**: build the durable primitives correctly (provenance fact + family predicate), but ship the thinnest **on-request advisory probe** first to validate that a different family catches things users act on, before paying for an auto-trigger or fixer.

## Summary / Differentiator

The AI code-review market is commoditized on a single axis — single-fixed-model PR review (CodeRabbit, Greptile), or same-model multi-pass (Cursor BugBot), or same-vendor self-review (Claude reviewing Claude). **Nobody risk-routes review to a deliberately different model *family* than the one that wrote the diff** — the exact control the oversight research calls for. atelier is uniquely positioned because it already hosts heterogeneous CLIs locally. The wedge: *"on a high-risk diff, an independent reviewer from a different model family — chosen by recorded provenance, never faked — that you watch happen in the transcript."* The guaranteed value is **auditor independence** (a reviewer with no stake in the producer's diff kills self-confirmation bias); the upside is **novel cross-family catch**, gated on real diversity.

## Problem

atelier runs autonomous, multi-step agents on high-risk changes, but every review signal ultimately traces back to a model that shares the producer's training lineage. The deterministic self-grading gate (shipped) grounds a Pass/Fail in real test/lint exit codes — strong, but blind to everything tests don't express: a plausible-but-wrong refactor, a silently weakened invariant, a misread of intent. That code compiles, passes, lints clean, and is still wrong. The only thing that catches it is a semantic reviewer — and if that reviewer shares the producer's blind spots, it rubber-stamps the mistake.

The naive fix ("have a different agent review it") fails on its own premise: a same-family reviewer is the configuration the evidence says doesn't work. atelier already has the raw materials — council members carry a per-member `runtime`, and dispatching a step to a chosen runtime is solved — but two gaps block a *trustworthy* cross-runtime review. First, **the producer's model lineage is never recorded** (the step event stores only the agent id), so "pick a reviewer different from the producer" can only be inferred from current config, which drifts. Second, **"different runtime" is not "different model family"**: two runtimes can front the same provider, so a runtime-keyed rule certifies a same-lineage review as "diverse" — security theater precisely where stakes are highest.

The deeper trap is that more review is not more catching. A second pair of eyes wired to the same visual cortex sees the same illusion; the value of this layer is governed entirely by how *uncorrelated* the two channels are. So the feature must record lineage as fact, decide diversity on family, and SKIP loudly when no independent reviewer exists — rather than manufacture false confidence.

### Market Data

- **Model similarity undermines AI oversight**, and an LLM judge favors models similar to itself; failures get *more* correlated as capability rises — *Great Models Think Alike* (arXiv:2502.04313, ICML 2025).
- **~60% error agreement when both models are wrong** across 350+ LLMs — "algorithmic monoculture," correlated even across providers at the top end (arXiv:2506.07962, ICML 2025).
- **64.5% self-correction failure rate** across 14 models on their own errors — a same-model reviewer structurally rubber-stamps its own mistakes (arXiv:2507.02778).
- **Cross-family juries are less biased AND ~7–8× cheaper** than a single GPT-4 judge; every model scores *itself* highest (PoLL, arXiv:2404.18796).
- **Market pull:** AI code-review ARR ≈ $420M in 2026 (+133% YoY); **44% of devs used an AI reviewer in the last 12 months** (vs 18% in 2023); **41–46% of new code is AI-generated** — directly growing the high-risk-diff surface.
- **Contrary (bounds the claim):** cross-family benefit shrinks at the frontier (convergence); consensus voting can amplify shared errors (arXiv:2510.21513); heterogeneous multi-agent teams underperformed their best member by up to 37.6% → *advisory, not auto-override*; untuned LLM reviewers run 40–80% false positives, trust collapses above ~50%.

## Core Features

| # | Feature | Priority | Description |
|---|---------|----------|-------------|
| F1 | Producer-lineage provenance on the step event | Critical | Record `(provider, model_family, model)` of the producing agent as an immutable field on the step event at production time, populated by every runtime. The load-bearing primitive — everything else reads it; it cannot be appended to history later. |
| F2 | Family-keyed reviewer-diversity predicate | Critical | A named, unit-tested boolean: select a council reviewer whose `model_family` differs from the recorded producer family; same family → **SKIP loudly** with a visible reason. Runtime kind is a dispatch hint only; never downgrade silently to a same-family reviewer. |
| F3 | On-request cross-family review (the probe) | Critical | Explicit invocation routes a semantic review of the diff to a different-family reviewer via council — advisory and non-blocking. The v1 milestone that validates the premise before any auto-trigger exists. |
| F4 | Actioned-catch + disagreement instrumentation | High | Emit events for findings surfaced, finding disposition (acted/dismissed), and per-lineage-pair disagreement-rate, **segmented attended-vs-unattended**. The data that earns the auto-trigger and the fixer. |
| F5 | Cross-runtime review round in chat | High | A `ChatLifecycleKey` + projection arm so a review round renders as one evolving transcript item (council events are diagnostic today). |
| F6 | Opt-in auto-trigger + bounded fixer (gated) | Medium | Config flags to (a) auto-fire on council's existing high-risk trigger and (b) auto-route a bounded fixer — **both default OFF**, documented to flip only once actioned-catch precision clears a floor. |

## KPIs

| KPI | Target | How to Measure |
|-----|--------|----------------|
| **Actioned unique-catch rate** *(North Star)* | ≥ 25% more real defects acted-on vs the same-family / self-grading baseline | Finding-disposition events, cross-family on vs off, labeled sample |
| Family-diversity correctness | 100% of fired reviews have reviewer family ≠ producer family (else a logged SKIP) | Producer vs reviewer lineage on dispatch events |
| Advisory precision (noise control) | < 30% of surfaced findings dismissed as noise | Finding disposition (acted vs dismissed) |
| SKIP transparency | 100% of no-diverse-reviewer cases emit a visible SKIP (never a fabricated pass) | Event audit |
| Cost overhead (scoped) | ≤ 1.3× median tokens on steps where the gate fires | Token accounting, gate-on vs off |
| Probe→trigger readiness | Actioned-catch precision measured on ≥ 30 high-risk diffs before the auto-trigger flips on | Instrumentation rollup |

## Feature Assessment

| Criteria | Question | Score |
|----------|----------|-------|
| **Impact** | How much more valuable does this make the product? | **Strong** — independent review on the high-risk diffs where a wrong "Completed" hurts most |
| **Reach** | What % of users would this affect? | **Maybe** — high-risk/on-request only, needs a reachable different family; multi-runtime users are a subset |
| **Frequency** | How often would users encounter this value? | **Maybe** — intermittent by design (council's trigger), not every step |
| **Differentiation** | Does this set us apart or just match competitors? | **Strong** — research-grounded white space; nobody risk-routes to a different model *family* |
| **Defensibility** | Is this easy to copy or does it compound over time? | **Strong/Maybe** — provenance + family routing woven into a local multi-runtime harness compounds as users add runtimes |
| **Feasibility** | Can we actually build this? | **Strong** — reuses council per-member runtime + dispatch + capability gates; one new primitive (record lineage) |

**Leverage type:** Strategic Bet (bounded), sequenced probe-first; leaning Compounding.

## Council Insights

- **Recommended approach:** Build a **lineage-based reviewer-diversity policy that council consumes** — record producer lineage as an immutable fact, decide fire/SKIP on **model family** (runtime kind is a dispatch hint), ship **advisory + on-request first**, and earn the auto-trigger/fixer with measured actioned-catch data. Recorded as **[ADR-001](adrs/adr-001.md)**.
- **Key trade-offs:** runtime-keyed boolean (cheap, but theater-prone) **vs** family-keyed boolean (chosen); standalone independence-*score* subsystem (reusable, but a calibration liability) **vs** a boolean now, score later (chosen); advisory-default (safe, measurable) **vs** forcing function (premature on unmeasured FPR — deferred).
- **Risks identified:** theater-diversity → family-keyed SKIP, never runtime-keyed; frontier convergence → instrument per-lineage-pair disagreement, demote correlated pairs; false-positive noise → advisory + precision floor; config-drift → lineage recorded on the immutable event; write-only advisory on unattended runs → name a triage surface or don't fire (Open Question).
- **Stretch goal (V2+):** promote the diversity policy to a reusable service consumed by the deterministic grading gate and a future N-version "diverse panel"; add the tunable independence score; flip the auto-trigger/fixer on once precision data justifies it.
- **Preserved dissent:** the Devil's Advocate holds that advisory output during *unattended* runs has no consumer — it is write-only token spend that poisons the catch-rate metric — and that the gate should either name a human-visible triage surface (+ segment the metric) or not surface advisory on unattended runs at all, only silently sampling disagreement-rate. The Thinker reframes the whole feature as **diverse redundancy / N-version programming**: promise the guaranteed floor (auditor independence), gate the uncertain ceiling (novel catch) on measured diversity.

## Integration with Existing Features

| Integration Point | How |
|-------------------|-----|
| `run_council_workflow` (`app/mod.rs:5544`) | Cross-family selection is a predicate council *consumes* when choosing which member/runtime reviews |
| `CouncilMemberProfile.runtime` (`config/mod.rs:512`) | Reused for dispatch; the policy picks the member whose family differs from the producer |
| `agent_step_started` event (`app/mod.rs:5844`) | Extended with the `(provider, model_family, model)` provenance triple |
| Self-grading gate (`orchestrator/mod.rs:397`) | Stays distinct (deterministic exit-code Pass/Fail); this is the orthogonal judgment layer for the no-oracle case |
| Chat projection + `ChatLifecycleKey` (`app/chat/projection.rs:146`) | New key + arm so a review round renders as one evolving item (council events are diagnostic today) |
| `RuntimeKind` dispatch (`runtime/mod.rs:554`) | Runtime availability tells the policy which families are reachable; an unknown/unmappable family SKIPs |

## Out of Scope (V1)

- **Independence/provenance-distance *score* + tunable threshold** — v1 is a boolean on family; a scalar threshold is a calibration liability deferred until data demands it.
- **High-risk auto-trigger ON by default** — v1 is the on-request probe; the auto-trigger is an opt-in flag earned by actioned-catch data.
- **Auto-routed fixer ON by default** — opt-in and gated on a measured precision floor; a fixer driven by a noisy reviewer amplifies error.
- **Hard merge/ship gate (forcing function)** — advisory only until this harness's real false-positive rate is measured.
- **Differential generation on the SKIP case (re-run task on a second family, diff outputs)** — a distinct, noisier feature, not this one.
- **Folding council and the deterministic grading gate into one "review" subsystem** — they stay distinct: cheap deterministic gate vs on-demand independent judgment.

## Architecture Decision Records

- [ADR-001: Cross-runtime review is a lineage-based reviewer-diversity policy over council, advisory by default, grounded in recorded producer provenance](adrs/adr-001.md) — record lineage as an immutable fact; decide diversity on model family (runtime = dispatch hint); boolean v1, score later; advisory-default, fixer earned; promise the floor, gate the ceiling; narrow trigger.

## Open Questions

- **Unattended-consumer fork (preserved dissent):** on unattended runs, does the advisory fire and project as a human-visible triage item + non-blocking `marked_for_human_review` flag (with the catch-rate metric segmented attended-vs-unattended), *or* not surface at all and only silently sample disagreement-rate? Resolve before the auto-trigger ships.
- **`model_family` detection across runtimes:** how reliably is family resolved across Codex/Claude/Cursor/Z.ai given aliased model names and fallback chains (e.g. Cursor running a Claude model, Z.ai running GLM)? What is the canonical family taxonomy, and how is an unknown family handled (must SKIP)?
- **Provenance placement & migration:** exact event (`agent_step_started` vs step-completed) and behavior for pre-existing history that lacks the field.
- **Reviewer selection among eligible members:** when several council members satisfy the family-difference constraint, how is one chosen — preset order, effort, cost?
- **Approval-mode interaction:** the reviewer's `Read`/`Command` capabilities under `normal` approval mode — does an on-request review need pre-approval, and what's the UX when it blocks?
- **Precision floor value:** the concrete actioned-catch precision that flips the auto-trigger and fixer defaults to ON.
