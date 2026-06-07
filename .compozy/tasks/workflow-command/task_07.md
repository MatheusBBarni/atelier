---
status: pending
title: Add TUI Help For Workflow Command
type: docs
complexity: low
dependencies:
  - task_01
---

# Task 07: Add TUI Help For Workflow Command

## Overview
Update the TUI help modal so users can discover `/workflow <prompt>` from the existing command reference. This is a small user-facing documentation change backed by the current help rendering test.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- The TUI help modal MUST include `/workflow <prompt>` after task 01 makes it a recognized command.
- Help text MUST describe workflow mode concisely without introducing unsupported saved workflows, worktrees, or background execution.
- Help rendering tests MUST assert that `/workflow <prompt>` appears in the modal.
- The `/help` intercept behavior MUST remain unchanged.
- If the README command reference lists slash commands, it SHOULD include `/workflow <prompt>` with the same V1 limitations.
</requirements>

## Subtasks
- [ ] 7.1 Add `/workflow <prompt>` to the TUI command list.
- [ ] 7.2 Keep wording aligned with the PRD's executing workflow behavior.
- [ ] 7.3 Extend the existing help modal rendering test.
- [ ] 7.4 Confirm `/help + Enter` still toggles help without dispatching an app event.
- [ ] 7.5 Update README command help if its slash-command section is still current.

## Implementation Details
Implement this in `src/tui/mod.rs` near `render_help_modal` and its tests. Reference the TechSpec "Impact Analysis" section for the intended scope: help text only.

### Relevant Files
- `src/tui/mod.rs` - renders the help modal, intercepts `/help`, and contains help rendering tests.
- `src/app/mod.rs` - defines the accepted slash-command surface after task 01.
- `README.md` - may contain a command reference that should stay aligned with TUI help when present.
- `.compozy/tasks/workflow-command/_prd.md` - describes workflow mode in user-facing terms.
- `.compozy/tasks/workflow-command/_techspec.md` - states that TUI help text is the only TUI change for V1.

### Dependent Files
- `src/app/chat/projection.rs` - not directly modified, but the TUI displays Chat items from projection during workflow runs.
- `src/runtime/fake.rs` - not directly modified, but app tests may use fake workflow prompts referenced in help examples.

### Related ADRs
- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - constrains wording to one normal Run, not saved scripts or background workflows.
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - constrains wording to executing workflow behavior.

## Deliverables
- TUI help modal entry for `/workflow <prompt>`.
- README command-reference update when applicable.
- Help rendering test assertion for `/workflow <prompt>`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for help visibility behavior **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `renders_help_modal_commands` includes `/workflow <prompt>`.
  - [ ] `help_command_toggles_modal_without_app_event` remains unchanged and still passes.
- Integration tests:
  - [ ] Rendering the help modal at the existing tested dimensions includes the workflow command without dropping existing command entries.
  - [ ] README command reference, if updated, does not describe saved workflows, worktrees, or background execution as V1 capabilities.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Users can discover `/workflow <prompt>` in the TUI help modal.
- Help text does not imply unsupported V1 capabilities.
