# PRD: Best-of-N Cross-Runtime Race (`/race`)

## Overview

`atelier` runs every step on a single model, leaving its multi-runtime fleet (Codex, Claude, Cursor, zai) unused as a quality lever. **`/race`** lets a user deliberately spend extra compute on a step that matters: it runs the same instruction across N≤3 different runtimes at once, lets the project's **own test suite** pick the winner, and promotes one whole patch through the normal approval gate. Every race is graded by objective tests, so its outcome becomes durable, attributable data — and `atelier` learns which runtime wins which kind of work, putting that learning to use immediately by choosing who competes. It is for users who already configured multiple runtimes and want a quality result they can trust on high-stakes changes — and it is something single-vendor tools structurally cannot offer.

## Goals

- Give users a deliberate, opt-in way to raise output quality on high-stakes steps, with the winner chosen by objective tests rather than a model's opinion.
- Make the multi-runtime fleet *legible*: the user sees N models compete and reads *why* the winner won.
- Turn every race into a compounding asset — accumulated win-rates that make the fleet smarter over time and visible to the user.
- Keep the feature safe-by-default: nothing lands without passing the same approval gate as any edit; cost is disclosed; failure is surfaced, never hidden.
- **Targets:** ≥ +15 pp external-check win-rate vs single-runtime on raced steps; ≥ 80% selection agreement with a verifiable oracle; 0 promotion scope-escapes; median cost ≤ 3.5× a single step.

## User Stories

- **As Quality-First Quinn (high-stakes autonomous edits),** I want to race several models on a tricky migration and keep the one that passes my tests, so that I'm not betting a critical change on a single model's guess.
- **As Quality-First Quinn,** when no test covers my change, I want the harness to tell me plainly that it couldn't verify the winner, so that I don't mistake a guess for a checked result.
- **As Fleet-Owner Fiona (configured several runtimes),** I want to see which runtime actually wins on which kind of task in *my* codebase, so that I get real ROI from paying for multiple providers.
- **As Fleet-Owner Fiona,** I want the harness to put the historically strongest runtimes into the race automatically, so that the fleet gets smarter without my hand-tuning.
- **As either persona,** when every attempt fails my tests, I want to see exactly what broke and choose whether to retry or stop, so that a failed race never silently lands or wastes more spend without my say-so.

## Core Features

| # | Feature | Priority | What it does |
|---|---------|----------|--------------|
| F1 | Opt-in `/race` workflow | Critical | A user-invoked command (`[ensemble]` config, default off) that starts a race for the given instruction. The start line announces N, the competing runtimes, and an estimated cost/latency multiplier, then proceeds — typing `/race` is the consent. |
| F2 | Cross-runtime competing attempts | Critical | N≤3 *distinct* runtimes attempt the same instruction concurrently, each fully isolated so no attempt sees or clobbers another's work. |
| F3 | Test-grounded winner selection | Critical | The project's own compile/test/lint result picks the winner; attempts that fail are disqualified before any model opinion. One whole winning patch is selected — never a stitched-together merge. |
| F4 | Safe promotion | Critical | The winning patch passes through the **existing** approval gate, re-validated like any edit — fail-closed, never widening scope. Nothing lands that wouldn't land as a normal action. |
| F5 | Legible live verdict | High | A live multi-pane view shows the N attempts as they run, collapsing to a verdict card: each attempt's test result, the judge's plain-language rationale ("B passed the nil-case check A missed"), and the winning diff. |
| F6 | No-oracle & all-fail handling | High | When tests can't discriminate (no coverage, or all pass), auto-pick the judge's top with a clear **low-confidence banner**, then the approval gate. When all attempts fail tests, promote nothing, surface each failure, and offer retry or abort. |
| F7 | Learning read-back | High | Accumulated per-runtime win-rates by task type are recorded and shown to the user (e.g. in `/provider:status`), with a visible "still learning" state until there's enough data. |
| F8 | Active roster routing | Medium | Using that history, the harness selects *which* runtimes enter the race for a given task type; on a cold start (no/low data) it races the default roster. Routing only picks competitors — it never skips the race or bypasses the test gate. |

## User Experience

1. **Discover:** `/race` appears in the command dropdown and help overlay next to `/workflow`; `--doctor`/`/config` surface the `[ensemble]` block and whether it's enabled.
2. **Invoke:** the user types `/race <instruction>`. The harness shows a start line: *"Racing 3 runtimes (Claude, Codex, Cursor) · est. ~3× cost"* and begins.
3. **Watch:** a live multi-pane view streams the competing attempts side by side. This is the spectacle — the user sees the fleet working in parallel.
4. **Verdict:** the panes collapse into one verdict card — winner, each attempt's test result, the judge's one-line rationale, and the diff. If no test could decide, the card carries a low-confidence banner.
5. **Approve:** the winning patch enters the standard approval modal (risk tier, diff preview, `y/t/n`). The user approves once or approves-and-trusts, exactly as for any edit.
6. **Learn:** the outcome updates the win-rate read-back; over repeated races the user sees the fleet's track record by task type, and the roster the harness chooses begins to reflect it.
7. **Failure path:** if every attempt fails tests, no card claims a winner — the user sees each failure and a retry/abort choice.

Accessibility/degradation: the verdict card is the durable artifact; if the terminal can't support live multi-pane, the race degrades to roster streaming + the card.

## High-Level Technical Constraints

- **Requires ≥2 configured runtimes** — `/race` is meaningful only with a multi-runtime fleet; with fewer, it must explain why it can't race rather than silently degrade.
- **Reuses the existing verification and approval surfaces** — selection is grounded in the project's real compile/test/lint; promotion uses the existing approval gate unchanged. No parallel "race-only" safety path.
- **Safe-by-construction isolation** — competing attempts must never affect the real workspace or each other until a winner is promoted; an interrupted race must leave the workspace clean.
- **Workspace-local data** — win-rate history stays within the workspace; no shared service or cross-project data movement.
- **Bounded cost** — N capped at 3; cost disclosed at start.

## Non-Goals (Out of Scope)

- **Synthesized/merged winning patch** — V1 promotes one whole attempt only; combining fragments from multiple attempts is deferred (it has no verified provenance).
- **Orchestrator auto-routing** — the orchestrator deciding to race on its own (unattended N× spend) is deferred to a later phase.
- **Full router (skip the race)** — routing a single trusted model and skipping the race is out; V1 routing only chooses *who competes*.
- **N > 3** — returns plateau past 3–5; the cap bounds cost.
- **Cross-project win-rate aggregation / shared routing service** — deferred.
- **Always-on confidence signal** — the background "agreement" surface is a separate packet, not part of `/race` V1.

## Phased Rollout Plan

### MVP (Phase 1) — `/race` + active roster routing

F1–F8 above: opt-in race, isolated cross-runtime attempts, test-grounded pick-one, safe promotion, live verdict, no-oracle/all-fail handling, win-rate read-back, and roster routing with cold-start fallback.

- **Proceed criteria:** raced steps beat single-runtime on the external-check metric by the target margin; 0 promotion scope-escapes; users invoke `/race` repeatedly (retention) and a verdict corpus accumulates.

### Phase 2 — Full router + synthesized merge

Route a single trusted model and skip the race when historical confidence is high (with a safety fallback to racing); add a synthesized-merge option strictly on top of pick-one's promotion path.

- **Proceed criteria:** routing predictions match race outcomes above a confidence threshold; merge promotions pass the same gate at parity with pick-one.

### Phase 3 — Orchestrator auto-route

The orchestrator auto-convenes a race on high-stakes steps, config-gated, with trigger predicates, cost cap, and pre-spend disclosure — graduating to default-on only after routing precision proves out.

## Success Metrics

| Metric | Target | How measured |
|---|---|---|
| External-check win-rate lift | ≥ +15 pp vs single-runtime on raced steps | promoted patch passes project compile/test/lint vs single-runtime on the same prompts |
| Selection quality | ≥ 80% agreement with a verifiable oracle; <5 pp order-swap swing | oracle-best match; A/B presentation-order test on the judge |
| Promotion safety | 0 scope-escapes | every winner re-runs path/approval validation; no out-of-scope write leaves isolation |
| Cost adherence | median ≤ 3.5× single step; hard cap at N | realized cost/latency multiplier per race |
| Verdict-data completeness | 100% of attempts recorded | win-rate records vs attempts run |
| Repeat usage | users run `/race` more than once per active week (among adopters) | invocation telemetry |

## Risks and Mitigations

- **Narrow reach (opt-in, high-stakes only)** → position as a deliberate quality/halo feature; the win-rate read-back creates a reason to return between races.
- **Cost perception (N× spend)** → start-line disclosure, N cap, opt-in consent; never auto-spend in V1.
- **Cold-start makes the "router" feel inert** → default-roster fallback + a visible "still learning" state so expectations match reality.
- **Trust erosion if a no-oracle pick is wrong** → explicit low-confidence banner; the result still passes the human approval gate.
- **Competitive copying** → the durable moat is the workspace-local, test-graded win-rate history, not the race mechanic; lean on that, not on being first.
- **Dependency on multi-runtime setup** → clear messaging when <2 runtimes are configured; tie into onboarding/`--doctor`.

## Architecture Decision Records

- [ADR-001: Oracle-Selected Pick-One Over LLM-Judged Synthesize-Merge](adrs/adr-001.md) — the test oracle selects; one whole attempt is promoted; merge/auto-route deferred.
- [ADR-002: Frame Around a Learning Fleet Router; the Race Is the Data Engine](adrs/adr-002.md) — router-led framing; record verdicts from day one.
- [ADR-003: PRD Approach — Race-Led, Router-Active V1 (user-invoked `/race`)](adrs/adr-003.md) — ship the full race + active routing in one release.
- [ADR-004: Minimal Routing in V1 — Route the Race Roster, Never Skip the Race](adrs/adr-004.md) — V1 routing only selects competitors; cold-start races the default roster.

## Open Questions

- **Task-type signature** — what coarse signal labels a task for win-rate tracking (language + change kind?), chosen to stay forward-compatible with the Phase 2 router. *(For TechSpec.)*
- **"No-oracle" detection** — how the harness decides tests can't discriminate (zero relevant tests vs all-pass tie) and the exact banner wording.
- **Routing threshold** — minimum samples before history overrides the default roster.
- **Thin-fleet behavior** — judge independence and roster choice when only 2 runtimes are configured.
- **History retention** — how win-rate data survives session-history compaction.
- **`/workflow` composition** — can a race be a step inside `/workflow`, or is it top-level only in V1?
