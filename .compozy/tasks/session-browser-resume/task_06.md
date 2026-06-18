---
status: completed
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
- [x] 6.1 Add a preview builder that opens a session and returns its history-only projected items. — `build_session_preview(root, session_id) -> SessionPreview` in the chat module.
- [x] 6.2 Ensure live/transient overlays and the welcome item are excluded from the preview. — `rebuild().items()` is inherently history-only (welcome/live/approval are added by `App::sync_chat_items`, never by `rebuild`); verified by test.
- [x] 6.3 Add transcript sanitization for control/ANSI sequences. — `sanitize_transcript_text` (CSI/OSC + C0/C1 controls stripped, tabs kept), applied to every rendered field via `sanitize_chat_item`.
- [x] 6.4 Add unit tests for isolation, fidelity, and sanitization.

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
  - [x] Preview of a session equals `ChatProjection::rebuild(events).items()` (fidelity, zero desync). — `preview_equals_a_direct_rebuild_of_the_log`
  - [x] Preview excludes the welcome item and any live-step/pending-approval overlay items. — `preview_excludes_welcome_and_live_overlays`
  - [x] A transcript line containing an ANSI escape / control sequence is rendered with that sequence stripped or escaped. — `sanitize_strips_ansi_and_control_sequences`
  - [x] Building a preview does not alter the live `App`/projection state. — `building_a_preview_does_not_mutate_a_live_projection`
- Integration tests:
  - [x] Record a multi-run session to a `tempdir`, build its preview, and assert the item sequence matches a direct fold of the on-disk log. — `preview_over_multi_run_session_matches_on_disk_fold` (incl. run_interrupted + session_resumed)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The preview is a faithful, sanitized, read-only render of a stored session, free of live-state artifacts.

## As-built notes
- `SessionPreview { session_id, items: Vec<ChatItemView> }` + `build_session_preview(root, session_id)` live in `src/app/chat/mod.rs`. The builder opens the session (`HistoryStore::open`, task_02), reads events, `ChatProjection::rebuild`s, and sanitizes — never touching live `App`/projection state (it owns a throwaway projection).
- History-only is **free**: `rebuild().items()` is the pure fold; the welcome item and live-step/pending-approval overlays are only ever added by `App::sync_chat_items`, so simply not calling that path yields the history-only preview.
- `sanitize_transcript_text` strips full CSI (`ESC [ … final`) and OSC (`ESC ] … BEL|ST`) sequences plus C0/C1 control chars (keeping `\t`); `sanitize_chat_item` applies it to title, summary, body lines, and detail labels. Both `pub` for reuse by task_08's render path.
