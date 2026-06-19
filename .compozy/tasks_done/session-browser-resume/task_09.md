---
status: completed
title: "Discoverability - /sessions command + welcome cue"
type: frontend
complexity: low
dependencies:
  - task_07
---

# Task 09: Discoverability - /sessions command + welcome cue

## Overview
Make the browser discoverable beyond the keybinding: register a `/sessions` slash command (surfaced in the command dropdown and help overlay) that opens the browser, and add a static cue to the welcome facts box pointing users at it. Counters the documented "resume gets buried" problem.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST register `/sessions` in the slash-command catalog so it appears in the command dropdown, the help overlay, and unknown-command guidance (all three stay aligned through the catalog).
- MUST make `/sessions` open the session browser (dispatch the same open command as `Ctrl-R`).
- MUST add a static cue in the welcome facts box pointing to the browser (alongside the existing `/help` cue).
- MUST NOT implement the dynamic "last session was interrupted" hint here — that is task_13.
</requirements>

## Subtasks
- [x] 9.1 Add the `/sessions` entry to the slash-command catalog with description.
- [x] 9.2 Wire `/sessions` execution to open the browser.
- [x] 9.3 Add a static browser cue to the welcome facts box.
- [x] 9.4 Add tests for catalog presence and the welcome cue.

## Implementation Details
Add the command to `src/slash_commands.rs` (catalog is the single source for dropdown/help/guidance; `/goal` at `:57` is a model entry). Route its execution to the browser-open command (TUI-local, like `/help`/`/reload:skills`). Add the cue line in `src/tui/welcome.rs` facts (`:312`+), styled with a muted theme token. See TechSpec "API Endpoints" and the PRD "User Experience" (discoverability).

### Relevant Files
- `src/slash_commands.rs` — command catalog (`/goal` at `:57` as the pattern).
- `src/tui/welcome.rs` — `WelcomeFacts` (`:92`), facts lines (`:312`).
- `src/tui/mod.rs` — browser open command (task_07); slash-command execution routing.

### Dependent Files
- `src/tui/welcome.rs` — task_13 extends the welcome cue with the dynamic post-crash variant.
- Slash-command catalog tests (under `tests/`) — must stay aligned.

### Related ADRs
- [ADR-005: Product approach — recovery-first, phased delivery](adrs/adr-005.md) — multi-entry discoverability is part of the approach.

## Deliverables
- `/sessions` registered and opening the browser.
- Static welcome cue for the browser.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: `/sessions` opens the browser **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] The slash-command catalog contains `/sessions` with a description (and the slash-command catalog test suite still passes). — `slash_commands` unit tests (`catalog_labels_are_exactly_the_fixed_v1_set`, `tui_local_and_app_command_entries_are_categorized_distinctly`) + `tests/slash_command_catalog.rs` (`len()==13` + `/sessions` TuiLocal) updated.
  - [x] The welcome facts box includes the browser cue line. — `welcome::tests::facts_box_includes_session_browser_cue`
  - [x] Executing `/sessions` produces the browser-open command. — `slash_sessions_routes_to_browser_open`
- Integration tests:
  - [x] Submitting `/sessions` in the TUI opens the session browser (same end state as `Ctrl-R`). — `submitting_slash_sessions_opens_browser_and_clears_input`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Users can find the browser via the command dropdown, help overlay, and welcome cue — not just the keybinding.

## As-built notes
- `/sessions` added to the metadata-only catalog (`TuiLocal`, "browse and preview past sessions"), so it surfaces in the dropdown, help overlay, and unknown-command guidance. Updated the catalog amendment comment + the `FIXED_V1_LABELS` (12→13) / categorization / `tests/slash_command_catalog.rs` (`len()==13`) assertions.
- Execution mirrors `/help`: a base-handler Enter arm (`input == "/sessions"` → `SessionBrowser(Open)`); the command dropdown completes the text first, then this fires on submit. The `Open` handler now `clear_input`s so the `/sessions` trigger doesn't linger (a Ctrl-R open is empty, so it's a no-op there).
- Welcome facts gain a static muted cue ("Ctrl-R or /sessions to browse and resume past sessions") beside the `/help` cue. The dynamic post-crash variant is task_13.
