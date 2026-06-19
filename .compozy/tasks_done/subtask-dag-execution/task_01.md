---
status: completed
title: "Config execution_graph feature flag"
type: backend
complexity: low
dependencies: []
---

# Task 01: Config execution_graph feature flag

## Overview
Add a default-off `features.execution_graph` flag that gates the entire DAG capability and coexists with the existing `parallel_step_groups` flag, and confirm that the existing `limits.max_parallel_agent_steps` ceiling is reused unchanged as the DAG's concurrency cap. This is the safe-posture enable switch every later task gates on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `execution_graph: bool` to the effective `Features` struct, defaulting to `false`.
- MUST add `execution_graph: Option<bool>` to `RawFeatures` (the struct carries `#[serde(deny_unknown_fields)]`, so an unknown key in `atelier.toml` would otherwise hard-fail config load) and a matching last-writer-wins merge arm in `apply_raw`.
- MUST reuse `limits.max_parallel_agent_steps` (u32, `0 = disable`) unchanged as the DAG concurrency ceiling — MUST NOT convert it to a `Limit` (the `Limit` deserializer rejects `0`).
- MUST keep `parallel_step_groups` and its semantics untouched (the two flags coexist; the DAG flag does not change flat-group behavior).
- MUST keep config `schema_version` at 1 (config additions are additive/optional; this is orthogonal to the orchestrator decision schema version).
</requirements>

## Subtasks
- [x] 1.1 Add the `execution_graph` field to `Features` with a `false` default.
- [x] 1.2 Add `execution_graph: Option<bool>` to `RawFeatures` and the merge arm in `apply_raw`.
- [x] 1.3 Confirm `PrintableConfig` surfaces the new flag automatically via the existing `features` clone.
- [x] 1.4 Document (in code/tests) that `max_parallel_agent_steps == 0` disables the DAG, mirroring the flat-group preflight contract.
- [x] 1.5 Add unit tests covering default-off, TOML round-trip, and the 0-ceiling-disables contract.

## Implementation Details
All changes are confined to `src/config/mod.rs`. Follow the existing `parallel_step_groups` wiring exactly (effective field → raw field → `apply_raw` arm). See TechSpec "Implementation Design → Data Models" and ADR-005 for the flag rationale. Do not add a new TOML section; extend `Features`/`RawFeatures`.

### Relevant Files
- `src/config/mod.rs` — `Features` (`:196`), `RawFeatures` (`:447`), `apply_raw` features arm (`:938`), `Limits.max_parallel_agent_steps` (`:179`), `PrintableConfig`/`build_printable_config` (`:1955`/`:2035`).

### Dependent Files
- `src/orchestrator/mod.rs` — task_02 reads `features.execution_graph` to gate prompt guidance and validation.
- `src/app/mod.rs` — task_04/task_07 read the flag and ceiling.

### Related ADRs
- [ADR-005: DAG user-surface integration](../adrs/adr-005.md) — chooses a distinct `execution_graph` flag over reusing `parallel_step_groups`.

## Deliverables
- `features.execution_graph` flag (default false) wired through effective + raw config and the merge path.
- The `max_parallel_agent_steps` ceiling reused unchanged for the DAG.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for config load/merge of the new flag **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Default config has `execution_graph == false`. (`execution_graph_defaults_off`)
  - [x] An `atelier.toml` with `[features] execution_graph = true` loads and yields `execution_graph == true`. (`execution_graph_parses_true`)
  - [x] An `atelier.toml` with an unrelated unknown `[features]` key still hard-fails (confirms `deny_unknown_fields` is intact and the new key is the only addition). (`execution_graph_unknown_features_key_still_hard_fails`)
  - [x] `max_parallel_agent_steps = 0` is accepted as the disable sentinel (existing contract preserved). (`execution_graph_ceiling_zero_disables_concurrency`)
  - [x] Local config `execution_graph` overrides home config (last-writer-wins via `apply_raw`). (`execution_graph_local_overrides_home`)
- Integration tests:
  - [x] `atelier --print-config` output includes `execution_graph` under features. (`print_config_includes_execution_graph_flag`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `execution_graph` defaults off and is overridable from TOML without breaking existing config load.
- `parallel_step_groups` behavior is unchanged.
