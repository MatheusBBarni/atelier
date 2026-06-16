---
status: pending
title: Keybinding doctor check and config surfaces
type: backend
complexity: medium
dependencies:
  - task_07
---

# Keybinding doctor check and config surfaces

## Overview
Surface keybinding diagnostics and make the effective keymap visible and authorable: a doctor check
plus a startup notice from `keybinding_warnings`, the effective keymap emitted in `--print-config`,
and a commented `[keybindings]` block in `--init-config`. This completes Wave 2's observability and
documentation surfaces.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `config.keybindings` `DoctorCheck` in `run_doctor` (`src/doctor/mod.rs:55`): status
  `Ok` when `EffectiveConfig.keybinding_warnings` is empty, `Warn` otherwise, with the warnings in
  the message and `context`.
- MUST surface the warnings as a TUI startup notice by extending `config_warning_messages()`
  (`src/app/mod.rs:5451`) so they appear in `ConfigStatusView.warnings` / `/config`.
- MUST emit the effective (post-merge) keybindings in `--print-config` by adding a `keybindings`
  field to `PrintableConfig` (`src/config/mod.rs:1956`) and populating it in
  `build_printable_config` (`:2035`) via `format_key`.
- MUST add a commented `[keybindings]` example block to `starter_config_text` (`:2195`)
  documenting the `ctrl+k` syntax, `action → key`, `false` to unbind, the `Ctrl-U` = kill-to-start
  note, and that the section is user-scope only.
</requirements>

## Subtasks
- [ ] 9.1 Add the `config.keybindings` doctor check driven by `keybinding_warnings`.
- [ ] 9.2 Extend `config_warning_messages()` to surface keybinding warnings at startup.
- [ ] 9.3 Add `PrintableConfig.keybindings` and populate it in `build_printable_config` via `format_key`.
- [ ] 9.4 Add the commented `[keybindings]` block to the `--init-config` starter template.
- [ ] 9.5 Add tests for the doctor check, print-config emission, init-config block, and startup notice.

## Implementation Details
`src/doctor/mod.rs`: `run_doctor` (`:55`) and the `DoctorCheck` type (`:11-45`); follow the
`model_fallback_check` pattern (`:164`). `src/config/mod.rs`: `PrintableConfig` (`:1956`),
`build_printable_config` (`:2035`), `to_redacted_toml` (`:2142`), `starter_config_text` (`:2195`).
`src/app/mod.rs`: `config_warning_messages()` (`:5451`) and `ConfigStatusView` (`:192`). Reads
`EffectiveConfig.keybindings`/`keybinding_warnings` (task_07) and uses `keybindings::format_key`.
See TechSpec "Monitoring and Observability" and "API Endpoints"; ADR-004 (diagnostics surface).

### Relevant Files
- `src/doctor/mod.rs` — new `config.keybindings` check in `run_doctor`.
- `src/config/mod.rs` — `PrintableConfig`, `build_printable_config`, `starter_config_text`.
- `src/app/mod.rs` — `config_warning_messages()`, `ConfigStatusView`.
- `src/keybindings.rs` — `format_key` for emitting the effective keymap.

### Dependent Files
- None downstream.

### Related ADRs
- [ADR-004: Config Trust Boundary and Validation Severity](adrs/adr-004.md) — diagnostics via doctor/startup.
- [ADR-002: Parity-First Delivery](adrs/adr-002.md) — Wave 2 doctor/emit surfaces.

## Deliverables
- `config.keybindings` doctor check; startup notice via `config_warning_messages()`.
- `--print-config` emits the effective keymap; `--init-config` includes a commented `[keybindings]` block.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for doctor and `--print-config`/`--init-config` output **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `config_warning_messages()` includes a keybinding warning when `keybinding_warnings` is non-empty.
  - [ ] `starter_config_text()` contains `[keybindings` and the `ctrl+k`/`false`/`Ctrl-U` syntax comments.
- Integration tests:
  - [ ] A config with a soft warning yields a `config.keybindings` doctor check with status `Warn` (tokio test via `load_effective_config` + `run_doctor`); a clean config yields `Ok`.
  - [ ] `to_redacted_toml` of a config with `toggle-roster = "ctrl+g"` contains a `[keybindings]` table showing the effective `ctrl+g` mapping.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Doctor warns on keybinding issues; `--print-config` shows the effective keymap; `--init-config` documents the section.
- Wave 2 is complete (remap engine + config + unbind + diagnostics).
