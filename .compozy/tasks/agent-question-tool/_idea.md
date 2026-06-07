# Clarification Select UI

## Overview

Add a focused Clarification Select UI to `atelier` so a run that needs user input does not stall behind a plain free-text prompt. When the orchestrator asks a clarifying question, the TUI Input Composer becomes a select input with a short title, recommended answers, and a final custom answer option.

V1 is intentionally simple: upgrade the existing orchestrator-owned `pending_clarification` flow. It is for the human TUI operator and should reduce abandoned `WaitingForUser` runs while making common answers faster.

## Problem

The harness already supports `WaitingForUser`, but the current flow is too implicit. The orchestrator records one `clarifying_question`, the run pauses, and the user must notice the state and type a free-text answer. That creates friction exactly when the agent needs a crisp decision.

Modern agent tools increasingly treat clarification as a first-class interaction. OpenCode exposes a `question` tool with title, options, and custom answers. Claude Code's `AskUserQuestion` flow similarly pauses execution and collects structured selections or free text. This validates the pattern, but V1 should stay narrower than a general any-agent question protocol.

### Market Data

Stack Overflow 2025 reports that 84% of respondents use or plan to use AI tools, while trust in AI output accuracy remains weak. Google DORA 2025 reports 90% AI adoption among surveyed technology professionals and frames AI value as dependent on better process and oversight. Structured clarification supports that trust gap by asking for human judgment at ambiguity points instead of guessing.

## Core Features

| # | Feature | Priority | Description |
|---|---|---|---|
| F1 | Pending Clarification Select | Critical | Render the active clarifying question as a selectable composer state instead of plain text input. |
| F2 | Recommended Options | Critical | Show 2-4 recommended answers with one default selection when available. |
| F3 | Custom Answer Option | Critical | Always include a final custom text path when none of the recommended answers fit. |
| F4 | Skip/Cancel Handling | High | Let the operator explicitly skip or cancel the question without corrupting run state. |
| F5 | History And Chat Events | High | Record compact lifecycle events for question requested, answered, skipped, and cancelled. |
| F6 | Run Resume | Critical | Resume the paused run after the selected or custom answer is recorded. |

## KPIs

| KPI | Target | How to Measure |
|---|---:|---|
| Question continuation rate | >= 85% | Percentage of question-paused runs that resume after an answer. |
| Median question response time | < 30s | Time from question_requested to question_answered. |
| Recommended-option usage | >= 70% | Share of answers using a provided option instead of custom text. |
| Abandoned waiting runs | -50% | Compare WaitingForUser runs without resolution before and after release. |
| History completeness | 100% | Every question lifecycle has durable Session History events. |

## Feature Assessment

| Criteria | Question | Score |
|---|---|---|
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Maybe |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: Quick Win.

## Council Insights

- **Recommended approach:** Ship the focused clarification select UI first.
- **Key trade-offs:** Simpler V1 is faster and easier to validate, but a later any-agent question tool may require model and history changes.
- **Risks identified:** Question spam, approval confusion, missed questions, and history noise. Mitigate through one active question, separate approval/question states, clear composer mode, and compact lifecycle events.
- **Stretch goal (V2+):** General agent interaction protocol where any specialized agent can request structured user input.

## Out of Scope (V1)

- **Any-agent question requests** — defer until the simple operator loop proves useful.
- **Multiple simultaneous questions** — V1 should keep one active question at a time.
- **Headless question protocol** — needs separate product decisions for non-TUI runs.
- **Rich question history browser** — compact Chat and Session History events are enough for V1.
- **Multi-select and validation schemas** — unnecessary for the first clarification-select flow.

## Architecture Decision Records

- [ADR-001: Scope Clarification Select UI](adrs/adr-001.md) — V1 upgrades existing orchestrator clarification instead of creating a broad agent question protocol.

## Open Questions

- Should skip resume the run with an explicit "user skipped" answer, or should it stop the run as blocked?
- What exact keybindings should switch between option selection and custom text entry?
- Should the orchestrator be prompted to provide structured answer options, or should the app derive options from a plain question when missing?
