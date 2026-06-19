---
status: pending
title: Finding feedback (thumbs up/down)
type: frontend
complexity: medium
dependencies:
  - task_07
---

# Task 08: Finding feedback (thumbs up/down)

## Overview
Let a developer rate a surfaced finding (👍/👎) to capture the precision and actioned-catch signal the PRD measures, without re-running the review or mutating run state. This is the lightweight feedback loop that later earns the V2 auto-trigger (ADR-006/001).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST emit a `review_finding_rated` event ({finding reference, rating}) when the user rates a finding.
- MUST wire TUI key routing to rate the focused finding within a `ReviewRound` item, consistent with the existing key-routing precedence.
- MUST NOT re-dispatch the reviewer or mutate run/file state when a rating is recorded.
- SHOULD reflect the rating on the rendered finding (e.g. a marker) via the projection.
</requirements>

## Subtasks
- [ ] 08.1 Define the `review_finding_rated` event and a stable finding reference.
- [ ] 08.2 Add TUI key handling to rate the focused finding.
- [ ] 08.3 Record the rating event without any state mutation.
- [ ] 08.4 Reflect the rating in the projection.
- [ ] 08.5 Unit/integration test rating capture and the no-side-effect guarantee.

## Implementation Details
Modify `src/tui/mod.rs` (key-routing precedence and focus handling), `src/app/mod.rs` (`record_event` for the rating), and `src/app/chat/projection.rs` (reflect the rating on the finding line). The finding reference is derived from the recorded `review_finding` events (task_07). See TechSpec "Feedback" and "Command & Event Surface".

### Relevant Files
- `src/tui/mod.rs` — key-routing precedence and item/finding focus.
- `src/app/mod.rs` — `record_event` for `review_finding_rated`.
- `src/app/chat/projection.rs` — reflect the rating marker on the finding (task_07 item).

### Dependent Files
- Monitoring/observability consumers of `review_finding_rated` (the precision metric).

### Related ADRs
- [ADR-006: Lightweight feedback on structured findings](../adrs/adr-006.md).
- [ADR-001: Advisory posture; measure precision before forcing](../adrs/adr-001.md).

## Deliverables
- `review_finding_rated` event + TUI rating interaction + projection reflection.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for the rating flow **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Rating a finding emits `review_finding_rated` with the correct finding reference and rating value.
  - [ ] Recording a rating does not change `run_state` or any file (no side effects).
- Integration tests:
  - [ ] In a projected `ReviewRound`, a 👍 then a 👎 on a finding records two rating events and updates the finding's rendered rating marker.
  - [ ] Rating a finding mid-session does not trigger a new review dispatch.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A finding can be rated from the TUI, emitting `review_finding_rated` with no state mutation
- The rating is reflected on the rendered finding
