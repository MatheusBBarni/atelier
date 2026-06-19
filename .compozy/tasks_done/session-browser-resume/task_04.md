---
status: completed
title: "New lifecycle event kinds + projection fold handlers"
type: backend
complexity: medium
dependencies: []
---

# Task 04: New lifecycle event kinds + projection fold handlers

## Overview
Introduce the two additive event kinds the resume boundary needs — `run_interrupted` and `session_resumed` — and add explicit `ChatProjection::apply_history_event` handlers so they fold faithfully (the dangling run renders as interrupted; the resume boundary renders a visible divider). Because `HistoryEvent` is a `{kind, payload}` struct, this is purely additive with no schema bump.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define the `run_interrupted` payload (`{ run_id, prior_state }`) and `session_resumed` payload (`{ resumed_at, cwd, head_sha, dirty, prior_end_state, approval_mode, prior_tail_hash }`) as documented in the TechSpec "Data Models" section.
- MUST add `apply_history_event` fold arms for both kinds: `run_interrupted` renders/marks the run as interrupted; `session_resumed` renders a visible "Resumed" divider in the transcript.
- MUST keep `schema_version = 1` (new kinds are additive) and remain backward compatible — old logs without these kinds fold unchanged.
- MUST NOT yet emit these events (emission is task_10/task_11); this task only defines + folds them.
</requirements>

## Subtasks
- [x] 4.1 Define the kind strings + payload shapes (shared constants/helpers). — `RUN_INTERRUPTED_KIND`/`SESSION_RESUMED_KIND` consts + `RunInterruptedPayload`/`SessionResumedPayload` structs in `history/mod.rs`.
- [x] 4.2 Add the `run_interrupted` fold arm (mark the dangling run interrupted in the projection). — already routed: `run_interrupted` folds via the existing `apply_run_summary` arm (keyed by `Run{run_id}`, status `Interrupted`), which the `{run_id, prior_state}` payload tolerates. Verified by test; no new arm needed.
- [x] 4.3 Add the `session_resumed` fold arm (insert a resume divider chat item). — `apply_session_resumed` + routing arm.
- [x] 4.4 Add fold-fidelity tests including a legacy-shaped log fixture.

## Implementation Details
Add fold arms in `src/app/chat/projection.rs` `apply_history_event` (`:58`; today unknown kinds hit `_ => {}`). Define payloads near the other event helpers in `src/history/mod.rs`. The divider can reuse an existing `ChatItemView`/`ChatLineView` style (see `src/app/chat/mod.rs:10/68`). See TechSpec "Data Models" and ADR-002/ADR-008. Do not convert `HistoryEvent` to an enum.

### Relevant Files
- `src/app/chat/projection.rs` — `apply_history_event` (`:58`), `rebuild` (`:50`); add the two arms.
- `src/app/chat/mod.rs` — `ChatItemView` (`:10`), `ChatLineView` (`:68`) for the divider styling.
- `src/history/mod.rs` — `HistoryEvent` (`:12`); payload helpers.

### Dependent Files
- `src/app/mod.rs` — task_10/task_11 emit these events; task_06 preview folds them.

### Related ADRs
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — defines the two events and their role.
- [ADR-008: Lifecycle events as additive string-kinds + self-healing metadata cache](adrs/adr-008.md) — additive string-kind model + fold handlers.

## Deliverables
- `run_interrupted` and `session_resumed` kind/payload definitions.
- Projection fold handlers for both kinds.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: fold a synthetic multi-kind log including both new events **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Folding a log whose last run has no terminal event followed by a `run_interrupted` event renders that run as interrupted. — `dangling_run_then_run_interrupted_renders_interrupted`
  - [x] Folding a `session_resumed` event inserts exactly one resume divider item at the right position. — `session_resumed_inserts_exactly_one_resume_divider` (+ position asserted in the mixed-log test)
  - [x] A log containing none of the new kinds folds identically to before (backward compatibility). — `log_without_new_kinds_folds_unchanged`
  - [x] An unrecognized future kind still no-ops without panicking. — `unrecognized_future_kind_folds_to_no_item`
- Integration tests:
  - [x] `ChatProjection::rebuild` over a fixture log mixing existing kinds + `run_interrupted` + `session_resumed` produces the expected ordered `items()`. — `rebuild_over_mixed_log_with_resume_kinds_is_ordered`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The fold renders the new lifecycle events faithfully; old logs are unaffected; `schema_version` stays `1`.

## As-built notes
- Added `RUN_INTERRUPTED_KIND`/`SESSION_RESUMED_KIND` consts (for task_10/11 emitters) and typed `RunInterruptedPayload {run_id, prior_state}` / `SessionResumedPayload {resumed_at, cwd, head_sha, dirty, prior_end_state, approval_mode, prior_tail_hash}` in `history/mod.rs`. Additive string-kinds; `schema_version` stays `1`. Not emitted here (task_10/11).
- **`run_interrupted` needed no new projection arm:** it already routes to `apply_run_summary`, which keys by `Run{run_id}` and renders status `Interrupted` — exactly "mark the dangling run interrupted". The `{run_id, prior_state}` payload is tolerated (the handler reads `event.run_id` + optional `summary`/`reason`). A test confirms the dangling-run fold.
- **`session_resumed`** gets a new `apply_session_resumed` arm rendering a standalone "Session resumed" divider (no lifecycle key → one item per resume; reuses `ChatItemKind::RunSummary` + `Info` styling per the task's "reuse existing style", so no new chat kind and **no tui change** — staying within the task's file scope). The body surfaces prior state / cwd / a dirty-workspace note.
