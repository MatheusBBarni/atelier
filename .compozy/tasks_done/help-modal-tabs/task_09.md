---
status: completed
title: Commands tab substring filter (Phase 2)
type: frontend
complexity: medium
dependencies:
  - task_06
  - task_07
---

# Task 09: Commands tab substring filter (Phase 2)

## Overview
Phase 2. Add a type-to-filter line to the Commands tab so users can narrow the slash-command
list by substring. The filter uses a dedicated `help_filter` buffer in `TuiUiState` and a
`.contains()` match over the catalog-derived rows; it never touches the live composer input.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `help_filter: String` to `TuiUiState` (default `""`), cleared on tab change and on modal close.
- MUST add `TuiCommand::HelpFilterCharacter(char)` and `TuiCommand::HelpFilterBackspace`, routed only while help is visible AND the active tab is `Commands`.
- MUST filter `commands_tab_lines` by case-insensitive `.contains()` over the command usage/label (mirroring `skill_suggestions` filtering at `:2067`); empty filter shows all commands.
- MUST render the current filter text and an empty-result indication when nothing matches.
- MUST NOT mutate `state.input`; MUST NOT submit filter text as a prompt.
</requirements>

## Subtasks
- [x] 09.1 Add the `help_filter` field with default + reset rules.
- [x] 09.2 Add the filter command variants and route them only on the Commands tab.
- [x] 09.3 Apply the substring filter inside `commands_tab_lines` and render the filter line.
- [x] 09.4 Add unit tests for filtering, backspace, reset, and composer isolation.

## Implementation Details
Reuse the established substring-filter pattern from `skill_suggestions` (`src/tui/mod.rs:2067`)
and the character-capture pattern (`InputCharacter` at `:1138`), but keep the typed text in
`help_filter` rather than `state.input`. The help-visible key branch (extended in task_07) is
where filter characters are captured when the Commands tab is active. See TechSpec "Development
Sequencing" step 9 and "Technical Considerations" (filter decision).

### Relevant Files
- `src/tui/mod.rs` — `TuiUiState` `:191`; help key branch `:790`; `skill_suggestions` filter `:2067`; `commands_tab_lines` (task_05).

### Dependent Files
- task_05 `commands_tab_lines` — gains real filtering behavior.
- task_07 help key branch — extended to capture filter characters on the Commands tab.

### Related ADRs
- [ADR-002: Phased Delivery Approach](../adrs/adr-002.md) — Commands filter is Phase 2.
- [ADR-003: Tabbed Help Modal — Technical Architecture](../adrs/adr-003.md) — dedicated `help_filter` buffer, not the composer input.

## Deliverables
- `help_filter` state + filter commands + filtered Commands tab.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the filter flow **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `commands_tab_lines("goal", &theme)` includes `/goal` and `/goal clear` and excludes `/workflow`. (`commands_tab_lines_filter_narrows_to_matching_usage`)
  - [x] `commands_tab_lines("", &theme)` shows all catalog commands (unfiltered). (`commands_tab_lines_empty_filter_shows_all_commands`)
  - [x] On the Commands tab with `help_visible`, `Char('g')` → `HelpFilterCharacter('g')`; on the Keys tab the same key does NOT route to the filter. (`help_filter_keys_route_only_on_commands_tab`)
  - [x] `HelpFilterBackspace` on `"go"` yields `"g"` and broadens the list; switching tabs resets `help_filter` to `""`. (`help_filter_backspace_broadens_and_tab_change_resets`)
  - [x] Applying the filter leaves `state.input` unchanged. (`help_filter_does_not_touch_composer_input`)
- Integration tests:
  - [x] Open help → Commands tab → type a substring → render shows only matching commands; an unmatched query renders the empty-result indicator. (`help_commands_filter_narrows_then_shows_empty_state`) NOTE: the spec's "doc" example matches no catalog command (those are CLI flags), so the test uses "goal" for the matching case and "goalz" for the empty case.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Filtering narrows commands without touching the composer; resets on tab change/close.
- `colors_live_only_in_theme_module` passes; `cargo clippy --all-targets` clean.
