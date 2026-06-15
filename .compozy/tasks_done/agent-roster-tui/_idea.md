# Live-Activity-First Agent Roster

## Overview

Reflow Atelier's always-on Agent Roster sidebar so it reads first as a **"what's happening now" board** and second as a static lineup. Today the roster is a cramped, flat list of three fixed lines per agent (name+status / runtime+model / effort+thinking) in ~28% width — functional but hard to scan, with the live state of a run buried in undifferentiated rows. This V1 makes working agents visually dominate and idle agents recede, surfaces the one agent that needs the human, and shows how long active work has been running — **without moving rows** (which would break agent color-identity and spatial memory). It's for the **daily driver** watching multi-agent runs and the **evaluating dev** judging "multi-agent" from a screenshot. V1 is a deliberately bounded presentation reflow plus the minimal app-layer seam (a joined view-model) that the live data requires anyway — no interactivity, no drill-down, no second layout.

## Problem

The roster is the always-on surface a user stares at during every run, yet it conveys liveness poorly. All agents render with identical visual weight; during an active run the only signal that an agent is working is a one-word status string on line 1, lost among idle peers sorted alphabetically. A user scanning mid-run cannot answer "who is working, on what, and for how long?" at a glance, and cannot quickly spot the agent **blocked waiting on their approval** — the one state that actually requires them. At rest, the same flat density makes the lineup (models, effort, capabilities) tiring to parse in the narrow column.

The naive fix — sort active agents to the top, htop-style — is actively harmful here. Agent accent colors are **positional** (`accent_for(index)` = `index % 5`) and pinned to the chat transcript's colors by test; reordering rows silently *recolors* agents and severs their identity link to the chat they're producing. With no animation infrastructure, reordered rows also "snap," destroying spatial memory precisely when cognitive load is highest. So the design must achieve live-activity dominance through **visual weight and a fixed summary**, not position.

The live data needed to do this isn't joined to the rows today: which agent is active, its `step_label`, and its parallel `group_id` live in a separate `AppState.live_steps` vec, and elapsed time isn't exposed in the public view at all. Making the roster live therefore requires a small, disciplined app-layer join — which, once paid, makes elapsed time and a clean `ActivityState` nearly free.

### Market Data

- **No first-party coding-agent CLI ships an activity-sorted roster.** OpenCode has *open, unshipped* RFCs ([#12463](https://github.com/anomalyco/opencode/issues/12463), [#15223](https://github.com/anomalyco/opencode/issues/15223)) proposing exactly this field set — status · current task · last tool · **duration** · "at-a-glance" monitoring of parallel execution. A validated, unclaimed gap.
- **Prior art:** htop default-sorts active-to-top and highlights running rows; **Agent Deck** (~2.7k★) uses symbol+color state badges (`●` running / `◐` waiting / `○` idle / `✕` error) and a sticky needs-attention band. A cottage industry (tmuxcc, Agent of Empires, AgentsRoom) exists *solely* to bolt rosters onto CLIs that lack one — confirming demand.
- **UX guidance:** pair color with icon+label (colorblind-safe); two-tier weight (active bold/leading, idle muted); animate reorders <300ms — *infeasible in a non-animated TUI*, which is direct evidence against reordering.

## Summary / Differentiator

Two things process monitors and single-agent CLIs structurally cannot show: **per-agent semantic activity** (the subtask each agent was routed to, its current step, how long) and the orchestrator's **ground-truth run state**. This V1 claims the first. Because the orchestrator already knows routing, the roster can later grow into the handoff view no competitor can fake (V2). The V1 move is small but lands on an open, demand-validated gap.

## Core Features

| # | Feature | Priority | Description |
|---|---|---|---|
| F1 | `ActivityState` + unified `RosterRow` view-model | Critical | App-layer join of `live_steps` onto agents → `RosterRow { identity, accent, ActivityState(Active\|NeedsInput\|Idle), step_label, group_id, started_at }`. Renderer becomes a pure function of this; one enum drives weight + ordering + glyph. |
| F2 | Weight-driven activity rendering | Critical | Active rows render bold with a row highlight + a live current-step line; idle rows dim and recede. State shown by glyph **and** label (never color alone). No row reordering for Active/Idle. |
| F3 | `NeedsInput` top-pin | High | The single allowed ordering exception: an agent waiting on approval/input pins to the top of the list. Blocking work earns position; busy work does not. |
| F4 | Thin summary header | High | One fixed, non-duplicating line above the roster — e.g. `▶ 2 working · ⏸ 1 waiting` — as the at-a-glance landmark that never reorders rows. |
| F5 | Accent-by-identity decoupling | High | Resolve accent from a stable agent key, not row position; chat and roster read the same source. Makes the `NeedsInput` pin safe and keeps `roster_names_carry_same_accents_as_chat` meaningful. |
| F6 | Coarse elapsed on active rows | Medium | Surface step `started_at`; render `now − start` at whole-second granularity on active rows only, to distinguish "working" from "stuck." Ticking redraw cadence deferred to V2. |
| F7 | Width resilience | Medium | Graceful truncation of long model names and step labels at 28%; legible degradation at narrow widths. |

## KPIs

| KPI | Target | How to Measure |
|---|---|---|
| Active/needs-input agent locatable without scroll | Visible in top region for rosters ≤8 agents at 24-row height | `TestBackend` snapshot at representative sizes |
| Layout-state snapshot coverage | ≥5 states: idle / single-active / parallel-active / needs-input / narrow-width | `TestBackend` snapshot tests |
| Agent↔chat accent identity preserved | 100% (color stable regardless of activity/state) | `roster_names_carry_same_accents_as_chat` stays green + new state tests |
| Inline color literals outside `theme.rs` | 0 | CI grep invariant `colors_live_only_in_theme_module` |
| Added render cost | < 1 ms (join is O(agents × live_steps)) | Render timing before/after on the same machine |

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Must do *(always-on, every user)* |
| **Frequency** | How often would users encounter this value? | Must do *(every run, continuously)* |
| **Differentiation** | Does this set us apart or just match competitors? | Strong *(validated open gap; no first-party CLI ships it)* |
| **Defensibility** | Easy to copy or compounds over time? | Strong *(rides the orchestration moat; view-model seam compounds)* |
| **Feasibility** | Can we actually build this? | Must do *(render reflow + one disciplined app-layer seam)* |

Leverage type: **Quick Win** (with a compounding `RosterRow` view-model seam)

## Council Insights

- **Recommended approach:** Stable canonical order — **no reorder, no duplicate band**. Express live activity through visual weight (active bold + row highlight + live step line; idle dimmed) plus a thin non-duplicating summary header. The *one* allowed exception: pin `NeedsInput` rows to the top. A first-class `ActivityState` enum drives weight + ordering; accent-by-identity decoupling makes the pin safe; the live/agent join lives in an app-layer `RosterRow` view-model so the renderer stays pure.
- **Key trade-offs:** Crosses from "pure render reflow" into modest app-layer work (accent decouple, join view-model, one timestamp field) — bigger diff/test surface than a cosmetic restyle, in exchange for a board that reads as genuinely live. Elapsed time is plumbed at its *smallest* (surface start, render `now−start`); the ticking cadence is the only piece deferred.
- **Risks identified:** (1) elapsed could read stale without a ticker → during streaming the render cadence already refreshes often, confirm in TechSpec; (2) reflow regresses roster render tests → ≥5 new snapshot states + keep accent test green via identity-derived accent; (3) "low-leverage polish" dissent (devil's-advocate) → bounded scope that *extends* the redesign (which owned color only), not duplicates it.
- **Stretch goal (V2+):** the orchestration **timeline/handoff board** (delegation graph, step sequence, parallel tree) and per-agent **last-tool + token/cost** readout — both made cheap by the V1 view-model seam, both differentiators no flat process list can match.

## Integration with Existing Features

| Integration Point | How |
|---|---|
| Roster render (`src/tui/mod.rs:1952–2008`) | Re-rendered from the new `RosterRow` view-model; three-fixed-line layout replaced by state-driven weight + glyph + step line |
| Agent sort (`src/app/mod.rs:4962`) | Canonical order retained as the stable spine; `NeedsInput` pin applied as a single ordering exception in the view-model |
| `AppState.live_steps` / `AgentView` | Joined into `RosterRow` in the app/projection layer (was: two separate projections) |
| Theme tokens (`src/tui/theme.rs:110–165`) | All new styling uses tokens; `accent_for` repointed to identity-derived index |
| Chat accents | Repointed at the same identity-derived accent source as the roster |
| `atelier-tui-redesign` F5 (live-orchestration identity) | This V1 is the information-architecture layer beneath F5's color identity; sequence to avoid collision |

## Out of Scope (V1)

- **Physical active-to-top row reordering** — breaks positional accent-identity and spatial memory; snaps without animation. Weight delivers dominance instead.
- **Duplicate "Active" band** — doubles vertical/horizontal pressure at 28% width and fractures "one agent, one identity"; a thin summary header delivers the at-a-glance signal cheaper.
- **Ticking / per-second elapsed redraw cadence** — V2; V1 renders coarse elapsed off existing render passes.
- **Interactivity** (enable/disable/select an agent, edit config from the roster) — explicitly de-prioritized; this is a presentation reflow.
- **Per-agent drill-down / expandable detail** — V2; the view-model leaves the door open.
- **Orchestration timeline / handoff graph** — V2 stretch; overlaps redesign F5 and is a Massive build.
- **Last-tool / token-cost telemetry** — V2 adjacent; needs telemetry plumbing the view-model will make cheap.
- **Context-adaptive multi-layout** — user chose one layout; not splitting mid-run vs at-rest views.

## Architecture Decision Records

- [ADR-001: V1 Mechanism and Scope for the Live-Activity-First Agent Roster](adrs/adr-001.md) — Stable order + visual weight + summary header; `NeedsInput` pins to top; `ActivityState` enum; accent-by-identity; unified `RosterRow` view-model; coarse elapsed, ticker deferred.

## Open Questions

- **Elapsed liveness without a ticker** — does the existing render cadence refresh often enough during streaming that whole-second elapsed reads as live, or is a bounded refresh needed? (TechSpec)
- **Glyph set + terminal coverage** — do `●◐○` / `▶⏸` render across target terminals, and what's the ASCII fallback?
- **Summary header at rest** — show `▶ 0 working` when idle, or hide the header until a run is active?
- **Idle-tail disclosure** — UX guidance caps ~5 visible items; for large rosters, collapse/summarize the idle tail or always show all? (Council leaned show-all stable; revisit if rosters grow.)
- **Sequencing vs. redesign F5** — confirm ordering so the IA layer (this) and the color-identity layer (F5) don't collide.
