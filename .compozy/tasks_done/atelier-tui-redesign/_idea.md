# Atelier TUI Visual Identity

## Overview

Give Atelier a distinct, screenshot-worthy visual identity modeled on the structure (not the look) of Claude Code's TUI: a branded welcome screen with an ASCII "Atelier" wordmark and info panels, an own-brand color palette applied consistently across every surface, and — the differentiating move — treating the **live multi-agent orchestration view as a first-class brand surface**, so the README asset is a GIF no competitor can fake.

- **Problem it solves:** Atelier is functionally a multi-agent harness but visually anonymous — the name appears once, in a transient loading box; screenshots are forgettable in a market where the screenshot is the landing page.
- **Who it's for:** evaluating developers (decide from a screenshot in seconds), daily drivers (hours of staring at the TUI), and OSS contributors (one theme file to learn instead of 88 scattered colors).
- **V1 ambition:** one-week timebox. Branded welcome + theme seam + full color migration + orchestration-view identity. No user-facing theme configuration yet.

## Problem

Atelier v0.1.1 starts with a transient "Loading skills..." box and drops into an unbranded three-panel layout. There is no welcome screen, no wordmark, no palette — and 88 inline `Color::` literals scattered through `src/tui/mod.rs` (~3,900 lines), with yellow carrying four unrelated meanings (input border, dropdowns, help modal, status). For a public open-source tool, every screenshot a potential user sees is indistinguishable from a generic ratatui demo.

Distribution for terminal dev tools happens through images: README, Show HN, X, awesome-lists. Each channel transmits exactly one screenshot, and an un-screenshottable product forfeits those channels before any quality judgment occurs. Meanwhile the category has consolidated around strong visual identities — and left a gap: no competitor's interface communicates *multi-agent orchestration*, which is Atelier's entire reason to exist.

The current code also makes any future visual work more expensive: with colors inline at 88 call sites, supporting `NO_COLOR` (a community-standard convention) or a 256-color fallback is impossible without consolidation first.

### Market Data

- **Claude Code**: orange gradient block-letter wordmark + bordered info box — so iconic it spawned imitation tooling (oh-my-logo).
- **Gemini CLI** (~105k stars): blue→purple gradient banner; most-praised polish of the big three.
- **OpenCode** (~172k stars, fastest-growing): best-in-class 62-slot theme system; **no ratatui-based agent ships anything comparable**.
- **Crush (Charm)**: reached top of HN largely on aesthetics ("glamourous" positioning).
- **Aider** (weakest visuals, 4.1M installs): plateaued relative to prettier newcomers.
- **Palette gap:** orange = Claude, blue/purple = Gemini, pink = Charm. **Copper + verdigris/teal** is unclaimed and fits the atelier/workshop name.
- Polish↔adoption correlation is directional, not causal (no controlled study) — measurement expectations set accordingly.

## Summary / Differentiator

Two moves competitors can't easily answer: (1) a claimable craft-studio palette in unclaimed color space; (2) **the live show** — per-agent accent colors, a signature working indicator, and polished parallel-agent progress views, demoed as a GIF of agents orchestrating in parallel. A palette is copyable in an afternoon; multi-agent orchestration on screen is not.

## Core Features

| # | Feature | Priority | Description |
|---|---------|----------|-------------|
| F1 | Semantic theme module | Critical | `theme.rs` with 10-15 semantic tokens (`accent`, `surface`, `text_muted`, `border_focused`, `agent_active`...) + a resolution layer: `NO_COLOR` honored, truecolor detection via `COLORTERM`, 256-color quantization fallback. Serde-ready struct; no config file exposed. |
| F2 | Branded welcome screen | Critical | ASCII "Atelier" wordmark (tui-big-text 0.7.x, gradient via per-row RGB interpolation) + bordered panels: version, session/model info, tips, project/git context, **agent roster**. Width degradation ladder: full → compact → plain text (~80/60/40 cols). Zero inline color literals. |
| F3 | Brand palette | High | Copper + verdigris/teal palette (proposal — final values need visual approval) defined once as theme tokens; resolves yellow-overload by giving each semantic role a distinct color. |
| F4 | Git context | High | One function → `Option<GitContext>` via `git rev-parse` subprocess with timeout; non-zero exit/timeout/missing binary = graceful omission. Consumers: persistent status-bar footer (repo + branch) and welcome panel. Debounced refresh (footer must not show stale branch after mid-session switches). Kill-switch: cut if > 0.5 day. |
| F5 | Live orchestration identity | High | Per-agent accent colors in roster and output views, signature working indicator/spinner, polished parallel-agent progress rendering, styled run-summary. This is the README GIF surface. |
| F6 | Full color migration | High | All 88 inline `Color::` literals migrated to theme tokens; CI grep invariant: zero literals outside `theme.rs`. Existing semantic helpers (`status_style`, `severity_badge_style`) repointed, not replaced. |
| F7 | Surface polish | Medium | Dropdowns, dialogs, help modal, borders, input composer restyled with tokens for full consistency. |
| F8 | README asset refresh | Medium | Live-orchestration GIF + welcome screenshot replacing current README visuals — the actual distribution deliverable. |

## KPIs

| KPI | Target | How to Measure |
|---|---|---|
| Startup overhead from welcome screen | < 150ms added to first render | Instrument render timing before/after; compare on the same machine |
| Inline color literals outside theme module | 0 (from 88) | CI grep check on `src/**/*.rs` excluding `theme.rs` |
| Welcome screen width resilience | Renders correctly at 80, 60, and 40 columns | `TestBackend` snapshot tests at 3 breakpoints |
| Terminal compatibility | 3/3 terminals verified + `NO_COLOR` honored | Manual release checklist: Terminal.app (256-color), iTerm2, Alacritty; `NO_COLOR=1` run |

*(Star velocity deliberately excluded — pre-registered as non-evidence per council; social traction is tracked qualitatively.)*

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Must do |
| **Frequency** | How often would users encounter this value? | Must do |
| **Differentiation** | Does this set us apart or just match competitors? | Must do *(upgraded from Strong by the Live Show hybrid)* |
| **Defensibility** | Easy to copy or compounds over time? | Strong *(orchestration view is not fakeable; theme seam compounds)* |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: **Strategic Bet** (with compounding infrastructure)

## Council Insights

- **Recommended approach:** theme seam and welcome screen land together (welcome writes zero inline literals); full 88-literal migration in the same release window; color *resolution* (NO_COLOR/256 fallback) is a hard V1 dependency because brand RGB means owning degradation that ANSI-named colors got free.
- **Key trade-offs:** ~1 week of solo-maintainer time displaced from harness capability work; permanent ownership of color degradation; `tui-big-text` couples the welcome module to the ratatui version line (isolate it).
- **Risks identified:** render-module regression (mitigated by 66 existing `TestBackend` tests + mechanical extraction); "polished but flaky reads as abandoned-after-the-redesign" (mitigated by timebox + no structural refactors beyond `theme.rs`); palette degradation on non-truecolor terminals (resolution layer + 3-terminal checklist); mid-implementation scope creep in the 3,900-line file (extract theme, touch nothing else structurally).
- **Stretch goal (V2+):** OpenCode-class user-facing theme system — serde JSON/TOML themes, dark/light variants, OSC background detection, possible standalone crate for the ratatui ecosystem. The serde-ready struct keeps this a small follow-up.

## Out of Scope (V1)

- **User-configurable theme files** — a support surface, not a feature, for a solo maintainer at v0.1.1; struct stays serde-ready so the door is open.
- **OSC light/dark terminal-background detection** — V2+ with the theme system; V1 assumes dark background (the overwhelming default for this audience).
- **`git2` dependency** — heavy native C dependency rejected; subprocess `git rev-parse` collapses all edge cases into one graceful-omission contract.
- **"What's new"/changelog panel** — explicitly deselected during clarification; requires changelog infrastructure that doesn't exist.
- **Structural refactor of `tui/mod.rs`** — only `theme.rs` is extracted; decomposing the 3,900-line module is separate, deliberate work.
- **Custom animations beyond the working indicator** — delight budget capped; spinner identity only.

## Architecture Decision Records

- [ADR-001: V1 Scope and Sequencing for the Atelier TUI Visual Identity](adrs/adr-001.md) — Theme seam ships with the welcome screen; full migration same window; subprocess git; no user theming in V1.

## Integration with Existing Features

| Integration Point | How |
|---|---|
| Skill-loading screen (`tui/mod.rs:1116`) | Replaced/absorbed by the branded welcome screen |
| Agent roster (`tui/mod.rs:1158`) | Gains per-agent accent colors; data reused on welcome splash |
| Status line (`tui/mod.rs:1883`) | Gains repo+branch from `GitContext` + themed working indicator |
| Semantic style helpers (`status_style`, `severity_badge_style`, `availability_style`) | Repointed at theme tokens — call sites unchanged |
| Agent/skill dropdowns, help modal | Restyled with tokens (F7) |

## Open Questions

- **Final palette values** — copper + verdigris is the proposal; exact RGB values and gradient stops need a visual round (prototype both Sextant and Quadrant `PixelSize` for the wordmark and pick by eye).
- **Git context on splash and footer, or footer only?** — Council leaned footer-primary; user originally asked for it on the welcome screen. Recommended: both (same `Option<GitContext>`, near-zero marginal cost), but confirm during PRD.
- **Tips content** — which 3-5 tips/commands earn welcome-screen space (e.g., `/help`, `/agent:`, `/skill:`)?
- **Git footer refresh cadence** — debounce interval TBD in TechSpec (event-driven on run start vs. timer).
