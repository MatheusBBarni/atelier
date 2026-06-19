# Rich Approval Modal + Per-Session Trust List

## Overview

Make each approval in `atelier` a **fast, confident, and safe** decision so guarded mode is genuinely usable — without changing the `Yolo` default. Today, write/command actions in `Normal` mode produce a **binary** approve/deny prompt over a one-line summary; the only relief from repeated prompts is `Yolo` (auto-approve everything), which is the default. This feature adds three layers: a **fail-closed destructive floor** that re-prompts on anything it can't prove safe — *even in Yolo*; a **rich decision-support modal** that shows the resolved command, diff, affected roots, and a risk tier; and a **minimal, session-scoped trust entry** ("trust this exact target this session") surfaced at the approval stop.

**Who it's for:** every atelier user (the floor protects even Yolo-default users), with the modal and trust panel serving operators who run `Normal`. **Why it's valuable:** it converts a 93%-rubber-stamped gate into a meaningful one and gives the default user a real safety net. **V1 ambition:** deliberately scoped — ship the safety core to everyone; defer sandboxing, undo, and richer trust keying to V2.

## Problem

In an autonomous multi-agent harness, the approval gate is the single enforcement point before an agent's `ActionRequest` touches the filesystem or shell. Atelier's gate is currently all-or-nothing: in `Normal` mode every write/command re-prompts with a one-line summary and a binary yes/no; in `Yolo` mode (the default) nothing is gated at all. There is no middle ground, so a user worn down by repeated identical prompts has exactly one escape — flip to Yolo and auto-approve *everything*, including the rare destructive action that should never have been automatic.

This is not merely an annoyance; it is a safety failure. When a user approves the same `cargo test` for the tenth time, attention collapses, and the one `git push --force` buried in the stream gets the same reflexive Enter. The binary prompt also gives the user too little to decide well: a one-line summary can't show what a patch actually changes, whether a write lands outside the workspace, or that an `rm` target expanded to something dangerous. So users either over-trust (Yolo) or suffer fatigue (Normal) — and both paths end in waved-through risk.

The opportunity is to spend the "friction budget" where it matters: make the safe majority effectively free, make the dangerous minority impossible to miss, and let the human carry context-rich, reversible-feeling trust within a session — while keeping a hard floor under even the most permissive mode.

### Market Data

- **Approval fatigue is measured and real:** Anthropic reports **~93% of permission prompts are approved** in Claude Code, explicitly naming "approval fatigue" where people stop reading what they approve. The value of a gate is the **~7% that should be "no."**
- **It's a security problem, not UX polish:** when prompts arrive faster than they're read, operators stop judging and start dismissing; agents then route around friction by requesting *broader* permissions. Security-warning research shows users dismiss ~half of warnings in under two seconds and notice content changes only ~14% of the time; **polymorphic (varied) warnings measurably reduce habituation**.
- **Competitors converged** on allowlist + denylist (denylist wins) + a sandbox/classifier middle tier + a Yolo escape (Claude Code, Codex CLI, Cursor, Copilot, Gemini CLI, Windsurf, Warp, Zed). Claude Code **circuit-breaks `rm -rf /` even in bypass mode** — precedent for a floor beneath Yolo. None surface a *live, revocable* trust panel; allowlists are buried in config files.
- **Denylists fail open:** Cursor shipped four documented denylist bypasses; shell evasions are unbounded (`find -delete`, `dd`, `git clean -fdx`, `python -c`, `base64|sh`). The real `rm -rf ~` incident fired because `~` expanded unquoted — classification must run on the **post-expansion** string.

## Summary / Differentiator

Every mature competitor buries an allowlist in a config file and offers a blunt Yolo escape. Atelier's angle is a **fail-closed floor that makes even the Yolo default auditable and safe**, paired with a **visible, revocable, in-session trust panel** — turning the approval modal from a yes/no box into a decision-support surface (resolved command, diff, which root/capability is being crossed, risk rationale). Because atelier is event-sourced, every auto-approval is recorded and projected to chat, so "fewer risky auto-approvals" is delivered to 100% of users without taking away the default they chose.

## Core Features

| # | Feature | Priority | Description |
|---|---|---|---|
| F1 | Fail-closed destructive floor | Critical | Reuse/extend `classify_command()` so only *provably-safe* actions auto-run; anything unproven re-prompts (`unknown → confirm`, never silent-allow). A non-bypassable core (`rm -rf /` & `~`, force-push, `sudo`, `chmod -R 777`, `curl\|bash`, cross-root writes/reads, secret reads) re-prompts **even in Yolo**. Classifies on the post-shell-expansion string at exec time. |
| F2 | Rich decision-support modal | Critical | Replaces the one-line prompt with the resolved/expanded command, a diff for `ApplyPatch`/`WriteFile`, affected paths relative to write-roots, the capability/root being crossed, a one-line risk rationale, and a risk tier (green/yellow/red from the classifier). |
| F3 | Habituation-resistant controls | High | Red tier requires a deliberate, non-default keystroke (no Enter-to-approve); "approve" never shares a key with "approve-and-trust." Optional polymorphic styling on the dangerous tier to break muscle memory. |
| F4 | Minimal floor-anchored session trust | High | Exactly one scope — "trust this **exact resolved target** for the rest of this session" — offered at the approval/floor stop. In-memory only, never persisted; no wildcards, patterns, action-types, or per-agent trust. |
| F5 | Live trust panel (list & revoke) | Medium | A TUI panel to inspect and revoke active session-trust entries — the differentiator competitors lack (their allowlists are invisible config). Restart is the fallback reset. |
| F6 | Auditable auto-approvals + safe parallel handling | Medium | Every trust grant and trust-driven auto-approval is recorded as an event and projected to chat. Trust applies only to *future* actions; a queued parallel batch is never retro-flushed without showing the full set and a count; floor items are re-classified at exec time and never auto-cleared. |

## KPIs

| KPI | Target | How to Measure |
|---|---|---|
| Destructive-floor coverage | **100%** (0 escapes) | Count actions classified destructive that executed with *no* preceding `approval_requested` event — must be 0, even in Yolo |
| Risky-auto-approval reduction | **≥ 95%** human-reviewed | High-risk actions with a user `approval_resolved` ÷ total high-risk actions (Yolo baseline ≈ 0%) |
| Repeat-prompt collapse | **≥ 70%** | Trust-auto-resolved events ÷ repeat approvals matching an existing exact-target trust this session |
| Novel-approval decision latency (median) | **≤ 8s** | median(`approval_resolved.ts − approval_requested.ts`) for first-seen actions |
| Scoped-trust adoption | **≥ 50%** of sessions | Sessions emitting ≥1 `trust_granted` ÷ sessions with ≥1 `approval_requested` |

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | **Strong** — converts a 93%-reflexive gate into a meaningful one + adds a real safety net |
| **Reach** | What % of users would this affect? | **Must do** — the floor touches *every* user, including the Yolo default |
| **Frequency** | How often would users encounter this value? | **Must do** — every run with a write/command action |
| **Differentiation** | Set us apart or match competitors? | **Strong** — visible/revocable trust + a floor-under-Yolo are absent from competitors |
| **Defensibility** | Easy to copy or compounds? | **Maybe** — allowlists are widely copied; the moat is the event-log audit trail + architecture coupling |
| **Feasibility** | Can we actually build this? | **Must do** — classifier, approval flow, modal patterns, persistence latch, event sourcing all already exist |

**Leverage type: Strategic Bet** (foundational to trust in an autonomous harness; the audit trail and trust patterns compound over time).

## Council Insights

- **Recommended approach:** The **fail-closed destructive floor is the core product** — the only layer the Yolo-default majority ever feels, and the sole mechanism delivering "fewer risky auto-approvals" to 100% of users. Ship floor → modal → minimal trust. Reframe the floor from a *denylist* (fails open, infinite evasions) to a **fail-closed allowlist** (auto-run only the provably-safe; everything else re-prompts), which makes an honest safety claim instead of "we block `rm`."
- **Key trade-offs:** A fail-closed floor re-prompts more, so it needs a solid provably-safe allowlist or it induces the very rubber-stamping it fights. The rich modal + trust panel mainly serve the opt-in `Normal` minority; the floor is what reaches the default. "Faster decisions" is a half-vanity metric — re-aimed at **decision quality on the dangerous ~7%**, with speed as a guardrail.
- **Risks identified:** (1) reflexive Enter / habituation → red-tier non-default keystroke + polymorphic prompt; (2) `RunCommand` opacity (semantic effect-class decays to name-matching for arbitrary exec) → fail-closed default + audit; (3) parallel-queue decision-laundering/TOCTOU → trust applies only to future actions, never retro-flush, floor items never auto-cleared; (4) the non-bypassable core must stay non-disablable — "a `floor_disabled=true` flag is the day the product dies."
- **Stretch goal (V2+):** **Safe-by-construction** — OS-level sandbox containment for `RunCommand` + snapshot/undo reversibility to shrink the prompt stream structurally (undo is defense-in-depth for local-fs, never a substitute for the floor since it can't reach network/push/publish). Plus richer `(agent, action-class, root)` trust keying — with the explicit caveat that agent identity is hijackable via prompt injection.

## Integration with Existing Features

| Integration Point | How |
|---|---|
| `classify_command()` / `is_vcs_mutation()` (`src/actions/mod.rs`) | Reframe classifier output as provably-safe vs. must-confirm; add the non-bypassable core that ignores `ApprovalMode::Yolo` |
| `resolve_pending_approval` + parallel approval queue (`src/app/mod.rs`) | Insert the trust check before re-prompting; gate queue auto-clear with the "future-only, show-the-batch" rules |
| `PendingApprovalView` / `AppState` snapshot | Extend with precomputed modal fields (resolved command, diff, risk tier) so the pure-function renderer can show them |
| Clarification options-list (`src/tui/mod.rs`) | Reuse the pattern for the "pick scope" affordance and the trust panel |
| Event sourcing → chat projection (`src/app/chat/projection.rs`) | New `trust_granted` / auto-approval events styled distinctly from user approvals |
| `.atelier/ui_state.json` latch (ADR-004) | Reuse only for a one-time modal explainer — **not** for trust persistence (trust is in-memory) |
| `src/tui/theme.rs` | Add semantic risk-tier tokens (respecting `colors_live_only_in_theme_module`) |

## Out of Scope (V1)

- **Pattern/glob, action-type, and per-agent trust** — broad scopes are "scoped invisible Yolo"; agent identity is hijackable via prompt injection. Deferred to V2 with a caveat. Justification: directly works against the "fewer *risky* approvals" goal.
- **Cross-session / disk-persisted trust** — keeps blast radius bounded to one process and avoids an opaque, accreting policy nobody re-reads. Restart is the reset.
- **Sandbox / OS-level containment for `RunCommand`** — a massive, platform-specific lift; atelier's read/write roots are policy, not an OS sandbox. V2+ stretch.
- **Snapshot/undo reversibility** — V2 defense-in-depth; structurally can't reach cross-boundary effects (network, push, publish), so it's not a substitute for the floor.
- **Changing the Yolo default or Normal-retention work** — the user explicitly keeps Yolo as default; this feature makes the default *safe*, it doesn't relitigate it.
- **Trust-management UI beyond list-and-revoke** — no editing, grouping, or import/export; restart is the fallback.

## Architecture Decision Records

- [ADR-001: V1 scope — fail-closed destructive floor, decision-support modal, and minimal floor-anchored session trust](adrs/adr-001.md) — Ships the safety floor first as a fail-closed allowlist (not a denylist), with a rich modal and a single exact-target session-trust scope; defers sandbox, undo, and richer trust keying to V2.

## Open Questions

- **Approve-tier histogram is unmeasured** — is repetitive Approve-tier work common enough to justify the trust list, or is it noise? Instrument from the event log before any V2 trust investment.
- **"Provably-safe" allowlist definition** — the concrete starter set that auto-runs (e.g., `cargo build`/`test`, `git status`, `ls`, `rg`, reads within roots) and how (or whether) users safely extend it.
- **Post-shell-expansion without reimplementing a shell** — a conservative normalization where false positives simply re-prompt; how far to take `~`/`$VAR`/alias resolution.
- **Canonical secret-path set** — which paths count as secret reads (`.env`, `~/.aws`, `~/.ssh`, etc.) and how it's configured.
- **Red-tier interaction details** — which keystroke confirms; whether the hard core requires type-to-confirm; polymorphism specifics. (Techspec-level.)
- **Confirm the non-bypassable core stays non-disablable** — the council's strong recommendation; verify no config escape hatch ships.
