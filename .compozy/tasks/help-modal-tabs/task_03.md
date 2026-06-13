---
status: pending
title: Add help_active_tab state to TuiUiState
type: frontend
complexity: low
dependencies:
  - task_01
---

# Task 03: Add help_active_tab state to TuiUiState

## Overview
Add the ephemeral `help_active_tab: HelpTab` field to `TuiUiState` so the modal knows which
tab is showing, defaulting to `GettingStarted`, and reset it to the default whenever help
closes. This state is UI-only and must never enter the event-sourced `AppState` snapshot.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `help_active_tab: HelpTab` to the `TuiUiState` struct (`src/tui/mod.rs:191`).
- MUST initialize it to `HelpTab::GettingStarted` in the `Default` impl (`:233`).
- MUST reset `help_active_tab` to `GettingStarted` when the modal closes, inside the `TuiCommand::ToggleHelp` handler (`:492`), so reopening always starts on Getting Started.
- MUST keep the field out of `AppState`; it is ephemeral UI state only.
- MUST NOT change any existing field behavior; `TuiUiState` keeps deriving `Clone, Debug, PartialEq, Eq`.
</requirements>

## Subtasks
- [ ] 03.1 Add the `help_active_tab` field to `TuiUiState`.
- [ ] 03.2 Initialize it to `GettingStarted` in `Default`.
- [ ] 03.3 Reset it to `GettingStarted` in the `ToggleHelp` handler when closing.
- [ ] 03.4 Add unit tests for the default value and the reset-on-close behavior.

## Implementation Details
`TuiUiState` follows a simple field + `Default` pattern; index fields like
`command_selection_index: usize` are initialized to `0`. The `ToggleHelp` arm currently flips
`help_visible` and clears input (`:492`); add the tab reset there. See TechSpec "Data Models"
for the field definition and reset rule.

### Relevant Files
- `src/tui/mod.rs` — `TuiUiState` struct `:191`, `Default` impl `:233`, `ToggleHelp` handler `:492`.

### Dependent Files
- task_06 (tabbed render) reads `ui_state.help_active_tab`.
- task_07 (navigation) mutates it.

### Related ADRs
- [ADR-003: Tabbed Help Modal — Technical Architecture](../adrs/adr-003.md) — ephemeral tab state in `TuiUiState`, never in the snapshot.

## Deliverables
- `help_active_tab` field with default + reset-on-close.
- Unit tests with 80%+ coverage **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `TuiUiState::default().help_active_tab == HelpTab::GettingStarted`.
  - [ ] After setting `help_active_tab = HelpTab::Cli` and executing `ToggleHelp` to close, the field is back to `GettingStarted` (drive via the same command-execution path used by other `TuiCommand` tests).
  - [ ] Opening help (toggle on) does not change other `TuiUiState` fields unexpectedly (no regression to `help_visible` toggle semantics).
- Integration tests:
  - [ ] N/A at this layer — exercised once render consumes the field in task_06.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Reopening the modal always lands on Getting Started.
- `cargo clippy --all-targets` clean; no snapshot/`AppState` change.
