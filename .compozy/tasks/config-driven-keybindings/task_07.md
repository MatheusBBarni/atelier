---
status: completed
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
- [x] 7.1 Add `keybindings` and `keybinding_warnings` fields to `EffectiveConfig`. (`keybinding_warnings` landed in task_06; `keybindings` added here.)
- [x] 7.2 Parse the `normal` context table into `KeyAction`→`KeyChord`/unbind overrides.
- [x] 7.3 Hard-validate via `validate_overrides`; produce precise load errors.
- [x] 7.4 Soft-fail unknown actions with a collected warning; merge in the local-ignored warning.
- [x] 7.5 Add tests for each severity class and the no-section default.

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
  - [x] A valid `toggle-roster = "ctrl+g"` yields `EffectiveConfig.keybindings` with `ToggleRoster`→`ctrl+g` — `valid_keybinding_yields_an_override` (+ `unbind_keybinding_yields_a_none_override`).
  - [x] No `[keybindings]` section ⇒ `keybindings` empty and `keybinding_warnings` empty — `no_keybindings_section_leaves_overrides_and_warnings_empty`.
- Integration tests (user-scope loader):
  - [x] `toggle-roster = "ctrl+c"` hard-fails with an error naming the file, field, and value (reserved) — `reserved_key_bind_hard_fails_with_file_field_value`.
  - [x] `toggle-roster = "ctrl+1"` hard-fails (non-portable) — `non_portable_key_hard_fails`; a `[keybindings.approval]` table hard-fails (unknown context) — `unknown_context_hard_fails`.
  - [x] `toggle-roster = "ctrl+g"` plus `scroll-top = "ctrl+g"` hard-fails (duplicate key) — `duplicate_key_hard_fails`.
  - [x] `frobnicate = "ctrl+g"` loads OK with the entry dropped and a warning present (unknown action) — `unknown_action_soft_fails_with_a_warning`.
  - [x] `toggle-roster = "ctrl+"` hard-fails (malformed key) — `malformed_key_hard_fails`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Each hard-fail class produces a precise file/field/value error; unknown actions soft-fail with a warning.
- The local-ignored warning from task_06 appears in `keybinding_warnings`.

## Implementation Notes
- **Validation runs in `apply_raw` (via a new `apply_keybindings`), not `into_effective`**
  as the spec sketch suggested — the file name (`source_name`) is only available per
  layer, and the binding requirement is a "file + field + value" error. `into_effective`
  just carries the already-validated `MergedConfig.keybindings` onto `EffectiveConfig`.
- `MergedConfig.keybindings` changed from task_06's raw map to `KeybindingOverrides`
  (parsed eagerly); the raw map + `RawKeyBinding` `allow(dead_code)` markers were removed
  (now read). Each user-scope layer is parsed, then `validate_overrides` runs on the merged
  accumulator so cross-layer duplicates are caught too.
- Severity split (ADR-004): hard-fail (anyhow `Err`) on unknown context (≠`normal`),
  malformed key, `= true`, reserved/non-portable, and duplicate; soft-fail (drop + warn)
  on unknown action. Errors are flattened with `anyhow!("... {err} ...")` so the file +
  field + value all appear in `to_string()`/`{:#}`.
- Added `keybindings::key_action_from_name` (inverse of `action_name`) in task_01's module
  for the action lookup, with a round-trip test.
- Verified `2026-06-16`: 1 keybindings test + 10 config tests pass; clippy `--all-targets`
  clean; fmt clean; full `cargo test --lib` 1031 passed, only pre-existing environmental
  failures (no config/keybindings regression).
