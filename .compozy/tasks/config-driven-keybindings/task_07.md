---
status: pending
title: Keybinding validation and EffectiveConfig wiring
type: backend
complexity: medium
dependencies:
  - task_06
---

# Keybinding validation and EffectiveConfig wiring

## Overview
Turn the parsed `[keybindings]` section into validated `EffectiveConfig.keybindings` overrides plus
`keybinding_warnings`, hard-failing the dangerous and structural mistakes at load and soft-failing
cosmetic ones. This is the severity-split validation the PRD/ADR-004 require.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `keybindings: KeybindingOverrides` and `keybinding_warnings: Vec<String>` to
  `EffectiveConfig` (`src/config/mod.rs:402`) and populate them in the merge→effective step.
- MUST hard-fail `load_effective_config` (anyhow `Err` with a precise `file + field + value`
  message) for: a reserved-key bind, a non-portable key, an unknown context (anything but `normal`
  in V1), a duplicate (two actions resolving to one key), or a malformed key string — using
  `keybindings::{parse_key, validate_overrides, KeyAction}` (task_01).
- MUST soft-fail (drop the entry, push a warning, keep the rest) for an unknown action name.
- MUST carry the local-ignored warning from task_06 into `keybinding_warnings`.
- MUST leave `keybindings` empty (defaults only) when no `[keybindings]` section is present.
</requirements>

## Subtasks
- [ ] 7.1 Add `keybindings` and `keybinding_warnings` fields to `EffectiveConfig`.
- [ ] 7.2 Parse the `normal` context table into `KeyAction`→`KeyChord`/unbind overrides.
- [ ] 7.3 Hard-validate via `validate_overrides`; produce precise load errors.
- [ ] 7.4 Soft-fail unknown actions with a collected warning; merge in the local-ignored warning.
- [ ] 7.5 Add tests for each severity class and the no-section default.

## Implementation Details
Within `src/config/mod.rs`: `EffectiveConfig` (`:402`), the `MergedConfig::into_effective` step
(`:606-620`) where validation runs, and the `load_from_temp` test helper (`:2417`). Consumes
`keybindings::{parse_key, validate_overrides, KeyAction, KeybindingOverrides}` (task_01) and the
parsed `RawConfig.keybindings` + layer warning (task_06). See TechSpec "Data Models"
(EffectiveConfig additions) and ADR-004 (severity split). Keep error messages field/value specific.

### Relevant Files
- `src/config/mod.rs` — `EffectiveConfig`, `into_effective`, validation wiring, tests.
- `src/keybindings.rs` — `parse_key`, `validate_overrides`, `KeyAction`, `KeybindingOverrides`.

### Dependent Files
- `src/tui/mod.rs` (task_08) — resolves `EffectiveConfig.keybindings` into the active `Keymap`.
- `src/doctor/mod.rs` + `src/config/mod.rs` (task_09) — read `keybindings`/`keybinding_warnings`.

### Related ADRs
- [ADR-004: Config Trust Boundary and Validation Severity](adrs/adr-004.md) — severity split, precise errors.
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — overrides type and validation.

## Deliverables
- `EffectiveConfig.keybindings` + `keybinding_warnings`, populated with severity-split validation.
- Precise hard-fail load errors; soft-fail warnings for unknown actions.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests via `load_from_temp` covering each severity class **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A valid `toggle-roster = "ctrl+g"` yields `EffectiveConfig.keybindings` with `ToggleRoster`→`ctrl+g`.
  - [ ] No `[keybindings]` section ⇒ `keybindings` empty and `keybinding_warnings` empty.
- Integration tests (`load_from_temp`):
  - [ ] `toggle-roster = "ctrl+c"` hard-fails with an error naming the file, field, and value (reserved).
  - [ ] `toggle-roster = "ctrl+1"` hard-fails (non-portable); a `[keybindings.approval]` table hard-fails (unknown context).
  - [ ] `toggle-roster = "ctrl+g"` plus `scroll-top = "ctrl+g"` hard-fails (duplicate key).
  - [ ] `frobnicate = "ctrl+g"` loads OK with the entry dropped and a warning present (unknown action).
  - [ ] `toggle-roster = "ctrl+"` hard-fails (malformed key).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Each hard-fail class produces a precise file/field/value error; unknown actions soft-fail with a warning.
- The local-ignored warning from task_06 appears in `keybinding_warnings`.
