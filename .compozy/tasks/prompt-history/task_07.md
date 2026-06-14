---
status: completed
title: "Recall discoverability hint line and help entry"
type: frontend
complexity: low
dependencies:
  - task_04
  - task_05
---

# Recall discoverability hint line and help entry

## Overview

Make recall discoverable without clutter: show a contextual "↑ recall" hint in the
existing input status line when the input is empty and history is available, and add
a recall entry to the `/help` overlay (PRD discoverability decision). No new colors.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST show a recall hint (e.g., "↑ recall") in `render_input_status` only when work is not active, the input is empty, and `prompt_history` is non-empty; otherwise keep the existing `/help` hint.
- MUST document the recall keys in the `/help` overlay (`render_help_modal`), consistent with existing entries.
- MUST NOT introduce inline `Color::` literals — use theme tokens so `colors_live_only_in_theme_module` stays green.
- SHOULD keep the hint copy concise and consistent with `QUEUE_HINT` / `CLARIFICATION_HINT_*`.
</requirements>

## Subtasks
- [x] 7.1 Add a recall hint constant and select it in `render_input_status` under the right condition.
- [x] 7.2 Add a recall line/entry to the help overlay.
- [x] 7.3 Confirm no new color literals; use theme tokens.
- [x] 7.4 Test the hint visibility conditions and the help entry presence.

## Implementation Details

Edit `src/tui/mod.rs`: `render_input_status` (~`:3469`, the `WORK_HINT` selection
~`:3520`) and `render_help_modal` (~`:3270`); add a `HISTORY_HINT` constant near
`WORK_HINT`/`QUEUE_HINT`. See TechSpec "System Architecture" (layer 3: recall
interaction → discoverability) and PRD "User Experience".

### Relevant Files
- `src/tui/mod.rs` — `render_input_status`, the `WORK_HINT`/`QUEUE_HINT` constants, `render_help_modal`, the `colors_live_only_in_theme_module` test, and render test helpers.

### Dependent Files
- (none.)

### Related ADRs
- [ADR-002: V1 Ships the Full Faithful-Parity Recall Set in One Release](../adrs/adr-002.md) — discoverability (hint + help) is part of the full-parity V1.

## Deliverables
- A contextual recall hint and a `/help` entry, with no new color literals.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Render/integration test asserting the hint appears and disappears correctly **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Empty input + non-empty `prompt_history` + work not active → the status line shows the recall hint.
  - [ ] Non-empty input → the status line shows the normal `/help` hint, not recall.
  - [ ] Empty input + empty `prompt_history` → normal hint (recall not advertised).
  - [ ] Work active → recall hint suppressed (the work indicator wins).
- Integration tests:
  - [ ] A `render_to_*` snapshot with history present and empty input contains "↑ recall"; the help overlay lists the recall keys.
  - [ ] `colors_live_only_in_theme_module` still passes after the change.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The hint shows only when recall is available; `/help` documents recall
- No inline color literals are introduced
