---
status: completed
title: Hook transcript projection
type: backend
complexity: low
dependencies:
  - task_01
---

# Task 6: Hook transcript projection

## Overview
Project `hook_started`/`hook_completed` events into the chat transcript so hook executions are visible (status, duration, exit code) rather than silent. This delivers the PRD's "transparency in transcript" feature and reuses the established event-projection pattern.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `ChatItemKind::HookInvocation` (or equivalently named) variant.
- MUST add `hook_started`/`hook_completed` handlers in `apply_history_event` that upsert a single chat item evolving across the hook's lifecycle (started → completed), keyed so the two events collapse into one item.
- MUST surface handler identity, action kind, status/outcome, duration, and exit code from the lifecycle payload (task_01).
- MUST update any exhaustive `match` on `ChatItemKind` (notably the TUI render path) to handle the new variant.
- MUST NOT let hook events re-enter dispatch (guaranteed by their exclusion from the public vocabulary in task_01).
</requirements>

## Subtasks
- [x] 6.1 Add the `ChatItemKind::HookInvocation` variant.
- [x] 6.2 Add `apply_hook_started`/`apply_hook_completed` and route them in `apply_history_event`.
- [x] 6.3 Collapse start→complete into one evolving chat item.
- [x] 6.4 Update the TUI render match for the new variant.
- [x] 6.5 Add unit tests for the projection and the render arm.

## Implementation Details
Edit `src/app/chat/projection.rs` (`apply_history_event`, `:58`) to add the two handlers, mirroring an existing pair (e.g. command_started/command_completed). Add the variant in `src/app/chat/mod.rs` (`ChatItemKind`, `:27`). Locate and update the `ChatItemKind` render match in `src/tui/` (the render path is a pure function of chat items) — confirm the exact site during implementation. Use the lifecycle payload type from task_01. See TechSpec "System Architecture → Projection".

### Relevant Files
- `src/app/chat/projection.rs:58` — `apply_history_event`; add the two handlers.
- `src/app/chat/mod.rs:27` — `ChatItemKind`; add the variant.
- `src/tui/` (render path) — exhaustive `ChatItemKind` match to extend (confirm exact file:line).

### Dependent Files
- `src/hooks/dispatch.rs` / `src/app/mod.rs` — emit the `hook_started`/`hook_completed` events this task projects (tasks 04/05).

### Related ADRs
- [ADR-003: Off-funnel hook dispatch](../adrs/adr-003.md) — hook events recorded for transparency; excluded from the public vocabulary (no recursion).
- [ADR-004: Normalized payload contract](../adrs/adr-004.md) — the lifecycle payload fields projected here.

## Deliverables
- A `HookInvocation` chat item evolving from started → completed with status/duration/exit code.
- Updated render handling for the new variant.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test: a recorded `hook_completed` event produces the expected chat item **(REQUIRED)**

## Tests
- Unit tests:
  - [x] A `hook_started` event creates a `HookInvocation` item in a running state with the handler/action labels. — `hook_started_creates_running_hook_invocation_item`
  - [x] A subsequent `hook_completed` for the same hook updates the SAME item to a completed state with duration + exit code (no duplicate item). — `hook_completed_collapses_into_one_item_with_duration_and_exit_code`
  - [x] A `hook_completed` with a non-zero exit code renders a failed/error status. — `hook_completed_with_nonzero_exit_renders_failed`
  - [x] The TUI render arm handles `ChatItemKind::HookInvocation` without panicking. — `tui::tests::hook_invocation_item_renders_without_panicking`
- Integration tests:
  - [x] Feeding a `hook_started`+`hook_completed` pair through `apply_history_event` yields one collapsed transcript item. — `hook_completed_collapses_into_one_item_with_duration_and_exit_code` (+ `distinct_hook_handlers_do_not_collapse`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Hook executions appear as a single evolving transcript item with status, duration, and exit code
- The render path compiles with the new exhaustive variant handled

## As-built notes
- New `ChatItemKind::HookInvocation` + a new `ChatLifecycleKey::Hook { run_id, handler_index, event }` whose `item_id` (`chat:hook:{run_id}:{handler_index}:{event}`) collapses the `hook_started`→`hook_completed` pair for one matched handler into a single evolving item.
- `apply_hook_started`/`apply_hook_completed` (routed in `apply_history_event`) deserialize the task_01 `HookLifecyclePayload`: started → Running/Info; completed `ok` → Completed/Success; completed non-`ok` → Failed/Error. Body surfaces `handler #N`, duration ms, exit code, and (on failure) the redacted excerpt.
- Two exhaustive `ChatItemKind` matches updated: `ChatItemKind::slug()` (`hook_invocation`) and the TUI `chat_kind_label()` (`hook`). `cargo check` passing confirms no other exhaustive match was missed.
- Recursion is structural (task_01): `hook_*` kinds are outside the public vocabulary, so projecting them never re-enters dispatch — task_05's `hook_lifecycle_events_are_recorded_into_history` test asserts exactly one `hook_completed`.
