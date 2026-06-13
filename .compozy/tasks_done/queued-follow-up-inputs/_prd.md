# Product Requirements: Queued Follow-Up Inputs

Status: Draft
Date: 2026-06-06

## Overview

Add explicit queued follow-up inputs to Atelier's interactive TUI so power users can capture follow-up intent while a Run is active.

V1 introduces `/queue <message>` and `/q <message>` as explicit queue-next commands. Queued items appear in a visible FIFO list, can be cancelled before replay, and run one at a time after clean completed Runs. If the previous Run fails, is interrupted, reaches a limit, or waits for user input, the queue pauses with a visible reason and resume/cancel controls.

## Goals

- Prevent follow-up instructions from being lost while the Harness is busy.
- Preserve the one-active-Run user model.
- Make pending follow-up intent visible and cancellable.
- Replay queued items predictably in FIFO order after clean completion.
- Teach users the command through help text, slash-command suggestions, and active-run rejection guidance.

## User Stories

- As a power TUI user, I want to queue a follow-up while a long Run is active so I do not lose the thought.
- As a power TUI user, I want to see pending queued items so I know what will run next.
- As a power TUI user, I want to cancel a queued item before it runs so stale intent does not execute.
- As a power TUI user, I want unsafe replay to pause with a clear reason so I can decide whether to resume or cancel.
- As a new slash-command user, I want `/queue` and `/q` to appear in help and suggestions so I can discover the workflow.

## Core Features

| Priority | Feature | Requirement |
| --- | --- | --- |
| Critical | Explicit queue commands | `/queue <message>` and `/q <message>` add a follow-up Prompt to the queue. |
| Critical | Visible FIFO queue | The TUI shows queued item order, count, and concise item summaries. |
| Critical | Cancel before replay | Users can cancel queued items until the item begins replay. |
| Critical | Safe replay gate | The queue auto-replays only after clean completed Runs. |
| Critical | Paused queue state | Non-clean Run endings pause the queue with reason, resume, and cancel options. |
| High | Normal send preservation | Normal prompt submission while a Run is active keeps existing behavior. |
| High | Discoverability | Help text and slash-command suggestions include `/queue` and `/q`. |
| High | Active-run guidance | If a user submits a normal prompt while busy, guidance points to `/queue`. |

## User Experience

1. User starts a long Run in the TUI.
2. While the Run is active, the user types `/q update the docs after this`.
3. The TUI confirms the follow-up was queued and shows it in the pending list.
4. The user can add more queued follow-ups or cancel any queued item.
5. When the current Run completes cleanly, the oldest queued item starts as the next normal Prompt.
6. If the Run does not complete cleanly, the queue pauses and shows why replay is blocked.
7. The user can resume the next queued item or cancel it.

The queue must feel intentional, not hidden. Users should always understand whether an item is pending, paused, cancelled, or replaying.

## High-Level Technical Constraints

- V1 is limited to the interactive TUI surface.
- The Harness must preserve one active Run at a time.
- Queued items are session-scoped in V1.
- Runtime adapters must not receive special queue semantics.
- Queue state should be visible enough for user trust and product-level auditability.

## Non-Goals

- Automatic queueing for every normal send while busy.
- Mid-run steering or interruption semantics.
- Editing queued items.
- Reordering queued items.
- Persistent queues across TUI restarts.
- Web or non-TUI queue surfaces.
- Runtime-specific queue APIs.

## Phased Rollout Plan

### MVP: Explicit Queue-Next

- Add `/queue` and `/q`.
- Show visible FIFO queue state.
- Support cancelling queued items before replay.
- Replay one queued item after each clean completed Run.
- Pause replay after unsafe endings.
- Add help text and slash-command suggestions.

Success criteria: users can queue, view, cancel, and safely replay follow-ups without creating overlapping Runs.

### Phase 2: Queue Management Polish

- Add edit support if cancellation is not enough.
- Add reorder support if users commonly queue multiple dependent items.
- Improve paused-queue explanations based on dogfood feedback.

Success criteria: queue management reduces stale-item cancellations without increasing confusion.

### Phase 3: Queued Intent Modes

- Explore steer-now, side-question, cancel-and-replace, persistence, and non-TUI surfaces.

Success criteria: new modes solve distinct user needs without weakening the one-active-Run model.

## Success Metrics

| Metric | Target |
| --- | --- |
| Lost follow-up reports | Reduce by 80%. |
| FIFO replay correctness | 100% in product tests. |
| Queue control coverage | 100% of queued items can be viewed and cancelled before replay. |
| Overlapping Runs caused by queue replay | 0. |
| Unsafe replay handling | 100% of non-clean Run endings pause replay. |
| Discoverability coverage | `/queue` and `/q` appear in help and slash-command suggestions. |

## Risks and Mitigations

| Risk | Mitigation |
| --- | --- |
| Users forget queued items exist | Keep queue count/list visible while items are pending. |
| Queued intent becomes stale | Pause replay after non-clean Run endings and allow cancellation. |
| Users expect normal sends to queue | Keep rejection guidance clear and point to `/queue`. |
| MVP feels weaker than competitor queues | Position edit/reorder as Phase 2, after core behavior is validated. |
| Queue replay feels surprising | Show item state before replay and preserve FIFO ordering. |

## Architecture Decision Records

- [ADR-001: Scope Queued Follow-Up Inputs V1](adrs/adr-001.md) — Use explicit `/queue` and `/q`, App-owned queue state, FIFO replay, and safe replay gating.
- [ADR-002: Select Explicit Queue-Next MVP For PRD](adrs/adr-002.md) — Use the focused queue-next MVP instead of rich queue management or hidden queueing.

## Open Questions

None for MVP.

## References

- Idea: `.compozy/tasks/queued-follow-up-inputs/_idea.md`
- GitHub Copilot SDK steering and queueing: https://docs.github.com/en/copilot/how-tos/copilot-sdk/use-copilot-sdk/steering-and-queueing
- Cursor queued and interrupting messages: https://cursor.com/changelog/1-4
- Replit Agent message queue: https://docs.replit.com/core-concepts/agent/message-queue
- Claude scheduled tasks idle behavior: https://code.claude.com/docs/en/scheduled-tasks
- Stack Overflow 2025 AI survey: https://survey.stackoverflow.co/2025/ai
