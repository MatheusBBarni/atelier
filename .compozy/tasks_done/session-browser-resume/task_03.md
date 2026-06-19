---
status: completed
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
- [x] 3.1 Define `SessionSummary`.
- [x] 3.2 Enumerate sessions newest-first and load each via `open()` / metadata.
- [x] 3.3 Implement label fallback (goal → first prompt → timestamp+outcome).
- [x] 3.4 Derive outcome from the fold and self-heal the metadata cache.
- [x] 3.5 Add unit tests for ordering, label fallback, and self-heal.

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
  - [x] Three sessions created in sequence list newest-first (most recent ULID first). — `list_session_summaries_orders_newest_first_by_id` (asserts the reverse-lexicographic-by-id contract, deterministic regardless of same-ms ULID collisions)
  - [x] A session with a `session_goal_set` event uses the goal as its label. — `summary_label_uses_goal_when_set`
  - [x] A session with no goal uses its first `prompt_submitted` text (truncated) as its label. — `summary_label_falls_back_to_first_prompt` (also skips a leading-space secret prompt)
  - [x] A session with neither goal nor prompt falls back to a timestamp + outcome label. — `summary_label_falls_back_to_timestamp_and_outcome`
  - [x] A session whose `metadata.json` lacks `outcome` gets it derived from the log and the cache rewritten (self-heal). — `summary_self_heals_missing_outcome_cache_from_log`
  - [x] A corrupt/unreadable session directory is skipped without erroring the whole list. — `corrupt_session_is_skipped_without_failing_the_list`
- Integration tests:
  - [x] Building summaries over a `tempdir` `.atelier` with several recorded sessions returns correct labels/outcomes/order. — `list_session_summaries_over_multi_session_fixture`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The picker has a correct, newest-first, human-scannable list source with no goal/outcome desync against the log.

## As-built notes
- `SessionSummary { session_id, label, started_at, outcome: RunState, working_directory }` (Serialize/Deserialize/Eq) in `src/history/mod.rs`.
- `list_session_summaries(root)` enumerates `root/sessions/*`, sorts ids and reverses (ULID-like ids → newest-first), and `summarize_session` opens each via `HistoryStore::open` (skipping any that fail to open/parse — tolerant). It **folds the log every browse** (the authoritative source) for `goal`/`outcome` and self-heals the metadata cache on missing-or-mismatch (`update_metadata_cache`, log wins). A future optimization could trust a fresh cache; folding now guarantees no desync per req-4.
- `derive_outcome` uses `RunState::is_terminal()` (task_01) to keep only terminal run states (`run_completed/failed/limit_reached/interrupted` or a terminal `session_ended.run_state`); `Idle` when none. `derive_goal` folds `session_goal_set`/`_cleared`. Label fallback: goal → first non-secret `prompt_submitted` (truncated to 72 chars, leading-space secrets skipped, mirroring `project_prompt_history`) → `started_at · outcome`.
- Tests assert the **reverse-lexicographic-by-id** ordering contract (not creation order) so back-to-back same-millisecond ULIDs can't flake them.
