---
status: pending
title: "Render Clarification Composer Layout"
type: frontend
complexity: medium
dependencies:
  - task_06
  - task_07
---

# Task 08: Render Clarification Composer Layout

## Overview
Render the pending clarification state as a select-style Input Composer with question text, 2-4 recommended options, a recommended marker, and an always-visible custom answer field. This task completes the user-facing TUI experience after app, Chat, and key-handling support exists.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render the active Clarifying Question in the Input Composer when `state.pending_clarification` is present.
- MUST render 2-4 recommended options with a visible selected state.
- MUST show the recommended option distinctly when `recommended_option_id` is present.
- MUST keep the custom answer text field visible below the option list.
- MUST keep the layout keyboard-oriented and avoid overlap at common terminal sizes.
- MUST keep Action Approval rendering distinct from clarification rendering.
- MUST keep interrupt availability visible or discoverable while the question is pending.
</requirements>

## Subtasks
- [ ] 8.1 Add clarification composer layout rendering.
- [ ] 8.2 Render selected and recommended option states.
- [ ] 8.3 Render always-visible custom answer field and cursor placement.
- [ ] 8.4 Preserve pending approval and normal input rendering.
- [ ] 8.5 Add render tests for common terminal sizes.
- [ ] 8.6 Add final regression coverage for full TUI-visible clarification flow.

## Implementation Details
Use TechSpec sections "TUI Composer Mode", "Chat Projection", and "Known Risks". This task depends on task 07 for interaction state and task 06 for Chat labels/statuses.

### Relevant Files
- `src/tui/mod.rs` — Owns rendering, input layout, cursor placement, pending approval prompt, dropdown rendering, and TUI tests.
- `src/app/mod.rs` — Provides `PendingClarificationView` through `AppState`.
- `src/app/chat/mod.rs` — Provides clarification status/kind labels consumed by TUI Chat rendering.
- `src/app/chat/projection.rs` — Provides clarification Chat items that should visually complement the composer state.

### Dependent Files
- `docs/tui-chat-improvements/prd.md` — Existing TUI Chat requirements should remain conceptually aligned with clarification rendering.
- `.compozy/tasks/agent-question-tool/_techspec.md` — Defines layout and interaction constraints for this feature.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires inline custom text and dedicated clarification UI semantics.
- [ADR-001: Scope Clarification Select UI](adrs/adr-001.md) — Requires the TUI to project pending clarification state.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — Keeps rendering focused on answer or interrupt, with no skip action.

## Deliverables
- Clarification composer rendering for question, options, selected option, recommended marker, and custom text.
- Cursor placement for the custom answer field.
- Regression-safe pending approval and normal input rendering.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- TUI render tests for layout and non-overlap **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Pending clarification renders the question text in the composer area.
  - [ ] Two recommended options render without overlap at 80x24.
  - [ ] Four recommended options render without overlap at 120x40.
  - [ ] The selected option has a visible marker distinct from unselected options.
  - [ ] The recommended option has a visible marker distinct from selection.
  - [ ] Custom answer field is always visible below the options.
  - [ ] Cursor lands in the custom answer field when custom text is active.
  - [ ] Pending approval rendering remains unchanged and does not show clarification labels.
- Integration tests:
  - [ ] Full fake-runtime pending clarification state renders Chat clarification context and composer answer controls together.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The TUI visibly presents Clarifying Questions as answer controls, not plain prompt text.
- The custom answer field remains available without opening a modal.
- Existing approval and normal prompt composer behavior remains intact.
