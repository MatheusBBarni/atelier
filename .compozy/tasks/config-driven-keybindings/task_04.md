---
status: completed
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
- [x] 4.1 Implement the exhaustive `command_for_action` match.
- [x] 4.2 Add the `keymap` field to `TuiUiState` and build it from `DEFAULTS` at init.
- [x] 4.3 Consult the keymap in the normal-input branch behind the reserved guard.
- [x] 4.4 Confirm all ten actions (incl. editing) are reachable by their default keys.
- [x] 4.5 Add a default-fidelity regression test for the full default key set.

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
  - [x] `command_for_action` maps each of the ten `KeyAction` variants to the correct `TuiCommand` (exhaustive) — `command_for_action_maps_all_ten_actions`.
  - [x] With the default keymap: `Ctrl-L`→ToggleRoster, `PageUp`→ScrollEvents(PageUp), `Ctrl-A`→cursor LineStart, `Ctrl-K`→InputKill(ToLineEnd) — `default_keymap_routes_all_ten_actions_by_their_default_keys`.
  - [x] Default-fidelity: for the representative pre-feature key set, routing output equals the prior mapping — `default_keymap_preserves_pre_feature_routing`.
  - [x] An unmapped key (a plain character) still falls through to `InputCharacter` — `unmapped_key_falls_through_to_input_character`.
- Integration tests:
  - [x] Routing a modal key is unaffected by the keymap; normal-context keys resolve via the keymap — `keymap_is_gated_to_the_normal_context` (approval Ctrl-A inert; normal Ctrl-A → LineStart).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- No-config behavior is byte-for-byte identical (regression test green).
- All ten actions reachable; keymap never resolves inside a modal context.

## Implementation Notes
- The keymap is consulted only in the final normal-input `else` branch of
  `key_event_to_tui_command_with_ui` (after the reserved guard + modal cascade);
  on a hit it maps via `command_for_action`, on a miss it falls through to the
  unchanged base handler — so keys the keymap doesn't own route byte-identically.
- `TuiUiState.keymap` is built by a new `default_keymap()` helper (DEFAULTS, no
  overrides) in the `Default` impl, so `run_tui` (via `with_skill_suggestions`) and
  every test inherit it. **task_08** replaces this with the config-resolved keymap.
- Removed the task_02 `#[allow(dead_code)]` markers on `InputKill`/`LineStart`/
  `LineEnd`/`InputKillCommand` — `command_for_action` now constructs them in prod.
- **Touched `src/keybindings.rs`**: added `PartialEq, Eq` to `Keymap` (content-based,
  order-independent) so it can be a field of the equality-deriving `TuiUiState`.
- **Known follow-up for task_08 (unbind):** the base handler still hardcodes the
  remappable keys (Ctrl-L, PageUp/Down, Home/End) as the miss fallback, so an
  *unbound* default would currently still be caught there. task_08 must ensure unbind
  truly removes a binding (e.g. stop the base handler from shadowing keymap-owned keys).
- Verified `2026-06-16`: 5 new tests pass; clippy `--all-targets` clean; fmt clean;
  full `cargo test --lib` passed 1010 → 1015 (exactly the 5 new tests), with only the
  pre-existing environmental skill/codex failures remaining.
