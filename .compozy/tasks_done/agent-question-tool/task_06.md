---
status: completed
title: "Add Clarification Chat Semantics And Projection"
type: frontend
complexity: medium
dependencies:
  - task_04
  - task_05
---

# Task 06: Add Clarification Chat Semantics And Projection

## Overview
Add dedicated Chat semantics for clarifying questions so they no longer appear as approval-like items. This task satisfies the PRD requirement that users can distinguish product clarification from Action Approval.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ChatItemKind::Clarification`.
- MUST add `ChatItemStatus::WaitingForUser`.
- MUST project `clarification_requested` as a clarification Chat item with waiting-for-user status.
- MUST project `clarification_answered` as the completed answer state for the clarification lifecycle.
- MUST stop using approval Chat kind/status for clarifying questions.
- MUST preserve existing Action Approval projection behavior.
- SHOULD include question, options, recommended marker, answer source, and answer text in concise Chat body/details where useful.
</requirements>

## Subtasks
- [x] 6.1 Add clarification Chat kind and waiting-for-user status labels.
- [x] 6.2 Project pending clarification requests into a dedicated Chat item.
- [x] 6.3 Project clarification answers into completed clarification state.
- [x] 6.4 Update TUI Chat item kind labels for clarification.
- [x] 6.5 Preserve existing approval projection tests.
- [x] 6.6 Add clarification projection tests.

## Implementation Details
Use TechSpec sections "Chat", "History Events", and "Known Risks". This task should consume events produced by tasks 04 and 05, not create new app lifecycle behavior.

### Relevant Files
- `src/app/chat/mod.rs` — Defines Chat item kind, status, slugs, labels, and view model primitives.
- `src/app/chat/projection.rs` — Projects history events into Chat items and currently maps blockers to `RunSummary`/`WaitingApproval`.
- `src/tui/mod.rs` — Maps Chat item kinds to visible labels in the terminal UI.
- `src/app/mod.rs` — App tests already assert approval Chat semantics and will need clarification assertions.

### Dependent Files
- `src/tui/mod.rs` — Task 08 renders pending clarification composer layout alongside Chat state.
- `src/app/chat/command_summary.rs` — Uses `ChatItemStatus` matching and may need status exhaustiveness updates.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires dedicated Chat semantics for clarification.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — Requires clear distinction from Action Approval.

## Deliverables
- New Chat kind and status for clarifications.
- Chat projection for requested and answered clarification events.
- Updated TUI kind label for clarification Chat items.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Projection regression tests for both clarification and approval **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `clarification_requested` projects as `ChatItemKind::Clarification`.
  - [x] Pending clarification item status is `ChatItemStatus::WaitingForUser`.
  - [x] `clarification_answered` projects as completed clarification with answer source visible.
  - [x] Existing `approval_requested` still projects as `ChatItemKind::Approval`.
  - [x] Existing pending approval status remains `WaitingApproval`.
  - [x] Chat kind label for clarification renders a distinct label from approval.
- Integration tests:
  - [x] Fake runtime clarification request and answer produce Chat items that never use `ChatItemKind::Approval`.
- Test coverage target: >=80% ✓
- All tests must pass ✓

## Success Criteria
- All tests passing
- Test coverage >=80%
- Clarifying Questions are visually and semantically distinct from Action Approval in Chat.
- Approval behavior is unchanged.
- Chat projection consumes the lifecycle events produced by app tasks.
