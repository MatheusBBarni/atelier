---
status: completed
title: "Render Command Dropdown And Empty State"
type: frontend
complexity: medium
dependencies:
  - task_04
---

# Task 05: Render Command Dropdown And Empty State

## Overview
Render the command dropdown and compact no-match state above the TUI composer using the same visual approach as the existing agent and skill dropdowns. This task turns the model from task_04 into a visible user-facing command discovery surface.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render command suggestions above the input composer.
- MUST use the same general visual approach as existing agent and skill dropdowns.
- MUST show command labels and short descriptions.
- MUST render compact "No commands found" empty state when the command dropdown has no matches.
- MUST preserve existing agent and skill dropdown rendering.
- SHOULD reuse existing truncation and row-width patterns to avoid narrow-terminal overflow.
</requirements>

## Subtasks
- [x] 5.1 Add command dropdown rendering to the TUI render precedence.
- [x] 5.2 Add command suggestion row rendering with labels and descriptions.
- [x] 5.3 Add compact no-match empty-state rendering.
- [x] 5.4 Preserve existing agent and skill dropdown render output.
- [x] 5.5 Add render tests for matching, filtering, empty state, and layout constraints.

## Implementation Details
Modify `render`, dropdown rendering helpers, and render tests in `src/tui/mod.rs`. Reference the TechSpec "Integration Points" and "Known Risks" sections for the visual reuse and truncation requirements.

### Relevant Files
- `src/tui/mod.rs` — Contains render precedence, `render_agent_dropdown`, `render_skill_dropdown`, dropdown row styling, and text truncation helpers.
- `src/slash_commands.rs` — Provides command labels and descriptions.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines rendering and empty-state requirements.

### Dependent Files
- `src/app/mod.rs` — Not directly modified, but app state drives pending approval and run-state rendering conditions.
- `README.md` — May need later visual-behavior documentation review.

### Related ADRs
- [ADR-004: Scope Slash Dropdown Activation And Keyboard Semantics](adrs/adr-004.md) — Requires compact no-match state and existing dropdown visual approach.
- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) — Defines the V1 command discovery surface.

## Deliverables
- Visible command dropdown rendering for matching suggestions.
- Visible compact no-match state.
- Render tests proving command dropdown output and preserving existing dropdowns.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for TUI render precedence **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Rendering `/` shows a `Commands` dropdown with fixed V1 command labels.
  - [x] Rendering `/g` shows `/goal` and `/goal clear`.
  - [x] Rendering `/zz` shows `No commands found`.
  - [x] Command rows include descriptions without overflowing the input area.
  - [x] Existing agent dropdown render test still shows `Agents`.
  - [x] Existing skill dropdown render test still shows `Skills`.
- Integration tests:
  - [x] Help modal rendering still suppresses dropdown rendering while help is visible.
  - [x] Command dropdown renders only after agent and skill dropdowns are not active.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Command suggestions and no-match state are visible and readable.
- Existing agent and skill dropdown visuals remain stable.
