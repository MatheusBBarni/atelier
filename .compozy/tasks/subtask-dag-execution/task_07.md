---
status: pending
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
- [ ] 7.1 Add the DAG/approval fields to `ConfigStatusView`.
- [ ] 7.2 Populate them in `build_config_status` from the effective config.
- [ ] 7.3 Render them in `config_status_display`.
- [ ] 7.4 Add the enabled-but-ceiling-zero warning note.
- [ ] 7.5 Add/refresh unit tests for the rendered output and the `config_viewed` payload.

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
  - [ ] With `execution_graph = true` and ceiling 3, `config_status_display` shows DAG enabled and `max_parallel_agent_steps = 3`.
  - [ ] With `execution_graph = false`, the display shows DAG disabled.
  - [ ] With `execution_graph = true` and ceiling 0, a warning note is present.
  - [ ] `approval_mode` (yolo/normal) is shown in the display.
  - [ ] The `config_viewed` event payload serializes the extended `ConfigStatusView` without error.
- Integration tests:
  - [ ] Submitting `/config` records a `config_viewed` event whose display contains the DAG fields.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can tell from `/config` whether DAG planning is on, the ceiling, and the approval mode.
- No change to config defaults or merge behavior.
