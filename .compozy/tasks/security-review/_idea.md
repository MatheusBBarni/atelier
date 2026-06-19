# Idea: In-Session Security Review — the Local Pre-PR Audit, Act One of a Trust Layer for AI-Written Code

## Overview

atelier generates and edits code autonomously, but it has no way to ask *"is this diff dangerous?"* before the change leaves the machine. This feature adds an **on-demand, manual security review**: the user invokes `/security-review`, the harness gathers the **current branch's git diff**, and a dedicated read-only reviewer returns an **advisory, severity-ranked findings report** in chat — a close port of Claude Code's `/security-review`, but running **in-session, on the working tree, locally, through a runtime the user already trusts**.

It's for the **Solo Shipper** doing a pre-PR gut-check, the **Regulated developer** who *cannot* send diffs to a cloud SAST, and the operator vetting a chunk of autonomous work. V1 is a deliberately tight, **runtime-agnostic standalone wedge** — but it is positioned and architected as **act one of a "trust layer for AI-written code"**: it ships shared `Severity`/`Finding` primitives and a convergence path so that a V2 *independent* audit (review by a different model family) and a V3 per-repo findings ledger build on it rather than replace it.

## Problem

atelier's defining behavior — autonomous, multi-step code generation — is also its sharpest unmanaged risk. Industry data is blunt: **~45% of AI-generated code ships with security vulnerabilities** (Veracode, 2025), **90% of developers now use AI** to write code, and **31% of PRs merge with no review at all** (DORA, 2025) as AI inflates output volume faster than humans can review it. The harness that *produces* this code offers no in-loop check on it. The only security signals available today are the deterministic self-grading gate ("did the tests pass?") and the council ("is this work good?") — neither asks the security question, and tests don't express injection, broken authz, SSRF, secret exposure, or weak crypto.

Today the user's only options are to push the branch and wait for cloud CI SAST (Semgrep, Snyk, CodeQL/Autofix, CodeRabbit), or to eyeball the diff. Both fail the moment that matters: **CI fires after the code is already a PR** — past the point where a fix is up to **640× cheaper** (HackerOne), and after the diff has already left the machine. For a regulated or IP-sensitive team, that egress is itself disqualifying. And the cloud tools are blind to what never becomes a PR — exactly the autonomous changes atelier makes.

The naive fix — "have an agent scan the diff" — walks into the documented failure mode of LLM security review: **high recall, poor precision**. Untuned LLM reviewers run 40–80% false-positive rates, and alert fatigue is the consistently-cited adoption killer. The deeper problem is therefore **trustworthy advisory output**: surfacing enough real findings to matter without burying them in noise the user learns to ignore. The reference implementation answers this with a confidence threshold plus a hard-exclusion list — a recipe atelier can adopt, while adding what no cloud tool can: the review runs *before the artifact exists*, *without the diff leaving the machine*.

### Market Data

- **Demand:** ~45% of AI-generated code contains vulnerabilities (Veracode 2025); 90% of devs use AI, 31% of PRs merge unreviewed (DORA 2025); fixing at coding stage is up to 640× cheaper than in production (HackerOne).
- **Reference:** Claude Code shipped `/security-review` (Aug 2025) — agentic, diff-scoped, with a **confidence≥8 gate** and a **hard-exclusion list** (DoS, rate-limiting, open redirects, already-secured secrets) to fight false positives. Caught real RCE/SSRF pre-merge internally.
- **Open niche:** Semgrep, Snyk, CodeQL/Copilot Autofix, CodeRabbit, Greptile, DryRun, Socket all run at **PR/CI time, in the cloud**. The **in-session, local, working-tree-diff** lane is essentially unoccupied. The AI-code-security market is hot (Socket raised $60M at a $1B valuation in 2026).
- **Precision is the lever:** LLMs are better at *triaging/filtering* than detecting from scratch; Semgrep's Assistant auto-handles ~60% of triage at 96% agreement. Advisory + diff-scoped + aggressive filtering + context is the regime where LLM review becomes useful.
- **Caution:** Anthropic's own security-review Action was "not hardened against prompt injection" and shipped a permission-bypass CVE (patched v1.0.94) — the reviewer is itself an attack surface.

## Core Features

| # | Feature | Priority | Description |
|---|---------|----------|-------------|
| F1 | `/security-review` diff-as-data workflow | Critical | A thin app-orchestrated command gathers the branch diff (merge-base vs default branch), redacts it, and dispatches the reviewer with the diff supplied as **untrusted data** — never fetched by the agent itself. |
| F2 | Read-only `security-reviewer` agent + `security-review` skill | Critical | A built-in agent profile with **no Edit and no general Command** capability; a `SKILL.md` rubric supplies vulnerability classes, severity scale, confidence gate, exclusion list, and untrusted-content framing. |
| F3 | Severity-ranked findings event family + chat card | Critical | New `security_review_started/completed` events carrying `Vec<Finding>`, projected via `ChatLifecycleKey::SecurityReview` into one evolving, **redacted** chat card with `Severity`/`Finding` as shared leaf types. |
| F4 | Precision controls (confidence gate + hard-exclusions) | High | Drop low-confidence findings; auto-exclude the documented noisy classes; rank by exploitability. Never render a green "0 findings = safe" affordance. |
| F5 | Finding-disposition instrumentation | High | Record whether each finding is acted on or dismissed, so actioned-precision and wholesale-dismissal KPIs are measurable from day one. |
| F6 | Prompt-injection resistance + CI corpus | High | Diff-as-data + untrusted framing, guarded by a CI corpus of adversarial diffs (finding-suppression, capability-escalation, secret-bait) asserting the reviewer neither suppresses seeded findings nor attempts out-of-scope actions. |
| F7 | Sane-default reviewer model + weak-model warning | Medium | Runtime-agnostic, but ships pointing at a capable default and warns visibly when configured on a weak/uncalibrated model. |

## KPIs

| KPI | Target | How to Measure |
|-----|--------|----------------|
| Actionable-finding precision | ≥ 60% | (findings acted on or confirmed real) ÷ (findings surfaced), from F5 disposition data |
| Recall on seeded high-severity vulns | ≥ 80% | % of injected high-severity issues detected in the CI seeded-vuln eval set |
| Median review latency (<500-LoC diff) | < 90s | elapsed between `security_review_started` and `_completed` events |
| Weekly adoption among branch-shippers | ≥ 30% within 60 days | distinct users emitting a `security_review_*` event ÷ active branch-shipping users |
| Wholesale-dismissal rate | < 20% | % of reviews where every finding is dismissed, from F5 disposition data |

## Feature Assessment

| Criteria | Question | Score |
|----------|----------|-------|
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Maybe |
| **Differentiation** | Does this set us apart or just match competitors? | Strong |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe (→ Strong via the V2/V3 trust-layer path) |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: **Quick Win, sequenced as the first act of a Compounding Feature** (the trust layer).

## Summary / Differentiator

Every named competitor fires at **PR/CI time, in the cloud, after the diff has left the machine**. atelier's defensible lane is the opposite corner: *"the security review that runs inside the coding session, on your working-tree diff, through a runtime you already trust — catching the 45% of AI-written vulnerabilities before the code is ever a PR, without your diff leaving the laptop."* Local/private execution is the one angle a cloud SAST structurally cannot copy; pre-PR timing is the behavioral wedge. The copyable command becomes a compounding asset only when welded to atelier's own assets — event-sourced auditable findings now, model-family-diverse audit and a per-repo findings ledger later.

## Council Insights

- **Recommended approach:** A standalone, app-orchestrated advisory workflow — a read-only `security-reviewer` agent + `security-review` skill rubric, driven by a thin workflow that supplies the redacted diff as untrusted data. **Not** a council preset (a single reviewer is a degenerate council-of-one) and **not** skill-only (advisory/read-only must be a structural guarantee, not prose).
- **Key trade-offs:** More net-new code (workflow + event family + chat key) than skill-only, in exchange for an enforced read-only guarantee and structured, instrumented findings; diff-only scope trades cross-file recall for low noise; runtime-agnostic trades consistent quality for reach.
- **Risks identified:** (1) *Alert fatigue / low precision* → confidence gate + hard-exclusions + disposition metric + <20% dismissal guardrail. (2) *The reviewer is a prompt-injection surface* → diff-as-data, read-only capabilities, untrusted framing, CI corpus (ADR-002). (3) *False assurance* → honest disclaimer; never "0 findings = safe." (4) *Weak configured model* → sane default + warning. (5) *Redundancy vs `cross-runtime-verification-gate`* → shared leaf types + explicit V2 convergence; distinct purpose and rubric.
- **Stretch goal (V2+):** Converge with `cross-runtime-verification-gate` so the security audit runs on a deliberately **different model family** (an independent auditor with no stake in the producer's diff); then a **V3 per-repo findings ledger / suppression memory** that improves precision over time and creates real lock-in. SARIF export for CI interop.

## Out of Scope (V1)

- **Auto-trigger on risk** — V1 is manual-only; auto-firing on security-sensitive diffs is earned after precision is measured.
- **Blocking gate / merge-commit enforcement** — advisory only; gating on uncalibrated LLM output trains users to disable the feature.
- **Auto-fix / remediation** — advisory output decoupled from any patch; remediation is a later, approval-gated step.
- **Cross-runtime / model-family-diverse audit** — deferred to V2 to converge with `cross-runtime-verification-gate` rather than duplicate its provenance machinery.
- **Whole-repo audit** — diff-scoped only; whole-repo is a different, noisier product better served by deterministic CI SAST.
- **Persistent findings ledger / suppression memory** — V3; V1 reviews are stateless.
- **Configurable diff-base range / scan-scope knobs** — one fixed diff-base rule in V1 to avoid premature configuration surface.
- **SARIF export** — V2 interop concern; V1 surfaces findings in chat only.

## Architecture Decision Records

- [ADR-001: Standalone read-only reviewer agent + skill rubric, app-orchestrated diff-as-data workflow, advisory and diff-scoped, own event family](adrs/adr-001.md) — V1 mechanism and data model; rejects council-preset and skill-only.
- [ADR-002: The security reviewer is a hostile-input boundary — diff-as-data, read-only, redacted findings, honest disclaimer, CI injection corpus](adrs/adr-002.md) — threat model and non-negotiable guardrails.

## Integration with Existing Features

| Integration Point | How |
|-------------------|-----|
| Skills (`src/skills/mod.rs`) | `security-review` SKILL.md supplies the rubric via the existing injection path; `/skill:security-review` is the manual fallback. |
| Actions & capabilities (`src/actions/mod.rs:272`) | Read-only enforced by omitting Edit/Command from the reviewer profile at the single validation gate. |
| Event sourcing + chat projection (`src/app/chat/projection.rs`) | New `security_review_*` events + `ChatLifecycleKey::SecurityReview` projection arm. |
| Agent profiles (`src/config/mod.rs:480`) | Built-in `[agents.security-reviewer]` mirroring the no-Edit reviewer template. |
| Secret redaction (`src/file_index.rs:305`, `config::to_redacted_toml`) | Reused to redact diffs and findings before they persist; coordinates with `run-transcript-export`. |
| `cross-runtime-verification-gate` packet | Shares `Severity`/`Finding` leaf types; V2 convergence to a family-diverse audit. |
| Slash-command catalog (`src/slash_commands.rs`) | `/security-review` added as a recorded exception to the ADR-frozen catalog (thin wrapper over the `/agent:`+`/skill:` path). |

## Cost Estimate

| Type | Volume | Estimated Cost |
|------|--------|----------------|
| Reviewer LLM call | 1 per invocation, bounded by diff size (token-capped) | Usage-based on the configured runtime; no new infrastructure. Cost scales with diff size; manual invocation keeps volume low. |

## Open Questions

- **Diff-base edge cases:** detached HEAD, no default/upstream branch, first commit, dirty working tree — confirm the fallback chain for the single diff-base rule.
- **Cross-file recall:** how far may read-only context reads follow a changed source toward an unchanged sink before it stops being "diff-scoped"? Needs a bound.
- **Precision tuning:** the initial confidence threshold and the exact hard-exclusion list contents should be calibrated against the seeded-vuln eval set, not guessed.
- **Default reviewer model + weak-model threshold:** which model ships as the default, and how is "weak/uncalibrated" defined for the F7 warning?
- **V1 command flags:** minimal flag set (e.g., severity floor) vs none.
- **Large-diff handling:** truncate-with-note vs refuse, and where the token cap sits.
