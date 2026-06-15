---
status: completed
title: "History reader and prompt projection"
type: backend
complexity: medium
dependencies: []
---

# History reader and prompt projection

## Overview

Add the read-only projection that powers recall: a primitive to enumerate every
session's event log under this project, and a function that folds `prompt_submitted`
events into a newest-first, deduped, capped, leading-space-filtered list. This is
the single data source for the whole feature (ADR-001) and adds no new persistence.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `list_session_event_paths(root)` enumerating `<root>/sessions/*/events.jsonl`, returning an empty list when `sessions/` is absent.
- MUST add `project_prompt_history(root, max)` that reads all session logs, keeps `prompt_submitted` events, extracts the `prompt` field, sorts by event `timestamp` descending, applies consecutive-dedup, and truncates to `max`.
- MUST exclude prompts whose text begins with a leading space (leading-space-skip) at read time.
- MUST tolerate unreadable or legacy-schema files by skipping them; one bad file MUST NOT fail the whole projection (note: `read_events_from_path` currently errors on `schema_version != 1`).
- SHOULD keep memory bounded to the capped result and avoid retaining full event objects longer than needed.
</requirements>

## Subtasks
- [x] 1.1 Add `list_session_event_paths` to the history module.
- [x] 1.2 Add `project_prompt_history` folding `prompt_submitted` payloads newest-first.
- [x] 1.3 Apply consecutive-dedup, leading-space-skip, and `max` truncation.
- [x] 1.4 Make per-file reads tolerant (skip on parse/schema error).
- [x] 1.5 Cover ordering, dedup, cap, skip, and tolerance with tests.

## Implementation Details

Add both functions to `src/history/mod.rs`, alongside `read_events_from_path`,
`HistoryStore`, and `clean_sessions`. Reuse `read_events_from_path` per file inside
a tolerant wrapper that swallows per-file errors. See TechSpec "Implementation
Design → Core Interfaces" (history primitives) and "Data Models" for the projection
invariants.

### Relevant Files
- `src/history/mod.rs` — home of `HistoryStore`, `HistoryEvent`, `read_events_from_path`, `clean_sessions`; the two new functions live here.

### Dependent Files
- `src/tui/mod.rs` — task_04's async loader will call `project_prompt_history`.

### Related ADRs
- [ADR-001: V1 Prompt History as Per-Project ↑/↓ Recall Projected from the Event Log](../adrs/adr-001.md) — recall is a projection over the event log; this implements that projection.
- [ADR-004: Asynchronous Background History Projection](../adrs/adr-004.md) — defines the read-all + timestamp-sort + dedup + cap + tolerance contract built here.

## Deliverables
- `list_session_event_paths` and `project_prompt_history` in the history module.
- Tolerant per-file reading that skips bad/legacy files.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test running the projection against a real `.multiagent/` produced by the `fake` runtime **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Two sessions with three prompts total → result is newest-first by event `timestamp`.
  - [ ] Same prompt twice consecutively → collapses to one entry; non-consecutive duplicates are preserved.
  - [ ] `max = 2` over five distinct prompts → returns exactly the two newest.
  - [ ] A prompt beginning with a single leading space → excluded from the result.
  - [ ] A session file with `schema_version = 2` or a malformed line → skipped; prompts from valid files still returned.
  - [ ] Missing `sessions/` directory → returns an empty `Vec`, no error.
- Integration tests:
  - [ ] Submit "alpha" then "beta" through the `fake` runtime, then `project_prompt_history` on the produced root returns `["beta", "alpha"]`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `project_prompt_history` returns a correct newest-first, deduped, capped, leading-space-filtered list
- A single bad or legacy session file never empties the result
