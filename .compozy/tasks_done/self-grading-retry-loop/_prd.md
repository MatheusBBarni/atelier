# PRD: Externally-Grounded Auto-Verification Loop

## Overview

When an editing agent finishes a step, atelier records it as `Completed` and moves on — with no
independent check that the work is correct or that the agent actually verified it. The only quality
circuit that exists (a reviewer→fixer cycle) fires only when the orchestrator *chooses* to route a
reviewer, which it routinely skips. So confidently-wrong edits land silently, and catching them still
requires a human.

This feature adds an **automatic, externally-grounded verification loop**: after an editing step, a
grader runs the project's real checks (tests/build/lint), and on failure re-dispatches the same agent
with the concrete failures — bounded by a retry cap, visible in the transcript, and escalated to the
user if it can't converge. The grader **runs the checks and derives the verdict from their result** — the
agent doesn't get to mark its own homework. It is for users running autonomous work who want quality
without babysitting, and for users who distrust "Completed" claims and re-review everything by hand. It
is valuable because it raises baseline output trust across every runtime while reusing machinery atelier
already ships, and because the *grounding* (not the loop, which is now table stakes) is the honest edge.

## Goals

- **Reduce escaped defects.** Cut the rate at which wrong or unverified work reaches the user on
  grading-enabled runs, measured against an internal baseline (target: ≥ 25% relative reduction).
- **Make "Completed" trustworthy.** Every graded step carries real, cited verification evidence (which
  check ran, its result) instead of a self-attested claim.
- **Stay bounded and cheap.** A single verification pass per step, capped retries, no runaway loops:
  ≤ 1.5× median run cost with grading on (≈ 2× only on steps that actually fail and retry).
- **Cost nothing for non-adopters.** Opt-in, default OFF — zero behavior, cost, or latency change for
  users who don't enable it.
- **Milestones are phase-gated** (see Phased Rollout): MVP proves defect reduction on opt-in runs;
  Phase 2 makes verification authoritative and discoverable; Phase 3 earns default-on.

## User Stories

**Unattended Uma** (runs autonomously in the default mode, walks away):
- As Uma, I want the harness to automatically run my tests after each edit and fix failures, so that I
  come back to working code instead of a confident-but-broken "done."
- As Uma, I want a hard cap on retries and to be asked what to do if it can't fix something, so that a
  stuck loop never silently burns my budget or ships bad work.

**Skeptical Sam** (distrusts agent "Completed" claims, re-reviews by hand today):
- As Sam, I want to *see* the actual check run and its red→green result in the transcript, so that I
  trust the verdict without re-running everything myself.
- As Sam, I want the agent to be honest when it *couldn't* verify (no tests to run), so that "unverified"
  is never disguised as "passed."

**Either persona (control):**
- As a user, I want verification off by default and a one-line switch to turn it on, so that it never
  changes my runs unless I ask for it.

## Core Features

**Priority: Critical (MVP)**

- **Automatic post-edit verification.** After any editing step reports Completed, the loop triggers
  automatically (no orchestrator discretion). It runs only on steps that actually changed files.
- **Externally-grounded verdict.** The grader runs the project's real checks and derives PASS/FAIL from
  the result. A PASS requires cited evidence that a real check passed; the agent cannot self-attest a pass.
- **Honest skip when unverifiable.** If no real check runs (e.g. no tests exist), the step is recorded as
  **skipped / done-unverified** with a visible note — never a fabricated pass, never a punitive fail.
- **Bounded grade→fix retries.** On FAIL, the same agent is re-dispatched with the concrete failures
  appended, up to a configured cap (reusing the existing review/fix cycle limit). The loop converges or stops.
- **Visible loop with a counter.** The whole cycle renders as one evolving transcript item that updates in
  place — "verifying… → FAIL (check, retry 1/2) → fixing… → PASS ✓" — showing the check, its result, and
  `retry N/max`. This directly answers the #1 competitor complaint (invisible runaway loops).
- **User escalation on exhaustion.** When retries exhaust and it still fails, the run pauses and asks the
  user: **accept anyway / retry / abort** — surfacing the last failure so a human makes the final call.
- **Opt-in control.** A single switch enables the feature; it is OFF by default.

**Priority: High (Phase 2)**

- **Authoritative verify command.** The user can configure (or atelier can auto-detect) the verify command
  so verification is reproducible and harness-asserted instead of agent-guessed.
- **Discoverability.** A health-check entry and a config status line tell the user when grading is on (and
  warn when it's on but has nothing to run), so the opt-in feature isn't buried.

**Priority: Medium (Phase 3)**

- **Default-on where grounded.** Enabled by default wherever a real check exists; degrades to
  done-unverified where none does.
- **Stuck detection.** Warn when retries repeat the identical failure (not converging).
- **Trusted-command list.** A user-extensible allowlist so custom verify commands stop prompting in the
  oversight ("normal") mode.

## User Experience

**First contact / onboarding.** The feature is off until the user sets a one-line switch. On enabling it,
nothing else is required for a standard project — the grader uses the project's canonical checks. (Phase 2
adds a health-check note and config status line so users discover its state and configure a custom command.)

**Primary flow — the common case (default mode):**
1. The user gives a task; an agent edits files and reports done.
2. The grader automatically runs the project's checks. For a standard project these run silently (they're
   already trusted commands), so there's no interruption.
3. If checks pass, the step shows **PASS ✓** with the check and its result, and the run continues.
4. If checks fail, the transcript item shows **FAIL** with the failure and **retry 1/max**; the same agent
   is re-dispatched with the concrete failures; the item updates in place until it reaches **PASS ✓** or the cap.
5. If the cap is hit and it still fails, the run **pauses and asks**: accept / retry / abort.

**Skip flow:** If there's nothing to run (no tests), the step shows a muted **"skipped — unverified"** note
and continues. The user always knows verification didn't happen; it is never disguised as a pass.

**UI / accessibility considerations.** The loop is one collapsing transcript item (not a flood of step
lines), bounded in length, with a visible retry counter — legible at a glance and consistent with how
atelier already collapses multi-round lifecycles. Escalation reuses the existing pause/answer affordance.

**Discoverability (Phase 2).** Because the switch is off by default, a health-check entry and a `/config`
status line surface whether grading is active and flag misconfiguration (on, but no check to run), reusing
surfaces users already consult rather than adding a new command.

## High-Level Technical Constraints

- **Verification must be externally grounded.** A PASS requires the result of a real executed check; an
  LLM judging its own prose is explicitly not a valid pass signal (see ADR-001).
- **Command-approval behavior is inherited, not rebuilt.** In the default mode all checks run without
  prompting. In the oversight ("normal") mode, canonical project checks (e.g. the standard test/lint/build
  commands) also run silently because they're already trusted, but a *custom* or *chained* command prompts
  the user each time. V1 documents this and recommends canonical commands; a trusted-command list is Phase 3.
- **Bounded by the existing cycle cap.** Retries reuse atelier's review/fix cycle limit; the loop cannot run
  unbounded.
- **Performance from the user's view.** One verification pass per step; median run cost ≤ 1.5× with grading
  on, ≈ 2× only on steps that fail and retry. Verification of already-passing work adds a single check.
- **No data leaves the machine.** Checks run locally through the existing command path; no new external calls.

## Non-Goals (Out of Scope)

- **Pure LLM self-grade** — an ungrounded verdict is cut entirely; a no-check step skips rather than guessing.
- **Configurable / auto-detected verify command** — deferred to Phase 2; V1 uses the agent-discovered command.
- **Default-on** — Phase 3, gated on defect-reduction data; V1 is opt-in default-OFF.
- **Stuck / repeated-failure detection and a trusted-command list** — Phase 3.
- **Automatic test generation when none exist** — out of scope; V1 skips-and-notes (a possible future).
- **Folding this into the council review** — the grader stays a distinct, cheap, automatic check; council
  remains the deep, on-demand review.
- **Static "which files are test-covered" analysis** — replaced by simply observing whether a real check ran.

## Phased Rollout Plan

### MVP (Phase 1)
- **Includes:** automatic post-edit trigger; externally-grounded verdict (agent-discovered command);
  honest skip when unverifiable; bounded grade→fix retries; one evolving transcript item with a retry
  counter; user escalation on exhaustion; opt-in default-OFF; inherited approval behavior (documented).
- **Success criteria to proceed:** on opt-in runs, a measurable reduction in escaped defects / corrective
  re-prompts vs the off baseline; false-pass rate < 10%; ≥ 70% of loops converge within the cap; cost
  multiplier within target; no runaway-loop or silent-deny incidents.

### Phase 2 — Authoritative grounding
- **Adds:** configurable / auto-detected verify command (reproducible, harness-asserted verification);
  discoverability via a health-check entry and a `/config` status line/warning.
- **Success criteria to proceed:** majority of enabled projects run a configured/detected command;
  reduced "agent ran no check" skip rate; positive qualitative trust signal from skeptical users.

### Phase 3 — Trust + hardening
- **Adds:** default-on wherever a real check exists (degrade to done-unverified otherwise); stuck /
  repeated-failure detection; user-extensible trusted-command list.
- **Long-term success:** broad default-on adoption with stable defect-reduction and within-budget cost;
  approval fatigue eliminated for custom commands.

## Success Metrics

| Metric | Target | Notes |
|--------|--------|-------|
| **Escaped-defect reduction** *(North Star)* | ≥ 25% relative fewer corrective re-prompts/reverts on grading-on vs off | Internal A/B baseline, not an imported benchmark delta |
| First-pass / repair success against the gate | ≥ 70% of grade→fix loops reach PASS within the cap | Loop converges rather than escalates |
| False-pass rate | < 10% of PASS verdicts later reverted/corrected | Guards self-preference bias / false greens |
| Cost multiplier | ≤ 1.5× median run cost with grading on (≈ 2× on failing steps) | Bounded single pass; research puts unbounded loops at 5–30× |
| Escalation actionability | ≥ 30% of exhaustion escalations acted on (retry/abort, not rubber-stamped) | Detects reflexive accept |
| Adoption / discovery (Phase 2+) | ≥ 20% of active configs enable grading within 60 days of Phase 2 | Measures the opt-in discoverability fix |

## Risks and Mitigations

- **Adoption risk (opt-in is undiscoverable).** Default-OFF features get buried (seen across competitors).
  → Phase 2 health-check + config surfacing; Phase 3 default-on-where-grounded with a documented flip
  criterion (preserved council dissent: don't make a must-have permanently opt-in).
- **Competitive risk (the loop is table stakes).** Claude Code 2.1, Cursor, Cline, Devin, and free OSS all
  ship a verify-fix loop. → Differentiate on **grounding by default**, clean grader/implementer separation,
  and bounded, observable cycles — not on "unlocks what others paywall" (that framing is refutable).
- **Value-premise risk (ungrounded self-grade degrades correct work).** Documented in the literature.
  → Grounding is mandatory; a no-check step skips rather than guessing.
- **Approval fatigue (custom commands prompt every time in oversight mode).** → Document; recommend
  canonical commands; Phase 3 trusted-command list. (The default mode never prompts.)
- **Agent skips verification to avoid the loop.** With an agent-discovered command, the agent could run no
  check and claim a skip. → V1 records skips honestly as "done-unverified" (never a false pass); Phase 2's
  configured command makes verification mandatory.
- **Runaway / cost risk.** → Hard retry cap, visible counter, escalation on exhaustion; Phase 3 stuck detection.

## Architecture Decision Records

- [ADR-001: Externally-grounded auto-verification loop, not an LLM self-grader](adrs/adr-001.md) — grounding
  is mandatory (grader runs real checks, PASS only on cited exit-0); typed machine-derived verdict; broad
  trigger + skip-when-no-oracle; reuse the existing path; default OFF with a documented V2 flip criterion.
- [ADR-002: Phased delivery — agent-discovered verification in V1, config-asserted in Phase 2](adrs/adr-002.md)
  — MVP uses the agent-discovered command (verdict still exit-code-derived); authoritative configurable/
  auto-detected verification and discoverability follow in Phase 2; default-on + hardening in Phase 3.

## Open Questions

- **What counts as "a real check"?** Exit-code of tests only, or also build and lint? Do lint *warnings*
  (exit 0) count as verified, or only hard failures? (Refined in Phase 2 with the configured command.)
- **Step-budget accounting:** should grader/retry sub-steps consume the run's step budget (risking an early
  limit-stop before convergence) or be accounted separately? (Resolve before MVP.)
- **Parallel edits:** how grading and the evolving transcript item behave when several parallel children
  edit and verify concurrently.
- **Phase 2 verify-command resolution:** config-only vs auto-detected vs both, and how it spans non-cargo /
  multi-language projects.
- **V2 flip threshold:** the concrete defect-reduction / false-pass data that justifies flipping the default
  to on-where-grounded.
- **Test generation on "no tests":** is skip-and-note enough for V1, or should a future phase offer to
  scaffold a check (whitespace no competitor fills)?
