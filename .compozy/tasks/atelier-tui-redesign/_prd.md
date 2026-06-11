# PRD: Atelier TUI Visual Identity

## Overview

Atelier is a terminal-native multi-agent orchestration harness whose interface is currently anonymous: startup shows a transient loading box, the product name appears nowhere persistent, and colors carry inconsistent meanings. For an open-source tool whose adoption funnel runs through README screenshots and social posts, this is a distribution handicap — the screenshot is the landing page, and Atelier's is forgettable.

This feature gives Atelier a complete visual identity: a branded welcome screen with an adaptive ASCII "Atelier" wordmark and an information box, the existing web brand palette applied consistently across every TUI surface, a persistent status footer, and per-agent color identity in the orchestration views — culminating in refreshed README assets (welcome screenshot + a live parallel-agents GIF that no competitor can replicate).

**Who it's for:** evaluating developers deciding from a screenshot, daily drivers spending hours in the TUI, open-source contributors, and users relying on accessibility conventions.

## Goals

- Make every Atelier screenshot recognizable as Atelier (brand consistency with the website from day one).
- Communicate "multi-agent orchestration" visually — the differentiator no competitor shows.
- Give daily users ambient awareness: which repo/branch agents will act on, what the harness is doing, how many agents are active.
- Eliminate the color ambiguity in the current TUI (one color = one meaning).
- Meet terminal-citizenship conventions users expect: `NO_COLOR`, narrow terminals, non-truecolor terminals.
- Deliver in a ~1-week timebox as one coherent release (ADR-002).

## User Stories

**Evaluating Developer** (sees Atelier before installing)

- As an evaluating developer, I want the README to show a distinctive, polished interface so that I judge the project as credible and maintained.
- As an evaluating developer, I want to see agents visibly orchestrating in parallel so that I understand what makes Atelier different in seconds.

**Daily Driver** (runs Atelier every day)

- As a daily user, I want the welcome screen to show version, configured agents, and the current repo/branch so that I confirm my session context before letting agents act.
- As a daily user, I want a persistent footer with repo + branch, run state, and active agent count so that I never start a run against the wrong branch and always know what the harness is doing.
- As a daily user, I want each agent to keep a consistent accent color across the roster and output views so that I can track who did what at a glance.
- As a daily user, I want the welcome content to stay in scrollback so that I can scroll up to re-check session facts mid-session.

**OSS Contributor**

- As a contributor, I want all colors defined in one place with semantic names so that UI changes don't require hunting through render code.

**Accessibility-Dependent User**

- As a screen-reader user, I want startup to skip decorative ASCII art so that my session begins with meaningful content.
- As a user with `NO_COLOR` set or a 256-color terminal, I want the interface to remain fully legible so that branding never costs me usability.

## Core Features

### F1 — Branded Welcome Screen (Critical)

Replaces the transient "Loading skills..." screen. Renders on every startup and persists in scrollback with the input anchored beneath it (Claude Code model). Contains:

- Adaptive "Atelier" wordmark: full lettering on wide terminals, compact on medium, plain styled text below ~60 columns; hideable via configuration; skipped entirely in `NO_COLOR`/screen-reader contexts.
- Facts box: version, working directory, repo + branch (omitted gracefully outside a git repo), configured agents summary (count, names, models), active preset, and config warnings count if any.
- One getting-started hint: `/help`.

### F2 — Unified Brand Theme (Critical)

The web palette (`web/src/styles/global.css`) becomes the single source of visual truth for the TUI: warm off-white text, green/amber/cyan/red accents with one semantic meaning each. Every TUI surface — panels, borders, dropdowns, dialogs, help modal, input composer, chat severity colors — uses the shared palette. Resolves the current yellow-overload (input border, dropdowns, modal, status all yellow today).

### F3 — Persistent Status Footer (High)

The existing status line grows into an ambient-state footer showing: repo + branch · run state (Idle/Planning/Running/Waiting) · active agent count (e.g., "3 agents · 2 running"). Git information refreshes so mid-session branch switches are reflected; outside a git repo the segment is omitted, not errored. The `/help` hint remains.

### F4 — Live Orchestration Identity (High)

Each configured agent receives a stable accent color used consistently in the roster and its output/progress views, making parallel runs visually traceable. The run-summary view is restyled with the theme. The working indicator keeps its current form, restyled with theme colors (signature animated indicator deferred — ADR-003).

### F5 — Compatibility & Accessibility Behavior (High)

`NO_COLOR` honored (decoration suppressed, content intact); non-truecolor terminals receive a quantized palette that preserves contrast; the welcome screen renders correctly at 80, 60, and 40 columns; decorative elements never carry information that exists nowhere else.

### F6 — README Asset Refresh (Medium)

New hero assets: welcome-screen screenshot + a GIF of multiple agents running in parallel with per-agent colors. Ships only when all surfaces match (ADR-002) — this is the distribution deliverable the rest of the feature exists to enable.

## User Experience

**First contact (pre-install):** README hero GIF shows agents orchestrating in parallel, each with its accent color, over the branded interface — answering "what is this?" before a word is read.

**Every session start:** Skill loading happens behind the welcome render. The user lands on: wordmark (or its width-appropriate fallback), facts box confirming session context, `/help` hint, input ready below. No extra keystrokes versus today; startup feels identical or faster.

**During a run:** The footer continuously answers the three ambient questions — where am I (repo + branch), what is happening (run state), who is working (agent count). Chat output uses per-agent accents so interleaved parallel output remains attributable.

**Accessibility paths:** screen-reader users get facts without art; `NO_COLOR` users get a monochrome but complete experience; narrow-terminal users get the plain-text identity. No information lives only in color or decoration.

**Discoverability:** `/help` remains the single taught entry point; everything else is discoverable from it.

## High-Level Technical Constraints

- Welcome screen must add less than 150ms to first render (user-perceived startup unchanged).
- The TUI palette must derive from the existing web palette tokens — no second brand definition.
- Git information must come from the user's existing git installation; absence of git or a repo degrades silently.
- All colors must resolve through one central definition (enforceable by automated check).
- Honors `NO_COLOR`; functions on 256-color terminals including macOS Terminal.app.

## Non-Goals (Out of Scope)

- **User-configurable themes / theme files** — V2+ stretch; V1 ships one palette (ADR-001).
- **Signature animated working indicator with rotating verbs** — deferred to V2 (ADR-003).
- **Light-terminal adaptive palette** — V1 assumes dark backgrounds; revisit with the V2 theme system.
- **Multi-item tips panel / onboarding wizard** — single `/help` hint only (ADR-003).
- **"What's new"/changelog panel** — no changelog infrastructure exists; deselected in idea phase.
- **Website changes** — the web palette is consumed, not modified.
- **Structural refactor of the TUI module** — only the theme extraction; decomposition is separate work (ADR-001).

## Phased Rollout Plan

### Phase 1 — Foundation & Welcome (MVP)

Theme definition from web palette + color resolution behavior (NO_COLOR/256-color) + branded welcome screen replacing the loading screen + full migration of existing colors.

**Success criteria:** welcome renders at all three width breakpoints; zero hard-coded colors outside the central definition; startup overhead <150ms; existing rendering tests pass.

### Phase 2 — Ambient State

Persistent footer (repo + branch, run state, agent count) + git context with graceful omission + welcome facts box consumes the same git data.

**Success criteria:** footer reflects branch switches mid-session; non-git directories show no errors; git work stayed within its half-day kill-switch (ADR-001).

### Phase 3 — Orchestration Identity & Launch

Per-agent accent colors in roster and output views + restyled run summary + remaining surface polish (dropdowns, dialogs, help modal) + README screenshot and parallel-agents GIF.

**Success criteria:** parallel-run output is visually attributable per agent; README assets match the shipped product exactly; 3-terminal compatibility checklist passes.

## Success Metrics

| Metric | Target | Measurement |
|---|---|---|
| Startup overhead from welcome | < 150ms added to first render | Render timing before/after on the same machine |
| Color single-source compliance | 0 hard-coded colors outside the central definition (from 88) | Automated check in CI |
| Width resilience | Correct rendering at 80 / 60 / 40 columns | Snapshot tests at three breakpoints |
| Terminal compatibility | 3/3 terminals + `NO_COLOR` pass | Release checklist: Terminal.app, iTerm2, Alacritty |
| Brand consistency | TUI and website share identical accent values | Visual comparison at release |

Star velocity is pre-registered as non-evidence (council decision); social traction is observed qualitatively (does the GIF travel without prompting).

## Risks and Mitigations

- **"Polished but flaky" perception** — a beautiful interface over an early-stage harness can read as misplaced priorities. *Mitigation:* hard 1-week timebox; capability work resumes immediately after; no announcement until everything matches.
- **Aesthetic subjectivity** — the maintainer's taste may not be the audience's. *Mitigation:* palette is already production-proven on the website; structure copies patterns validated by the category leaders.
- **Banner fatigue/pushback** — power users file issues asking to hide banners (documented for Claude Code and Gemini CLI). *Mitigation:* hide setting ships in V1, not as a reactive patch.
- **Opportunity cost** — a solo maintainer's week not spent on harness capabilities. *Mitigation:* timebox + kill-switches; the color consolidation pays down real maintenance debt regardless of brand outcome.
- **Brand drift** — future web palette changes could desynchronize the TUI. *Mitigation:* web tokens documented as canonical source (ADR-003); divergence is a release-checklist item.

## Architecture Decision Records

- [ADR-001: V1 Scope and Sequencing for the Atelier TUI Visual Identity](adrs/adr-001.md) — Theme seam ships with the welcome screen; full migration same window; subprocess git; no user theming in V1.
- [ADR-002: Unified Single-Release Rollout](adrs/adr-002.md) — One coherent release with three internal phases; README refresh gates announcement.
- [ADR-003: Web Palette as Canonical Brand Source; Signature Spinner Deferred](adrs/adr-003.md) — TUI derives from web tokens; craft-gerund spinner and multi-tip panel deferred.

## Open Questions

- **Wordmark gradient treatment** — which palette accents (and in what direction) color the lettering; needs a visual prototyping round during implementation.
- **Per-agent color assignment policy** — stable assignment by configuration order vs. by agent name; affects whether colors persist across config edits (user-facing behavior, decide in TechSpec).
- **Config warning display depth** — welcome facts box shows a count; whether it previews warning text inline is open.
- **Footer git refresh cadence** — how quickly a mid-session branch switch must appear (event-driven vs. periodic; TechSpec decision).
