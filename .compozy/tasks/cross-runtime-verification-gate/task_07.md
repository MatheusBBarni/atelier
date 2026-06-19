---
status: pending
title: ReviewRound chat lifecycle
type: backend
complexity: medium
dependencies:
  - task_03
  - task_06
---

# Task 07: ReviewRound chat lifecycle

## Overview
Render the review as one evolving transcript item by adding a coalescing `ReviewRound` chat lifecycle, mirroring the existing `GradeLoop` pattern. Without this, the new review events fall through the projection's `_ => {}` arm and render nothing (ADR-006).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ChatItemKind::ReviewRound` (with its `slug()` arm) and `ChatLifecycleKey::Review { run_id }` (with its `item_id()` arm).
- MUST add a `review_key(event)` helper and dispatch arms for `review_started`/`review_finding`/`review_completed`/`review_skipped` in `apply_history_event`.
- MUST implement `apply_review_round` (mirroring `apply_grade_round`) that coalesces all events for a run into ONE item via `upsert`, appending finding lines and recomputing status/severity.
- MUST render the provenance header (reviewer family + producer-family set), a leading severity tally, high-confidence-first ordering, and progressive-disclosure rationale.
- MUST render `review_skipped` as a clear, visible SKIP item.
- MUST update any `ChatItemKind` / `ChatLifecycleKey` variant-enumeration or drift tests.
</requirements>

## Subtasks
- [ ] 07.1 Add the `ReviewRound` item kind and `Review` lifecycle key (with `item_id` and `slug`).
- [ ] 07.2 Add the `review_key` helper and the projection dispatch arms.
- [ ] 07.3 Implement `apply_review_round` coalescing via `upsert`.
- [ ] 07.4 Render the header, tally, high-confidence-first ordering, and the SKIP item.
- [ ] 07.5 Update variant-drift tests and add projection tests.

## Implementation Details
Modify `src/app/chat/mod.rs` (`ChatItemKind` `:28`, `ChatLifecycleKey` `:134`, `item_id` `:248`, `slug` `:304`) and `src/app/chat/projection.rs` (dispatch `:77`; `apply_grade_round` `:1842` as the template; `grade_key` `:2514`; `upsert` `:2006`). See TechSpec "Review chat item".

### Relevant Files
- `src/app/chat/mod.rs` — item-kind/lifecycle-key enums and their `item_id`/`slug` arms.
- `src/app/chat/projection.rs` — `apply_history_event` dispatch, `apply_grade_round`/`grade_key`/`upsert` templates.
- `src/review/mod.rs` — `ReviewFinding` (task_03) for finding-line formatting.

### Dependent Files
- `src/app/chat/projection.rs` tests and any test enumerating chat variants.
- Feedback (task_08) attaches its rating marker to the rendered finding.

### Related ADRs
- [ADR-006: ReviewRound lifecycle coalescing review events](../adrs/adr-006.md).
- [ADR-002: Finding surfacing — tally, provenance, progressive disclosure](../adrs/adr-002.md).

## Deliverables
- `ReviewRound` item kind + `Review` lifecycle key + `apply_review_round` projection arm.
- Updated variant-drift tests.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for the coalesced review item **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A `review_started` event opens exactly one `ReviewRound` item.
  - [ ] Two `review_finding` events append into the SAME item (one `ChatLifecycleKey::Review`), not two items.
  - [ ] A `review_completed` event sets the item to a terminal status.
  - [ ] A `review_skipped` event renders a visible SKIP item naming the producer families.
  - [ ] Findings render high-confidence-first with a correct leading tally; the header shows reviewer vs producer families.
- Integration tests:
  - [ ] An end-to-end `/review` run (fake runtime) projects exactly one `ReviewRound` item containing the scripted findings.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Review events coalesce into one evolving `ReviewRound` item; SKIP renders clearly
- Findings render high-confidence-first with a provenance header and tally
