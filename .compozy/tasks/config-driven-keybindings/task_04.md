---
status: pending
title: Default keymap wiring into key routing
type: frontend
complexity: high
dependencies:
  - task_01
  - task_02
  - task_03
---

# Default keymap wiring into key routing

## Overview
Bridge `KeyAction` to `TuiCommand` and consult a default-built `Keymap` in the normal-input branch
of key routing, so all ten remappable actions (including the new editing ones) route through the
keymap while no-config behavior stays byte-for-byte identical to today.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `command_for_action(KeyAction) -> TuiCommand` as an exhaustive `match` (compile-checked)
  in `src/tui/mod.rs`, covering all ten `KeyAction` variants.
- MUST add a `keymap: Keymap` field to `TuiUiState` (`src/tui/mod.rs:291`), built from
  `keybindings::DEFAULTS` at TUI initialization (Wave 1: defaults only, no config input yet).
- MUST consult `ui_state.keymap.action_for(&key)` at the start of the normal-input branch of
  `key_event_to_tui_command_with_ui` — after the reserved guard (task_03) and the modal cascade —
  mapping a hit via `command_for_action`; on a miss, fall through to the existing hardcoded handler.
- MUST default-bind all ten actions per `DEFAULTS`, including the editing actions from task_02.
- MUST keep the keymap lookup gated to the normal-input context only (never the modal branches).
- MUST keep no-config behavior byte-for-byte identical (regression test).
</requirements>

## Subtasks
- [ ] 4.1 Implement the exhaustive `command_for_action` match.
- [ ] 4.2 Add the `keymap` field to `TuiUiState` and build it from `DEFAULTS` at init.
- [ ] 4.3 Consult the keymap in the normal-input branch behind the reserved guard.
- [ ] 4.4 Confirm all ten actions (incl. editing) are reachable by their default keys.
- [ ] 4.5 Add a default-fidelity regression test for the full default key set.

## Implementation Details
Within `src/tui/mod.rs`: routing wrapper (`:1056`) and the normal-input fallback
`key_event_to_tui_command` (`:1388`), `TuiUiState` (`:291`) and its construction (TUI init,
`run_tui` near `:478-520`), and the command execution match (`:669-723`, already extended in
task_02). Consumes `keybindings::{KeyAction, Keymap, DEFAULTS}` from task_01. See TechSpec "Core
Interfaces" (`command_for_action`) and "Development Sequencing" step 4; ADR-003 (lookup placement,
exhaustive match).

### Relevant Files
- `src/tui/mod.rs` — `command_for_action`, `TuiUiState.keymap`, routing lookup, init.
- `src/keybindings.rs` — `KeyAction`, `Keymap`, `DEFAULTS`.

### Dependent Files
- `src/tui/mod.rs` Keys tab (task_05) — renders from `ui_state.keymap`.
- `src/tui/mod.rs` init (task_08) — swaps `DEFAULTS`-only build for the resolved keymap.

### Related ADRs
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — lookup placement, exhaustive match, TuiUiState-resident keymap.
- [ADR-002: Parity-First Delivery](adrs/adr-002.md) — Wave 1 default wiring.

## Deliverables
- `command_for_action` exhaustive match; `TuiUiState.keymap` built from `DEFAULTS`; normal-branch lookup.
- All ten actions reachable by default keys (Ctrl-L, PageUp/PageDown, Home/End, Ctrl-A/E/K/U/W).
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test routing keys through `key_event_to_tui_command_with_ui` across contexts **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `command_for_action` maps each of the ten `KeyAction` variants to the correct `TuiCommand` (exhaustive).
  - [ ] With the default keymap: `Ctrl-L`→ToggleRoster, `PageUp`→ScrollEvents(PageUp), `Ctrl-A`→cursor LineStart, `Ctrl-K`→InputKill(ToLineEnd).
  - [ ] Default-fidelity: for the representative pre-feature key set, routing output equals the prior mapping.
  - [ ] An unmapped key (a plain character) still falls through to `InputCharacter`.
- Integration tests:
  - [ ] Routing a modal key (e.g. approval `Esc`/`Enter`) is unaffected by the keymap; normal-context keys resolve via the keymap.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- No-config behavior is byte-for-byte identical (regression test green).
- All ten actions reachable; keymap never resolves inside a modal context.
