---
status: completed
title: Config keybindings section and ConfigLayer trust boundary
type: backend
complexity: medium
dependencies:
  - task_01
---

# Config keybindings section and ConfigLayer trust boundary

## Overview
Add the `[keybindings]` config section and a `ConfigLayer` marker so keybindings are honored only
from user-scope layers (home config / explicit `--config`), and a project-local `./atelier.toml`
`[keybindings]` is ignored with a warning. This is the trust boundary that keeps an untrusted
repo from reconfiguring control keys.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `keybindings: Option<BTreeMap<String, BTreeMap<String, RawKeyBinding>>>` (context →
  action → value) to `RawConfig` (`src/config/mod.rs:417`), keeping `#[serde(deny_unknown_fields)]`;
  `RawKeyBinding` is an untagged enum accepting a key string or `false`.
- MUST introduce `enum ConfigLayer { Builtin, Cli, Home, Local }` and thread it through
  `apply_config_file` (`:1713`) and `apply_raw` (`:909`) at every call site in
  `load_effective_config` (`:1654-1688`).
- MUST apply `[keybindings]` only when `layer != Local`; when a `Local` layer carries
  `[keybindings]`, skip it and record a warning naming the file (consumed by task_07/task_09).
- MUST NOT change how existing sections merge — they continue to apply on every layer.
</requirements>

## Subtasks
- [x] 6.1 Define `RawKeyBinding` (untagged key-string | `false`) and add the `keybindings` field to `RawConfig`.
- [x] 6.2 Add the `ConfigLayer` enum.
- [x] 6.3 Thread `ConfigLayer` through `apply_config_file`/`apply_raw` and all `load_effective_config` call sites.
- [x] 6.4 Gate `[keybindings]` on `layer != Local`; record a warning when ignored from local config.
- [x] 6.5 Add tests for user-scope parsing and local-layer ignore-with-warning.

## Implementation Details
Within `src/config/mod.rs`: `RawConfig` (`:417`, keep `deny_unknown_fields`), `apply_raw` (`:909`),
`apply_config_file` (`:1713`), the layer call sites in `load_effective_config` (`:1654-1688`), and
`MergedConfig` (`:606`) as the warning carrier. The `load_from_temp` test helper (`:2417`) drives
config tests; for the local layer, write `working_directory/atelier.toml` and call
`load_effective_config` with `config_path: None`. See TechSpec "Core Interfaces" (ConfigLayer),
"Data Models" (RawConfig addition), and ADR-004.

### Relevant Files
- `src/config/mod.rs` — `RawConfig`, `apply_raw`, `apply_config_file`, `load_effective_config`, `MergedConfig`.
- `src/keybindings.rs` — types referenced by parsing (full parse/validate lands in task_07).

### Dependent Files
- `src/config/mod.rs` (task_07) — consumes the parsed section + the local-ignored warning.

### Related ADRs
- [ADR-004: Config Trust Boundary and Validation Severity](adrs/adr-004.md) — ConfigLayer, user-scope, ignore+warn on local.
- [ADR-001: V1 Scope](adrs/adr-001.md) — user-scope loading.

## Deliverables
- `RawKeyBinding` + `RawConfig.keybindings`; `ConfigLayer` threaded through the loader.
- Local-layer `[keybindings]` ignored with a recorded warning; user-scope honored.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests via `load_from_temp` / working-dir config **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `RawKeyBinding` deserializes both a key string (`"ctrl+g"`) and `false` — `raw_keybinding_deserializes_key_string_and_false`.
  - [x] `deny_unknown_fields` still rejects a typo'd top-level section — `deny_unknown_fields_still_rejects_unknown_top_level_key`.
- Integration tests:
  - [x] A user-scope (`--config`/home) `[keybindings.normal]` is accepted without an ignore warning — `user_scope_keybindings_are_accepted_without_warning`. (Full parse into overrides is asserted end-to-end in task_07.)
  - [x] A `working_directory/atelier.toml` `[keybindings]` with `config_path: None` is ignored and a warning naming the file is recorded — `local_keybindings_are_ignored_with_a_warning_naming_the_file`.
  - [x] An existing section (`[ui]`) in that same local file still merges (unchanged behavior) — same test asserts `config.ui.hide_banner`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Keybindings parse only from user scope; local config is ignored with a precise warning.
- `deny_unknown_fields` preserved; other sections' merge behavior unchanged.

## Implementation Notes
- `apply_raw`/`apply_config_file` gained a `ConfigLayer` param; the three
  `load_effective_config` call sites pass `Cli` (explicit `--config`/env), `Home`
  (default home), and `Local` (`./atelier.toml`). `Builtin` exists for completeness
  (`#[allow(dead_code)]` — base defaults are built directly, not via `apply_raw`).
- Raw `[keybindings]` is accumulated on `MergedConfig.keybindings` (merged per
  context/action, later user-scope layers win) for task_07 to parse — the field is
  `#[allow(dead_code)]` until task_07 reads it. `RawKeyBinding` payloads likewise carry
  `allow(dead_code)` (deserialized now, read in task_07). A `Local`-layer `[keybindings]`
  is skipped with a warning pushed to `MergedConfig.keybinding_warnings`, surfaced on the
  new `EffectiveConfig.keybinding_warnings`.
- **Scope boundary with task_07:** task_06 proves the trust boundary (accept user-scope,
  ignore+warn local) and the schema/`deny_unknown_fields`; the actual parse+validate of
  the accumulated raw map into `EffectiveConfig.keybindings` (KeybindingOverrides) is
  task_07, so the user-scope test asserts "accepted, no warning" rather than the resolved
  overrides.
- **`load_from_temp` caveat:** it passes the temp file as both `config_path` (Cli) and the
  working-dir local file, so it double-applies. The user-scope test uses a separate
  `load_user_scope_config` helper (config file named `home-config.toml`, not re-discovered
  as local) to exercise the Cli layer cleanly.
- Verified `2026-06-16`: 4 new tests pass; clippy `--all-targets` clean; fmt clean; full
  `cargo test --lib` shows only pre-existing environmental failures (no config/keybindings
  regression).
