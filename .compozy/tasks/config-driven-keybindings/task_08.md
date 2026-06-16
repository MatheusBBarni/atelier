---
status: pending
title: Resolve keybinding customizations end-to-end
type: frontend
complexity: medium
dependencies:
  - task_04
  - task_05
  - task_07
---

# Resolve keybinding customizations end-to-end

## Overview
Build the active `Keymap` from `EffectiveConfig.keybindings` (defaults + user overrides) at TUI
init, so routing and the Keys tab reflect user rebinds and unbinds. This closes the end-to-end
remap path and completes the customization half of the feature.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST build `ui_state.keymap = Keymap::resolve(&keybindings::DEFAULTS, &effective.keybindings)` at
  TUI init, replacing the `DEFAULTS`-only construction from task_04, reading from the loaded
  `EffectiveConfig`.
- MUST ensure a rebind changes routing (the new key triggers the action; the old default key no
  longer does) and that an unbind removes the action's key entirely.
- MUST ensure the Keys tab (task_05) shows customized bindings (it already renders from
  `ui_state.keymap`, so no further Keys-tab change is needed).
- MUST keep reserved keys (task_03) and all modal-context keys unaffected by overrides.
</requirements>

## Subtasks
- [ ] 8.1 Thread `EffectiveConfig.keybindings` into `TuiUiState` construction.
- [ ] 8.2 Resolve defaults + overrides into the active keymap at init.
- [ ] 8.3 Verify routing reflects rebinds and unbinds.
- [ ] 8.4 Verify the Keys tab reflects the customized keymap.
- [ ] 8.5 Add end-to-end tests from config to routed behavior.

## Implementation Details
Within `src/tui/mod.rs`: the `TuiUiState` construction / TUI init (`run_tui` near `:478-520`) where
the keymap is currently built from `DEFAULTS` (task_04). Reads `EffectiveConfig.keybindings`
(task_07) and calls `keybindings::Keymap::resolve` (task_01). The Keys tab (task_05) and routing
(task_04) consume `ui_state.keymap` unchanged. See TechSpec "Development Sequencing" step 8 and
ADR-003.

### Relevant Files
- `src/tui/mod.rs` — TUI init / `TuiUiState` construction, routing, Keys tab (consumers).
- `src/config/mod.rs` — `EffectiveConfig.keybindings` source.
- `src/keybindings.rs` — `Keymap::resolve`.

### Dependent Files
- None downstream — this is the integration endpoint for the remap path.

### Related ADRs
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — resolution and lookup.
- [ADR-002: Parity-First Delivery](adrs/adr-002.md) — Wave 2 remap layer.

## Deliverables
- Active keymap resolved from defaults + `EffectiveConfig.keybindings` at TUI init.
- Routing and Keys tab reflect rebinds/unbinds; reserved/modal keys unaffected.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test from a loaded config through to routed behavior **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] With overrides `{ToggleRoster: Some(ctrl+g)}`, routing maps `ctrl+g`→ToggleRoster and `ctrl+l` no longer toggles (falls through).
  - [ ] With `{ToggleRoster: None}` (unbind), `ctrl+l` no longer toggles and no other key does.
  - [ ] `Ctrl-C` still interrupts regardless of overrides (reserved guard intact).
- Integration tests:
  - [ ] A `load_from_temp` config with `[keybindings.normal] toggle-roster = "ctrl+g"` builds a `TuiUiState` whose routing and Keys-tab output reflect `ctrl+g`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- End-to-end rebind and unbind work; reserved and modal-context keys are unaffected.
- The Keys tab shows the customized bindings.
