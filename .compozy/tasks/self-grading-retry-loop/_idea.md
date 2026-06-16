# Externally-Grounded Auto-Verification Loop

## Overview

- **Problem:** An Edit-capable agent can report `Completed` work that is wrong or unverified, and nothing automatically checks it. atelier already *bounds* a reviewer→fixer cycle (`max_review_fix_cycles`), but that cycle is **orchestrator-discretionary** — the model chooses whether to route a reviewer, and usually doesn't. There is no automatic gate that re-runs the project's own tests/lint after an edit and forces a fix when they fail.
- **Who:** Power users running autonomous work who want quality without babysitting ("Unattended Uma"), and users who distrust agent "Completed" claims and hand-re-review everything today ("Skeptical Sam"). The harness itself benefits: a self-correcting loop raises baseline output trust across every runtime.
- **Why valuable:** It converts atelier's existing primitives — a no-Edit `reviewer` profile, `Capability::Command` + `RunCommand`, a live cycle bound — into an **externally-grounded** verification loop: the harness runs the real toolchain and re-dispatches on failure. The differentiator is **grounding by default**, not the loop (which is now table stakes).
- **V1 ambition:** A **Quick Win leaning Compounding** — high feasibility (reuses live machinery), conditional value (must be externally grounded), and a trust surface that compounds as users learn the agent "can't mark its own homework."

## Summary / Differentiator

Built-in self-correction has commoditized: Claude Code 2.1 ships native generate→lint/type/test→self-correct, Cursor/Cline/Devin bundle run-and-fix loops, and CrewAI guardrails give this exact grade-and-retry design away free. The literature is equally clear that the *naive* version is worse than nothing — pure LLM self-grade is unreliable and can **degrade correct work** (Huang et al. 2310.01798; CRITIC ~54% self-verify; self-preference bias passes a model's own buggy code). The white space is **how** verification is grounded: the grader **runs the project's real compile/test/lint** and a PASS is emittable *only* with cited exit-0 evidence — the agent doesn't get to mark its own homework, it has to show you the test output. atelier's wedge: **"externally-grounded, bounded, observable verification you watch happen in the transcript — so even a cheap local model gets a check it cannot fake."**

## Problem

atelier runs autonomous, multi-step agents whose only signal of success is the agent's own `AgentResult`. `AgentResultStatus` means "how the turn ended" (`Completed`/`Blocked`/`NoChanges`…), never "the work is correct," and the `verification` field is free-text **self-attested by the producing agent**. So a confidently-wrong edit lands as `Completed` with a plausible self-report, and nothing independent checks it. The one quality circuit that exists — the reviewer→fixer cycle bounded by `max_review_fix_cycles` — only fires when the orchestrator *chooses* to route a reviewer, which it routinely skips. Catching bad work therefore still requires a human, or an explicitly-requested council review the orchestrator reserves for high-risk prompts.

The deeper problem is that the obvious fix is a trap. An LLM grader that judges its own (or a peer's) prose verdict is exactly the configuration the evidence says fails: intrinsic self-correction flips correct answers to wrong, self-evaluation hovers near random on hard tasks, and self-preference bias inflates a model's confidence in its own output. The only self-critique that reliably *improves* code is grounded in **external** feedback — the compiler, the test suite, the linter. atelier is unusually well-placed to supply exactly that: it already ships a read-only `reviewer` profile that can `RunCommand`, a live cycle bound, and a `WaitingForUser` pause for escalation. The missing piece is a loop that runs the real toolchain automatically after an edit, derives a verdict from the exit code, and re-dispatches the implementer with the concrete failures — bounded, and visible in the transcript.

### Market Data

- **Self-critique works only when externally grounded.** Reflexion's HumanEval gain (80%→91% pass@1) came from **unit-test** feedback, not self-grade; Self-Refine improved ~20% but plateaus fast; CRITIC self-verifies at ~54% (near random) without tools; Olausson found human/executable feedback beats GPT-4 self-feedback **1.58×**; self-preference bias (2410.21819) measured GPT-4 over-rating its own outputs.
- **Unbounded refinement is harmful, not just wasteful** — iterative AI code generation accumulated *more* security vulnerabilities per round (2506.11022); Devin users report agents "stuck in expensive loops … racking up compute while failing to deliver working code."
- **The loop is table stakes, not a moat** — Claude Code 2.1 native auto-verify; Cursor/Cline/Devin run-and-fix; **CrewAI guardrails ≈ this exact design, free**. No one sells "a critic loop" as a SKU; paid products charge for compute/packaging.
- **The pain is real and growing** — ~46% of new code is AI-generated; rework rises 30–60% within 6 months of heavy AI adoption (≈4–6 of every 10 hours saved); code churn rose 5.5%→7.9% (GitClear, 2020→2024). A single LLM-grounded verify pass costs ~$0.001–0.045, far below the $30–40/dev/mo paid for in-product reviewers (Cursor BugBot, Qodo).

## Core Features

| # | Feature | Priority | Description |
|---|---------|----------|-------------|
| F1 | External-grounding verification gate | Critical | The grader (built-in `reviewer`, Read/Command/Verify, **no Edit**) runs the project's real compile/test/lint via `RunCommand`. A PASS is emittable **only** with cited exit-0 evidence (command + exit code + captured output). Pure-LLM-self-grade is **not** a code path. |
| F2 | Auto-trigger + dynamic oracle skip | Critical | Fires after every Edit-capable step that reports `Completed`. Resolves oracle-existence *dynamically inside the gate*: if a check yields a pass/fail signal, grade; if none exists, **SKIP silently and emit no verdict** — never fabricate a pass, never punish good work. |
| F3 | Typed, machine-derived verdict | Critical | A new verdict record (not an overload of `status`): `verified` = deterministic AND of external exit codes with cited evidence; `correct` = at most an LLM **triage** of concrete failures. FAIL payload = structured failures + triage (routes the fixer). Updates the codex/claude/cursor schema briefs. |
| F4 | Bounded grade→fix re-dispatch | Critical | On FAIL, re-dispatch the same implementer with the concrete failures appended, bounded by `max_review_fix_cycles`. Extract the shared per-step-limit helper **first** so the loop reuses the bound instead of adding a 6th duplicated check. |
| F5 | User escalation on exhaustion | High | When cycles exhaust and the gate still fails, escalate via `WaitingForUser` with **accept / retry / abort** — new tri-state routing in the resume path (today it only appends text and re-drives). |
| F6 | One evolving chat item | High | Grade/retry rounds collapse into a single transcript item ("working → graded FAIL → retry 1/N → PASS") via a new `ChatLifecycleKey` (e.g. `GradedStep`) + projection arm — explicitly **not** the council `Workflow` key, which does not collapse. |
| F7 | Opt-in config + discoverability | Medium | `[grading] enabled = false` default; surfaced in `--doctor` / first-run / `/config` so it's findable. Carries a **documented V2 flip criterion**: default-ON wherever an external check exists, once the verdict field and escaped-defect data justify it. |

## KPIs

| KPI | Target | How to Measure |
|-----|--------|----------------|
| **Rework reduction** *(North Star)* | ≥ 25% fewer corrective re-prompts/reverts on grading-on vs off runs | A/B by config flag over run-outcome events |
| Grade precision (false-pass) | < 10% of PASS verdicts later reverted/corrected | verdict events vs subsequent revert/re-prompt |
| Grade recall (catch rate) | ≥ 60% of FAIL verdicts map to a real defect on a labeled sample | sampled manual audit vs verdict |
| Loop convergence | ≥ 70% of grade→fix loops reach PASS within the cap (no escalation) | grade-round events per run |
| Cost multiplier | ≤ 1.4× median tokens/run with grading on | token accounting, grading-on vs off |
| Escalation actionability | ≥ 30% of exhaustion escalations acted on (retry/abort, not rubber-stamped) | `WaitingForUser` resolution events |

## Feature Assessment

| Criteria | Question | Score |
|----------|----------|-------|
| **Impact** | More valuable? | **Strong** — raises trust/quality, *conditional on external grounding* |
| **Reach** | % of users affected? | **Strong** — every Edit-producing run once enabled (opt-in caps near-term) |
| **Frequency** | How often encountered? | **Strong** — fires on every Edit-capable step |
| **Differentiation** | Set us apart? | **Maybe** — the loop is table stakes; only grounding-by-default + grader/implementer separation + bounded/observable is differentiated |
| **Defensibility** | Compounds / hard to copy? | **Maybe** — the loop is copyable; the moat is the integrated externally-grounded default woven into the multi-runtime harness, which compounds modestly |
| **Feasibility** | Can we build it? | **Strong** — reuses the `reviewer` profile, `RunCommand`, the live cycle bound, and the council executor template |

Leverage type: **Quick Win leaning Compounding Feature**.

## Council Insights

- **Recommended approach:** Build an externally-grounded auto-verification loop, not an LLM self-grader. The grader runs real compile/test/lint; the verdict is machine-derived from exit codes; the LLM only triages failures. Reuse the existing reviewer→fixer path, extract the shared per-step-limit helper first, and **defer** a generic virtual-target registry (rule-of-two). Recorded as **[ADR-001](adrs/adr-001.md)**.
- **Key trade-offs:** minimal exit-code gate (cheap, can't enforce "correct AND verified" cleanly, overloads `status`) **vs** typed verdict + reuse (chosen); registry now **vs** explicit second branch + fast-follow extraction (chosen); per-step grading **vs** scope-to-test-covered (resolved: broad trigger + dynamic oracle skip).
- **Risks identified:** self-preference bias → grader ≠ producer + exit-code-derived verdict; unbounded loops / security drift (2506.11022) → bounded + skip-on-no-oracle + convergence; `status` overload → separate verdict field; 6th duplicated limit check → extract helper first; invisible events → add the projection arm; step-budget interaction → decide whether grader sub-steps consume `max_agent_steps`.
- **Stretch goal (V2+):** default-ON wherever an external check exists; extract the virtual-target registry (council + grader as entries); richer triage-driven fix routing.
- **Preserved dissent (default posture):** product-mind holds that permanent default-OFF makes a must-have into dead code and the design should commit *now* to the flip criterion; pragmatic-engineer held firm that "grounded ⇒ safe" isn't machine-enforceable until the verdict field exists. Resolution: **OFF for V1 + documented V2 flip criterion + discoverability hints.**

## Integration with Existing Features

| Integration Point | How |
|-------------------|-----|
| Built-in `reviewer` profile (`config/mod.rs:837-855`) | Reused as the grader (Read/Command/Verify, no Edit); a PASS without cited external evidence is structurally invalid |
| `Capability::Command` + `RunCommand` (`actions/mod.rs`) | The grader runs the real compile/test/lint that grounds the verdict |
| `max_review_fix_cycles` + `review_fix_cycle_limit_reached` (`app/mod.rs:3704-3724`) | The existing cycle bound the grade→fix loop reuses; escalation replaces the current hard-stop |
| Council sentinel + `run_council_workflow` (`orchestrator/mod.rs:9`, `app/mod.rs:3155`) | Template for the grader dispatch branch (explicit second branch; registry deferred) |
| `AgentResult` + runtime schema briefs (`orchestrator/mod.rs:147-160`, `runtime/codex.rs:320`) | New verdict record alongside `AgentResult`; briefs updated so the contract is typed, not convention |
| `WaitingForUser` / `resolve_pending_clarification` (`app/mod.rs:1689-1736`) | Escalation transport; needs new accept/retry/abort routing on `selected_option_id` |
| Chat projection + `ChatLifecycleKey` (`app/chat/projection.rs`, `chat/mod.rs`) | New key + `apply_grade_round` arm to collapse rounds into one evolving item (fix the `step_limit_reached` projection gap as precedent) |
| Per-step limit checks (~6 methods, `app/mod.rs`) | Extract one shared `stop_for_agent_step_limit` helper before the grader lands |

## Out of Scope (V1)

- **Pure LLM self-grade** — cut entirely; the evidence says an ungrounded self-grade is negative expected value, and a no-oracle step SKIPs rather than manufacturing a verdict.
- **Generic virtual-target registry** — deferred to a fast-follow (rule-of-two); the grader ships as an explicit second branch modeled on council.
- **Default-ON** — V2, gated on the verdict field existing and escaped-defect data; V1 is opt-in default-OFF.
- **Static "which changes are test-covered" analysis** — replaced by a dynamic in-gate oracle check; no coverage/dependency engine.
- **Grading non-Edit / read-only / no-change steps** — the trigger is Edit-capable steps with actual `changed_files`.
- **Folding council and the grader into one "review" subsystem** — they stay distinct (cheap automatic gate vs deep on-demand council).

## Architecture Decision Records

- [ADR-001: Externally-grounded auto-verification loop, not an LLM self-grader](adrs/adr-001.md) — grounding is mandatory (grader runs real tests/lint, PASS only on cited exit-0); typed machine-derived verdict (not a `status` overload); broad trigger + dynamic oracle skip; reuse the existing path, extract the limit helper first, defer the registry; default OFF v1 with a documented V2 flip criterion.

## Open Questions

- **Verify-command resolution:** how does the gate discover the project's compile/test/lint command — config-driven (`[grading.command]` / per-agent), auto-detected (cargo/npm/…), or both? How does it resolve across atelier's multiple runtimes and languages?
- **Approval-mode interaction:** the grader's `RunCommand` surfaces an approval prompt in `normal` mode. Does auto-running tests as a gate need a pre-approved/allowlisted command, or does it inherit normal approval gating (and what's the UX when it blocks)?
- **Step-budget accounting:** should grader sub-steps consume `run.step_count` / `max_agent_steps` like council members do (risking mid-loop `run_limit_reached` before convergence), or be accounted separately / off-budget?
- **Verdict-field shape:** exact schema for the typed verdict (`verified`/`correct` axes, evidence citation, structured-failures payload) and how the three runtime schema briefs express it without bloating the agent contract.
- **Parallel groups:** how grading applies to parallel-child Edit steps (per-child gate vs group-level), and how rounds render when several children grade concurrently.
- **V2 flip threshold:** the concrete escaped-defect / precision data that would justify flipping the default to ON-where-grounded.
