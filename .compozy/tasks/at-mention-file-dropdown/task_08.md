---
status: pending
title: Render the file-mention dropdown
type: frontend
complexity: medium
dependencies:
  - task_06
---

# Task 08: Render the file-mention dropdown

## Overview
Render the file-mention dropdown as an upward overlay above the composer, with matched-character highlighting, a folder affordance, row capping, truncation, and the "No matching files" no-match row. This is the visual layer over the task_06 model.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `render_file_mention_dropdown(frame, input_area, &FileMentionDropdown, &Theme)` mirroring `render_skill_dropdown`.
- MUST open upward above the input (Clear + List/Block) and cap visible rows at `DROPDOWN_MAX_ITEMS`.
- MUST highlight the matched characters using each suggestion's match offsets.
- MUST show a folder affordance (a trailing `/`) so folders are visually distinct from files.
- MUST render a single "No matching files" row when the model is `empty`.
- MUST truncate long paths to the available width and MUST NOT render while the help overlay is visible.
</requirements>

## Subtasks
- [ ] 8.1 Add the render function with the upward overlay.
- [ ] 8.2 Apply the selected-row highlight.
- [ ] 8.3 Emphasize the matched characters from the offsets.
- [ ] 8.4 Show the folder trailing-slash affordance.
- [ ] 8.5 Render the "No matching files" row for the empty state.
- [ ] 8.6 Add render tests via the test backend.

## Implementation Details
Modify `src/tui/mod.rs`, mirroring `render_skill_dropdown` (overlay, list, block) and the command dropdown's no-match row. Use the match offsets from the suggestion to style matched characters. See TechSpec "Component Overview". Do NOT add the render-chain branch here — that is task_09.

### Relevant Files
- `src/tui/mod.rs` — `render_skill_dropdown`, `render_command_dropdown` (no-match row), `truncate_to_char_width`, `DROPDOWN_MAX_ITEMS`, and the theme/selection styling helpers.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Component Overview".

### Dependent Files
- `src/tui/mod.rs` — task_09 adds the render-chain branch that calls this function.

### Related ADRs
- [ADR-005: Component Placement and Dropdown Integration](../adrs/adr-005.md) — render placement.
- [ADR-002: Package as a Complete Single-Release V1](../adrs/adr-002.md) — highlighting/recents are V1 (not deferred).

## Deliverables
- `render_file_mention_dropdown` in `src/tui/mod.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests via the render backend **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A dropdown with two suggestions renders both paths above the input area.
  - [ ] The selected row shows the selection marker/highlight.
  - [ ] The matched characters for a query are emphasized in the rendered row.
  - [ ] A folder suggestion renders with a trailing `/`.
  - [ ] An `empty` model renders the single "No matching files" row.
  - [ ] More than six suggestions render at most six rows.
  - [ ] A path longer than the width is truncated.
- Integration tests:
  - [ ] Rendering a realistic `FileMentionDropdown` on an 80x24 test backend produces the expected overlay text and highlight layout.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Overlay, highlight, folder affordance, no-match row, capping, and truncation render correctly
