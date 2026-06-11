---
status: completed
title: "Expose Pending Clarification View And Request Lifecycle"
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 04: Expose Pending Clarification View And Request Lifecycle

## Overview
Expose structured pending clarification state through `AppState` and record the request lifecycle when the orchestrator pauses for input. This gives Chat and TUI tasks an explicit app-owned source of truth instead of inferring state from raw events.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `PendingClarificationView` to `AppState`.
- MUST keep private paused-run resume context separate from the public view.
- MUST populate the public view when the orchestrator returns a valid waiting clarification decision.
- MUST record `clarification_requested` with question id, question text, options, recommended option id, and run id.
- MUST clear public pending clarification state on interrupt and when the run leaves `WaitingForUser`.
- MUST keep pending approval and pending clarification distinct even though both use `RunState::WaitingForUser`.
</requirements>

## Subtasks
- [x] 4.1 Add the public pending clarification view to app state.
- [x] 4.2 Populate pending clarification state from the validated orchestrator decision.
- [x] 4.3 Record explicit `clarification_requested` history events.
- [x] 4.4 Clear pending clarification state on interrupt and terminal state transitions.
- [x] 4.5 Update app state fixtures and tests to include the new field.

## Implementation Details
Use TechSpec sections "App Clarification State" and "History Events". This task should not implement answer submission; that belongs to task 05.

### Relevant Files
- `src/app/mod.rs` — Owns `AppState`, private `PendingClarification`, `handle_orchestrator_decision`, event recording, interrupt behavior, and app tests.
- `src/history/mod.rs` — Generic history storage that persists the new lifecycle event payload.
- `src/runtime/fake.rs` — Provides deterministic waiting clarification decisions from task 02.
- `src/tui/mod.rs` — Contains `AppState` test literals that must include the new optional field.

### Dependent Files
- `src/app/chat/projection.rs` — Task 06 consumes `clarification_requested` events.
- `src/tui/mod.rs` — Tasks 07 and 08 consume `state.pending_clarification`.
- `src/app/chat/mod.rs` — Task 06 adds clarification view semantics.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires app-owned pending clarification state and request lifecycle.
- [ADR-001: Scope Clarification Select UI](adrs/adr-001.md) — Requires the UI to project app state rather than own the source of truth.

## Deliverables
- `AppState.pending_clarification` with a public view model for TUI rendering.
- `clarification_requested` event recording when the run pauses.
- Updated interrupt and state-clearing behavior.
- Updated app and TUI test fixtures.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for fake-runtime pause state and history payloads **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] App state defaults to no pending clarification.
  - [x] Valid waiting clarification decision populates run id, question id, question, options, and recommended option id.
  - [x] Interrupt clears both private pending clarification and public pending clarification view.
  - [x] Pending approval state does not populate pending clarification view.
- Integration tests:
  - [x] Fake runtime `needs clarification` prompt records `clarification_requested` with expected question and options.
  - [x] Existing pending approval tests still see `pending_approval` and no pending clarification view.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- App state exposes pending clarification state for UI consumers.
- Session History records the clarification request before the answer path exists.
- Approval and clarification remain distinct app concepts.
