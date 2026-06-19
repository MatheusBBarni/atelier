---
status: completed
title: Chat projection for trust & floor events
type: backend
complexity: medium
dependencies:
  - task_05
---

# Task 06: Chat projection for trust & floor events

## Overview
Make trust and floor activity visible in the transcript by projecting the new events into chat items and adding the risk tier to the existing Approval item. This is what fulfills the PRD's "auditable auto-approvals" and "would-have-blocked" visibility — without it, trust and warn-only actions happen invisibly.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST project `approval_auto_resolved`, `floor_warned`, `trust_granted`, `trust_revoked`, and `trust_cleared` into chat items via `apply_history_event`.
- MUST reuse existing `ChatItemKind` variants (e.g., `Diagnostic`/`Approval`) and `ChatSeverity` rather than adding new kinds (YAGNI); `floor_warned` uses `Warning`, trust events use `Info`.
- MUST add the risk tier to the pending `Approval` item in `apply_pending_approval` so the transcript shows it.
- MUST render targets/reasons in human-readable terms (the command or path), never an opaque key.
- MUST tolerate events missing the newer fields (older sessions) without panicking.

## Subtasks
- [x] 06.1 Add `apply_history_event` arms for the five new event kinds.
- [x] 06.2 Map each to a `ChatItemView` with the right kind/severity and human-readable text.
- [x] 06.3 Add the tier to the projected `Approval` item.
- [x] 06.4 Add projection unit tests for each event and the tiered approval item.

## Implementation Details
Work in `src/app/chat/projection.rs`: `apply_history_event` (~58, dispatch on `event.kind`), `apply_pending_approval` (~192), and the existing `apply_approval_requested`/`apply_approval_resolved` arms (~72–73). Use `ChatItemView`/`ChatItemKind`/`ChatSeverity`/`ChatLineStyle` from `src/app/chat/mod.rs` (~11–85). See TechSpec "System Architecture → Component Overview (projection)" and "Data Models (event payloads)".

### Relevant Files
- `src/app/chat/projection.rs` — projection dispatch and approval item building.
- `src/app/chat/mod.rs` — `ChatItemView`, `ChatItemKind`, `ChatSeverity`, `ChatLineStyle`.

### Dependent Files
- `src/tui/mod.rs` (task_07) — renders the projected approval item, including the tier.

### Related ADRs
- [ADR-001: V1 scope — fail-closed destructive floor](../adrs/adr-001.md) — auditability of auto-approvals.
- [ADR-004: In-memory exact-target session trust](../adrs/adr-004.md) — trust events shown in transcript.

## Deliverables
- Projection arms for the five new events + tier on the Approval item.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test replaying an event sequence into chat items **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `approval_auto_resolved { target }` → a `Diagnostic` item, `Info`, text naming the trusted target.
  - [x] `floor_warned { tier, reason }` → a `Diagnostic` item, `Warning`, text containing the reason and a "warn-only" marker.
  - [x] `trust_granted`/`trust_revoked` → `Info` items naming the target; `trust_cleared { count }` names the count.
  - [x] `apply_pending_approval` produces an `Approval` item whose body includes the tier label.
  - [x] An `approval_requested` event missing the `risk`/`tier` field projects without panic.
- Integration tests:
  - [x] Replaying a recorded sequence (approval_requested → floor_warned → trust_granted) yields the expected ordered `ChatItemView` set.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Every trust/floor event surfaces a human-readable transcript item; the Approval item shows its tier; older events still project.
