# PRD: Cross-Runtime Verification Gate — `/review`

## Overview

When an agent (or the developer) changes code in atelier, the only judgment of whether that change is *correct* comes from a model that shares the producer's training lineage — and a model is structurally bad at catching its own blind spots. `/review` gives the developer a one-command **independent second opinion**: it routes a semantic review of the current working changes to a reviewer whose **model family differs** from the families that produced the diff, and surfaces advisory, non-blocking findings in the transcript.

It is for developers running high-stakes changes in a multi-runtime atelier setup — primarily **Skeptical Sam**, who today distrusts a model grading its own work and manually re-runs a different model, and **Multi-runtime Mia**, who has Codex/Claude/Z.ai configured and wants the harness to exploit that diversity on demand. It complements the shipped deterministic self-grading gate (which answers "did the tests pass?") by adding the judgment layer where there is no test oracle: "is this actually right, judged by a different mind?"

## Goals

- Give developers a discoverable, one-step way to get a review from a genuinely independent model family, replacing the manual "re-run a second model and compare" ritual.
- Make independence **true and visible**: every review names the producer families it diversified against and the reviewer family chosen, and refuses (SKIPs loudly) rather than fake it.
- Keep the worst case to observable transcript noise: advisory and non-blocking, so existing workflows are untouched while the premise is validated.
- **Milestone:** ship the on-request probe, then measure whether a different family surfaces defects developers act on — the gate to fund the V2 auto-trigger and panel.

## User Stories

**Primary — Skeptical Sam (distrusts self-review):**
- As Sam, I want to type `/review` after a risky change so a *different model family* checks it, without me wiring up a second tool.
- As Sam, I want each finding to tell me where (`file:line`), what, why, and how confident the reviewer is, so I can act in seconds and ignore noise.
- As Sam, I want the review to clearly say "reviewed by *<family>*, independent of the families that wrote this" so I know it's a real second opinion — or to tell me plainly when it can't give me one.

**Primary — Multi-runtime Mia (has the runtimes):**
- As Mia, I want `/review` to automatically pick a reviewer from a family I have configured that didn't write the diff, so my runtime diversity pays off without manual selection.

**Secondary — Unattended Uma (deferred):**
- As Uma, I want high-risk diffs in unattended runs reviewed and surfaced for later triage — *acknowledged as out of scope for V1; addressed in Phase 2.*

## Core Features

| Feature | What it does | Why it matters |
|---|---|---|
| **`/review` command** | A new slash command that requests an independent review of the current uncommitted working changes. On-request only. | The discoverable, explicit entry point; mirrors the mental model of Claude Code's `/code-review`. |
| **Independent reviewer selection** | Picks one reviewer whose model family is absent from the producer-family set of the diff (ADR-003). If none is reachable, **SKIPs with a clear reason + guidance to add a second family**. | Makes the "different mind" promise true; never presents a same-family review as independent. |
| **Advisory findings** | Returns a severity-ranked list (Important / Nit), each with `file:line`, a one-line claim, expandable rationale, a suggested direction, a confidence level, and a provenance label. Never blocks or mutates state. | Research-backed anatomy is what makes findings actionable instead of ignored. |
| **Confidence gating + recall knob** | Shows high-confidence findings by default; an effort/`--all` option widens recall ("may include uncertain findings"). | Keeps the false-positive rate — the top abandonment driver — under control while letting power users dig deeper. |
| **Provenance display** | Each review names the producer families it diversified against and the reviewer family chosen. | Independence is the differentiator; surface it, don't bury it. |
| **One evolving review item** | The review round renders as a single coalescing transcript item (not scattered diagnostics), updating as findings arrive. | Legibility; matches how grade-loops and workflows already render. |
| **Lightweight feedback** | A one-key 👍/👎 (accept/dismiss) per finding that records signal without re-running or changing state. | Captures the data that measures precision and earns the V2 auto-trigger. |

## User Experience

**Primary flow (Sam):**
1. Sam finishes (or an agent finishes) a risky change and types `/review`.
2. The harness computes the producer-family set of the working diff, selects a reviewer from a configured family **not** in that set, and shows a one-line header: *"Reviewing 7 changed files — independent reviewer: GLM (Z.ai), diversified against {Claude, hand-edits}."*
3. Findings stream into a single review item, **high-confidence first**, led by a tally ("2 important, 3 nits"). Each finding is one scannable line with `file:line`; rationale is collapsed by default and expandable.
4. Sam acts: edits the code, or `/queue`s a follow-up, or 👍/👎s a finding. Nothing blocks; the run status is untouched.
5. If no independent family is reachable, instead of a fake review Sam sees: *"Skipped — every configured model family contributed to this diff. Add a runtime from a different family to enable independent review."*

**Discoverability:** `/review` appears in the slash-command dropdown, help overlay, and unknown-command guidance. The SKIP message doubles as onboarding toward configuring a second family.

**Accessibility/consistency:** output uses the existing theme tokens and transcript conventions; findings are plain scannable text with progressive disclosure, no blocking modals.

## High-Level Technical Constraints

- **Value requires ≥2 configured model families.** With one family, `/review` SKIPs loudly by design rather than degrade to a same-family review.
- **Strictly advisory.** The review must never block, approve, gate, or mutate run state.
- **Located findings only.** A surfaced finding must cite a real `file:line`; no inference-only claims (verify-before-surface).
- **Bounded cost/latency.** On-request and single-reviewer keeps cost to roughly one extra review pass on the diff.
- **Reuses the existing approval posture.** A read-only reviewer (read + data-returning commands, no edits) should not introduce new blocking approval prompts in normal mode.

## Non-Goals (Out of Scope)

- **Auto-trigger on high-risk steps** — V1 is on-request only; auto-firing is Phase 2, earned by precision data.
- **Unattended-run surfacing / durable triage record** — deferred with the auto-trigger (Phase 2); V1 is attended by definition.
- **Auto-routed fixer** — no automatic re-dispatch to fix findings (Phase 3).
- **Diverse panel / consensus / disagreement-voting** — single reviewer in V1; the multi-reviewer panel is the Phase 2 stretch (needs a parallel council mode).
- **Tunable independence score / threshold** — V1 uses a boolean family rule; a scored policy is later.
- **Differential generation** (re-run the task on a second family and diff outputs) — a different feature.
- **Merging `/review` with the deterministic self-grading gate** — they stay distinct.
- **Blocking / merge gates** — never; advisory only.

## Phased Rollout Plan

### MVP (Phase 1) — the on-request probe
- `/review` over the working diff; single different-family reviewer with family-set independence + loud SKIP; advisory findings with full anatomy; confidence-gated default + recall knob; provenance recorded per producing step and displayed; one evolving review item; 👍/👎 feedback + instrumentation.
- **Proceed to Phase 2 when:** actioned-catch precision is measured on ≥ 30 high-risk diffs, the dismiss / false-positive rate stays under the trust threshold, and family-diversity correctness is 100%.

### Phase 2 — earned automation + panel
- Opt-in auto-trigger on high-risk steps (default off), resolving the unattended-consumer fork (durable, separately-addressable triage record + metric segmentation).
- Diverse panel / consensus with disagreement foregrounding (parallel council execution mode).
- **Proceed to Phase 3 when:** auto-trigger precision holds in unattended runs and the panel measurably beats the single reviewer on actioned catches.

### Phase 3 — full feature set
- Opt-in bounded auto-fixer driven only by high-independence findings; tunable independence score; the diversity policy promoted to a reusable service consumed by the deterministic gate and the panel.

## Success Metrics

| Metric | Target | From the user's view |
|---|---|---|
| **Actioned unique-catch rate** (North Star) | ≥ 25% more real defects acted-on vs same-family / self-grading baseline | The independent reviewer catches things worth fixing that a same-family review misses |
| Family-diversity correctness | 100% of completed reviews have reviewer family ∉ producer-family set (else a logged SKIP) | "Independent" is always true, never theater |
| Advisory precision | < 30% of surfaced findings dismissed as noise | Findings are worth reading; trust doesn't collapse |
| SKIP transparency | 100% of no-independent-family cases SKIP visibly | The tool never fakes a second opinion |
| Cost overhead (scoped) | ≤ 1.3× tokens on a `/review` invocation vs no review | A second opinion is cheap relative to its value |
| Probe→V2 readiness | precision measured on ≥ 30 high-risk diffs before auto-trigger ships | Automation is earned by data, not optimism |

## Risks and Mitigations

- **Advisory findings get ignored.** *Mitigation:* lead with a tally, high-confidence-first, one-key feedback, and prominent provenance; measure dismiss-rate as a first-class metric.
- **Single-family users hit frequent SKIPs and feel the feature is broken.** *Mitigation:* the SKIP message explains why and nudges toward adding a different-family runtime — turning a dead-end into onboarding.
- **False positives erode trust ("almost right" is the #1 frustration).** *Mitigation:* confidence gating, verify-before-surface, nit caps; precision over volume.
- **Competitive overlap (Claude Code `/code-review`, 2ndOpinion).** *Mitigation:* differentiate on **family-set independence grounded in recorded provenance, inside a local multi-runtime harness** — neither competitor risk-routes against the diff's actual producers.
- **Premise ceiling — frontier convergence** weakens cross-family benefit on the hardest diffs. *Mitigation:* promise the guaranteed floor (auditor independence kills self-confirmation), instrument disagreement to manage expectations, don't over-promise novel catches.

## Architecture Decision Records

- [ADR-001: Cross-runtime review is a lineage-based reviewer-diversity policy over council, advisory by default, grounded in recorded producer provenance](adrs/adr-001.md) — record lineage as fact; decide on family; boolean v1; advisory; promise the floor, gate the ceiling.
- [ADR-002: V1 ships as an on-request `/review` command — a single independent (different-family) reviewer over the working diff, advisory](adrs/adr-002.md) — Approach A; on-request only; research-backed finding anatomy; panel deferred to V2.
- [ADR-003: Independence is defined over the producer-family *set* of the working diff, with a loud SKIP when it collapses](adrs/adr-003.md) — reviewer family ∉ producer-family set; `unknown` hand-edits are permissive; never downgrade silently.

## Open Questions

- **`model_family` detection & taxonomy.** How reliably is family derived across Codex/Claude/Cursor/Z.ai given aliased model names and fallback chains (e.g. Cursor running a Claude model)? What canonical family set does the SKIP/independence logic use, and how is an unknown model family treated?
- **Working-diff → producer-step attribution.** How accurately can working-diff hunks be mapped back to the agent steps that produced them, and is the conservative `unknown` default acceptable when attribution is uncertain?
- **Confidence-gating defaults & recall knob.** What is the default confidence threshold, and is the recall control an effort level, a `--all` flag, or both?
- **Feedback semantics.** How is 👍/👎 stored and used — purely for the precision metric in V1, or does it tune future reviews?
- **V2 precision floor.** The concrete actioned-catch precision and dismiss-rate ceiling that gate shipping the auto-trigger.
- **Deterministic-gate interaction.** When both the self-grading gate and `/review` could apply, how are they sequenced or presented so they don't duplicate or contradict?
