---
status: completed
title: Add [grading] config section
type: backend
complexity: low
dependencies: []
---

# Task 01: Add [grading] config section

## Overview
Introduce an opt-in `[grading]` configuration section (`enabled`, default false; `max_attempts`, default 2) so users can turn the auto-verification loop on per project. This is the gate every other grading component checks; it ships visible-but-off and changes no default run behavior.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a resolved `GradingConfig { enabled: bool, max_attempts: u32 }` with `enabled` defaulting to false and `max_attempts` defaulting to 2 (matching `limits.max_review_fix_cycles`).
- MUST add `grading: GradingConfig` to `EffectiveConfig` and initialize it in the built-in config constructor.
- MUST add a `RawGradingConfig` mirror carrying `#[serde(deny_unknown_fields)]` and an `apply_raw` merge arm following the `Features`/`RawFeatures` precedent.
- MUST render a `[grading]` block (`enabled = false`, `max_attempts = 2`) in the init-config scaffold so the knob is discoverable.
- MUST NOT add a `verify_command` field (the verify command is agent-discovered in V1 per ADR-002).
</requirements>

## Subtasks
- [x] 01.1 Define the resolved `GradingConfig` struct with its manual `Default` impl.
- [x] 01.2 Wire `grading` into `EffectiveConfig` and the built-in constructor.
- [x] 01.3 Add the `RawGradingConfig` mirror and the `apply_raw` merge arm.
- [x] 01.4 Add a `[grading]` block to the init-config scaffold text.
- [x] 01.5 Cover defaults, layered override, and unknown-field rejection with tests.

## Implementation Note
`GradingConfig { enabled, max_attempts }` threaded through every config representation: resolved
`EffectiveConfig`, internal `MergedConfig` (+ builtin default), `RawConfig`/`RawGradingConfig`
(`deny_unknown_fields`), the `apply_raw` merge arm (after features), and `PrintableConfig` (so
`--print-config` renders it — verified). Init scaffold gained a commented-rationale `[grading]` block.
Five tests cover defaults, local-enable, max_attempts, home→local override, and unknown-key rejection.

## Follow-up (out of scope)
The `config-setup-skill` packet's `references/config-schema.md` does not yet document the new
`[grading]` section. Its drift test only covers the five enums (grading is a struct), so nothing
breaks — but that reference should gain a `[grading]` row for completeness in a separate change.

## Implementation Details
Follow the `Features.parallel_step_groups` opt-in precedent end-to-end (resolved struct, raw mirror, merge arm, scaffold). See TechSpec "Data Models" (the `GradingConfig` definition) and "Build Order" step 1. Exact insertion points are recorded in the config-discoverability findings of `_research-techspec.json`.

### Relevant Files
- `src/config/mod.rs` — add `GradingConfig` near `Features` (~:196), `EffectiveConfig.grading` (~:409) + builtin default (~:898), `RawGradingConfig` + `RawConfig.grading` (~:424), the `apply_raw` arm after the features arm (~:942), and the init scaffold `[grading]` block (~:2200).

### Dependent Files
- `src/app/mod.rs` — later reads `self.config.grading.enabled` / `.max_attempts` (tasks 05, 06); no change in this task.

### Related ADRs
- [ADR-002: Phased delivery — agent-discovered verification in V1](../adrs/adr-002.md) — why there is no `verify_command` field in V1.
- [ADR-003: Harness-driven bounded grade→fix loop](../adrs/adr-003.md) — `max_attempts` bounds the loop.

## Deliverables
- A `[grading]` config section that parses, merges across home/local layers, and defaults to disabled.
- Init-config scaffold updated with a visible-but-off `[grading]` block.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for config load/merge **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Absent `[grading]` → `enabled == false` and `max_attempts == 2`.
  - [ ] `[grading] enabled = true` in local config overrides `enabled = false` from home config.
  - [ ] `[grading] max_attempts = 5` parses and resolves to 5.
  - [ ] Unknown key under `[grading]` (e.g. `verify_command = "x"`) → hard load error from `deny_unknown_fields`.
- Integration tests:
  - [ ] Loading an `EffectiveConfig` from a temp local config with `[grading] enabled = true` yields `config.grading.enabled == true`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `[grading]` parses with correct defaults and layered overrides
- Default run behavior is unchanged when `[grading]` is absent
