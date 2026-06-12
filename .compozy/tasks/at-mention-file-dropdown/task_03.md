---
status: completed
title: FileIndex fuzzy query and ranking
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 03: FileIndex fuzzy query and ranking

## Overview
Add fuzzy querying to `FileIndex`: score the walked entries with `nucleo-matcher`, rank them with the recency/depth blend, cap the result count, and return matched-character offsets for highlighting. This turns the raw candidate list into the ranked suggestions the dropdown shows.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `FileSuggestion` carrying the relative path, an is-directory flag, and the matched character offsets (per TechSpec "Core Interfaces").
- MUST score the cached `FileEntry` list with `nucleo-matcher` for a non-empty query.
- MUST rank: for an empty query, most-recently-modified first, then shallower path, then alphabetical; for a non-empty query, fuzzy score descending, then shallower path, then most-recently-modified, then alphabetical (per ADR-004).
- MUST cap results to a caller-provided limit (the dropdown passes 6).
- MUST return the matched character offsets so the renderer can highlight them.
- MUST perform matching synchronously and reuse a single matcher instance.
</requirements>

## Subtasks
- [x] 3.1 Define `FileSuggestion` with match offsets.
- [x] 3.2 Implement the empty-query (recents) ordering.
- [x] 3.3 Implement fuzzy scoring with `nucleo-matcher`.
- [x] 3.4 Implement the non-empty ranking blend and the result cap.
- [x] 3.5 Collect and return matched character offsets.
- [x] 3.6 Add unit tests for ordering, cap, and highlight offsets.

## Implementation Details
Extend `src/file_index.rs` with `FileSuggestion` and the query function. See TechSpec "Core Interfaces" (`FileSuggestion`, `FileIndex::query`) and ADR-004 for the exact ranking blend. Keep the matcher reusable across calls; the candidate list is already in memory from task_02.

### Relevant Files
- `src/file_index.rs` — extended with `FileSuggestion` and the query logic (built on task_02's `FileEntry`).
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Core Interfaces" for the query signature.

### Dependent Files
- `src/tui/mod.rs` — task_06's activation calls the query to build dropdown suggestions.

### Related ADRs
- [ADR-004: Fuzzy Matching via nucleo-matcher with a Ranking Blend](../adrs/adr-004.md) — matcher choice and ranking order.

## Deliverables
- `FileSuggestion` and the query function in `src/file_index.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests over a known candidate set **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] An empty query returns up to the limit, most-recently-modified first.
  - [ ] Query `rcm` ranks `src/runtime/claude.rs` above a deeper coincidental subsequence match.
  - [ ] On equal fuzzy score, the shallower path ranks first.
  - [ ] The result count never exceeds the provided limit.
  - [ ] `match_indices` identify exactly the characters matched for `tuimod` against `src/tui/mod.rs`.
  - [ ] A query matching nothing returns an empty vector.
  - [ ] Matching is case-insensitive (`CLAUDE` matches `claude.rs`).
- Integration tests:
  - [ ] Building entries from a known tree and querying a fragment returns an ordered, capped, highlight-annotated suggestion list.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Ranking, result cap, and highlight offsets behave per ADR-004
