# PRD — Session Browser & Transcript Resume

## Overview

`atelier` persists every session as a durable, event-sourced log on disk, yet every launch starts a fresh, empty session and there is no in-app way to find, reopen, or continue a past one. A crash, a quit, or an end-of-day context switch silently loses the working thread even though the full record is sitting on disk.

This feature gives users an in-TUI **session browser** — a newest-first list of past sessions with a label, timestamp, and run outcome — plus a **read-only transcript preview** and a **Resume** action that re-adopts a chosen session and continues it with new prompts. It is built primarily for the developer who just lost a session to a crash or quit and wants the thread back without re-establishing context, and secondarily for anyone auditing what an agent did across a long run.

Its value is twofold: it turns an ephemeral REPL into a recoverable workspace (reclaiming the ~23-minute refocus tax of a lost thread), and it makes the durable history — the system's most valuable asset — actually usable instead of write-only.

## Goals

- **Deliver crash/quit recovery**: a user can reopen a lost session and continue it within seconds of launching `atelier`.
- **Make the transcript the trust anchor**: a user can always *see* a session's prior history before and after resuming, never act on context they can't inspect.
- **Activate the durable history**: past sessions become browsable and auditable, not just persisted.
- **Stay safe by default**: resuming a session whose workspace has moved on cannot silently corrupt the repo.
- **Earn the deferred scope with data**: instrument resume behavior so the cross-session search investment is justified by evidence, not assumption.

Measurable targets are in **Success Metrics**. Milestone sequencing is in **Phased Rollout Plan**.

## User Stories

**Primary — The Recoverer** (session died mid-task)
- As a developer whose `atelier` was killed mid-run, I want to reopen the session I just lost so that I continue the task instead of restarting cold.
- As a recoverer, I want to *see* the full prior transcript when I resume so that I trust the agent has the same context I do.
- As a recoverer, I want resume to be safe when my branch or working directory has changed since the session paused so that I don't apply changes to the wrong tree.

**Secondary — The Auditor** (reviewing a past run)
- As a developer reviewing a long run after the fact, I want to open a past session read-only and scroll its full transcript so that I understand exactly what the agent decided and did.
- As an auditor, I want to see each session's outcome (completed / failed / interrupted) in the list so that I can find the run I care about quickly.

**Tertiary — The Returner** (continuing later)
- As a developer coming back the next day, I want to find yesterday's long-running session by its goal or opening prompt so that I pick up where I left off.

## Core Features

Grouped by delivery phase (see Phased Rollout Plan).

**F1 — Browse past sessions (Phase 1)**
- A modal lists sessions **newest-first** with: a **label** (the session goal if set, otherwise the session's first user prompt, truncated), a **timestamp**, and a **run outcome** badge (color-coded, with a text label so it reads under `NO_COLOR`).
- A case-insensitive **substring narrow** filters the visible list as the user types.
- Loads without blocking the UI, even for large histories.

**F2 — Preview a session read-only (Phase 1)**
- Selecting a session shows its **full transcript** rendered read-only, scrollable.
- Stored transcript text is rendered safely (control/escape sequences neutralized) so a malformed or hostile log cannot disrupt the terminal.
- The preview is the user's verification surface — it doubles as the "is this the right session?" check before Resume.

**F3 — Resume a session (Phase 2)**
- A **Resume** action re-adopts the selected session: the live view **re-renders the full prior transcript**, the session's goal is restored, and the view lands at a blank prompt ready for new input.
- New prompts continue the **same** session record (append-in-place; ADR-002).
- If the session ended mid-run, that run is closed as **interrupted** and the session opens idle — V1 does **not** auto-re-execute the interrupted step.
- Resume is gated by the existing one-active-run rule (you can't resume while a run is active).

**F4 — Honest recovery record (Phase 2)**
- Resuming records the recovery as part of the durable history (the interruption and the resume boundary are written, not just applied in memory), so the transcript remains a faithful, auditable account of what happened across the crash (ADR-002).

**F5 — Stay safe when the workspace changed (Phase 2)**
- Resume defaults to the **cautious** approval mode; prior one-off approvals are **not** carried forward as standing permissions, and the resumed roster/capabilities are shown to the user.
- If the working directory or git branch/commit has **changed** since the session paused, the **first action that would modify files or run a command** requires explicit acknowledgment of that drift before proceeding. Browsing and previewing are never gated. A merely *dirty* tree (expected right after a crash) does not trigger the gate (ADR-004).

**F6 — Discover & open the browser (Phase 1 → 2)**
- The browser opens via **a global keyboard shortcut** (a collision-free key — not Ctrl-S/XOFF) **and** a **slash command** (discoverable through the existing command dropdown + help overlay).
- After an unclean exit, the next launch shows a **proactive hint** that the last session was interrupted and can be reopened.

**F7 — Deferred: cross-session search & audit (Phase 3)**
- Fuzzy/content search across all sessions. Out of scope for Phases 1–2; see Non-Goals.

## User Experience

**Personas & goals:** the Recoverer wants speed back to their thread; the Auditor wants legibility of a past run; the Returner wants findability.

**Primary flow — recover a crashed session:**
1. User relaunches `atelier`; the welcome view notes the last session was interrupted and how to reopen it.
2. User opens the browser (shortcut or slash command). The list appears newest-first; the lost session is at or near the top.
3. User optionally types a few characters to narrow the list; selects a session.
4. The read-only preview shows the full transcript; the user confirms it's the right thread.
5. User triggers Resume. The live view re-renders the full transcript and lands at a blank prompt.
6. User types the next prompt. If the workspace has drifted and that prompt leads to a file/command action, they're asked to acknowledge the drift once before it runs.

**Audit flow:** open browser → filter by recalling a goal/prompt → preview → scroll the full transcript read-only → close (no resume).

**UI/UX considerations (match existing idioms so it feels native):**
- Navigation with **↑/↓**; select with **Enter**; type to **filter** in place; **Esc** closes the modal — consistent with existing dropdowns/help.
- The browser takes routing precedence like other modals; it never opens while a run is mid-flight without honoring the active-run rule.
- The outcome badge reuses the existing run-state color scheme and **always carries a text label** (accessible under `NO_COLOR` / monochrome).
- Discoverability via the welcome facts box (which only appears when chat is empty) and the post-crash hint.

## High-Level Technical Constraints

(Boundaries that shape the product, not implementation choices.)

- **Local-only**: operates entirely on the existing on-disk per-session history; no network surface, no cross-machine sync.
- **Reuse the durable event log** as the single source of truth; the list's label/outcome data must stay consistent with the transcript.
- **Honor existing guarantees**: the approval/capability model and the one-active-run rule apply unchanged to resumed sessions.
- **Performance (user-perceived)**: opening the browser feels instant for a large history (target in Success Metrics); preview and resume render without a visible stall.
- **Backward compatibility**: sessions recorded by older versions of `atelier` must still list, preview, and (where valid) resume — or fail clearly, never silently mis-render.
- **Terminal compatibility**: the trigger key must not be one terminals intercept (no Ctrl-S/XOFF).
- **Data at rest**: session files hold potentially sensitive transcript content; protect them with restrictive file permissions. Redaction is explicitly deferred (Non-Goals).

## Non-Goals (Out of Scope)

- **Cross-session fuzzy/content search** — deferred to Phase 3; earned by resume-rate data. (Adds a searchable index over sensitive prompts and serves a separate archival hypothesis.)
- **Auto re-execution of an interrupted step** — V1 reconciles to idle; auto-resuming a dangling run risks re-running side-effecting actions.
- **Branch / fork from an arbitrary point** in a transcript — a future stretch; requires fork semantics, not the append-in-place model committed here.
- **Generated summaries on resume** — V1 shows the real transcript; auto-summarization is deferred ("a bad summary is worse than none").
- **Snapshot / compaction of long logs** — deferred until open/resume latency is user-visible.
- **Redaction-at-rest of secrets/PII** — accepted as a known risk for V1 with file-permission protection; revisited at first shared-host deployment.
- **Remote / cloud session sync or sharing** — local-only.

## Phased Rollout Plan

### MVP (Phase 1) — Browse + read-only preview
- Features: F1 (picker), F2 (preview), F6 (entry points; shortcut + slash command + post-crash hint).
- Read path only; **no state mutation**. De-risks activating the history-replay path and immediately serves the Auditor + trust-verification.
- **Success criteria to proceed to Phase 2:** the list renders correctly and fast for real and large histories; previews match the on-disk record exactly (no desync) across a representative sample including older-format sessions; users open the browser at a meaningful rate.

### Phase 2 — Resume (the crash-recovery anchor)
- Features: F3 (resume), F4 (honest recovery record), F5 (safety model). Completes F6's post-crash "reopen" action.
- **Success criteria to proceed to Phase 3:** measurable crash-recovery adoption (see metrics); resumed sessions complete at a healthy rate (no confused/abandoned threads); zero reported incidents of silent wrong-tree corruption.

### Phase 3 — Cross-session search & audit (deferred)
- Feature: F7. Triggered only if Phase 1–2 data shows users scrolling deep / hunting older sessions.
- **Long-term success:** search adoption justifies the added surface; the browser is a routine part of the workflow.

## Success Metrics

| Metric | Target | Notes |
| --- | --- | --- |
| Crash-recovery adoption — of sessions ending mid-run, share reopened/resumed within 7 days | > 40% | The anchor's headline metric (Phase 2) |
| Time-to-continue — median from launch to first new prompt in a resumed session | < 20 s | Recovery should feel fast |
| Browser open latency — render the list for a 200-session history | < 200 ms p95 | User-perceived "instant" (Phase 1) |
| Transcript fidelity — previewed/resumed transcripts that match the on-disk record | 100% (zero desync) | Trust depends on this |
| Resumed-session completion (guard) — resumed sessions that reach a completed run | > 60% | Confirms append-in-place doesn't produce abandoned threads |
| Browser discovery — share of active users who open the browser at least once | rising trend | Validates the discoverability investment |

## Risks and Mitigations

- **Discoverability ("nobody finds it")** — the documented #1 CLI weakness; resume features get buried. *Mitigation:* multiple entry points (shortcut + slash command) and a proactive post-crash hint (F6).
- **Stale-state distrust / wrong-tree action** — the top trust-killer for resume; users fear acting on drifted context. *Mitigation:* the drift acknowledgment gate + cautious-default approvals (F5); always show the full transcript (F2/F3).
- **"Which session is active?" confusion** — *Mitigation:* clear goal/first-prompt labels + outcome badges in the list, and the re-rendered transcript on resume.
- **Unproven frequency** — the crash-recovery anchor's value rests on an unmeasured resume rate. *Mitigation:* Phase 1 instrumentation; Phase 2 gated on adoption data.
- **Perceived "incomplete" Phase 1** — read-only browse without resume may feel partial. *Mitigation:* frame Phase 1 explicitly as browse/audit; ship Phase 2 as the immediate fast-follow.
- **Competitive timing** — in-TUI resume is now table stakes; absence is a leak. *Mitigation:* phased delivery gets the read path out quickly; the faithful mid-crash resume + no-desync is a differentiator to lead with.
- **Opportunity cost** — must not crowd out the offensive bets (mcp-integration, subtask-dag). *Mitigation:* tight per-phase scope; defer Phase 3.

## Architecture Decision Records

- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — V1 = list + preview + Resume; fuzzy/cross-session search → data-earned V2/Phase 3.
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — continue the same log; write interrupted + resume-boundary records; metadata is a derived cache.
- [ADR-003: Production replay fold as a maintained schema-compatibility contract](adrs/adr-003.md) — activate the history replay path under a backward-compatibility contract; atomic session adoption.
- [ADR-004: Resume safety model](adrs/adr-004.md) — drift interlock at first mutation, cautious-default capability re-consent, untrusted-transcript rendering, file permissions.
- [ADR-005: Product approach — recovery-first, phased delivery](adrs/adr-005.md) — browse+preview MVP → resume → search.

## Open Questions

- **Exact trigger key** — collision-free key (Ctrl-R has fzf "recall" precedent) decided in the techspec, avoiding existing bindings (e.g. prompt-history); should ↑/↓ also be aliased to `j/k`?
- **"Where you left off" cue** — beyond re-rendering the transcript, does resume need a one-line status line marking the interruption point, and what wording?
- **Run-outcome labels for mid-run sessions** — how to display a session that ended in flight (e.g. `Interrupted` vs `In progress (recovered)`).
- **Unclean-exit detection** — what signal drives the post-crash hint, and how to avoid false positives on a normal quit.
- **Resume-rate baseline** — unknown until Phase 1 instrumentation lands; the gate to Phase 2/3.
- **Session-boundary enforcement** (techspec) — the unresolved architectural nuance from ADR-003.
