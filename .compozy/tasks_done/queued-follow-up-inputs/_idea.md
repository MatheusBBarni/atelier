# Queued Follow-Up Inputs

## Overview

Add explicit queued follow-up inputs to Atelier's interactive TUI so users can capture follow-up intent while a Run is active without starting a second Run or sending text to the Orchestrator too early.

V1 is a focused TUI feature: `/queue <message>` and `/q <message>` add a follow-up Prompt to a visible FIFO queue. Users can view and cancel queued items before replay. The Harness replays one queued Prompt after each clean completed Run and pauses the queue when replay would be unsafe.

## Problem

Atelier currently preserves a one-active-Run model. That is the right orchestration boundary, but it creates friction during long-running work: users often think of follow-up instructions while the Harness is still planning, running, waiting on tools, or producing output.

Today that follow-up intent can be rejected, forgotten, or accidentally hidden behind transport-level buffering. None of those outcomes is good product behavior. Users need a visible, controllable way to stage the next Prompt without weakening the current Run lifecycle.

### Market Data

Modern agent products increasingly treat queueing and slash commands as normal workflow controls. Cursor supports queued and interrupting agent messages. Claude Code documents slash commands as session controls, including workflow and background-task commands. OpenAI's Codex agent-loop writeup reinforces that each user input drives a turn until control returns to the user.

Stack Overflow's 2025 survey shows broad AI-tool adoption, with 84% of respondents using or planning to use AI tools, but also high friction: 66% report frustration with AI output that is almost right but not quite. Queueing must therefore preserve user control, not silently chain stale instructions.

## Summary / Differentiator

Atelier can differentiate by treating queued follow-ups as Harness-owned user intent, not as hidden terminal buffering. V1 stays narrow, but it creates a foundation for future modes such as steer-now, side-question, and cancel-and-replace.

## Integration with Existing Features

| Integration Point | How |
| --- | --- |
| Input Composer | Accept `/queue <message>` and `/q <message>` as explicit queue commands. |
| Run lifecycle | Preserve one active Run; queued items become normal Prompts only after safe replay. |
| Chat | Show queued, cancelled, paused, and replayed follow-up state clearly. |
| Slash-command help/discovery | Include `/queue` and `/q` in help and command metadata. |
| Session History | Record queue lifecycle events when useful for auditability. |

## Core Features

| # | Feature | Priority | Description |
| --- | --- | --- | --- |
| F1 | Explicit Queue Commands | Critical | `/queue <message>` and `/q <message>` add a follow-up Prompt while a Run is active. |
| F2 | FIFO Replay | Critical | Replay queued Prompts one at a time, oldest first, after clean completed Runs. |
| F3 | Visible Queue State | Critical | Show queue count and queued item summaries in the TUI. |
| F4 | Cancel Before Replay | Critical | Allow users to cancel queued items before they begin replay. |
| F5 | Safe Replay Gate | Critical | Pause replay after failed, interrupted, limit-reached, approval-waiting, or clarification-waiting states. |
| F6 | Normal Send Preservation | High | Keep normal prompt submission behavior unchanged while a Run is active. |
| F7 | Command Discoverability | High | Document `/queue` and `/q` in help and slash-command suggestions. |

## KPIs

| KPI | Target | How to Measure |
| --- | --- | --- |
| Lost follow-up reduction | -80% | Compare rejected/forgotten follow-up reports before and after release. |
| Replay ordering accuracy | 100% | Tests verify FIFO replay across multiple queued Prompts. |
| Queue control coverage | 100% | Tests verify every queued item can be viewed and cancelled before replay. |
| Active-run invariant | 0 overlapping Runs | Tests assert queue replay never creates concurrent Runs. |
| Safe replay behavior | 100% | Tests verify non-clean endings pause queued replay. |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Strong |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: Quick Win

## Council Insights

- **Recommended approach:** Ship explicit, visible, cancellable queue-next behavior for the TUI, with Harness-owned queue state and replay policy.
- **Key trade-offs:** TUI-only state is faster but weaker; app-visible queue ownership is more coherent and testable. Automatic replay is useful, but only after clean completion.
- **Risks identified:** stale queued intent, hidden buffering becoming product behavior, slash-command ambiguity, and replay after unsafe Run endings.
- **Stretch goal (V2+):** Expand queued intent into modes such as steer-now, side-question, cancel-and-replace, edit, reorder, and persistence.

## Out of Scope (V1)

- **Automatic busy-send queueing** — V1 requires explicit `/queue` or `/q`.
- **Interrupt or steering semantics** — Mid-run steering changes active Run behavior and needs separate design.
- **Edit and reorder controls** — Valuable later, but view/cancel is enough for V1.
- **Persistent queues across TUI restarts** — V1 is session-scoped.
- **Web or non-TUI surfaces** — The clarified V1 target is the interactive TUI only.
- **Runtime-specific queue APIs** — Runtime adapters continue receiving normal Prompts only when the Harness starts a Run.

## Architecture Decision Records

- [ADR-001: Scope Queued Follow-Up Inputs V1](adrs/adr-001.md) — Use explicit `/queue` and `/q`, App-owned queue state, FIFO replay, and safe replay gating.

## Open Questions

None for V1.

## References

- Cursor queued/interrupting agent messages: https://cursor.com/changelog/1-4
- Claude Code commands: https://code.claude.com/docs/en/commands
- Claude scheduled prompt idle behavior: https://code.claude.com/docs/en/scheduled-tasks
- OpenAI Codex agent loop: https://openai.com/index/unrolling-the-codex-agent-loop/
- Stack Overflow 2025 Developer Survey: https://survey.stackoverflow.co/2025/
