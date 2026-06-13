---
status: completed
title: "Route TUI Help Command Rows Through Catalog"
type: frontend
complexity: low
dependencies:
  - task_01
---

# Task 03: Route TUI Help Command Rows Through Catalog

## Overview
Update the TUI help modal so slash command rows are generated from the shared command catalog. This keeps the help surface aligned with the new dropdown and app unknown-command guidance while preserving non-command help content.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST use shared command metadata for TUI help command rows.
- MUST preserve existing non-command help rows such as keyboard shortcuts, scrolling, diagnostics, and maintenance commands.
- MUST keep `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/agent:`, `/skill:`, and `/reload:skills` visible in help.
- MUST NOT change command execution behavior or dropdown behavior in this task.
- SHOULD preserve the current help modal layout and readability.
</requirements>

## Subtasks
- [x] 3.1 Identify help modal rows that represent slash commands.
- [x] 3.2 Replace slash command row literals with catalog-derived rows.
- [x] 3.3 Preserve existing non-command rows and help modal styling.
- [x] 3.4 Update help modal tests to assert the fixed V1 command set remains visible.
- [x] 3.5 Confirm no command execution tests are affected.

## Implementation Details
Modify `render_help_modal` and related help test expectations in `src/tui/mod.rs`. Reference the TechSpec "Integration Points" section for the boundary: help consumes metadata, but TUI-local command handling remains unchanged.

### Relevant Files
- `src/tui/mod.rs` — Contains `render_help_modal` and `renders_help_modal_commands`.
- `src/slash_commands.rs` — Provides shared command metadata created by task_01.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines help as a catalog consumer.

### Dependent Files
- `src/app/mod.rs` — App guidance consumes the same catalog in task_02; help text must stay consistent.
- `README.md` — Should be reviewed for command list consistency after implementation.

### Related ADRs
- [ADR-003: Use Shared Metadata-Only Slash Command Catalog](adrs/adr-003.md) — Requires TUI help to consume shared command metadata.
- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) — Fixes the visible V1 command set.

## Deliverables
- TUI help command rows generated from shared metadata.
- Updated help modal tests.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for unchanged local help command behavior **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Help modal includes every fixed V1 command label.
  - [x] Help modal includes `/reload:skills`.
  - [x] Help modal still includes non-command rows like `Ctrl-L`, `Arrow keys`, and mouse wheel help.
  - [x] Help modal does not duplicate command rows.
- Integration tests:
  - [x] Existing `/help` local command test still toggles the modal without sending an app event.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- TUI help and shared catalog command metadata are aligned.
- Non-command help behavior and layout remain intact.
