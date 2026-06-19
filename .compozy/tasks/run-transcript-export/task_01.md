---
status: pending
title: "session_exported event kind and pub(crate) private-file writer"
type: backend
complexity: low
dependencies: []
---

# Task 01: session_exported event kind and pub(crate) private-file writer

## Overview
Add the `SESSION_EXPORTED_KIND` event constant and promote the module-private `write_private_file`/`set_private_file_permissions` helpers to `pub(crate)`, so the export feature can append its audit event and write the out-of-`.atelier` transcript with the same owner-only (`0600`) guarantee. Pure enabling plumbing that tasks 04–06 build on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `pub const SESSION_EXPORTED_KIND: &str = "session_exported";` alongside the other kind constants in `src/history/mod.rs`, keeping `schema_version` at 1 (additive — no schema bump).
- MUST change `write_private_file` and `set_private_file_permissions` from private to `pub(crate)` with NO behavior change (`0700` dir / `0600` file).
- MUST NOT alter the existing record-time redaction or the `append_event` path.
- The new kind MUST remain audit-only — it MUST fall through the projection's `_ => {}` arm and produce no chat item.
</requirements>

## Subtasks
- [ ] 1.1 Add `SESSION_EXPORTED_KIND` next to the existing kind constants.
- [ ] 1.2 Promote `write_private_file` and `set_private_file_permissions` to `pub(crate)`.
- [ ] 1.3 Confirm the projection ignores the new kind (audit-only) and add a regression test.
- [ ] 1.4 Add a unit test asserting a file written via the helper has mode `0600`.

## Implementation Details
Add the constant and relax two visibilities in `src/history/mod.rs`; verify the projection match in `src/app/chat/projection.rs`. See TechSpec "Data Models" (the `session_exported` payload) and "System Architecture". Do not introduce a new module.

### Relevant Files
- `src/history/mod.rs` — kind constants block (~189-221), `write_private_file`/`set_private_file_permissions` (~1001-1022), `HistoryEvent::new`.
- `src/app/chat/projection.rs` — `apply_history_event` match; confirm `_ => {}` covers the new kind.

### Dependent Files
- `src/export.rs` (task_04) — consumes the constant and the `pub(crate)` writer.

### Related ADRs
- [ADR-001: V1 scope and redaction-security architecture](../adrs/adr-001.md) — the `session_exported` audit event.
- [ADR-003: Component architecture](../adrs/adr-003.md) — promoting the single 0600 writer instead of duplicating it.

## Deliverables
- `SESSION_EXPORTED_KIND` constant in `src/history/mod.rs`.
- `write_private_file` / `set_private_file_permissions` as `pub(crate)`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration coverage of the appended event is provided transitively by task_06 **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `write_private_file` writes a file with Unix mode `0600` (parent dir `0700`) at a `tempfile` path.
  - [ ] A `HistoryEvent` with kind `session_exported` applied to `ChatProjection::rebuild` yields zero new chat items (audit-only).
  - [ ] `SESSION_EXPORTED_KIND == "session_exported"`.
- Integration tests:
  - [ ] (covered in task_06) a `session_exported` event written via `append_event` round-trips through `read_events`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Constant added; both helpers `pub(crate)`; existing redaction/append behavior unchanged
- Projection produces no chat item for the new kind
