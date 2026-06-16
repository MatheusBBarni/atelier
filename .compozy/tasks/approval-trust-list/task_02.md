---
status: completed
title: Approval floor configuration
type: backend
complexity: low
dependencies: []
---

# Task 02: Approval floor configuration

## Overview
Introduce the `[approval]` config section that controls the gray-area floor's posture (`warn` default, `enforce` opt-in), the lever ADR-002's phased rollout depends on. The catastrophic core is intentionally NOT configurable. Surface the resolved value in `--print-config` and `--doctor` so users can see and tune it.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `FloorPolicy { Warn, Enforce }` (serde snake_case, default `Warn`) and `ApprovalConfig { floor }` in `src/config/mod.rs`.
- MUST add `approval: ApprovalConfig` to `EffectiveConfig` and a raw counterpart merged through the existing defaults → home → local → CLI chain.
- MUST default `floor` to `Warn` when `[approval]` is absent, preserving today's behavior for gray-area actions.
- MUST NOT expose any config that disables the catastrophic core.
- MUST include the resolved `approval_mode` and `floor` in `--print-config` output and add an approval check to `--doctor`.
- SHOULD follow the existing `Features`/`UiConfig` raw-merge pattern exactly.
</requirements>

## Subtasks
- [x] 02.1 Add `FloorPolicy` and `ApprovalConfig` types with defaults.
- [x] 02.2 Wire `approval` into `EffectiveConfig`, `RawConfig`, and the merge path.
- [x] 02.3 Ensure `--print-config` renders the `[approval]` section and `approval_mode`.
- [x] 02.4 Add a `--doctor` check reporting `approval_mode` and `floor`.
- [x] 02.5 Add config-merge and default tests.

## Implementation Details
Work is in `src/config/mod.rs` (mirroring `Features` ~196 and `UiConfig` ~203, and `EffectiveConfig` ~402 / `RawConfig` ~419), plus the doctor check in `src/doctor/mod.rs` and print-config in `src/cli.rs`/config render. No new files. See TechSpec "Data Models" for the config shape and "Monitoring and Observability" for the doctor/print-config surfacing.

### Relevant Files
- `src/config/mod.rs` — `EffectiveConfig` (~402), `RawConfig` (~419), `Features` (~196), `UiConfig` (~203) patterns; default config template (~2195).
- `src/doctor/mod.rs` — checks list to extend.
- `src/cli.rs` — `--print-config` / `--init-config` entry points.

### Dependent Files
- `src/app/mod.rs` (task_04) — reads `config.approval.floor` to build the action context.
- `src/actions/mod.rs` (task_03) — `FloorPolicy` is consumed by the enforcement matrix.

### Related ADRs
- [ADR-002: Phased floor rollout with a non-bypassable catastrophic core](../adrs/adr-002.md) — `warn` default, opt-in `enforce`, catastrophic non-configurable.

## Deliverables
- `[approval] floor` config with merge + defaults in `src/config/mod.rs`.
- `--print-config` and `--doctor` visibility for `approval_mode` + `floor`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test asserting `--print-config` shows the section **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Absent `[approval]` → `floor == Warn` (default preserved).
  - [x] `[approval] floor = "enforce"` in local config overrides home `warn`.
  - [x] Invalid `floor` value → config load error names the field.
  - [x] `--doctor` check reports the resolved `approval_mode` and `floor`.
- Integration tests:
  - [x] `atelier --print-config` output contains `[approval]` with the resolved `floor` (via the `tests/cli` suite).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `floor` defaults to `warn`, is overridable per the merge chain, and is visible in `--print-config`/`--doctor`.
- No configuration path can disable the catastrophic core.
