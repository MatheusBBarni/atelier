# PRD: In-Session Security Review

## Overview

atelier writes and edits code autonomously, but offers no way to ask *"is this change dangerous?"* before it leaves the machine. This feature adds an on-demand, manual **`/security-review`**: the user runs it inside a session, atelier reviews the **current branch's diff**, and returns a **curated, read-only security report** in chat — verdict, scope, and high-confidence findings (each with severity, location, why it's exploitable, and an advisory fix).

It is for developers shipping AI-authored branches who want a security gut-check *before* opening a PR, and especially for developers in **regulated or IP-sensitive environments who cannot send code to a cloud scanner** — the review runs locally, through a runtime they already trust, and the diff never leaves the machine. It is valuable because it catches the well-documented ~45% of AI-generated code that ships with vulnerabilities at the cheapest possible moment (in the editing loop, pre-PR), in a niche every cloud/CI competitor leaves open. V1 is a deliberately tight, trust-first wedge, framed as act one of a longer "trust layer for AI-written code."

## Goals

- Give every atelier user a one-command, in-session security check on their branch diff that they actually trust enough to run repeatedly.
- Catch high-severity vulnerabilities in AI-authored changes **before** they become a PR, without the diff leaving the machine.
- Establish a credible, scope-honest security-report experience that fights both alert fatigue (curation) and false assurance (explicit coverage + disclaimer).
- Lay the product and data foundation (shared severity/finding vocabulary) for a later independent cross-model audit and a per-repo findings ledger.
- Reach ≥30% weekly adoption among branch-shipping users within 60 days of release (see Success Metrics).

## User Stories

**Solo Shipper Sam (primary) — ships AI-authored branches.**
- As a developer finishing a chunk of work, I want to run one command and see whether my branch diff has security issues, so that I fix them before opening a PR instead of after review.
- As a developer, I want each finding to tell me *where*, *why it's exploitable*, and *how to fix it*, so that I can act without further research.
- As a developer, I want the review to show me only findings worth my attention, so that I keep trusting it and keep running it.

**Regulated Rita (primary) — IP-sensitive / regulated context.**
- As a developer who is not allowed to send source to cloud SAST, I want a security review that runs entirely locally through my configured runtime, so that I get the check without an egress violation.
- As a compliance-minded developer, I want the review to state exactly what it did and did not cover, so that I never mistake "no findings" for "secure."

**Unattended Uma (secondary) — vets autonomous work.**
- As an operator reviewing a batch of autonomous changes, I want to run a security pass over the resulting diff, so that I have a security signal before I trust the work.

**Skeptical Sam (edge) — distrusts AI output.**
- As a skeptical user, I want to know which model produced the review and be warned if it's a weak one, so that I can calibrate how much to trust this particular run.

## Core Features

| # | Feature | Priority | Description |
|---|---------|----------|-------------|
| F1 | On-demand `/security-review` | Critical | A built-in command, discoverable in the command dropdown, that runs a security review over the current branch diff and streams a single evolving report card (Scanning → Completed). |
| F2 | Curated, high-confidence findings | Critical | By default the report shows only high-confidence findings and suppresses the known-noisy classes, ranked by severity. Protects trust and fights alert fatigue. |
| F3 | Full, actionable findings | Critical | Each finding states severity, file:line location, a short why-it's-exploitable explanation, and an advisory suggested fix. The fix is guidance only — never applied automatically. |
| F4 | Credible, scope-honest report shape | Critical | A one-line verdict header (severity counts), an explicit scope/coverage statement (what was reviewed: branch diff vs default branch, file/hunk counts), severity-grouped findings, the reviewing model named, and a persistent honest disclaimer. Never a green "secure" affordance. |
| F5 | Trustworthy reviewer guarantees | High | The review never modifies code and never leaks secrets into the transcript: the reviewer is read-only, and findings are redacted before they appear (see Technical Constraints). |
| F6 | Reviewer-quality awareness | Medium | The report names the reviewing model and shows a visible warning when configured on a weak/uncalibrated model, so confident-but-unreliable output is flagged, not trusted blindly. |

## User Experience

**Discovery.** The user types `/` and sees `/security-review` in the existing command dropdown alongside `/goal`, `/config`, etc., with a one-line description. No new UI paradigm to learn.

**Primary flow (Sam):**
1. After a chunk of work, Sam types `/security-review` and submits.
2. A single **Security Review card** appears in chat with a *Scanning* status and a one-line scope statement ("reviewing branch diff vs `main`…"), evolving in place like the existing Verification card — no wall of intermediate noise.
3. On completion the card shows: a **verdict header** ("2 findings: 1 high, 1 medium" — or "no high-confidence findings surfaced"), the **scope/coverage** line, then findings **grouped by severity**, each as `[HIGH] SQL injection — search.rs:42 · why … · fix …` using the existing severity colors/labels.
4. A persistent muted **disclaimer** sits with the report: best-effort review, absence of findings ≠ secure, coverage is the branch diff only.
5. Sam reads, fixes issues in his own flow (the review is read-only), and optionally re-runs.

**Regulated Rita** runs the same flow; her reassurance comes from the scope statement, the local execution, and the named runtime — nothing about her diff leaves the machine.

**UI/UX considerations.** Reuse the evolving single-card pattern, severity color/label vocabulary, findings-as-labeled-lines, and one-time-disclaimer precedent already in the TUI, so the feature feels native and respects `NO_COLOR`/accessibility. The card scales to large diffs via severity grouping and the count-based verdict header.

**Onboarding/discoverability.** The command dropdown is the entry point; the `/help` overlay gains a one-line entry describing scope and the advisory nature.

## High-Level Technical Constraints

- **Local-only execution / no diff egress.** The review runs through the user's already-configured runtime; the branch diff and file contents must not be sent anywhere the user has not already chosen. This is a load-bearing differentiator and a hard requirement for the regulated persona.
- **Read-only and non-destructive.** The review never edits, patches, or writes code, and never runs arbitrary commands on the user's behalf — it is purely observational.
- **No secret leakage.** Findings and any quoted diff/code must be redacted of credential-shaped content before they appear in chat or durable history.
- **Hostile-input resilience.** The reviewed diff is attacker-influenceable; the review must not be subvertible into suppressing findings or taking out-of-scope actions, and this resistance must be regression-guarded.
- **Performance from the user's view.** A review of a typical diff (<500 changed lines) should complete fast enough to stay in-flow (target median < 90s), and large diffs must degrade gracefully (bounded, with a clear note) rather than hang or balloon cost.

(Mechanism and threat-model specifics are fixed in ADR-001/002 and deferred to the TechSpec.)

## Non-Goals (Out of Scope)

- **Per-finding interaction / disposition** — V1 is read-only; acknowledge/dismiss is deferred.
- **Non-interactive CLI / CI mode** — V1 is TUI-only; `--security-review`/JSON/exit-code modes are a later phase.
- **Blocking gate or merge/commit enforcement** — advisory only; gating on uncalibrated output erodes trust.
- **Auto-fix / remediation** — fixes are advisory text, never applied.
- **Auto-trigger on risky diffs** — manual invocation only in V1.
- **Cross-runtime / model-family-diverse audit** — deferred to Phase 2 to converge with the `cross-runtime-verification-gate` packet rather than duplicate it.
- **Whole-repo audit** — diff-scoped only; whole-repo is a noisier, different product.
- **Persistent findings ledger / cross-review suppression memory** — Phase 3.
- **SARIF / external report formats** — Phase 2 interop concern.
- **Configurable diff-base or scan-scope knobs** — one fixed diff-base rule in V1.

## Phased Rollout Plan

### MVP (Phase 1)
- F1–F6: TUI `/security-review`, curated high-confidence credible report card, full findings, read-only/redacted/honest guarantees, reviewer-quality awareness.
- **Proceed to Phase 2 when:** weekly adoption among branch-shippers ≥ 30%; seeded-vuln recall ≥ 80% and inferred precision ≥ 60% on the eval set; review latency target met; no trust-eroding noise complaints (median findings/review stays small).

### Phase 2
- Independent **cross-model-family audit** (converge with `cross-runtime-verification-gate`): the security review can run on a deliberately different model lineage than the producer.
- **Non-interactive CLI/CI mode** (JSON + exit codes) and SARIF export.
- Optional per-finding acknowledge/dismiss.
- **Proceed to Phase 3 when:** cross-family audit demonstrably catches issues single-model review misses, and CI mode sees real adoption.

### Phase 3
- **Per-repo findings ledger / suppression memory** that improves precision over time and creates lock-in.
- Opt-in **auto-trigger** on high-risk diffs.
- The full "trust layer for AI-written code."

## Success Metrics

| Metric | Target | How measured |
|--------|--------|--------------|
| Adoption | ≥ 30% of branch-shipping users run a review weekly within 60 days | distinct users invoking `/security-review` ÷ active branch-shippers |
| Repeat use (trust proxy) | ≥ 40% of first-time users run another review within 2 weeks | cohort re-invocation rate |
| Recall on seeded vulns | ≥ 80% of injected high-severity issues detected | CI seeded-vulnerability eval set |
| Inferred precision | ≥ 60% of surfaced findings judged real | periodic eval-set + sampled manual audit (read-only V1 has no live disposition signal) |
| Latency | median < 90s on <500-LoC diffs | review start→complete timing |
| Noise guardrail | median surfaced findings per review stays small (e.g. ≤ 5) | finding counts per review |

## Risks and Mitigations

- **Alert fatigue → abandonment.** *Mitigation:* curated high-confidence default + severity-ranked report + noise guardrail metric; if curation underperforms, tighten the confidence threshold before adding features.
- **False assurance ("no findings = safe").** *Mitigation:* explicit scope/coverage statement + persistent disclaimer + never a green "secure" verdict.
- **"Claude already has this for free."** *Mitigation:* lead positioning on the structurally-uncopyable angle — local/private, in-session, pre-PR — not on parity.
- **Weak configured model erodes whole-harness trust.** *Mitigation:* capable default model + visible weak-model warning (F6).
- **Perceived redundancy with `cross-runtime-verification-gate`.** *Mitigation:* distinct purpose (security scan vs general correctness) and rubric; shared severity/finding vocabulary and an explicit Phase-2 convergence rather than two competing systems.
- **Low measurable signal in read-only V1.** *Mitigation:* lean on the CI eval set and adoption/repeat-use as primary signals; treat live precision as inferred until Phase-2 disposition exists.

## Architecture Decision Records

- [ADR-001: Standalone read-only reviewer agent + skill rubric, app-orchestrated diff-as-data workflow, advisory and diff-scoped, own event family](adrs/adr-001.md) — V1 mechanism and data model.
- [ADR-002: The security reviewer is a hostile-input boundary — diff-as-data, read-only, redacted findings, honest disclaimer, CI injection corpus](adrs/adr-002.md) — threat model and guardrails.
- [ADR-003: Security review output is a read-only, scope-honest "security report" card](adrs/adr-003.md) — product shape of the output (Approach B).

## Open Questions

- **Inferred precision measurement:** without live disposition, what's the most reliable proxy (subsequent edits to flagged locations? sampled manual audit cadence?) and what sample size makes the ≥60% target credible?
- **"No findings" presentation:** should a clean review still render the full report card (scope + disclaimer) to reinforce coverage honesty, or a compact line?
- **Diff-base edge cases:** detached HEAD, no default/upstream branch, first commit, dirty working tree — confirm the user-facing behavior for each.
- **Default reviewer model + "weak model" definition:** which model ships as default, and what threshold triggers the F6 warning?
- **Large-diff behavior:** at what size does the review truncate-with-note vs decline, and how is that communicated?
- **Help/onboarding depth:** is a one-line `/help` entry enough, or does the advisory nature warrant a short dedicated explainer on first use?
