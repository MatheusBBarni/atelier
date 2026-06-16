---
status: pending
title: Keybindings foundation module
type: backend
complexity: medium
dependencies: []
---

# Keybindings foundation module

## Overview
Create the shared `src/keybindings.rs` module that owns the key vocabulary (a `KeyChord`
wrapper with parse/format, the portable-key allowlist), the closed `KeyAction` enum, the default
binding table, and the `Keymap` resolver plus validator. It is pure and dependency-free; every
other task in this feature builds on it.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `src/keybindings.rs` and declare it as `pub mod keybindings;` in `src/lib.rs`.
- MUST implement `parse_key`/`format_key` for the `ctrl+k` syntax — lowercase canonical output,
  case-insensitive parsing — over crossterm `KeyEvent`/`KeyCode`/`KeyModifiers`, per the TechSpec
  "Core Interfaces" section.
- MUST enforce the portable allowlist in `is_portable`: Ctrl+letter except `C/D/I/M/[`, `F1..F12`,
  arrows, `PageUp`/`PageDown`/`Home`/`End`; everything else is rejected.
- MUST define the closed `KeyAction` enum (the 10 actions in the TechSpec "Data Models" DEFAULTS
  table) with serde kebab-case rename, the `DEFAULTS` table, the `KeybindingOverrides` type, and
  `Keymap` with `resolve`/`action_for`/`entries`.
- MUST implement `validate_overrides` rejecting reserved-key binds, non-portable keys, and
  duplicates (two actions resolving to the same key).
- MUST NOT depend on the `tui` or `config` modules (both depend on this one); the
  `KeyAction → TuiCommand` bridge lives in `tui` (task_04).
- SHOULD surface parse/validation failures with precise `field + value` messages using the
  codebase's `anyhow`/`.context()` style (no new `thiserror` dependency).
</requirements>

## Subtasks
- [ ] 1.1 Create `src/keybindings.rs` and declare the module in `src/lib.rs`.
- [ ] 1.2 Implement `KeyChord`, `parse_key`, and `format_key` (case-insensitive parse, canonical lowercase output).
- [ ] 1.3 Implement `is_portable` and reject keys outside the allowlist.
- [ ] 1.4 Define `KeyAction` (10), the `DEFAULTS` table, and `KeybindingOverrides`.
- [ ] 1.5 Implement `Keymap::resolve`/`action_for`/`entries` and `validate_overrides`.
- [ ] 1.6 Add unit tests covering parse/format, allowlist, resolve, and each validation failure class.

## Implementation Details
New module file `src/keybindings.rs`, declared in `src/lib.rs` next to `pub mod config;`. The
crate is the library `multiagent` (binary `atelier`); crossterm/serde are already dependencies —
no new crates. See TechSpec "Core Interfaces" (KeyChord/parse/format/Keymap signatures), "Data
Models" (the DEFAULTS action→key table), and ADR-003 for the data model and naming. Keep the
module free of `tui`/`config` imports.

### Relevant Files
- `src/lib.rs` — add `pub mod keybindings;` (module declaration site, `src/lib.rs:1-18`).
- `src/keybindings.rs` — new file containing all types and functions.
- `Cargo.toml` — confirm crossterm/serde present (no edits expected).

### Dependent Files
- `src/tui/mod.rs` — will consume `KeyAction`/`Keymap`/`format_key` (task_04, task_05, task_08).
- `src/config/mod.rs` — will consume `parse_key`/`validate_overrides`/`KeyAction` (task_06, task_07).

### Related ADRs
- [ADR-003: Keymap Data Model and Resolution](adrs/adr-003.md) — schema, KeyAction, Keymap, syntax.
- [ADR-001: V1 Scope](adrs/adr-001.md) — portable allowlist and reserved set.

## Deliverables
- `src/keybindings.rs` with `KeyChord`, `parse_key`/`format_key`, `is_portable`, `KeyAction`,
  `DEFAULTS`, `KeybindingOverrides`, `Keymap` (`resolve`/`action_for`/`entries`), `validate_overrides`.
- `pub mod keybindings;` added to `src/lib.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test exercising parse → resolve together **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `parse_key("ctrl+k")` yields `Char('k')`+CONTROL; `parse_key("CTRL+K")` equals it (case-insensitive); `parse_key("pageup")` yields `PageUp`.
  - [ ] `format_key` round-trips each allowlisted chord to its canonical lowercase form (`ctrl+k`, `pageup`, `home`, `f5`).
  - [ ] `parse_key("ctrl+")` and `parse_key("squiggle")` return an error.
  - [ ] `is_portable`: `ctrl+a` true; `ctrl+c`, `ctrl+i`, `ctrl+m` false; `ctrl+1` false; `alt+a` false; `f5`/`pageup` true.
  - [ ] `validate_overrides`: ToggleRoster→`ctrl+c` errors (reserved); →`ctrl+1` errors (non-portable); two actions →`ctrl+g` errors (duplicate); a valid map is Ok.
  - [ ] `Keymap::resolve`: overriding ToggleRoster→`ctrl+g` removes the `ctrl+l` default; unbinding it removes the action; untouched actions keep defaults; `action_for` returns the expected action.
- Integration tests:
  - [ ] `Keymap::resolve(DEFAULTS, {ToggleRoster:Some(ctrl+g), ScrollTop:None})` produces a lookup with `ctrl+g`→ToggleRoster, no `ctrl+l`, and no `home`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Module compiles with no `tui`/`config` dependency; `cargo clippy --all-targets` and `cargo fmt --check` clean.
- `DEFAULTS` reproduces today's bindings plus the five editing actions, per the TechSpec table.
