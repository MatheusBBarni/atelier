---
status: completed
title: "Surface DAG state in /config"
type: backend
complexity: low
dependencies:
  - task_01
---

# Task 07: Surface DAG state in /config

## Overview
Make the DAG capability discoverable in-app by extending the `/config` output to show whether DAG planning is enabled, the concurrency ceiling, and the current approval mode — none of which are visible today (they only appear via the CLI `--print-config`). This closes the discoverability gap the PRD calls out for a default-off feature.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extend `ConfigStatusView` with the DAG-relevant fields (DAG enabled, `max_parallel_agent_steps`, `approval_mode`) and populate them in `build_config_status`.
- MUST render the new fields in `config_status_display` so `/config` shows them in the TUI.
- SHOULD add a warning/note when `execution_graph` is enabled but `max_parallel_agent_steps == 0` (enabled-but-disabled-by-ceiling), mirroring the existing config-warning pattern.
- MUST keep the `config_viewed` event payload (a serialized `ConfigStatusView`) valid; update any golden/snapshot assertions over the config display or payload.
- MUST NOT change config merge or defaults (that is task_01); this task is presentation only.
</requirements>

## Subtasks
- [x] 7.1 Added `approval_mode`, `execution_graph_enabled`, `max_parallel_agent_steps` to `ConfigStatusView` (all `#[serde(default)]` so old `config_viewed` payloads still deserialize).
- [x] 7.2 Populated them in `build_config_status` from the effective config.
- [x] 7.3 Rendered them in `config_status_display` (`approval: …; dag: enabled (max_parallel_agent_steps=N) | disabled`) + `approval_mode_label` helper.
- [x] 7.4 Added the enabled-but-ceiling-zero warning in `config_warning_messages`.
- [x] 7.5 Unit + integration tests for the rendered output and the `config_viewed` payload. Updated the two `ConfigStatusView` test literals in `tui/mod.rs` (compiler-forced).

## Implementation Details
Changes are in `src/app/mod.rs` (`ConfigStatusView`, `build_config_status`, `config_status_display`, `handle_config_command`). The new fields read from `EffectiveConfig` (`features.execution_graph`, `limits.max_parallel_agent_steps`, `approval_mode`). See TechSpec "Component Overview → Config" and ADR-005 (close the `/config` visibility gap). The `/config` command grammar is unchanged (exact-match `/config`).

### Relevant Files
- `src/app/mod.rs` — `ConfigStatusView` (`:192`), `build_config_status` (`:5420`), `config_warning_messages` (`:5451`), `config_status_display` (`:6067`), `handle_config_command` (`:1196`).
- `src/config/mod.rs` — `EffectiveConfig` fields (`:401`), `PrintableConfig` (`:1955`) as the format reference.

### Dependent Files
- `src/tui/mod.rs` — Approvals & Modes / help surfaces may reference the same concepts (keep wording aligned).

### Related ADRs
- [ADR-005: DAG user-surface integration](../adrs/adr-005.md) — surface enable-state + ceiling + approval mode in `/config`.

## Deliverables
- `/config` shows DAG-enabled, concurrency ceiling, and approval mode.
- Enabled-but-ceiling-zero warning note.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the `/config` command output **(REQUIRED)**

## Tests
- Unit tests:
  - [x] enabled + ceiling 3 → display shows DAG enabled + max_parallel_agent_steps=3. (`config_display_shows_dag_enabled_with_ceiling`)
  - [x] disabled → display shows DAG disabled. (`config_display_shows_dag_disabled_by_default`)
  - [x] enabled + ceiling 0 → warning present. (`config_warns_when_dag_enabled_but_ceiling_zero`)
  - [x] approval mode (yolo/normal) shown. (`config_display_shows_approval_mode`)
  - [x] `config_viewed` payload serializes the extended view. (`config_status_view_serializes_with_dag_fields`)
- Integration tests:
  - [x] Submitting `/config` records a `config_viewed` event whose payload + display carry the DAG fields. (`config_command_records_dag_fields`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can tell from `/config` whether DAG planning is on, the ceiling, and the approval mode.
- No change to config defaults or merge behavior.
