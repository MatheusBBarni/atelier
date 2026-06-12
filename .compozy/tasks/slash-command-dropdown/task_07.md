---
status: completed
title: "Preserve Prefix Handoff And Final Regression Coverage"
type: frontend
complexity: medium
dependencies:
  - task_06
---

# Task 07: Preserve Prefix Handoff And Final Regression Coverage

## Overview
Finish the feature by proving `/agent:` and `/skill:` suggestions accepted from the command dropdown immediately hand off to the existing specialized dropdowns. This task also performs the focused regression coverage and documentation review needed before the implementation can be considered complete.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST ensure accepting `/agent:` from the command dropdown immediately enables the existing agent dropdown.
- MUST ensure accepting `/skill:` from the command dropdown immediately enables the existing skill dropdown.
- MUST ensure accepted prefix suggestions do not submit app events or add unwanted trailing text that suppresses specialized dropdowns.
- MUST preserve approval and clarification input gating.
- MUST preserve `/goal` follow-on text entry after command suggestion acceptance.
- MUST review README command documentation for consistency with the final fixed V1 command behavior.
- MUST run the focused test set covering catalog, app guidance, TUI dropdowns, and regressions.
</requirements>

## Subtasks
- [x] 7.1 Add handoff tests for `/agent:` and `/skill:` accepted from the command dropdown.
- [x] 7.2 Verify specialized dropdowns still render and accept suggestions after prefix handoff.
- [x] 7.3 Add regression tests for `/goal` follow-on text, approval gating, and clarification gating.
- [x] 7.4 Review README command documentation and update only if implementation changed documented behavior.
- [x] 7.5 Run the focused test set and fix regressions within the feature scope.

## Implementation Details
Modify `src/tui/mod.rs` tests and any final TUI behavior needed for prefix handoff. Review `README.md` after the implementation; update it only if needed to keep user-visible command behavior accurate. Do not add new V1 commands or broaden command palette behavior.

### Relevant Files
- `src/tui/mod.rs` — Contains command dropdown behavior, existing agent/skill dropdown behavior, and focused TUI tests.
- `src/app/mod.rs` — Contains clarification and approval behavior that must remain stable.
- `README.md` — Documents TUI commands and should be reviewed for final behavior consistency.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines final regression expectations and non-goals.

### Dependent Files
- `src/slash_commands.rs` — Supplies `/agent:` and `/skill:` prompt-prefix metadata.
- `src/lib.rs` — Exports the shared metadata module.

### Related ADRs
- [ADR-004: Scope Slash Dropdown Activation And Keyboard Semantics](adrs/adr-004.md) — Requires immediate prefix handoff and no-match behavior.
- [ADR-003: Use Shared Metadata-Only Slash Command Catalog](adrs/adr-003.md) — Keeps `/agent:` and `/skill:` as metadata prompt-prefix entries.
- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) — Preserves existing specialized dropdown behavior.

## Deliverables
- Handoff behavior verified for `/agent:` and `/skill:`.
- Final focused regression coverage for dropdown, app guidance, gating, and follow-on text behavior.
- README reviewed and updated only if necessary.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for final TUI/app regression coverage **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Accepting `/agent:` from the command dropdown immediately makes the agent dropdown visible.
  - [x] Accepting `/skill:` from the command dropdown immediately makes the skill dropdown visible.
  - [x] Prefix handoff does not dispatch an app event.
  - [x] Prefix handoff does not add trailing text that prevents specialized filtering.
  - [x] Accepting `/goal` allows the user to type goal text afterward.
  - [x] Command dropdown remains disabled during pending approval.
  - [x] Command dropdown remains disabled during `WaitingForUser`.
- Integration tests:
  - [x] Existing app clarification answer `/tmp/project` still completes as clarification input.
  - [x] Existing `/agent:` and `/skill:` prompt-prefix submission tests still pass.
  - [x] Focused TUI and app slash-command test commands pass together.
- Test coverage target: >=80%
- All tests must pass

## Follow-up Notes (recorded during execution, 2026-06-12)
- **Handoff**: accepting `/agent:`/`/skill:` inserts the bare prefix (no trailing
  text), so the specialized dropdowns' token detection takes over immediately;
  tests confirm `fixer` filters after typing `fi` post-handoff.
- **Clarification gating**: added an explicit `pending_clarification` guard to
  `command_dropdown` (defense-in-depth; the app already sets `WaitingForUser`
  alongside clarifications) so `/`-prefixed answers like `/tmp/project` stay
  normal input.
- **README**: updated the "TUI commands" section to document the new command
  dropdown and the now-visible `/queue`/`/help`, preserving the wording pinned
  by `readme_skill_command_wording_matches_help_language` and
  `readme_workflow_command_wording_matches_v1_limits`.

## Success Criteria
- All tests passing
- Test coverage >=80%
- Prefix handoff is immediate and preserves existing specialized dropdowns.
- Approval and clarification inputs remain protected from slash dropdown activation.
- README has been reviewed for consistency with implemented V1 behavior.
