# PRD: Tabbed Help Modal

## Overview

The atelier TUI's only built-in help is a single `/help` overlay that concatenates every
slash command, keybinding, and CLI flag into one undifferentiated, scrolling block — hard
to scan and silent on the things that make atelier distinctive (prompt → orchestrator →
specialized agents, runtime availability, installed skills, approval modes). This forces new
users to quit to the README or run `atelier --doctor` in a separate shell.

This feature replaces that overlay with a **keyboard-navigable, tabbed help surface** that
separates information by type and surfaces **live session state**. It defaults to a
**Getting Started** front door optimized for new and occasional users, and adds a lightweight
**empty-state onboarding hint** that points newcomers toward help at the moment they need it.
The primary outcome we optimize for is **faster time-to-first-successful-run** for new users.

## Goals

- **Primary:** Reduce the time it takes a new user to reach their first successful run by
  making the core mental model and a runnable example reachable in seconds, without leaving
  the TUI.
- Make any command, key, or capability **locatable in under ~10 seconds** once help is open,
  by grouping information into scannable tabs.
- Surface **live, accurate** agent/runtime and skill state inside help, so it is more useful
  than a static cheatsheet and cannot drift from reality.
- Improve **discoverability** of help and core affordances (`/help`, `/`, `@`) for
  first-session users.
- **Milestones:** MVP (tabbed modal + Getting Started + live Skills + empty-state hint),
  Phase 2 (Commands filter + first-approval explainer), Phase 3/V2 (live approvals, command
  palette, context-adaptive help).

## User Stories

**Newcomer Nadia (primary — first launch):**

- As a first-time user, I want a Getting Started view that explains how my prompt becomes
  work, so I can run something useful in my first few minutes.
- As a newcomer staring at an empty prompt, I want a one-line hint that tells me what to type
  and where help lives, so I'm not stuck.
- As a newcomer, I want to see the actual agents that will act on my request, so the tool
  feels concrete rather than abstract.

**Returning Rafael (secondary — occasional use):**

- As an occasional user, I want commands on their own tab, so I can find the exact syntax
  without scrolling past unrelated keybindings and CLI flags.
- As a returning user who half-remembers a command, I want to type a few letters and narrow
  the list (Phase 2), so I find it instantly.

**Config-tweaker Cora (tertiary — environment checks):**

- As someone who edits config, I want to see which agents/runtimes are live and which models
  they use, so I can confirm my setup without leaving the TUI.
- As a config user, I want a plain-language explanation of approval modes and read/write
  roots, so I understand what agents are allowed to do.

## Core Features

| Priority | Feature | What it does / why it matters | Phase |
| -------- | ------- | ----------------------------- | ----- |
| Critical | **Tabbed help overlay + keyboard nav** | Replaces the flat overlay with labeled tabs; cycle with keyboard, Esc closes from any tab. Default = Getting Started. Foundation for everything else. | MVP |
| Critical | **Getting Started front door** | Leads with a one-line routing **mental model**, then **2 runnable example prompts**, then a **live agent/runtime summary**. The fastest path to a first successful run. | MVP |
| High | **Live Skills tab** | Lists discovered skills (project + personal) with the "skills are guidance, don't bypass approvals" disclaimer. Reflects the real installed set. | MVP |
| High | **Empty-state onboarding hint** | One muted line in the empty-chat/welcome area pointing to `/help` and explaining routing. Self-gating: shows only on an empty chat, disappears once the user types. | MVP |
| Medium | **Commands tab** | The slash-command reference, derived from the single command catalog so it never drifts. | MVP |
| Medium | **Keys tab** | Keybindings reference, separated from commands. | MVP |
| Medium | **Approvals & Modes tab** | Plain-language explanation of yolo vs normal approval, capabilities, and read/write roots. Static prose in MVP. | MVP |
| Medium | **CLI tab** | `atelier` flags for the operator/CI audience, as the last, non-default tab. | MVP |
| High | **Commands filter** | Type-to-narrow the Commands tab list for fast lookup. The modal's first in-place input. | Phase 2 |
| Medium | **First-approval explainer** | A one-line explainer the first time a user hits an approval prompt, shown at most once. | Phase 2 |

## User Experience

**First contact (Nadia):** On first launch, the welcome/empty-chat area shows a muted hint —
*"Describe a task; it routes through an orchestrator to named agents — `/help` for the map."*
Pressing `/help` opens the modal on **Getting Started**: a one-line model, two
copy-pasteable example prompts, and a live list of the configured agents and their
availability. Nadia copies an example, runs it, and reaches a result — the hint disappears
the moment she starts typing.

**Lookup (Rafael):** Rafael opens `/help`, moves to the **Commands** tab, and scans a clean
command list (Phase 2: types a few letters to filter). He never sees CLI flags or
keybindings mixed in.

**Environment check (Cora):** Cora opens **Getting Started** (live agent summary) or the
**Approvals & Modes** tab to confirm her runtimes are healthy and understand what agents may
write.

**UI/UX & accessibility:**

- **Keyboard-first:** every tab and action is reachable without a mouse; a visible active-tab
  indicator and a persistent close affordance.
- **Legibility:** must remain readable on monochrome / `NO_COLOR` terminals and small
  viewports.
- **Scannability:** each tab stays chunked and focused — no walls of text; lead with examples.
- **Onboarding is pull, not push:** the modal is opt-in; the only proactive nudge is the
  self-gating empty-state hint. No forced tutorials.

## High-Level Technical Constraints

*(Boundaries that shape the product, not implementation prescriptions.)*

- **Consistency with existing surfaces:** help must stay aligned with the live command
  catalog, the agent roster, and the skill list — these are the sources of truth; help
  reflects them, it does not restate them.
- **No drift:** command, agent, and skill content must derive from live sources so help
  cannot fall out of sync with the app.
- **Keyboard-only operability** and **monochrome legibility** are required.
- **No new persisted user data** in the MVP; nothing leaves the user's machine. (The
  empty-state hint is self-gating and needs no stored state; the Phase 2 first-approval
  "show-once" behavior is the first place persistence is even considered.)
- **Responsiveness:** opening help and switching tabs must feel instant.

## Non-Goals (Out of Scope)

- **Live approval-mode & read/write-roots data** — shown as static prose in MVP; live values
  are a V2 concern.
- **Cross-tab / fuzzy search (command palette)** — MVP/Phase 2 ship only a Commands-tab
  substring filter.
- **Context-adaptive (RunState-aware) help content** — deferred to V2.
- **Standalone "Agents & Runtimes" tab** — the live agent view lives inside Getting Started
  to avoid duplicating the always-on roster.
- **First-approval explainer in the MVP** — deferred to Phase 2 (needs show-once tracking).
- **Mouse-driven tab selection, persisting the last-open tab, recency ranking** — not needed
  to validate the core value.

## Phased Rollout Plan

### MVP (Phase 1)

- **Included:** Tabbed overlay + keyboard nav; Getting Started (mental model → examples →
  live agent summary); Commands, Keys, CLI tabs; Approvals & Modes (static prose); live
  Skills tab; empty-state hint.
- **Success criteria to proceed:** new-user time-to-first-successful-run improves vs. the
  pre-redesign baseline; users can locate a command/key in under ~10s in a quick usability
  check; no regression in existing help/Esc/keybinding behavior.

### Phase 2

- **Included:** Commands-tab substring filter; first-approval explainer (shown at most once).
- **Success criteria to proceed:** filter measurably speeds command lookup for returning
  users; the approval explainer fires ≤ once with no fatigue complaints.

### Phase 3 / V2

- **Included:** Live approval-mode/roots data; fuzzy command palette across
  commands/agents/skills; context-adaptive help reflecting the current run state; recency
  ranking.
- **Long-term success:** help becomes the primary in-tool reference; measurable reduction in
  users leaving the TUI to consult external docs.

## Success Metrics

*(All locally derivable from the event-sourced session history; benchmark numbers from SaaS
tools don't transfer to an offline TUI, so we use definitions and track trends against our
own baseline.)*

| Metric | Definition | Target |
| ------ | ---------- | ------ |
| **Time-to-first-successful-run** (primary) | Median elapsed time from first launch to first `Completed` run | Establish baseline, then **reduce** (goal: median **< 5 min**) |
| **Help-assisted success rate** | Of sessions where help or the hint was shown, % that then reach a `Completed` run without quitting first (instrument *help-shown → task-completed*, never bare opens) | **> 60%**, trending up |
| **Capability discovery (first session)** | % of new sessions that open `/help` or use `/` or `@` at least once | **> 60%** |
| **Time-to-locate in help** | Time from opening help to landing on the right tab/answer (usability check) | **< 10 s** |
| **Help content accuracy** | Commands/agents/skills shown are 100% derived from live sources (0 hardcoded) | **0 drift** |
| **Hint non-annoyance** (Phase 2+) | Each hint fires ≤ once; manual-dismiss rate watched as a fatigue signal | **≤ 1 impression**; low dismiss rate |

## Risks and Mitigations

| Risk | Type | Mitigation |
| ---- | ---- | ---------- |
| Users never open `/help`, so the redesign goes unseen | Adoption | The self-gating empty-state hint points newcomers to it at the right moment; discovery is a tracked metric |
| Onboarding hint feels naggy | Adoption | Hint only renders on an empty chat and vanishes once the user types; first-approval explainer (Phase 2) is show-once |
| Over-organizing hides content behind tabs | Adoption | Visible tab strip, Getting Started as the front door, and a filter (Phase 2) for direct lookup |
| Phase 2 slips and the filter never lands | Timeline | Keep Phase 2 small and explicitly tracked; the filter reuses an established type-to-filter pattern |
| Delivery collides with other in-flight TUI work | Dependency | Sequence delivery after the adjacent TUI branches land (per ADR-001) |
| Help merely matches competitors' cheatsheets | Competitive | The live, agent-aware, single-sourced content is the differentiator no static cheatsheet can match |

## Architecture Decision Records

- [ADR-001: V1 Scope for the Tabbed Help Modal](adrs/adr-001.md) — Tabbed overlay with live
  Agents/Skills via a render-signature change, Getting Started front door, static Approvals
  prose, deferred approval/roots plumbing, plus an in-flow onboarding hint.
- [ADR-002: Phased Delivery Approach](adrs/adr-002.md) — Approach A (Lean onboarding MVP,
  enrich in Phase 2): front-load the north-star onboarding value, isolate the net-new
  Commands filter and the first-approval explainer into Phase 2.

## Open Questions

- **Example-prompt copy:** the exact two runnable example prompts for Getting Started (must be
  real, current, and produce a fast result).
- **Empty-state hint wording:** final copy, and whether it should also name the `/` and `@`
  affordances (research favors pointing at real controls).
- **Phase 2 filter affordance:** how the user enters/exits filter mode within the modal from a
  user's mental model (not implementation) — e.g., does typing filter immediately, or after a
  keypress.
- **Approvals tab depth:** how much of capabilities/roots a newcomer actually needs vs. what
  overwhelms — validate the prose length with a real user.
- **Tab labels & order:** confirm the tab names and left-to-right order read naturally for
  newcomers (Getting Started first, CLI last).
