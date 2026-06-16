---
status: pending
title: Hooks config through the ladder + drop local-layer hooks
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 2: Hooks config through the ladder + drop local-layer hooks

## Overview
Thread a `[hooks]` section through atelier's config ladder (Raw → Merged → Effective) so handlers parse, validate, and surface via `--print-config`/`--init-config`. Critically, hooks from the **local (`./atelier.toml`) layer are dropped** so a cloned repo cannot register shell commands — the ADR-001 security posture.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `RawHooksConfig` (with `#[serde(deny_unknown_fields)]`) and wire `hooks: Option<RawHooksConfig>` into `RawConfig`, a merged field into `MergedConfig`, and `hooks: HooksConfig` into `EffectiveConfig` — mirroring how `[limits]`/`[ui]` are threaded.
- MUST apply hooks in `apply_raw` ONLY when the source is not the local project layer; hooks present in `./atelier.toml` MUST be ignored (other local overrides MUST still apply).
- MUST validate each handler: `on` accepts a string or a list of public event names; exactly one action (`notify` xor `command`) per handler; unknown public event names are rejected with a clear error.
- MUST add a commented `[[hooks.handler]]` example to `starter_config_text` so `--init-config` scaffolds it.
- SHOULD surface configured hooks through `--print-config` automatically via the existing config projection.
</requirements>

## Subtasks
- [ ] 2.1 Add `RawHooksConfig`/`RawHookHandler` and the `hooks` field to `RawConfig`, `MergedConfig`, `EffectiveConfig`.
- [ ] 2.2 Add the `apply_raw` branch that merges hooks, mirroring `[limits]`/`[ui]`.
- [ ] 2.3 Implement local-layer drop: ignore `raw.hooks` when the source is `./atelier.toml`, with a diagnostic.
- [ ] 2.4 Validate handlers (`on` string|list, exactly-one-action, known public events).
- [ ] 2.5 Add a commented `[[hooks.handler]]` block to `starter_config_text`.
- [ ] 2.6 Add unit tests for parsing, validation, and the local-drop behavior.

## Implementation Details
All edits live in `src/config/mod.rs`. Mirror an existing section end-to-end: `RawConfig` (`:417`, note `deny_unknown_fields`), `MergedConfig` (`:606`), `EffectiveConfig` (`:401`), the `apply_raw` branch (`:909-978`), and `into_effective`. The local override is applied at `:1680-1688` via `apply_config_file`; the local-drop check keys off the source identity passed into `apply_raw` (`source_name`/source dir). Add the scaffold to `starter_config_text` (`:2195-2334`). The reusable `HooksConfig`/`HookHandler` types come from task_01. See TechSpec "System Architecture → Config ladder".

### Relevant Files
- `src/config/mod.rs:417` — `RawConfig` (`deny_unknown_fields`); add `hooks`.
- `src/config/mod.rs:606` — `MergedConfig`; add merged hooks field.
- `src/config/mod.rs:401` — `EffectiveConfig`; add `hooks: HooksConfig`.
- `src/config/mod.rs:909` — `apply_raw`; add the merge branch with the local-drop guard.
- `src/config/mod.rs:1680` — local (`./atelier.toml`) merge call site; source identity for the drop.
- `src/config/mod.rs:2195` — `starter_config_text`; commented `[[hooks.handler]]` example.

### Dependent Files
- `src/app/mod.rs` — task_05 reads `self.config.hooks` at the tap.
- `src/doctor/mod.rs` — task_08 reads configured handlers.
- `src/cli.rs` — `--print-config` projection (`build_printable_config`, `src/config/mod.rs:2035`) shows `[hooks]` automatically.

### Related ADRs
- [ADR-001: V1 ships cross-runtime observer hooks](../adrs/adr-001.md) — ignore repo-local hooks (delete RCE-on-clone).
- [ADR-004: Handler-array config schema](../adrs/adr-004.md) — the `[[hooks.handler]]` shape and validation rules.

## Deliverables
- `[hooks]` section parsing through the full config ladder.
- Local-layer hook drop with a diagnostic; home-layer hooks and other local overrides preserved.
- Handler validation and a scaffolded `--init-config` example.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test: `--print-config` shows a configured `[hooks]` section **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A `[[hooks.handler]]` array with `on = "run_completed"` + `notify = true` parses into one handler.
  - [ ] `on = ["run_completed","run_failed"]` parses to a two-event handler.
  - [ ] A handler with both `notify` and `command` is rejected (exactly-one-action error).
  - [ ] `on = "not_an_event"` is rejected with an unknown-public-event error.
  - [ ] Hooks in `./atelier.toml` are dropped while a home-config `[ui]` override in the same local file still applies.
  - [ ] An unknown key under `[hooks]` is rejected by `deny_unknown_fields`.
- Integration tests:
  - [ ] `atelier --print-config` renders a configured `[hooks]` section from home config.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `[hooks]` parses, validates, and merges through Raw→Merged→Effective
- Local-layer hooks are ignored; other local overrides unaffected
- `--init-config` scaffolds a commented hooks example
