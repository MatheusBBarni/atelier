# Idea: Best-of-N Cross-Runtime Race → a Fleet Router that Learns

## Overview

atelier integrates four agent runtimes (Codex, Claude, Cursor, the zai/`HttpApi` runtime) but runs every step on exactly one model. This feature turns that fleet into a **compounding quality asset**. The mechanism is a cross-runtime *race*: on a user-invoked `/race`, N≤3 runtimes attempt the **same** instruction concurrently in isolated scratch scopes; the project's **own test suite** (not a model's opinion) selects the winner; one whole patch is promoted through the existing approval gate. The strategic payoff is the **Fleet Router that Learns** — every race is oracle-graded, attributable training data ("runtime R won on task-shape S, proven by your tests"), and over time the harness learns which model to trust for which work, eventually routing without racing where confidence is high.

**Who it's for:** *Quality-First Quinn* (pays N× on the step that matters), *Fleet-Owner Fiona* (configured the fleet and wants ROI from it), and the harness itself (every test-verified promotion compounds atelier's trust charter). **V1 ambition:** ship the race as a de-risked, opt-in, fail-closed *foundation* that records verdicts from day one; the router is the explicit V2+ North Star, not a V1 deliverable.

## Problem

atelier's defining asset — a multi-runtime fleet — is never used for quality. Each step runs on one model, so a hard or high-stakes step (a tricky migration, a security-sensitive change, a subtle algorithm) is a single dice roll. There is no way to race competing attempts and keep the one that actually works, and nothing accumulates *which* model tends to be right for *which* kind of task. The fleet sits idle as a quality lever precisely when quality matters most.

The naive fix — race N attempts and let an LLM judge pick — walks straight into the documented failure mode of LLM-as-judge: coverage is not selection, and the selector is biased. So the deeper problem is **trustworthy selection**: how do you capture best-of-N's large accuracy lift without betting the outcome on the least reliable component? atelier is unusually well-placed to answer, because it *already ships* an externally-grounded verifier (`self-grading-retry-loop`: real compile/test/lint, PASS only on cited exit-0 evidence) and an exact-disjoint write fence. The missing pieces are isolation (so N attempts can touch the same files safely) and a learning loop (so the verdicts aren't thrown away).

The widest problem is **leverage decay**: even a working race is a one-shot improvement that forgets everything. The compounding version — learning a routing policy from the user's own oracle-graded results — is an asset no single-vendor tool can build, because building it requires racing across providers in the first place.

### Market Data

- **Best-of-N works, and code is the ideal domain (answers are verifiable):** self-consistency lifts GSM8K **+17.9%** (arXiv:2203.11171); repeated sampling took SWE-bench Lite **15.9%→56%** (Large Language Monkeys, arXiv:2407.21787). The gain is captured only by a reliable selector — coverage ≠ selection.
- **Cross-model ensembles beat the best single model:** Mixture-of-Agents **+7.6 pts over GPT-4o** (arXiv:2406.04692); ensemble routing beats every fixed model by **~7%**. Provider *diversity* is the active ingredient.
- **The LLM judge is the binding constraint:** ~80% human agreement (arXiv:2306.05685) but **>10% pairwise accuracy swing from reordering alone**, plus **self-preference bias** (a Claude judge over-rates a Claude attempt — arXiv:2410.21819) and verbosity bias.
- **Diminishing returns past N=3–5;** compute-optimal adaptive allocation matches best-of-N at a fraction of cost (arXiv:2408.03314) — uniform N× on every step is the documented wrong default; gate it.
- **Competitive gap:** no shipping tool races the *same* instruction across *different providers* then auto-selects and promotes a patch. Cursor 2.0 (8 agents, one vendor), Claude Code subagents (one vendor, task-split), Aider (many models but sequential plan→edit), Conductor (multi-model but the *human* judges/merges). Single-vendor tools structurally cannot race across providers.
- **Market:** AI code tools ~$8B (2025) → ~$30–90B by 2031–2035, ~27% CAGR; autonomous agents the fastest-growing slice.

## Summary / Differentiator

Everyone can parallelize within one vendor; nobody races the *same task across different providers* and lets **your own test suite** pick the winner. The shareable moment isn't the race — it's the **legible, test-grounded verdict** ("Attempt B passed all 47 tests including the nil-case check A and C failed"). And the durable moat isn't the race at all: it's the **routing asset** the races accumulate from your codebase's own results — proprietary, compounding, and impossible to replicate without a cross-provider fleet.

## Core Features

| # | Feature | Priority | Description |
|---|---------|----------|-------------|
| F1 | Cross-runtime race step (`/race`) | Critical | Sentinel-routed workflow (council-style) that fans out N≤3 attempts on *distinct* runtimes against the same instruction. Opt-in; rejects empty/malformed usage. |
| F2 | Per-attempt scratch isolation | Critical | New `ActionScope::AttemptScope{attempt_id, scratch_dir}` at the `validate_action_scope` chokepoint lets N attempts write the *same* files safely by isolation. Guaranteed cleanup on every exit path; atomic promotion. |
| F3 | Oracle-gated selection | Critical | The external compile/test/lint oracle (reuse `self-grading-retry-loop` evidence path) disqualifies non-passing attempts **before** any model opinion. All-fail ⇒ promote nothing, fail-closed. |
| F4 | Pick-one promotion through the existing fence | Critical | Re-derive a `ParallelFileScope` from the winner's *actual* write-set; replay as a fresh `ApplyPatch` through `validate_action_request_with_scope` + the governance-spine approval gate; union-bounded, fail-closed. |
| F5 | Judge-as-narrator (independent runtime) | High | An LLM judge on a runtime *distinct* from every attempt tie-breaks **survivors only** and narrates the rationale grounded in oracle evidence; in the no-oracle/all-tie case it ranks **advisory only** with low-confidence disclosed and surfaces top + runner-up to the user. |
| F6 | Structured verdict telemetry | High | Per attempt, emit a durable `ensemble_attempt_verdict` event: runtime/model, task-shape features, oracle result (+evidence ref), judge rank/rationale, won/lost, cost. The data engine for the V2 router. |
| F7 | Single evolving chat item | High | New `RunStepResult::BestOfNEnsemble` + `ChatLifecycleKey`/`ChatItemKind` + projection arm so the race collapses into one transcript item (like `Plan`/`GradeLoop`). |
| F8 | Config block + cost disclosure | Medium | `[ensemble]` (default off); N cap; pre-spend cost/latency disclosure before convening; surfaced in `--doctor` / `/config` for discoverability. |

## KPIs

| KPI | Target | How to Measure |
|---|---|---|
| External-check win-rate lift | **≥ +15 pp** vs single-runtime baseline on raced steps | promoted patch passes project compile/test/lint, compared to single-runtime on the same prompt set |
| Selection quality | **≥ 80%** agreement with a verifiable oracle; position-bias swing **< 5 pp** | when a ground-truth oracle exists, does the promoted patch match oracle-best; A/B order-swap test on the judge |
| Promotion safety | **0** scope escapes | every winning patch re-runs path/approval validation; no out-of-scope write leaves scratch (fail-closed) |
| Cost-cap adherence | median **≤ 3.5×** single-step; hard cap at N_max | realized token/latency multiplier per race |
| Verdict-telemetry completeness | **100%** of attempts emit a structured verdict record | count `ensemble_attempt_verdict` events vs attempts run (router data integrity) |
| Wall-clock overhead | **≤ 1.5×** single-attempt median | attempts run concurrently; overhead ≈ judge latency |

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | **Strong** |
| **Reach** | What % of users would this affect? | **Maybe** (fleet-configured, opt-in steps) |
| **Frequency** | How often would users encounter this value? | **Maybe** (gated by design) |
| **Differentiation** | Does this set us apart or just match competitors? | **Must do** (cross-provider race + test-grounded select) |
| **Defensibility** | Is this easy to copy or does it compound over time? | **Must do** (the routing asset compounds from your own data) |
| **Feasibility** | Can we actually build this? | **Maybe** (greenfield scratch layer is the lift) |

**Leverage type:** Compounding Feature (a Strategic Bet whose races accumulate a proprietary routing asset).

## Council Insights

- **Recommended approach:** opt-in `/race` → N≤3 cross-runtime attempts in `AttemptScope` scratch → external oracle disqualifies non-passing attempts (hard gate) → judge narrates / tie-breaks survivors only (advisory + disclosed when no oracle) → pick-one winner re-validated as a fresh `ApplyPatch` through the existing fence + approval gate. Record every verdict as telemetry; the learning router is the V2+ North Star.
- **Key trade-offs:** oracle *selects* (bias-free) vs judge *narrates* (legible) — split the two jobs; pick-one (intact provenance) vs synthesize-merge (scope-orphaned new trust boundary); opt-in (safe, proven) vs auto-route (frictionless, unattended N× spend).
- **Risks identified:** scratch lifecycle bugs (→ RAII cleanup + atomic promotion); scope escape (→ re-derive from actual write-set, union-bound, fail-closed); judge self-preference (→ judge-runtime independence + oracle precedence); N× cost on low-value steps (→ opt-in, N cap, pre-spend disclosure); narrative outrunning delivery (→ mark router explicitly V2+).
- **Stretch goal (V2+):** the **Fleet Router that Learns** (route from accumulated verdicts; race only where it pays), then synthesize-merge as a layer strictly on top of pick-one's promotion path, then config-gated auto-route with a documented maturity criterion.

## Out of Scope (V1)

- **Synthesize-merge winning patch** — a merged artifact has no producing attempt and no inherited scope; it bypasses the one safety fence on the highest-stakes change. V2 only, strictly on top of pick-one's promotion path.
- **Orchestrator auto-route** — unattended N× spend by an unproven router; needs routing-precision data first. V2 config-gated flag with cost cap + pre-spend disclosure.
- **The learning router itself** — worthless without an accumulated verdict corpus (cold start). V1 generates the data; the router consumes it later.
- **N > 3** — returns plateau past 3–5; the cap bounds cost and matches the integrated fleet size.
- **Cross-project verdict aggregation / shared routing service** — verdicts stay workspace-local in `.atelier/`; no shared service, no cross-project data movement in V1.

## Architecture Decision Records

- [ADR-001: Best-of-N V1 — Oracle-Selected Pick-One Over LLM-Judged Synthesize-Merge](adrs/adr-001.md) — V1 selects via the external test oracle and promotes one whole attempt through the existing fence; synthesize-merge and auto-route deferred.
- [ADR-002: Frame the Feature Around a Learning Fleet Router; the Race Is the Data Engine](adrs/adr-002.md) — headline thesis is the compounding router; V1 must record structured oracle verdicts from day one.

## Integration with Existing Features

| Integration Point | How |
|---|---|
| Council sentinel routing (`COUNCIL_WORKFLOW_AGENT_ID`) | `/race` is a parallel sibling workflow with its own sentinel + validation |
| `run_parallel_group` / `ExecutionGraph` IR | race = degenerate graph (N sibling nodes, one target, isolated scopes); reuse the fan-out scheduler |
| `self-grading-retry-loop` oracle | reused verbatim as the bias-free selection gate |
| `validate_action_scope` / approval gate | new `AttemptScope` variant + winner re-validated as a fresh `ApplyPatch` |
| `.atelier/` event-sourced history | durable store for `ensemble_attempt_verdict` telemetry |
| Per-agent runtime/model selection | each attempt references a different runtime — no new runtime code |

## Cost Estimate

Operationally bounded by N× single-step inference (N≤3) on the opt-in subset of steps. No new paid infrastructure; cost is user-incurred model spend, disclosed before each race and capped by `[ensemble]` config. Verdict telemetry is local disk only.

## Open Questions

- **Task-shape feature schema** — which features label a verdict (language, file kinds, change kind, diff size?) so the V2 router has the right dimensions without churn.
- **"No-oracle" detection** — how the gate decides an attempt has no discriminating test (zero relevant tests vs all-N-pass tie) and what UX the advisory fallback uses.
- **Judge independence with a thin fleet** — fallback ordering when only one or two runtimes are configured (deterministic oracle-score + index?).
- **History retention** — how verdict telemetry interacts with `.atelier/` retention/compaction so the router's corpus survives.
- **Composition with `/workflow`** — can a race be a step inside a `/workflow`, or is it top-level only in V1?
