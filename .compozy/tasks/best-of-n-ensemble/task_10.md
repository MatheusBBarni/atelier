---
status: pending
title: "Race chat projection with live panes"
type: backend
complexity: high
dependencies:
  - task_02
  - task_07
---

# Task 10: Race chat projection with live panes

## Overview
Render the race in the transcript: a live multi-pane view of the competing attempts that collapses into a single verdict card showing each attempt's oracle result, the judge's rationale, the winning diff, and a low-confidence banner when applicable. This is the feature's visible "spectacle + legible verdict".

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `ChatItemKind::Race` variant and a `ChatLifecycleKey::Race` so all race events collapse into one evolving chat item.
- MUST route the `race_started` / `ensemble_attempt_verdict` / `race_selected` / `race_promoted` / `race_all_failed` event kinds to a race projection handler.
- The collapsed verdict card MUST show per-attempt oracle outcomes, the judge rationale, the winning diff summary, and a low-confidence banner when `low_confidence`.
- The live multi-pane view MUST degrade gracefully to roster/summary streaming where the terminal cannot render panes; the verdict card is the durable artifact.
- MUST use theme tokens only (no inline color literals) per the existing TUI invariant.
</requirements>

## Subtasks
- [ ] 10.1 Add `ChatItemKind::Race` + `ChatLifecycleKey::Race`.
- [ ] 10.2 Add the race event-kind routes + projection handler.
- [ ] 10.3 Render the collapsed verdict card (attempts, rationale, diff, banner).
- [ ] 10.4 Implement the live multi-pane view with graceful degradation.
- [ ] 10.5 Add projection tests feeding race events and asserting the single evolving item.

## Implementation Details
Mirror the grade-loop/plan single-evolving-item pattern (see TechSpec "System Architecture"). The projection dispatcher routes on `event.kind` strings; add the race kinds there and build the `ChatItemView` like the grade-loop handler. Respect the `colors_live_only_in_theme_module` invariant.

### Relevant Files
- `src/app/chat/mod.rs:30` — `ChatItemKind`; add `Race`.
- `src/app/chat/mod.rs:136` — `ChatLifecycleKey`; add `Race`.
- `src/app/chat/projection.rs:78` — event-kind dispatcher; route the race kinds.
- `src/app/chat/projection.rs:3672` — `grade_rounds_collapse_into_one_item` as the test/projection template.
- `src/tui/theme.rs` — theme tokens for the card/banner.

### Dependent Files
- Task 13 — the no-oracle banner + all-fail rendering build on this projection.
- `src/tui/mod.rs` — render path for the new item kind.

### Related ADRs
- [ADR-003: Live multi-pane + low-confidence banner](../adrs/adr-003.md) — the presentation decisions.
- [ADR-005: RunStepResult::Race](../adrs/adr-005.md) — the result the projection renders.

## Deliverables
- `ChatItemKind::Race` + `ChatLifecycleKey::Race` + projection handler.
- Live multi-pane view + collapsed verdict card with banner.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the projection lifecycle **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Feeding `race_started` + two `ensemble_attempt_verdict` + `race_selected` yields exactly one `ChatItemKind::Race` item.
  - [ ] The verdict card body contains each attempt's outcome and the winner's rationale.
  - [ ] A `low_confidence` selection renders the low-confidence banner.
  - [ ] `race_all_failed` renders a failed item with per-attempt failures and no winner.
  - [ ] No inline color literal is introduced outside the theme module (invariant test stays green).
- Integration tests:
  - [ ] A full race run projects a single evolving item from "running" through the final verdict card.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A race collapses into one evolving chat item with a legible verdict card.
- Multi-pane degrades gracefully; the theme invariant holds.
