# PRD — Live-Activity-First Agent Roster

## Overview

Atelier's always-on Agent Roster sidebar lists every configured agent but conveys *liveness* poorly: all agents render with equal visual weight, the only activity signal is a one-word status buried among idle peers, and a hung agent looks identical to an idle one. This effort reflows the roster into a **progress-confident live board**: working agents dominate the eye, an agent waiting on the human pins to the top, a stalled/hung agent is flagged distinctly, and each active agent shows what it's doing and for how long. It is built for the **daily driver** who watches multi-agent runs for hours; the legibility it produces also makes the "multi-agent" value obvious to anyone evaluating a screenshot. It is valuable because the roster is the surface a user stares at during every run, and today it cannot answer the one question that matters most mid-run: *is this run actually progressing, or stuck?*

## Goals

- **Stuck-detection confidence (primary):** the daily driver can distinguish *actively working* from *stalled/hung* at a glance, and is alerted when an agent stops making progress.
- **Never miss a block:** an agent waiting on the user's approval/input is unmissable — surfaced at the top of the roster, not only inline in chat.
- **Live legibility:** the user can read "who's working, on what, for how long" in a single glance during an active run.
- **Usable without color:** every state is distinguishable by glyph + text label under `NO_COLOR`, color vision deficiency, or missing-glyph terminals.
- **No regression:** the `Ctrl+L` toggle, footer summary, and each agent's color identity (consistent with the chat transcript) are preserved.

## User Stories

**Daily driver (primary):**
- As a daily driver, I want working agents to visually dominate so I can see who's active without hunting through the list.
- As a daily driver, I want each active agent's current step and elapsed time shown, so I can judge whether work is progressing.
- As a daily driver, I want a stalled/hung agent flagged distinctly, so I know when to intervene instead of waiting on a frozen run.
- As a daily driver, I want an agent waiting on my approval/input pinned to the top, so a run never sits silently blocked on a prompt I didn't notice.
- As a daily driver, I want a one-line run pulse (counts of working / waiting / stalled), so I get the run's state in one glance.
- As a daily driver using `NO_COLOR` or with color-blindness, I want each state readable from glyph + label, so the roster works without relying on color.
- As a daily driver, I want each agent's color to stay consistent with the chat transcript, so I can follow one agent across surfaces.

**Evaluating dev (secondary):**
- As someone evaluating Atelier, I want a screenshot of a live run to instantly read as real parallel multi-agent work, so the tool's value is obvious before I install it.

## Core Features

| # | Feature | Priority | What it does |
|---|---|---|---|
| CF1 | Live activity weighting | Critical | Active agents render loud (emphasis + a current-step line); idle agents dim and recede. Canonical order stays stable (no reordering of working/idle rows). |
| CF2 | Four-state model, glyph + label | Critical | Each agent reads as **working / waiting-on-you / stalled / idle**, distinguishable by a portable glyph **and** a short text token; color only reinforces. Existing terminal statuses (completed / failed / interrupted / disabled) keep their labels. |
| CF3 | Stalled detection | Critical | An active agent that stops making progress for a threshold flips to a distinct "stalled?" cue; the working indicator animates, so a frozen indicator reads as trouble. |
| CF4 | Needs-you top-pin | High | An agent waiting on approval/input pins to the top of the roster — the one ordering exception. Blocking work earns position. |
| CF5 | Elapsed time on active rows | High | Active rows show how long the current step has run, formatted human and low-frequency (whole seconds → `1m 20s` → minutes; no fast-ticking counter). |
| CF6 | Run pulse summary header | High | A single fixed line above the roster shows the run's pulse (counts of working / waiting / stalled) as the at-a-glance landmark, without reordering rows. |
| CF7 | Color-identity consistency | Medium | An agent's accent color stays constant regardless of its activity/state and matches its color in the chat transcript. |
| CF8 | Width resilience | Medium | Long model names and step labels truncate gracefully in the narrow sidebar; the roster stays legible at smaller widths. |

## User Experience

**Personas & goals.** The *daily driver* keeps the roster open (it's visible by default; `Ctrl+L` toggles) and glances at it throughout a run to answer "is everything moving, and does anything need me?" The *evaluating dev* sees it first in a README screenshot/GIF and should immediately read "parallel multi-agent system at work."

**Primary flow (mid-run).** The user submits a prompt and the orchestration begins. Active agents brighten and rise in visual weight, each showing its current step and a ticking-but-calm elapsed value; idle agents recede. The summary header reads the run's pulse ("2 working · 0 waiting · 0 stalled"). When an agent needs approval, its row jumps to the top with a clear "waiting on you" label and glyph — the user notices and answers (still via the existing inline y/n flow). If an agent stops making progress, its row flips to a distinct "stalled?" state with elapsed-since-activity, prompting the user to investigate rather than wait indefinitely. When the run completes, rows settle back to the calm idle lineup.

**Accessibility & discoverability.** State is conveyed redundantly: glyph shape + text token + (optional) color. Under `NO_COLOR` or for colorblind users, the glyph + label alone disambiguate every state. Only portable glyphs are used; an ASCII-safe rendering exists for terminals/fonts that can't show them. No new controls to learn — the roster is already visible by default and uses the same activity vocabulary the chat transcript already shows ("running", "waiting for approval", "interrupted", "completed").

## High-Level Technical Constraints

*(Boundaries from the user's perspective — not implementation.)*
- Must honor `NO_COLOR` and degrade to an ASCII-safe rendering; only column-safe, portable glyphs (no emoji-presentation/double-width symbols).
- Each agent's color must remain consistent between the roster and the chat transcript.
- The periodic refresh that powers elapsed/stall-detection must run **only during active runs** — no background activity (or battery cost) when the system is idle.
- No perceptible input lag introduced; the roster remains a passive, always-on panel.
- Preserve existing behaviors: `Ctrl+L` toggle, default visibility, and the footer summary.

## Non-Goals (Out of Scope)

- **Interactivity** — enabling/disabling, selecting, or configuring agents from the roster. This is a presentation surface.
- **Per-agent drill-down** — expandable detail views of capabilities/tools/prompts. Deferred to a later phase.
- **Reordering working/idle rows by activity** — only the `NeedsInput` (and possibly `Stalled`) attention states may move; ordinary active/idle rows keep stable positions.
- **Orchestration timeline / handoff graph** — the delegation-tree "live show" is a later, larger effort (and overlaps the `atelier-tui-redesign` F5 work).
- **Per-agent last-tool / token / cost telemetry** — high-value but needs telemetry not yet surfaced; deferred.
- **User-configurable stalled threshold or theme** — V1 ships a sensible default; configurability is later.
- **Context-adaptive multi-layout** — one layout serves both mid-run and at-rest.

## Phased Rollout Plan

### MVP (Phase 1) — the Progress-Confident Roster (this PRD)
CF1–CF8: live activity weighting, the four-state glyph+label model, stalled detection, the needs-you pin, coarse elapsed, the run-pulse header, color-identity consistency, and width resilience.
**Success criteria to proceed:** the daily driver can distinguish the four states at a glance; a stalled agent is flagged within the threshold; the roster is fully legible under `NO_COLOR`; no regression to toggle/footer/accent-identity.

### Phase 2 — Visibility & insight
Per-agent last-tool + token/cost readout; user-configurable stalled threshold; progressive disclosure of the idle tail for large rosters.
**Success criteria to proceed:** users rely on the roster (not chat scrollback) to judge per-agent cost/activity.

### Phase 3 — The live show
Orchestration timeline / handoff view (delegation graph, parallel tree), integrated with the redesign's live-orchestration identity as the README GIF surface.
**Long-term success:** the roster is the screenshot that communicates Atelier's multi-agent value on its own.

## Success Metrics

| Metric | Target | How measured |
|---|---|---|
| Distinguish the four states at a glance | Daily driver identifies working / waiting / stalled / idle without reading carefully | Snapshot legibility review + informal task timing |
| Stalled agent surfaced | Flagged within threshold + a few seconds of true inactivity | Scenario test with a simulated hang |
| Needs-you never missed | 100% of approval/input waits produce a top-pinned, labeled row | Scenario tests across single + parallel runs |
| Usable without color | Every state distinguishable by glyph+label under `NO_COLOR` | Manual `NO_COLOR` checklist + monochrome snapshot |
| No regression | Toggle, footer, and agent↔chat color identity preserved | Existing tests stay green + new state snapshots |
| No perceptible cost | No input lag; refresh active only during runs | Manual check on idle vs. active |

## Risks and Mitigations

- **Stalled false positives** (a slow model legitimately thinking reads as "stalled") erode trust → conservative default threshold, an animated working indicator that reassures during normal slow work, and "stalled?" phrased as a question, not a verdict.
- **Users don't notice/learn the new signals** → states are self-evident (glyph+label), the roster is visible by default, and labels mirror the chat's existing vocabulary; no new controls.
- **Competitive overlap** — Zed Parallel Agents and Cline Kanban shipped multi-agent panels in 2025/26 → Atelier's niche is an always-on, *in-terminal*, single-orchestrator roster with live-activity-first weighting and an explicit progress-confidence (stalled) angle those panels don't foreground.
- **Solo-maintainer opportunity cost** (polish vs. harness capability) → V1 is bounded and rides the existing theme/accent work rather than rebuilding it.
- **Sequencing with `atelier-tui-redesign` F5** — *resolved* (ADR-006): the redesign is complete and merged to `main` (F5/per-agent accents landed); this work is a successor, not a parallel collision. Land it before the redesign's still-pending manual asset capture (task 09).

## Architecture Decision Records

- [ADR-001: V1 Mechanism and Scope for the Live-Activity-First Agent Roster](adrs/adr-001.md) — Stable order + visual weight + summary header; `NeedsInput` pins to top; `ActivityState` enum; accent-by-identity; unified `RosterRow` view-model. *(Item 7, tick deferral, amended by ADR-002.)*
- [ADR-002: Progress-Confident Roster with a First-Class Stalled State](adrs/adr-002.md) — Adds a `Stalled` state, coarse elapsed, a bounded ~1 Hz refresh (active runs only), an animated indicator, and glyph+label/`NO_COLOR` accessibility with portable glyphs only.

## Open Questions

- **Stalled ordering:** does a `Stalled` agent also pin to the top (like `NeedsInput`), or is loud in-place weight + the header count enough? (Recommend: header count + loud in-place; confirm in TechSpec.)
- **Stalled threshold:** what no-progress duration triggers "stalled?", and is it fixed in V1 or configurable? (TechSpec)
- **Summary header at rest:** show "0 working" when idle, or hide the header until a run is active?
- **Idle-tail disclosure:** for large rosters, collapse/summarize idle agents beyond ~N, or always show all? (Phase 2 candidate.)
- **Glyph set & ASCII fallback:** confirm the exact portable glyph per state and its ASCII-safe equivalent (visual review).
- ~~**Sequencing vs. redesign F5**~~ — *resolved* (ADR-006): redesign code is merged to `main`; branch this work from `main`, land it before the redesign's manual asset capture (task 09).
