---
status: pending
title: Data-driven Keys help tab
type: frontend
complexity: low
dependencies:
  - task_04
---

# Data-driven Keys help tab

## Overview
Replace the static Keys help tab with one rendered from the active `Keymap`, showing each binding
via `format_key` and marking reserved/fixed keys as locked. This is the in-app discovery surface
and completes Wave 1.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST change `keys_tab_lines` (`src/tui/mod.rs:3737`) to take the active `Keymap` (and `&Theme`)
  and render one line per binding using `keybindings::format_key`, plus a short action label.
- MUST display reserved/fixed keys (the `Ctrl-C` interrupt; structural `Enter`/`Backspace`/arrows)
  as visibly locked / non-rebindable, distinct from remappable ones.
- MUST use theme tokens only — no `Color::` literals — honoring the `colors_live_only_in_theme_module`
  test (`src/tui/mod.rs:9200`).
- MUST update the help-modal render dispatch to pass `ui_state.keymap` into `keys_tab_lines`.
- MUST rewrite `keys_tab_lines_contains_expected_keybindings` (`src/tui/mod.rs:11685`) to assert
  the DEFAULT keymap renders the expected default keys (no-config ⇒ defaults present), replacing
  the hardcoded-string assertions.
</requirements>

## Subtasks
- [ ] 5.1 Change `keys_tab_lines` to render from the active `Keymap` via `format_key`.
- [ ] 5.2 Mark reserved/fixed keys as locked, distinct from remappable bindings.
- [ ] 5.3 Pass `ui_state.keymap` at the render dispatch site.
- [ ] 5.4 Rewrite the Keys-tab test to assert defaults render from the keymap.
- [ ] 5.5 Confirm theme-token compliance (no new `Color::` literals).

## Implementation Details
Within `src/tui/mod.rs`: `keys_tab_lines` (`:3737`), the help-modal render dispatch (`HelpTab::Keys`
arm near `:3708`), the styled-line pattern from `approvals_tab_lines` (`:3774`) using
`Span::styled(text, Style::default().fg(theme.token))`, the theme-token rule test (`:9200`), and
the existing test (`:11685`). Consumes `keybindings::{Keymap, format_key}` and reads
`ui_state.keymap` (task_04). See TechSpec "Implementation Design" (data-driven Keys tab) and ADR-003.

### Relevant Files
- `src/tui/mod.rs` — `keys_tab_lines`, render dispatch, theme test, Keys-tab test.
- `src/keybindings.rs` — `format_key`, `Keymap::entries`.

### Dependent Files
- `src/tui/mod.rs` init (task_08) — when the keymap becomes resolved, the tab reflects customizations with no further change here.

### Related ADRs
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — data-driven Keys tab.
- [ADR-002: Parity-First Delivery](adrs/adr-002.md) — Wave 1 completion.

## Deliverables
- `keys_tab_lines` rendering from the active keymap; reserved/fixed keys shown locked.
- Rewritten Keys-tab test asserting defaults render from the keymap.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test rendering the Keys tab via `TestBackend` without panic **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `keys_tab_lines(default_keymap, theme)` output contains `ctrl+l` with a toggle-roster label and the scroll/editing default keys.
  - [ ] Reserved `Ctrl-C` is rendered with the locked/non-rebindable marker.
  - [ ] No `Color::` literal is introduced (the `colors_live_only_in_theme_module` test still passes).
- Integration tests:
  - [ ] `render_help_modal` on the Keys tab renders a default `TuiUiState` via `TestBackend` without panic.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Keys tab reflects the active keymap with reserved keys locked; theme-token test passes.
- Wave 1 is shippable (parity + safety chokepoint + data-driven Keys tab) with no config dependency.
