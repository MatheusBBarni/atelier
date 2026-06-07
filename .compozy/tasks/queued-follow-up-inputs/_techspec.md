# Technical Specification: Queued Follow-Up Inputs

Status: Draft
Date: 2026-06-07

## Executive Summary

Implement queued follow-up inputs as App-owned session state. `App::submit_prompt` parses `/queue <message>` and `/q <message>` before unknown slash-command rejection, stores queue items in `AppState`, records lightweight history events, and replays at most one queued prompt after each clean completed Run.

Primary trade-off: App ownership touches shared state, history, and Chat projection for a TUI-only feature, but it avoids hidden worker-channel behavior and keeps replay under the same lifecycle guard that starts normal Runs.

## System Architecture

### Component Overview

| Component | Responsibility |
| --- | --- |
| `App` | Own queue state, command parsing, cancellation, resume, replay gating, and history events. |
| `AppState` | Expose queue view data to the TUI and tests. |
| `ChatProjection` | Convert queue lifecycle events into visible Chat items. |
| `TUI` | Render queue state, expose cancel/resume commands, and show help/suggestions. |
| Runtime adapters | Unchanged; receive only normal prompts when a Run starts. |

Data flow:

1. User submits `/q follow up`.
2. TUI sends `AppEvent::PromptSubmitted`.
3. `App::submit_prompt` parses the queue command and appends a queue item.
4. `App` records `follow_up_queued` and publishes updated `AppState`.
5. After a clean `RunState::Completed`, `App` starts one queued prompt as a normal Run.
6. Non-clean endings pause replay and record `follow_up_replay_paused`.

## Implementation Design

### Core Interfaces

Repository code is Rust, so interface examples use Rust types.

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedFollowUpView {
    pub id: String,
    pub prompt: String,
    pub created_at: String,
    pub status: QueuedFollowUpStatus,
    pub pause_reason: Option<String>,
}
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueuedFollowUpStatus {
    Pending,
    Paused,
    Replaying,
    Cancelled,
}
```

`AppEvent` should add explicit queue controls:

```rust
pub enum AppEvent {
    PromptSubmitted(String),
    FollowUpCancelled(String),
    FollowUpResumeRequested(String),
    // existing variants...
}
```

### Data Models

Add to `AppState`:

- `queued_follow_ups: Vec<QueuedFollowUpView>`
- optional queue-level status if rendering needs a global paused reason later

Internal `App` state should use a `VecDeque<QueuedFollowUp>` to preserve FIFO semantics. The view can expose a cloned ordered `Vec`.

Each item stores:

- stable id from `new_id()`;
- prompt text after stripping `/queue` or `/q`;
- created timestamp or monotonic order;
- status;
- optional pause reason.

### Command Parsing

Add a small parser near existing slash-command helpers:

- accept `/queue <message>`;
- accept `/q <message>`;
- reject empty `/queue` or `/q` with usage guidance;
- do not treat plain `q` as a command;
- parse before `reject_unknown_slash_command`.

Normal prompt submission while active still rejects with improved guidance that points to `/queue`.

### Replay Rules

Replay only when all conditions are true:

- current Run ended with `RunState::Completed`;
- `active_run_id` is cleared or about to be cleared;
- no pending approval;
- no pending clarification;
- at least one queued item is `Pending`.

Replay behavior:

- mark the oldest pending item `Replaying`;
- record `follow_up_replay_started`;
- start it through the same normal Run creation path used by submitted prompts;
- replay at most one item per completed Run.

Pause behavior:

- failed, interrupted, limit-reached, waiting-for-user, approval-waiting, and clarification-waiting states pause the oldest pending item;
- record `follow_up_replay_paused` with a reason;
- require resume or cancel before that item can run.

### History And Chat Events

Add lightweight history event kinds:

- `follow_up_queued`
- `follow_up_cancelled`
- `follow_up_replay_started`
- `follow_up_replay_paused`
- `follow_up_replay_resumed`

`ChatProjection` can initially map these to `ChatItemKind::Diagnostic` or `ChatItemKind::UserPrompt`-adjacent items. A dedicated `ChatItemKind::QueuedFollowUp` is not required for MVP unless rendering becomes awkward.

### API Endpoints

No HTTP or external API endpoints are required.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
| --- | --- | --- | --- |
| `src/app/mod.rs` | Modified | Central lifecycle change; highest risk is accidental double-run replay. | Add queue state, parsing, replay gate, and tests. |
| `src/app/chat/mod.rs` | Modified | May need a queue-specific kind or can reuse diagnostics. | Prefer reuse unless rendering requires a new kind. |
| `src/app/chat/projection.rs` | Modified | Queue events must appear coherently in Chat. | Add projection handlers and tests. |
| `src/tui/mod.rs` | Modified | Render queue state and controls without destabilizing layout. | Add help text, suggestions, queue display, and command dispatch tests. |
| `src/orchestrator/mod.rs` | Unchanged | `RunState` already has needed terminal states. | No new state unless implementation proves necessary. |
| Runtime modules | Unchanged | Queue semantics must not leak into adapters. | No action. |

## Testing Approach

### Unit Tests

App-level tests:

- `/queue` and `/q` create queued items without starting a Run.
- empty `/queue` and `/q` return usage errors.
- normal prompt while active still rejects and points to `/queue`.
- FIFO replay starts one item after clean completion.
- cancellation removes or marks queued items before replay.
- failed, interrupted, limit-reached, approval, and clarification states pause replay.
- queued prompt replay records normal `prompt_submitted` for the new Run.

Chat projection tests:

- queued, cancelled, replay started, paused, and resumed events render visible Chat items.
- paused reason appears in the Chat item summary/body.

TUI tests:

- help includes `/queue` and `/q`.
- slash suggestions include both commands.
- queue list/count renders in running and paused states.
- cancel/resume controls dispatch the correct `AppEvent`.

### Integration Tests

Use existing fake runtime patterns:

- successful prompt followed by queued prompt;
- `needs clarification` leaves queue paused;
- `approval action` leaves queue paused until user resolves;
- `always parse error` or limit-reached cases do not replay queued items.

## Development Sequencing

### Build Order

1. Add queue data types and `AppState` view fields - no dependencies.
2. Add `/queue` and `/q` parsing in `App::submit_prompt` - depends on step 1.
3. Add queue lifecycle history events - depends on step 2.
4. Add replay and pause transitions around Run completion paths - depends on steps 1-3.
5. Add cancellation and resume `AppEvent` handling - depends on steps 1 and 3.
6. Add Chat projection for queue events - depends on step 3.
7. Add TUI rendering, help text, and slash suggestions - depends on steps 1, 5, and 6.
8. Add app, projection, and TUI tests - depends on steps 1-7.
9. Update README or user-facing command docs - depends on step 7.

### Technical Dependencies

- No new crates.
- No runtime adapter changes.
- No durable queue recovery across restarts.

## Monitoring and Observability

Record enough history fields to debug queue behavior:

- queue item id;
- status transition;
- prompt summary or redacted prompt text consistent with existing prompt history behavior;
- pause reason;
- replayed run id when available.

No alerting is required for MVP.

## Technical Considerations

### Key Decisions

- Decision: App owns queue state and replay.
  Rationale: App already owns Run lifecycle, history, and prompt submission.
  Trade-off: More shared-state work for a TUI MVP, but clearer lifecycle safety.

- Decision: Parse queue commands in `App::submit_prompt`.
  Rationale: Keeps command semantics consistent across submit paths.
  Trade-off: TUI cannot handle queue commands as purely local commands.

- Decision: Use lightweight history events projected into Chat.
  Rationale: Queue state should be visible and auditable in the same path as run events.
  Trade-off: Adds new event kinds.

### Known Risks

- Replay starts more than one Run: guard by replaying at most one item per clean completion.
- Queue item remains paused incorrectly: cover status transitions with focused tests.
- TUI layout becomes crowded: start with compact queue count/list and avoid modal complexity in MVP.
- Command metadata drifts: update help, suggestions, and unknown-command guidance together.

## Architecture Decision Records

- [ADR-001: Scope Queued Follow-Up Inputs V1](adrs/adr-001.md) — Use explicit `/queue` and `/q`, App-owned queue state, FIFO replay, and safe replay gating.
- [ADR-002: Select Explicit Queue-Next MVP For PRD](adrs/adr-002.md) — Use the focused queue-next MVP instead of rich queue management or hidden queueing.
- [ADR-003: App-Owned Queue State And Replay](adrs/adr-003.md) — Store queued follow-ups in App state, parse queue commands in App, record queue events, and replay through normal Run lifecycle rules.
