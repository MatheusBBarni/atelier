# PRD: Rich Approval Modal + Per-Session Trust List

## Overview

`atelier` gates an agent's risky actions only in `Normal` mode, as a binary yes/no over a one-line summary; the only relief from repeated prompts is `Yolo` mode (auto-approve everything), which is the default. So users either suffer fatigue (`Normal`) or waive all oversight (`Yolo`) — and in `Yolo` a single catastrophic action can run unseen.

This feature makes oversight *fast, confident, and safe* without taking away the `Yolo` default. It adds: a **catastrophic safety floor** that always stops and asks — even in `Yolo`; a **rich decision-support prompt** that shows the exact command, the diff, the boundary being crossed, and a plain-language risk reason; a **per-session trust entry** ("trust this exact action for this session") with a `/trust` command to review and revoke it; and **deny-and-continue**, so saying "no" redirects the agent instead of killing the run. It targets every atelier user (the floor protects even `Yolo` users), with the richer experience serving operators who run `Normal`.

## Goals

- **Day-one catastrophic protection for 100% of users**, including the `Yolo` default: irreversible/cross-boundary actions always stop and ask.
- **Faster, more confident approvals**: median time to decide a *novel* approval ≤ 8s, achieved by giving the user enough context to decide once, well.
- **Fewer risky auto-approvals**: ≥ 95% of *enforced* high-risk actions are reviewed by a human (vs. ~0% in `Yolo` today).
- **Make "no" cheap**: a denial returns a reason and lets the agent try a safer path, instead of ending the run.
- **Ship the safety-default change without backlash**: introduce the floor via a phased, announced rollout with a first-run notice and an escape hatch.

## User Stories

**Cautious Operator (runs `Normal`):**
- As a cautious operator, I want to see the exact resolved command, the diff, and which workspace boundary an action crosses, so I can approve confidently without guessing.
- As a cautious operator, I want to trust a specific repeated action for this session, so I stop being re-prompted for the identical safe thing.
- As a cautious operator, I want to review and revoke what I've trusted via a single command, so a grant I regret doesn't linger.

**Yolo Defaulter (runs `Yolo`):**
- As a Yolo user, I want catastrophic actions (mass delete, force-push, secret exfiltration, fetch-and-run) to always stop and ask, so one mistake can't wipe my machine even though I auto-approve everything.
- As a Yolo user, I want a one-time explanation the first time the floor fires, so being asked despite `Yolo` doesn't feel like a broken promise.
- As a Yolo user, I want to see what the gray-area floor *would have* blocked (without it blocking yet), so I can decide whether to turn on stricter enforcement.

**Multi-agent Power User:**
- As a power user, I want a denial to redirect the agent to a safer path rather than fail the whole run, so saying "no" is cheap.
- As a power user, I want a trust grant to never silently auto-approve a batch of queued sibling actions I never saw, so nothing runs that I didn't actually decide on.

## Core Features

**Critical**

- **Catastrophic safety floor (always-on).** A small, high-precision set of clearly irreversible or cross-boundary actions (`rm -rf /` & `~`, force-push, secret-file exfiltration, fetch-and-run like `curl|bash`) always raises the approval prompt — in every mode, including `Yolo`, with no off switch. Evaluated against the *resolved* command (after shell expansion) so disguised targets can't slip through.
- **Rich decision-support prompt.** Replaces the one-line summary with: the exact resolved command (or a diff for file changes), the affected paths relative to the workspace, which boundary/capability is being crossed, whether the action is reversible, a one-line plain-language risk reason, and a risk tier (low/medium/high). Detail is progressively disclosed — lead with "what + why," expand for the full command/diff.

**High**

- **Gray-area floor (warn-only by default).** Everything not provably safe is treated as needing confirmation. In V1 this runs in *warn-only* mode by default: in `Yolo` it surfaces the risk and records a "would have blocked" note in the transcript but doesn't block; a setting lets users switch it to *enforce* now. (Default flips to enforce in a later phase.)
- **Per-session exact-target trust.** At an approval, the user can choose "trust this exact action for the rest of this session." Trust is exact-match only (no wildcards/patterns/action-types/per-agent), lasts only for the session, and resets when atelier restarts. Catastrophic-core actions are never eligible for trust.
- **`/trust` management + inline feedback.** A `/trust` command lists active session-trusted actions and revokes one or all. Every trust grant and every trust-driven auto-approval appears in the transcript in human-readable terms (what ran, under which trust entry).
- **Deny-and-continue.** A denial (by the user or the floor) returns a structured reason to the agent so it can attempt a narrower, safer path; the run continues rather than failing.
- **Habituation-resistant controls.** The high-risk tier requires a deliberate, non-default keystroke (no Enter-to-approve), and "approve" never shares a key with "approve-and-trust" (trusting always costs more friction than approving once).

**Medium**

- **First-run onboarding + Approvals help.** A one-time notice (reusing the existing first-run explainer pattern) the first time the floor/modal fires — what it does, why it can fire even in `Yolo`, and how to configure it — plus an updated Approvals help section documenting tiers and trust.
- **Auditable auto-approvals & safe batch handling.** Auto-approvals are always visible in the transcript. A trust grant applies only to *future* actions and never silently clears a queue of already-pending sibling actions without showing the full set and a count.

## User Experience

**First contact.** On the first gated action, the user sees a one-time explainer card (muted, dismissible) describing the gate and how to configure it. Thereafter, only the prompt itself appears.

**The approval prompt.** A `Normal`-mode user (or any user hitting the floor) sees a structured prompt: a one-line "what + why" with a risk-tier label, then the exact resolved command or a diff, the affected paths and boundary crossed, and a reversibility note. Choices are action-specific and clearly separated: *approve once*, *approve + trust this exact action this session*, *deny*. For the high-risk tier, approval requires a deliberate non-default keystroke (and, for the catastrophic core, a type-to-confirm step); the safe default never lands on the dangerous option.

**Trusting and revisiting.** Choosing "trust this exact action" suppresses re-prompts for that identical action for the session; the grant is announced inline. Running `/trust` lists what's trusted with human-readable scope ("this session only") and lets the user revoke any entry or clear all — directly answering the "approve once, regret forever" problem.

**Yolo experience.** A `Yolo` user is normally never prompted. When a *catastrophic* action occurs, the full prompt appears (with the first-run explainer the first time) — surprising but rare and clearly justified. Gray-area actions are *not* blocked by default; instead a "would have blocked: …" annotation accumulates in the transcript, and the help/`/trust` surfaces show a per-session count, nudging users to opt into enforcement.

**Discoverability & accessibility.** Keys are documented in the Help modal's Keys and Approvals tabs. Risk tiers are conveyed by an explicit text label (not color alone), so the experience holds under monochrome/`NO_COLOR` terminals. Consequences are named in words ("permanently deletes 3 files outside the repo"), never generic "are you sure."

## High-Level Technical Constraints

- **Local-only, no new external dependencies.** All behavior and metrics work offline; success metrics derive from atelier's existing local activity record, with no network calls or external telemetry.
- **Terminal color-capability support.** Risk signaling must remain legible across truecolor, 256-color, and monochrome (`NO_COLOR`) terminals — color may reinforce but never solely encode severity.
- **Trust is session-scoped.** Trust exists only for the lifetime of one atelier process and is never written to disk in V1.
- **The catastrophic core cannot be disabled.** No configuration may turn the always-on core off; only the gray-area tier's posture (warn vs. enforce) is configurable.
- **No change to the `Yolo` default mode selection.** This feature changes what `Yolo` *permits* (catastrophic floor), not which mode ships as default.

## Non-Goals (Out of Scope)

- **Broad trust scopes (pattern/glob, action-type, per-agent).** Deferred to a later phase with a caveat — broad scopes are "scoped invisible Yolo," and per-agent trust is unsafe because agent identity is hijackable via prompt injection.
- **Cross-session / disk-persisted trust.** Keeps blast radius bounded to one process; restart is the reset.
- **Runaway-denial backstop (auto-pause after N denials).** Deferred; atelier is interactive and the user is already re-prompted on each floor hit.
- **Sandbox/OS-level containment and snapshot/undo reversibility.** Larger, later bets (V2+); undo can't reach cross-boundary effects, so it never substitutes for the floor.
- **Changing the `Yolo` default or adding a mid-session approval-mode toggle.** Mode selection stays config-driven as it is today.
- **Trust management beyond list-and-revoke.** No editing, grouping, or import/export of trust entries.

## Phased Rollout Plan

### MVP (Phase 1)
- Catastrophic core **enforced everywhere, including `Yolo`**.
- Gray-area floor **warn-only by default**, with an opt-in `enforce` setting.
- Rich decision-support prompt; exact-target session trust; `/trust` list-and-revoke + inline feedback; deny-and-continue; habituation-resistant controls; first-run notice + Approvals help.
- **Success criteria to proceed:** zero catastrophic-action escapes; gray-area warn-only false-positive rate low enough that enforcing it wouldn't disrupt routine work; measurable `/trust` adoption; no significant "it blocked my workflow" reports.

### Phase 2
- **Flip the gray-area floor to enforce-by-default** (announced via changelog + first-run notice, escape hatch already shipped).
- Refine risk tiers and the provably-safe allowlist from Phase 1 data.
- **Success criteria to proceed:** enforce-by-default lands with low false-positive complaints; risky-auto-approval-reduction KPI met across all users.

### Phase 3
- Richer trust keying (agent/pattern/root, with prompt-injection safeguards), informed by the now-measured repeat-action data.
- Reversibility (snapshot/undo) and sandbox containment as defense-in-depth.
- **Long-term success:** guarded operation is the comfortable norm; catastrophic incidents trend to zero.

## Success Metrics

- **Catastrophic-floor coverage:** 100% (zero catastrophic actions executed without a prompt), even in `Yolo`.
- **Risky-auto-approval reduction:** ≥ 95% of enforced high-risk actions reviewed by a human (Phase 1 covers the catastrophic set; Phase 2 extends to the gray area).
- **Novel-approval decision latency:** median ≤ 8s for first-seen approvals.
- **Repeat-prompt collapse:** ≥ 70% of repeat approvals matching an existing exact-target trust are auto-resolved.
- **Scoped-trust adoption:** ≥ 50% of sessions that hit an approval grant at least one trust entry.
- **Warn-only signal health:** gray-area "would-have-blocked" annotations are noticed (enforce opt-in rate among users who see ≥1 annotation) and have an acceptable false-positive rate.

## Risks and Mitigations

- **Users ignore the warn-only signal**, so Phase 2 lands blind. *Mitigation:* make the annotation a clearly styled transcript item with a visible per-session count.
- **Phase-2 enforce flip causes backlash** (a safety-default change). *Mitigation:* announce via changelog + first-run notice; the escape hatch ships in Phase 1.
- **Reflexive "yes" / habituation** undermines the richer prompt. *Mitigation:* non-default keystroke + type-to-confirm for the top tier; "approve" separated from "approve-and-trust"; consider varied (polymorphic) styling on the dangerous tier.
- **Trust regret ("approve once, exploit forever").** *Mitigation:* exact-match only, session-scoped, fully visible and revocable via `/trust`.
- **False positives nag users.** *Mitigation:* warn-only calibration before enforce; deny-and-continue keeps a false block cheap (a redirect, not a dead run).
- **Differentiation erosion** — allowlists are table stakes. *Mitigation:* lean on the differentiators competitors lack: a visible/revocable trust list and a floor that protects even the auto-approve default.

## Architecture Decision Records

- [ADR-001: V1 scope — fail-closed destructive floor, decision-support modal, and minimal floor-anchored session trust](adrs/adr-001.md) — Ships the safety floor as a fail-closed allowlist (not a denylist), a rich modal, and a single exact-target session-trust scope.
- [ADR-002: Phased floor rollout with a non-bypassable catastrophic core](adrs/adr-002.md) — Catastrophic actions enforce immediately even in `Yolo`; the broader gray-area floor ships warn-only and flips to enforce later.

## Open Questions

- **Repeat-action frequency is unmeasured** — does routine work repeat enough to make session trust high-value, or is it noise? Instrument before investing in richer trust (Phase 3).
- **Exact catastrophic-core set** — the precise, high-precision list of always-block actions (irreversible/cross-boundary), kept small to avoid false positives.
- **Provably-safe allowlist starter set** — which actions auto-run with no prompt (e.g., reads within roots, common read-only commands) and whether/how users extend it.
- **Secret-path definition** — which paths count as secret reads/exfiltration for the catastrophic core, and whether it's configurable.
- **Warn-only annotation specifics** — exact presentation and where the per-session count surfaces.
- **Phase-2 trigger** — the concrete false-positive threshold and adoption signal that green-lights the enforce-by-default flip.
