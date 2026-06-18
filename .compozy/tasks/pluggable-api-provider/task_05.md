---
status: pending
title: "Config template, doctor migration hint, and docs"
type: docs
complexity: low
dependencies:
  - task_02
---

# Task 05: Config template, doctor migration hint, and docs

## Overview

Users need to discover that BYOK is available and know how to configure it. This task adds a commented-out HTTP provider example to the starter config template, adds a migration hint in `atelier --doctor` for legacy `type = "zai"` configs, and updates the README to document the new runtime kind.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC "Implementation Details" for the starter template insert point
- FOCUS ON "WHAT" — add discoverability surfaces for BYOK
- MINIMIZE CODE — template text and doctor hint only
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a commented-out HTTP provider example to `starter_config_text()` after the existing runtime sections (after line 2758 in `config/mod.rs`)
- MUST include inline comments explaining `type = "http_api"`, `base_url`, `api_key_env`, `auth_header_name`, `auth_header_prefix`
- MUST add a doctor migration hint that detects legacy `type = "zai"` in user configs and suggests updating to `type = "http_api"`
- MUST update the README runtimes section to document `http_api` as a runtime kind
- MUST NOT change runtime behavior — docs and template only
</requirements>

## Subtasks
- [ ] 05.1 Add commented-out HTTP provider example to `starter_config_text()` in `config/mod.rs` after line 2758
- [ ] 05.2 Add doctor migration hint: detect `type = "zai"` in config and print a warning suggesting `type = "http_api"`
- [ ] 05.3 Update README.md runtimes section (around line 319) to document `http_api` runtime kind
- [ ] 05.4 Verify `atelier --init-config` produces the updated template
- [ ] 05.5 Verify `atelier --doctor` shows migration hint for legacy configs

## Implementation Details

The starter config template is in `src/config/mod.rs:2684` (`starter_config_text()`). The HTTP provider example should be inserted between the existing `[runtimes.zai]` section (now `[runtimes.http_api]`, line 2758) and the `[limits]` section (line 2760).

The doctor migration hint goes in `src/doctor/mod.rs`. The existing runtime validation loop (line 79) already iterates all configured runtimes — add a check for `type = "zai"` string in the config source.

See TechSpec "Monitoring and Observability" section for the doctor output format.

### Relevant Files
- `src/config/mod.rs` — `starter_config_text()` (line 2684)
- `src/doctor/mod.rs` — runtime validation loop (line 79)
- `README.md` — runtimes section (line 319)

### Dependent Files
- `src/config/mod.rs` — already modified in Task 02 (rename)

### Related ADRs
- [ADR-002: Rename RuntimeKind::Zai to RuntimeKind::HttpApi](adrs/adr-002.md) — The migration hint supports this breaking change

## Deliverables
- Updated `starter_config_text()` with HTTP provider example
- Doctor migration hint for legacy `type = "zai"` configs
- Updated README runtimes section
- Unit tests with 80%+ coverage **(REQUIRED)**
- `cargo test --lib` passes **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `starter_config_text()` output contains `type = "http_api"`
  - [ ] `starter_config_text()` output contains commented-out HTTP provider example
  - [ ] Doctor detects legacy `type = "zai"` and produces migration warning
  - [ ] Doctor does NOT produce migration warning for `type = "http_api"`
- Integration tests:
  - [ ] `cargo test --lib` passes
  - [ ] `cargo clippy --all-targets` passes
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Starter config includes documented HTTP provider example
- Doctor shows migration hint for legacy configs
- README documents `http_api` runtime kind
