---
status: pending
title: "Add Command Dropdown Model And Activation Rules"
type: frontend
complexity: medium
dependencies:
  - task_01
---

# Task 04: Add Command Dropdown Model And Activation Rules

## Overview
Add the command dropdown model, filtering behavior, selection state, and activation rules in the TUI. This task creates the non-rendering model layer needed by later rendering and keyboard tasks.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST activate the command dropdown only when the input starts with `/` at character position 0.
- MUST disable command dropdown activation while `pending_approval` is present.
- MUST disable command dropdown activation while the run state is `WaitingForUser`.
- MUST preserve existing `/agent:` and `/skill:` dropdown precedence once those specialized prefixes are active.
- MUST filter shared command specs as the user types.
- MUST model compact no-match state without selecting a suggestion.
- MUST add command dropdown selection and dismissal state needed by later tasks.
</requirements>

## Subtasks
- [ ] 4.1 Add command dropdown state to the TUI UI state.
- [ ] 4.2 Add a command dropdown model that reads shared command specs.
- [ ] 4.3 Add beginning-of-input activation checks.
- [ ] 4.4 Add pending-approval and waiting-for-user gating.
- [ ] 4.5 Preserve agent and skill dropdown precedence.
- [ ] 4.6 Add model-level tests for activation, filtering, selection, and no-match state.

## Implementation Details
Modify `src/tui/mod.rs` around `TuiUiState`, dropdown reset helpers, and dropdown detection functions. Do not render the command dropdown or handle acceptance in this task; later tasks depend on this model.

### Relevant Files
- `src/tui/mod.rs` — Contains `TuiUiState`, `DropdownCommand`, existing agent/skill dropdown models, and run-state-aware key routing.
- `src/slash_commands.rs` — Provides shared command metadata created by task_01.
- `src/orchestrator/mod.rs` — Defines run states used by TUI gating.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines activation, filtering, and gating requirements.

### Dependent Files
- `src/app/mod.rs` — Provides pending approval and run-state values through `AppState`.
- `README.md` — May need later review if activation behavior is documented.

### Related ADRs
- [ADR-004: Scope Slash Dropdown Activation And Keyboard Semantics](adrs/adr-004.md) — Defines beginning-of-input activation, gating, and no-match state.
- [ADR-003: Use Shared Metadata-Only Slash Command Catalog](adrs/adr-003.md) — Defines the shared command metadata source.

## Deliverables
- Command dropdown model and selection/dismissal state in `src/tui/mod.rs`.
- Activation and filtering helpers backed by shared metadata.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for state-aware dropdown precedence **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Input `/` produces command dropdown suggestions for all fixed V1 commands.
  - [ ] Input `/g` filters to `/goal` and `/goal clear`.
  - [ ] Input `please /g` does not activate the command dropdown.
  - [ ] Input `/unknown` produces a no-match model with no selected suggestion.
  - [ ] Pending approval suppresses command dropdown activation.
  - [ ] `WaitingForUser` suppresses command dropdown activation.
  - [ ] `/agent:` and `/skill:` continue to resolve to specialized dropdown models.
- Integration tests:
  - [ ] Key routing continues to prefer help modal, then agent dropdown, then skill dropdown before the command dropdown.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Command dropdown model exists without rendering or dispatch behavior.
- Activation follows the TechSpec and ADR-004 boundaries.
