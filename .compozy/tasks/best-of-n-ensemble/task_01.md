---
status: pending
title: "Ensemble config block and feature flag"
type: backend
complexity: medium
dependencies: []
---

# Task 01: Ensemble config block and feature flag

## Overview
Add the `[ensemble]` configuration block and the `features.ensemble` flag that gate the `/race` workflow. This is the foundational plumbing every downstream task reads — the roster preset, attempt cap, routing threshold, and the default-off enablement.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add an `EnsembleConfig` struct mirroring the shape in TechSpec "Data Models" (`enabled` default false, `max_attempts` default 3 and hard-capped at 3, `timeout_seconds`, `default_preset`, `presets` map of `RosterMember`, `min_route_samples`).
- MUST add `ensemble: bool` to the `Features` struct and `RawFeatures`, wired into the home→local→flags merge exactly like the existing flags.
- `RosterMember` MUST carry runtime/model/effort, mirroring `CouncilMemberProfile`.
- Defaults MUST keep the feature OFF; an absent `[ensemble]` block MUST deserialize to the default config.
- MUST surface the enabled state and roster in `--print-config` round-trip without losing fields.
</requirements>

## Subtasks
- [ ] 1.1 Define `EnsembleConfig` + `RosterMember` and their `Default` impls.
- [ ] 1.2 Add `ensemble` to `Features` + `RawFeatures` and the merge logic.
- [ ] 1.3 Wire `EnsembleConfig` into the `EffectiveConfig` root and the raw/merge layer.
- [ ] 1.4 Enforce the `max_attempts` cap (3) at load time.
- [ ] 1.5 Add config tests for defaults, merge precedence, and the cap.

## Implementation Details
Follow the existing config-section pattern (see TechSpec "Data Models" and "Command & Config Surface"). The `[council]`/`[grading]` blocks are the closest templates: a typed struct + a `Raw*` optional-field struct + a merge arm. Keep the cap enforcement at the merge/load boundary so downstream code can trust `max_attempts <= 3`.

### Relevant Files
- `src/config/mod.rs:261` — `Features` struct; add `ensemble: bool`.
- `src/config/mod.rs:690` — `RawFeatures`; add `ensemble: Option<bool>`.
- `src/config/mod.rs:1331` — feature merge arm; add the `ensemble` merge.
- `src/config/mod.rs:287` — `GradingConfig` as the simplest block template.
- `src/config/mod.rs:512` — `CouncilConfig`/`CouncilMemberProfile` as the preset/roster template.
- `src/config/mod.rs:566` — `EffectiveConfig` root; add `pub ensemble: EnsembleConfig`.

### Dependent Files
- `src/cli.rs:181` — `--print-config` rendering must round-trip the new block.
- Task 05 / Task 07 / Task 12 — read `ensemble.*` (consumers).

### Related ADRs
- [ADR-003: Race-Led, Router-Active V1](../adrs/adr-003.md) — the `[ensemble]` gate and default-off posture.
- [ADR-004: Minimal Routing in V1](../adrs/adr-004.md) — `min_route_samples` semantics.

## Deliverables
- `EnsembleConfig` + `RosterMember` types with defaults and the `max_attempts` cap.
- `features.ensemble` flag wired through raw + merge.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for config load/merge of `[ensemble]` **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Absent `[ensemble]` block deserializes to defaults with `enabled=false`, `max_attempts=3`.
  - [ ] `max_attempts = 5` in TOML is clamped to 3 at load.
  - [ ] `features.ensemble = true` in local config overrides a home-config `false` (merge precedence via `load_from_temp` + `load_user_scope_config`).
  - [ ] Unknown field under `[ensemble]` is rejected (`deny_unknown_fields`).
- Integration tests:
  - [ ] `--print-config` round-trips an `[ensemble]` preset roster without dropping members.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- An absent block keeps the feature off; a configured block exposes roster + thresholds to downstream tasks.
- `--print-config` faithfully round-trips the block.
