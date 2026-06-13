---
status: pending
title: Tabbed render_help_modal with active-tab dispatch
type: frontend
complexity: medium
dependencies:
  - task_03
  - task_04
  - task_05
---

# Task 06: Tabbed render_help_modal with active-tab dispatch

## Overview
Rewrite `render_help_modal` into a tabbed overlay: change its signature to consume the live
snapshot, draw a theme-token tab strip highlighting the active tab, and render the active
tab's body by dispatching to the per-tab builders. This is the integration point that turns
the flat overlay into the tabbed surface and requires updating the three Commands-asserting
tests whose default view changes to Getting Started.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST change `render_help_modal(frame, &Theme)` to `render_help_modal(frame, state: &AppState, ui_state: &TuiUiState, theme: &Theme)` and update both call sites (`src/tui/mod.rs:2194`, `:2236`).
- MUST render a tab strip listing all `HelpTab::ALL` titles with the active tab visually distinguished using theme tokens only (no inline `Color::`).
- MUST dispatch on `ui_state.help_active_tab` to the matching builder from task_04/task_05; the default (`GettingStarted`) MUST render on open.
- MUST keep `render_help_modal` a pure function of `(AppState, TuiUiState, Theme)` — no reads of `App` internals.
- MUST update the three breaking tests (`renders_help_modal_commands` `:4587`, `help_modal_command_rows_are_catalog_derived` `:4624`, `readme_skill_command_wording_matches_help_language` `:4658`) to set `help_active_tab = HelpTab::Commands` before asserting; the catalog-derived and README-wording contracts MUST be preserved, not deleted.
- MUST keep existing behavior intact: Esc closes, help suppresses dropdown rendering, mouse-wheel ignored.
</requirements>

## Subtasks
- [ ] 06.1 Change the `render_help_modal` signature and update both call sites.
- [ ] 06.2 Render the theme-token tab strip with active-tab highlight.
- [ ] 06.3 Dispatch on `help_active_tab` to the per-tab builders and render the active body.
- [ ] 06.4 Update the three Commands-asserting tests to select the Commands tab first.
- [ ] 06.5 Add tests for default-tab render and tab-strip presence.

## Implementation Details
`render_help_modal` is currently stateless (`:3257`) and both call sites already have `state`
and `ui_state` in scope (inside `render`, `:2085`). Replace the single `Vec<Line>` body with
strip + active-body composition. Do not implement key navigation here (task_07); the body just
reflects whatever `help_active_tab` holds. See TechSpec "Core Interfaces" and "System
Architecture".

### Relevant Files
- `src/tui/mod.rs` — `render_help_modal` `:3257`; call sites `:2194`, `:2236`; `render` `:2085`; test harness `render_to_text_with_ui` `:6551`; breaking tests `:4587`/`:4624`/`:4658`.

### Dependent Files
- task_07 (navigation) drives `help_active_tab` that this render reads.
- Existing help tests (Esc/dropdown-suppression/mouse-wheel) — must remain green.

### Related ADRs
- [ADR-003: Tabbed Help Modal — Technical Architecture](../adrs/adr-003.md) — signature change + theme-token strip; rejects `ratatui::Tabs`.

## Deliverables
- Tabbed `render_help_modal` with strip + active-body dispatch.
- Updated three breaking tests (contracts preserved).
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for tabbed render **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Updated `help_modal_command_rows_are_catalog_derived` sets `help_active_tab = Commands` and still asserts every catalog usage appears exactly once.
  - [ ] Updated `readme_skill_command_wording_matches_help_language` selects Commands and retains `/skill:<skill_name>` + "load skill context" README alignment.
- Integration tests:
  - [ ] `help_visible == true`, default state → `render_to_text_with_ui` contains the Getting Started routing line and a tab strip listing "Getting Started" and "Commands".
  - [ ] Setting `help_active_tab = HelpTab::Commands` → render contains `/help` and its catalog description.
  - [ ] `help_modal_suppresses_command_dropdown_rendering` and the mouse-wheel-ignore test still pass unchanged.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing (including the three updated tests)
- Test coverage >=80%
- Opening help shows Getting Started + tab strip; no regression to Esc/dropdown/mouse behavior.
- `colors_live_only_in_theme_module` passes; `cargo clippy --all-targets` clean.
