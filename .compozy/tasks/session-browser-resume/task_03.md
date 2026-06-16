---
status: pending
title: "Session summaries (SessionSummary + list_session_summaries)"
type: backend
complexity: medium
dependencies:
  - task_01
  - task_02
---

# Task 03: Session summaries (SessionSummary + list_session_summaries)

## Overview
Produce the newest-first rows the picker renders: a `SessionSummary { session_id, label, started_at, outcome, working_directory }` and a tolerant `list_session_summaries(root)` that reads each session's metadata, self-heals the goal/outcome cache from the log when missing or stale, and orders newest-first. This is the data layer behind the browse list.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define `SessionSummary` with session id, label, started-at timestamp, outcome (`RunState`), and working directory.
- MUST order results newest-first by leveraging the ULID-ordered session ids (reverse of the lexicographic path order).
- MUST derive the label as: session `goal` if set, else the session's first `prompt_submitted` text (truncated), else a timestamp + outcome string.
- MUST derive `outcome` from the folded log (last terminal run / `session_ended`), using `RunState::is_terminal()`, and self-heal the metadata cache when it is missing or disagrees with the log (log wins).
- MUST be tolerant: skip unreadable/corrupt sessions without failing the whole list.
</requirements>

## Subtasks
- [ ] 3.1 Define `SessionSummary`.
- [ ] 3.2 Enumerate sessions newest-first and load each via `open()` / metadata.
- [ ] 3.3 Implement label fallback (goal → first prompt → timestamp+outcome).
- [ ] 3.4 Derive outcome from the fold and self-heal the metadata cache.
- [ ] 3.5 Add unit tests for ordering, label fallback, and self-heal.

## Implementation Details
Work in `src/history/mod.rs`, reusing `list_session_event_paths` (`:294`, lexicographic = chronological for ULIDs) and the prompt-extraction approach from `project_prompt_history` (`:331`) for the first-prompt fallback. Outcome derivation folds events (or reads the cached `outcome`, healing on mismatch). See TechSpec "Core Interfaces"/"Data Models" and ADR-008.

### Relevant Files
- `src/history/mod.rs` — `list_session_event_paths` (`:294`), `project_prompt_history` (`:331`), `SessionMetadata`, `open()` (task_02).
- `src/orchestrator/mod.rs` — `RunState` + `is_terminal()` (task_01) for outcome.

### Dependent Files
- `src/tui/mod.rs` — task_07 renders `Vec<SessionSummary>` in the picker.
- `src/app/mod.rs` — task_13 uses the newest summary's outcome for the post-crash hint.

### Related ADRs
- [ADR-008: Lifecycle events as additive string-kinds + self-healing metadata cache](adrs/adr-008.md) — outcome derivation + self-heal; label fallback to first prompt.
- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — list columns (label/timestamp/outcome); no fuzzy.

## Deliverables
- `SessionSummary` type.
- `list_session_summaries(root) -> Vec<SessionSummary>` (newest-first, tolerant, self-healing).
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test over a multi-session `.atelier` fixture **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Three sessions created in sequence list newest-first (most recent ULID first).
  - [ ] A session with a `session_goal_set` event uses the goal as its label.
  - [ ] A session with no goal uses its first `prompt_submitted` text (truncated) as its label.
  - [ ] A session with neither goal nor prompt falls back to a timestamp + outcome label.
  - [ ] A session whose `metadata.json` lacks `outcome` gets it derived from the log and the cache rewritten (self-heal).
  - [ ] A corrupt/unreadable session directory is skipped without erroring the whole list.
- Integration tests:
  - [ ] Building summaries over a `tempdir` `.atelier` with several recorded sessions returns correct labels/outcomes/order.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The picker has a correct, newest-first, human-scannable list source with no goal/outcome desync against the log.
