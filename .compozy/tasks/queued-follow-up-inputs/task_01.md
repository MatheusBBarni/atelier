---
status: completed
title: "Add App Queue State And Command Parsing"
type: backend
complexity: medium
dependencies: []
---

# Task 01: Add App Queue State And Command Parsing

## Overview
Add the app-owned queue data model and parse explicit `/queue <message>` plus `/q <message>` commands before unknown slash-command rejection. This task establishes the queue as product state without adding replay behavior yet.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add app-owned queued follow-up view data to `AppState`.
- MUST add internal FIFO queue state that can preserve insertion order.
- MUST parse `/queue <message>` and `/q <message>` in `App::submit_prompt` before unknown slash-command rejection.
- MUST reject empty `/queue` and `/q` with clear usage guidance.
- MUST NOT start a Run when a queue command is accepted.
- MUST preserve normal prompt submission and existing `/goal`, `/config`, `/subtask`, `/agent:`, and `/skill:` behavior.
</requirements>

## Subtasks
- [x] 1.1 Add queued follow-up status and view data types.
- [x] 1.2 Add queue storage to `App` and queue view data to `AppState`.
- [x] 1.3 Add parser support for `/queue <message>` and `/q <message>`.
- [x] 1.4 Wire accepted queue commands into `App::submit_prompt`.
- [x] 1.5 Add usage errors for empty queue commands.
- [x] 1.6 Add app-level tests for parsing and state updates.

## Implementation Details
Modify the app layer first. Follow the TechSpec "Core Interfaces", "Data Models", and "Command Parsing" sections for fields and behavior. Do not implement replay, pause, cancellation, resume, Chat projection, or TUI rendering in this task.

### Relevant Files
- `src/app/mod.rs` — Contains `AppState`, `AppEvent`, `App`, `submit_prompt`, slash-command handlers, and app tests.
- `src/ids.rs` — Provides stable id generation used throughout the app.
- `src/history/mod.rs` — Provides timestamped history events; useful as the source pattern for queue timestamps.
- `.compozy/tasks/queued-follow-up-inputs/_techspec.md` — Defines the app-owned queue model and command parsing boundary.

### Dependent Files
- `src/tui/mod.rs` — Later tasks render `AppState` queue data and dispatch queue controls.
- `src/app/chat/projection.rs` — Later tasks project queue lifecycle events into Chat.
- `README.md` — Later documentation should describe the accepted commands.

### Related ADRs
- [ADR-001: Scope Queued Follow-Up Inputs V1](adrs/adr-001.md) — Fixes explicit queue-command-only scope.
- [ADR-003: App-Owned Queue State And Replay](adrs/adr-003.md) — Requires queue state and command parsing to live in `App`.

## Deliverables
- Queue status and view model exposed through `AppState`.
- Internal FIFO queue storage in `App`.
- `/queue` and `/q` command parsing in `App::submit_prompt`.
- Usage errors for empty queue commands.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for app prompt submission behavior **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Submitting `/queue update docs` creates one pending queued follow-up and does not create `run_started`.
  - [x] Submitting `/q update docs` creates one pending queued follow-up and preserves prompt text after the alias.
  - [x] Submitting `/queue` returns usage guidance and leaves the queue empty.
  - [x] Submitting `/q` returns usage guidance and leaves the queue empty.
  - [x] Submitting plain `q` remains a normal prompt path and is not parsed as a queue command.
- Integration tests:
  - [x] Existing `/goal`, `/config`, and `/subtask` tests still pass.
  - [x] Existing `/agent:` and `/skill:` prompt-prefix tests still pass.
  - [x] Existing unknown slash-command test still rejects `/doctor`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Queue commands create visible app state without starting a Run.
- Empty queue commands fail with clear usage guidance.
- Existing command and prompt submission behavior remains unchanged.
