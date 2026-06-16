---
status: pending
title: "Read-only preview fold + transcript sanitization"
type: backend
complexity: medium
dependencies:
  - task_02
  - task_04
---

# Task 06: Read-only preview fold + transcript sanitization

## Overview
Build the read-only transcript a user previews before resuming: fold a chosen session's events through `ChatProjection::rebuild` into a `Vec<ChatItemView>`, deliberately skipping the live-step/approval/welcome overlays that the live `sync_chat_items` adds, and sanitize control/ANSI sequences so a hostile or malformed log can't disrupt the terminal.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST produce a session preview (`Vec<ChatItemView>`) purely from the session's history via `ChatProjection::rebuild`, WITHOUT applying `apply_live_steps`, `apply_pending_approval`, or the synthetic welcome item.
- MUST sanitize rendered transcript text by stripping/escaping terminal control and ANSI escape sequences.
- MUST read the session via `HistoryStore::open()` and fold faithfully, including the `run_interrupted`/`session_resumed` kinds from task_04.
- MUST NOT mutate any live `App` state (the preview is read-only).
</requirements>

## Subtasks
- [ ] 6.1 Add a preview builder that opens a session and returns its history-only projected items.
- [ ] 6.2 Ensure live/transient overlays and the welcome item are excluded from the preview.
- [ ] 6.3 Add transcript sanitization for control/ANSI sequences.
- [ ] 6.4 Add unit tests for isolation, fidelity, and sanitization.

## Implementation Details
Add a preview builder (e.g. in the chat module) that calls `HistoryStore::open()` (task_02) → `read_events()` → `ChatProjection::rebuild` (`src/app/chat/projection.rs:50`) → `items()`. Contrast with the live path `sync_chat_items` (`src/app/mod.rs:4267`), which prepends welcome and overlays transients — those must be skipped. Sanitization applies to `ChatLineView` text before render. See TechSpec "System Architecture" (preview = throwaway rebuild) and ADR-004 (untrusted transcript).

### Relevant Files
- `src/app/chat/projection.rs` — `rebuild` (`:50`), `items` (`:242`).
- `src/app/chat/mod.rs` — `ChatItemView` (`:10`), `ChatLineView` (`:68`).
- `src/app/mod.rs` — `sync_chat_items` (`:4267`) as the contrast (what NOT to apply).
- `src/history/mod.rs` — `open()`/`read_events` (task_02).

### Dependent Files
- `src/tui/mod.rs` — task_08 loads this preview off-thread and renders it.

### Related ADRs
- [ADR-004: Resume safety model](adrs/adr-004.md) — untrusted-transcript sanitization.
- [ADR-003: Production replay fold as a maintained schema-compatibility contract](adrs/adr-003.md) — first production use of the fold.
- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — preview is the confirmation/verification view.

## Deliverables
- A read-only preview builder returning sanitized history-only `ChatItemView`s.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: preview a recorded session and assert it matches a full rebuild **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Preview of a session equals `ChatProjection::rebuild(events).items()` (fidelity, zero desync).
  - [ ] Preview excludes the welcome item and any live-step/pending-approval overlay items.
  - [ ] A transcript line containing an ANSI escape / control sequence is rendered with that sequence stripped or escaped.
  - [ ] Building a preview does not alter the live `App`/projection state.
- Integration tests:
  - [ ] Record a multi-run session to a `tempdir`, build its preview, and assert the item sequence matches a direct fold of the on-disk log.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The preview is a faithful, sanitized, read-only render of a stored session, free of live-state artifacts.
