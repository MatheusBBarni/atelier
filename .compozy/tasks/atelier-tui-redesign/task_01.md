---
status: completed
title: Add `[ui]` config section with `hide_banner`
type: backend
complexity: low
dependencies: []
---

# Task 1: Add `[ui]` config section with `hide_banner`

## Overview

Extend the TOML configuration schema with an optional `[ui]` section carrying `hide_banner: bool` (default `false`), so the welcome screen (task_04) can be suppressed by configuration. This follows the exact raw/effective/merge pattern the `[features]` section already uses and ships the hide setting proactively (PRD risk: banner fatigue).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. `EffectiveConfig` MUST gain a `ui: UiConfig` field with `hide_banner: bool` defaulting to `false` (TechSpec "Data Models").
2. `RawConfig` MUST gain `ui: Option<RawUiConfig>` with `#[serde(deny_unknown_fields)]`, matching every other `Raw*` struct in the module.
3. A config file without a `[ui]` section MUST parse identically to today (backward compatible).
4. An unknown key inside `[ui]` MUST fail parsing (deny_unknown_fields behavior preserved).
5. The starter config template SHOULD include a commented `[ui]` example.
6. `PrintableConfig`/`to_redacted_toml` MUST include the `ui` section so `/config`-adjacent output stays complete.
</requirements>

## Subtasks
- [x] 1.1 Define `UiConfig` (public, Default/Serialize/Deserialize) and `RawUiConfig` structs mirroring the `Features`/`RawFeatures` pair.
- [x] 1.2 Add the field to `RawConfig`, `EffectiveConfig`, the built-in defaults init, and the `apply_raw` merge.
- [x] 1.3 Extend `PrintableConfig` and `to_redacted_toml` with the `ui` section.
- [x] 1.4 Add a `[ui]` example to `starter_config_text()`.
- [x] 1.5 Add parse tests: absent section default, explicit `hide_banner = true`, unknown-key rejection.

## Implementation Details

Single-file change in `src/config/mod.rs`. Mimic the `[features]` flow end to end: `RawFeatures` (:403-405) → `Features` (:195-198) → `RawConfig.features` (:380) → `EffectiveConfig.features` (:366) → builtin init (:759) → `apply_raw` merge (:795-799). See TechSpec "Data Models" for the target shape.

### Relevant Files
- `src/config/mod.rs` — `RawConfig` (:375-386), `apply_raw` (:769-863), `EffectiveConfig` (:359-371), `PrintableConfig` (:1770-1781), `to_redacted_toml` (:1842-1944), `starter_config_text()` (:1995-2128), test module with `load_from_temp` helper (:2229-2237).

### Dependent Files
- `src/config/mod.rs` test module (:2224-2892) — gains the new `[ui]` parse tests alongside existing section tests.
- `src/app/mod.rs` — `build_config_status` (:4594-4623) reads `EffectiveConfig`; no change required for a count-only status view, but compilation confirms the struct extension is complete.
- `src/tui/mod.rs` — future consumer (task_04 reads `config.ui.hide_banner`); no change in this task.

### Related ADRs
- [ADR-001: V1 Scope and Sequencing](../adrs/adr-001.md) — hide setting ships in V1, not as a reactive patch.

## Deliverables
- `UiConfig`/`RawUiConfig` structs wired through raw → merge → effective → printable.
- Starter config example updated.
- Unit tests with 80%+ coverage of the new merge/default paths **(REQUIRED)**
- Integration test via `load_from_temp` round-trip **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Config with no `[ui]` section parses and `ui.hide_banner == false`.
  - [x] Config with `[ui]\nhide_banner = true` parses and `ui.hide_banner == true`.
  - [x] Config with `[ui]\nunknown_key = 1` fails to parse (deny_unknown_fields).
  - [x] `to_redacted_toml` output contains the `[ui]` section with the effective value.
- Integration tests:
  - [x] `load_from_temp` round-trip with a preset plus `[ui]` section yields merged `hide_banner` from the project file.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Existing config tests pass unchanged (backward compatibility proven).
- `cargo build` succeeds with the extended schema.
