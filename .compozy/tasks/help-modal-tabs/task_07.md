---
status: pending
title: Help tab navigation keys and commands
type: frontend
complexity: medium
dependencies:
  - task_06
---

# Task 07: Help tab navigation keys and commands

## Overview
Make the tabbed modal interactive: add `HelpNextTab`/`HelpPrevTab` commands, handle them in the
executor by advancing `help_active_tab`, and bind Arrow keys + Tab/Shift-Tab inside the
help-visible key branch so users can cycle tabs. Esc must still close from any tab.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `TuiCommand::HelpNextTab` and `TuiCommand::HelpPrevTab` variants (enum at `src/tui/mod.rs:91`).
- MUST handle both in the command executor by setting `ui_state.help_active_tab = help_active_tab.next()/.prev()`.
- MUST bind, inside the help-visible branch of `key_event_to_tui_command_with_ui` (`:790`): Right and Tab → `HelpNextTab`; Left and Shift-Tab → `HelpPrevTab`; Esc → `ToggleHelp` (unchanged); all other keys → `None`.
- MUST handle navigation entirely within the help-visible branch so keys never leak to the base handler.
- MUST NOT break the existing Esc/Ctrl-C behavior in that branch.
</requirements>

## Subtasks
- [ ] 07.1 Add the `HelpNextTab`/`HelpPrevTab` command variants.
- [ ] 07.2 Handle them in the executor to advance/retreat `help_active_tab`.
- [ ] 07.3 Bind Arrows + Tab/Shift-Tab in the help-visible key branch.
- [ ] 07.4 Add routing tests for each nav key and an end-to-end cycle integration test.

## Implementation Details
The help-visible branch currently maps Esc and Ctrl-C and returns `None` for everything else
(`:790`) — insert the nav mappings there. `KeyEvent` matching for Tab uses `KeyCode::Tab` with
`KeyModifiers::SHIFT` distinguishing Shift-Tab. The executor pattern mirrors the existing
`ToggleHelp` arm (`:492`). See TechSpec "Core Interfaces" for the new variants.

### Relevant Files
- `src/tui/mod.rs` — `TuiCommand` enum `:91`; executor `:492`; key routing help branch `:790`; routing test pattern `esc_closes_help_modal_only_when_visible` `:6244`.

### Dependent Files
- task_06 render reads the `help_active_tab` this task mutates.
- task_09 (Phase 2 filter) extends the same help-visible branch for character input.

### Related ADRs
- [ADR-003: Tabbed Help Modal — Technical Architecture](../adrs/adr-003.md) — Arrows/Tab navigation handled within the help-visible branch.

## Deliverables
- `HelpNextTab`/`HelpPrevTab` commands + executor handling + key bindings.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test for tab cycling **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] With `help_visible`, `KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)` → `Some(HelpNextTab)`; `KeyCode::Tab`/NONE → `Some(HelpNextTab)`.
  - [ ] `KeyCode::Left`/NONE → `Some(HelpPrevTab)`; `KeyCode::Tab`/SHIFT → `Some(HelpPrevTab)`.
  - [ ] `KeyCode::Esc` still → `Some(ToggleHelp)`; an unrelated key (e.g. `Char('x')`) → `None`.
  - [ ] Executing `HelpNextTab` from `GettingStarted` sets `help_active_tab == Commands`; from `Cli` wraps to `GettingStarted`.
- Integration tests:
  - [ ] Open help, send Right ×6 → render returns to the Getting Started body (full cycle); send Esc from the Skills tab → modal closes (`help_visible == false`).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Tabs cycle with Arrows/Tab; Esc closes from any tab; no key leakage to the base handler.
- `cargo clippy --all-targets` clean.
