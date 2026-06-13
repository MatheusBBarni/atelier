# Idea: Tabbed Help Modal

## Overview

Replace the atelier TUI's single scrollable help overlay — today a flat wall of text
mixing slash commands and CLI flags — with a **keyboard-navigable, tabbed help surface**
that separates information by type and surfaces **live session state**. The redesign is
optimized for **new and occasional users**: it defaults to a **Getting Started** front
door that teaches the core mental model (your prompt → orchestrator → named specialized
agents), shows a **live summary of the actually-configured agents and runtime health**,
and offers two copy-pasteable example prompts. Reference tabs (Commands with a filter,
Keys, Skills, Approvals, CLI) serve fast lookup. Two lightweight **in-flow nudges** (an
empty-state hint and a one-line explainer at the first approval) guide newcomers to the
modal at the moment of confusion.

V1 is a **Quick Win**: most data is already in the render snapshot, so live tabs are
largely a signature change rather than new plumbing. The one genuinely-expensive piece
(live approval-mode/roots) is deferred without losing user value.

## Problem

A first-time atelier user lands in an unfamiliar chat-style TUI with no obvious way to
learn what it can do. The only built-in help is `/help`, which opens a single overlay
(`render_help_modal`, `src/tui/mod.rs:3085`) that concatenates every slash command,
keybinding, and CLI flag into one undifferentiated, scrolling block. To find "how do I run
a bounded child task," a user must visually scan past unrelated CLI flags and keybindings.
On smaller terminals, content overflows with no paging affordance.

Worse, the help says nothing about the things that make atelier *atelier*: that prompts are
routed through an orchestrator to specialized agent profiles, which runtimes
(Codex/Claude/Cursor/Z.ai) are available, which models each agent uses, what skills are
installed, or how approval modes gate writes. A newcomer can't answer "is my environment
ready?" or "what agents will touch my code?" without quitting to read the README or running
`atelier --doctor` in a separate shell. The static text also silently **drifts** from
reality whenever a default changes.

The result is avoidable onboarding friction and context-switching out of the tool —
precisely when a new user is deciding whether atelier is worth learning.

### Market Data

- Best-in-class terminal tools (**lazygit, k9s, helix, which-key.nvim**) win discoverability
  through **context-adaptive** help that shows what's valid *now*, plus searchable
  cheatsheets — not flat dumps. AI-agent TUIs (**OpenCode, Claude Code**) standardize on
  **searchable, Esc-dismissible overlays with inline keybindings**.
- **NN/G "Help & Documentation"** (heuristic #10): favor *pull* over *push*, **chunk
  content**, group by topic **and** skill level, and lead with examples. **clig.dev**:
  "display the most common commands at the start"; prefer examples over reference prose.
- Directional onboarding data: ~**70%** of enterprise developer onboarding "fails" because
  static docs lack contextual decision support; **progressive disclosure** is associated
  with **30–50% faster** initial task completion; gold-standard time-to-first-value is
  **< 5 min**. (NN/G and clig.dev are authoritative; the percentage figures come from
  vendor blogs — treat as directional.)
- **Differentiator:** a *live, agent-aware, single-sourced* help surface — real configured
  agents, models, runtime health, and discovered skills — is something static cheatsheets
  structurally cannot match.

## Summary / Differentiator

atelier's help can do what lazygit's and k9s's cannot: render the user's **actual**
orchestrator/agent topology, model assignments, runtime availability, and installed skills,
all derived from live session state and the single-source command catalog — so help is both
more useful *and* immune to drift.

## Core Features

| #  | Feature                          | Priority | Description                                                                                                                                                                                  |
| -- | -------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| F1 | Tabbed help overlay + keyboard nav | Critical | Replace the flat overlay with a tab strip; Left/Right/Tab cycles tabs, Esc closes from any tab. Default tab = Getting Started. Tabs render via per-tab pure builders using theme tokens.    |
| F2 | Getting Started front door       | Critical | Default tab: the prompt→orchestrator→agents mental model, a **compact live agent/runtime summary** (configured agents, model, availability), and 2 copy-pasteable example prompts.          |
| F3 | In-flow onboarding nudges        | High     | Empty-state hint on the input composer ("describe a task; it routes through an orchestrator to named agents — `/help` for the map") + a one-line explainer at the first approval prompt.     |
| F4 | Commands tab + substring filter  | High     | Commands derived from `slash_commands::catalog()` (no drift), with a type-to-filter line (`contains()` over catalog-derived rows) for fast lookup.                                          |
| F5 | Live Skills tab                  | High     | Lists discovered skills from `ui_state.skill_suggestions` (project + personal roots) with the "skills are guidance, don't bypass approvals" disclaimer.                                      |
| F6 | Keys tab                         | Medium   | Keybindings reference (Enter, Ctrl-L, arrows, PageUp/Down, Home/End, mouse wheel, etc.), separated from commands.                                                                          |
| F7 | Approvals & Modes tab            | Medium   | **Static explanatory prose** for V1: how yolo vs normal approval works, capabilities, and the read/write-roots concept.                                                                     |
| F8 | CLI tab                          | Medium   | `atelier` flags for the operator/CI audience, as the last, non-default tab.                                                                                                                |

## KPIs

| KPI                                          | Target              | How to Measure                                                                                  |
| -------------------------------------------- | ------------------- | ---------------------------------------------------------------------------------------------- |
| Time-to-locate a command/key once help open  | **< 10 s**          | Moderated usability test; tab-switch instrumentation in the event log                          |
| Help self-sufficiency (no context-switch)    | **> 90%**           | `help_open` events not followed by `quit` within 60 s, from event-sourced history (`.multiagent/`) |
| New-user time-to-first-successful-run        | **< 5 min**         | Timestamp delta: first launch → first `Completed` run                                           |
| Help content drift                           | **0** / **100%** derived | Test asserting Commands/Skills tabs render from `catalog()` + `skill_suggestions`, not literals |
| Help discovery in first session              | **> 60%**           | `help_open` events ÷ new sessions                                                               |

## Feature Assessment

| Criteria            | Question                                            | Score    |
| ------------------- | --------------------------------------------------- | -------- |
| **Impact**          | How much more valuable does this make the product?  | Strong   |
| **Reach**           | What % of users would this affect?                  | Strong   |
| **Frequency**       | How often would users encounter this value?         | Maybe    |
| **Differentiation** | Does this set us apart or just match competitors?   | Strong   |
| **Defensibility**   | Is this easy to copy or does it compound over time? | Maybe    |
| **Feasibility**     | Can we actually build this?                         | Must do  |

Leverage type: **Quick Win** (with compounding onboarding value)

## Council Insights

- **Recommended approach:** Tabbed overlay with live Agents/Skills enabled by a single
  `render_help_modal(frame, &AppState, &TuiUiState)` signature change (data already in scope
  at both call sites). Fold the live agent view *into* Getting Started instead of a
  standalone tab (the always-on Ctrl-L Roster already shows agents). Commands tab stays
  catalog-derived with a substring filter (the cut-line item). Approvals tab is static prose
  for V1. Pair with in-flow onboarding nudges.
- **Key trade-offs:** live data vs. snapshot plumbing (resolved: agents/skills free,
  approval/roots deferred); tabs vs. fuzzy search (resolved: substring filter now, full
  palette V2); standalone Agents tab vs. Roster duplication (resolved: fold into Getting
  Started + share an `agent_roster_items` builder); modal *pull* vs. in-flow *push*
  (resolved: ship both).
- **Risks identified:** merge conflict in the hot `src/tui/mod.rs` (→ sequence after
  in-flight TUI branches; use new per-tab builder fns); over-hiding behind tabs (→ visible
  tab strip, Getting Started front door, filter); test churn in ~3 catalog/README contracts
  (→ update to select Commands tab, never drop the contracts); scope creep toward live
  approvals/fuzzy search (→ fixed by ADR-001).
- **Stretch goal (V2+):** context-adaptive help that reflects the current `RunState`, a full
  fuzzy command palette over commands/agents/skills, and a live approval-mode segment in the
  always-on footer + `ConfigStatusView` roots projection.

## Integration with Existing Features

| Integration Point                                | How                                                                                                       |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `render_help_modal` (`src/tui/mod.rs:3085`)      | Becomes tabbed; consumes `(&AppState, &TuiUiState)` as a pure snapshot function                            |
| Agent Roster (`Ctrl-L`, `src/tui/mod.rs:1946`)   | Extract a shared `agent_roster_items(state, theme)` builder reused by the Roster and Getting Started       |
| `slash_commands::catalog()`                      | Single source for the Commands tab; grouping (if any) derived from `SlashCommandKind`, not a new `category` |
| `ui_state.skill_suggestions`                     | Backs the live Skills tab                                                                                  |
| Key routing (`key_event_to_tui_command_with_ui:763`) | Add Left/Right/Tab cycling + filter input within the help-visible branch; Esc still closes            |
| Chat empty-state / projection                    | Hosts the in-flow onboarding hint                                                                          |
| `theme.rs`                                       | Tab strip / active-tab styling via semantic tokens (respect `colors_live_only_in_theme_module`)            |

## Out of Scope (V1)

- **Live approval-mode & read/write-roots data** — shown as static prose instead; live
  values require extending the serialized `AppState`/`ConfigStatusView`, the only genuine
  plumbing cost, not justified for V1.
- **Standalone "Agents & Runtimes" tab** — folded into Getting Started to avoid duplicating
  the always-on Ctrl-L Roster.
- **Full fuzzy / cross-tab search** — V1 ships a substring `contains()` filter on the
  Commands tab only; the nucleo fuzzy matcher is path-scoped and not reusable here.
- **Context-adaptive (RunState-aware) help content** — a larger concern deferred to V2.
- **Mouse-driven tab selection & persisting the last-open tab across sessions** —
  keyboard-first; not needed to validate the core value.

## Architecture Decision Records

- [ADR-001: V1 Scope for the Tabbed Help Modal](adrs/adr-001.md) — Tabbed overlay with live
  Agents/Skills via a signature change, Getting Started front door (live agent summary folded
  in), Commands filter as cut-line, static Approvals prose, deferred approval/roots plumbing,
  plus in-flow onboarding nudges.

## Open Questions

- **Tab-cycling keys:** Left/Right, Tab, or both? Confirm Tab doesn't collide with any
  existing completion affordance in the modal context.
- **Filter scope:** Commands tab only (current plan) or all text-bearing tabs?
- **Getting Started examples:** exact copy for the 2 runnable example prompts (need real,
  current commands).
- **In-flow hint frequency:** show the empty-state hint only when there's no session history
  (avoid nagging returning users)?
- **Skills freshness:** Skills tab reflects the cached `skill_suggestions` (refreshed at
  startup / `/reload:skills`); is on-open refresh wanted, or is cached acceptable for V1?
