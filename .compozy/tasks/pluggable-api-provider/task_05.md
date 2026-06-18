---
status: completed
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
- [x] 05.1 Add commented-out HTTP provider (BYOK) example to `starter_config_text()` after the `[runtimes.http_api]` section, documenting `type`/`base_url`/`api_key_env`/`auth_header_name`/`auth_header_prefix`
- [x] 05.2 Add doctor migration hint: `legacy_runtime_type_checks` scans loaded config sources for `type = "zai"` and emits a Warning suggesting `type = "http_api"` (split on a `&[PathBuf]` seam for testing)
- [x] 05.3 Update README.md runtimes section to document `http_api` runtime kind (features list, requirements, hooks-payload line, and the `## Runtimes` entry)
- [x] 05.4 Verified `atelier --init-config --config <tmp>` produces the updated template (contains `type = "http_api"` + the commented `# [runtimes.openrouter]` BYOK example)
- [x] 05.5 Verified doctor migration-hint detection (unit-tested); documented that a `type = "zai"` config fails at load before `--doctor` (the parse error itself names `http_api`) — see follow-up note

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
  - [x] `starter_config_text()` output contains `type = "http_api"` (`starter_config_text_documents_http_api_byok_example`)
  - [x] `starter_config_text()` output contains commented-out HTTP provider example (same test; asserts `# [runtimes.openrouter]`, `# auth_header_name`, `# auth_header_prefix`; template still parses)
  - [x] Doctor detects legacy `type = "zai"` and produces migration warning (`legacy_runtime_type_check_warns_on_zai_and_not_http_api` + `config_text_uses_legacy_zai_type_detects_legacy_and_ignores_http_api`)
  - [x] Doctor does NOT produce migration warning for `type = "http_api"` (same tests)
- Integration tests:
  - [x] `cargo test --lib` passes (1360 passed; 12 failures are the unchanged external skill-discovery baseline)
  - [x] `cargo clippy --all-targets` passes; `cargo fmt --check` clean
- Test coverage target: >=80% (3 new tests + verified template render via `--init-config`)
- All tests must pass

## Follow-up Notes
- **Doctor migration hint only fires for configs that still load.** Because task_02's clean rename makes `type = "zai"` an unknown enum variant, a config whose active runtime uses it fails to deserialize before `--doctor` reaches the check. In practice the user sees the load-time error ``unknown variant `zai`, expected one of ... `http_api` ...`` — which already names the correct replacement — plus the README migration note. The doctor warning covers configs that load yet reference the legacy literal in a source file. **Recommended follow-up:** surface the same migration hint on the config-load failure itself (wrap the deserialize error), which would cover the common case directly; deferred here to keep this task docs/diagnostics-only.
- **README `zai` references retained where correct:** `ZAI_API_KEY` (the default env var name) and `https://api.z.ai/api/paas/v4` (the default endpoint) are unchanged; only runtime-*kind* mentions became `http_api`. The one remaining `type = "zai"` in the README is inside the migration note.
- **Developer home config** (`~/.config/.multiagent/multiagent.toml`) was already migrated to `type = "http_api"` during task_02 (recorded there); no change needed here.

## Success Criteria
- All tests passing
- Test coverage >=80%
- Starter config includes documented HTTP provider example
- Doctor shows migration hint for legacy configs
- README documents `http_api` runtime kind
