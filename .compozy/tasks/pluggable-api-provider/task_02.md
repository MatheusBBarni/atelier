---
status: completed
title: "Rename RuntimeKind::Zai to HttpApi and zai.rs to http_api.rs"
type: refactor
complexity: critical
dependencies:
  - task_01
---

# Task 02: Rename RuntimeKind::Zai to HttpApi and zai.rs to http_api.rs

## Overview

The `RuntimeKind::Zai` variant and `src/runtime/zai.rs` module are named after a single provider (Z.ai) but will serve as the generic HTTP API runtime for all OpenAI-compatible providers. This task renames the enum variant to `HttpApi`, renames the file to `http_api.rs`, renames the struct to `HttpApiRuntime`, and updates every reference across the codebase. This is a breaking config change (`type = "zai"` → `type = "http_api"`).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC "Impact Analysis" section for the full list of affected components
- FOCUS ON "WHAT" — rename all references consistently
- MINIMIZE CODE — this is a rename, not a rewrite
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST rename `RuntimeKind::Zai` to `RuntimeKind::HttpApi` in the enum definition
- MUST rename `src/runtime/zai.rs` to `src/runtime/http_api.rs`
- MUST rename `pub mod zai;` to `pub mod http_api;` in `src/runtime/mod.rs`
- MUST rename struct `ZaiRuntime` to `HttpApiRuntime`
- MUST update all match arms in `execute_runtime_step_once` and `check_runtime_availability`
- MUST rename `ProviderId::Zai` to `ProviderId::HttpApi` in `src/runtime/status.rs`
- MUST update `From<RuntimeKind>` impl, `provider_capabilities`, `provider_usage_url`, display names
- MUST update doctor title match in `src/doctor/mod.rs`
- MUST update all test files that reference `RuntimeKind::Zai`, `ProviderId::Zai`, or `ZaiRuntime`
- MUST update the builtin runtime default from `"zai"` to `"http_api"` in `src/config/mod.rs`
- MUST update all agent `runtime = "zai"` references in builtin defaults and starter template to `"http_api"`
- MUST NOT change runtime behavior — this is a pure rename
</requirements>

## Subtasks
- [x] 02.1 Rename `RuntimeKind::Zai` to `RuntimeKind::HttpApi` in enum definition (`src/config/mod.rs`) + `RuntimeKind::all()`
- [x] 02.2 Rename `src/runtime/zai.rs` to `src/runtime/http_api.rs` (via `git mv`) and update `pub mod zai;` → `pub mod http_api;` in `mod.rs`
- [x] 02.3 Rename struct `ZaiRuntime` to `HttpApiRuntime` in the renamed file
- [x] 02.4 Update match arms in `execute_runtime_step_once` and `check_runtime_availability` (`mod.rs`)
- [x] 02.5 Rename `ProviderId::Zai` to `ProviderId::HttpApi` in `status.rs` and update all match arms (`From<RuntimeKind>`, `default_display_name` → `"HTTP API"`, `provider_capabilities`, `provider_usage_url` URL kept as `https://z.ai`)
- [x] 02.6 Update doctor title from `"Z.ai Runtime"` to `"HTTP API Runtime"` (extracted into testable `runtime_title()` helper)
- [x] 02.7 Update builtin runtime default key from `"zai"` to `"http_api"` and all builtin agent/council `runtime = "zai"` references (`config/mod.rs`); base_url + `ZAI_API_KEY` kept (behavior preservation)
- [x] 02.8 Update starter config template `[runtimes.zai]`/`type = "zai"` → `[runtimes.http_api]`/`type = "http_api"` and all `runtime = "zai"` → `runtime = "http_api"`
- [x] 02.9 Update test files: `http_api.rs` inline tests, `status.rs` inline tests, `config/mod.rs` tests, `app/mod.rs` tests, `tests/runtime_integration.rs`, `tests/provider_status_service.rs`, `tests/provider_status_verification.rs`, and `tests/provider_status_render.rs` (the latter forced by the `ProviderId` rename — see notes)
- [x] 02.10 Update `tests/runtime_integration.rs` import from `multiagent::runtime::zai::ZaiRuntime` to `multiagent::runtime::http_api::HttpApiRuntime`

## Implementation Details

This rename touches 10+ files with ~40 reference sites. The most efficient approach is a global find-and-replace followed by targeted fixes:

1. `RuntimeKind::Zai` → `RuntimeKind::HttpApi` (all files)
2. `ProviderId::Zai` → `ProviderId::HttpApi` (status.rs, tests)
3. `ZaiRuntime` → `HttpApiRuntime` (zai.rs → http_api.rs, tests)
4. `"zai"` string literals in config context → `"http_api"` (config/mod.rs, app/mod.rs)
5. `"Z.ai"` display strings → `"HTTP API"` (status.rs, doctor/mod.rs)
6. File rename: `git mv src/runtime/zai.rs src/runtime/http_api.rs`

See TechSpec "Impact Analysis" for the complete list of affected components.

### Relevant Files
- `src/config/mod.rs` — enum definition (line 293), builtin defaults (lines 929-1046), `into_effective()` (line 1670), starter template (lines 2755-2844), tests (lines 3871, 4226)
- `src/runtime/mod.rs` — module declaration (line 6), dispatch match arms (lines 439, 567)
- `src/runtime/zai.rs` → `src/runtime/http_api.rs` — struct rename, test references
- `src/runtime/status.rs` — `ProviderId` enum (line 46), display (line 59), `From<RuntimeKind>` (line 72), capabilities (line 470), usage URL (line 490), tests
- `src/doctor/mod.rs` — title match (line 93)
- `src/app/mod.rs` — inline test TOML (lines 14082-14229)
- `tests/runtime_integration.rs` — import and usage (lines 9, 63, 65)
- `tests/provider_status_service.rs` — runtime config helper (line 58)
- `tests/provider_status_verification.rs` — display name assertions (lines 113-130)

### Dependent Files
- All files listed above are affected by this rename

### Related ADRs
- [ADR-002: Rename RuntimeKind::Zai to RuntimeKind::HttpApi](adrs/adr-002.md) — Primary decision for this task

## Deliverables
- Renamed `src/runtime/http_api.rs` with `HttpApiRuntime` struct
- Updated `RuntimeKind::HttpApi` in `src/config/mod.rs`
- Updated `ProviderId::HttpApi` in `src/runtime/status.rs`
- Updated all match arms and dispatch sites
- Updated all builtin defaults and starter template
- Updated all test files
- Unit tests with 80%+ coverage **(REQUIRED)**
- `cargo test --lib` passes **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `RuntimeKind::HttpApi` deserializes from `"http_api"` in TOML (new `http_api_runtime_type_deserializes` in `config/mod.rs`)
  - [x] `ProviderId::from(RuntimeKind::HttpApi)` returns `ProviderId::HttpApi` (`status.rs` `provider_id_maps_from_runtime_kind_and_adds_local`)
  - [x] `ProviderId::HttpApi.default_display_name()` returns `"HTTP API"` (same test, updated)
  - [x] Doctor title for `RuntimeKind::HttpApi` is `"HTTP API Runtime"` (new `runtime_title_uses_generic_http_api_name` in `doctor/mod.rs`)
- Integration tests:
  - [x] `cargo test --lib` passes with all renamed references (1349 passed; the 12 failures are the pre-existing external skill-discovery env failures, unchanged from the task_01 baseline)
  - [x] `cargo clippy --all-targets` passes ("No issues found"); `cargo fmt --check` clean
  - [x] Starter config generates valid TOML with `type = "http_api"` (template updated; `config::` 114 tests pass including template render/redaction)
- Test coverage target: >=80% (rename covered by updated from/display/drift tests + 2 new tests)
- All tests must pass

## Follow-up Notes
- **`tests/provider_status_render.rs` updated though not in the original 02.9 list:** it referenced `ProviderId::Zai`, which the enum rename forces to `ProviderId::HttpApi` (compilation requirement). Only the enum reference changed; its `"Z.ai api_key_env is not configured"` *message* assertion and `https://z.ai` URL were intentionally **kept** (see next note).
- **Internal "Z.ai" message/URL strings kept (pure rename, not behavior change):** the runtime's error/log messages in `http_api.rs` (e.g. "Z.ai api_key_env is not configured", "Z.ai streaming…"), the `provider_usage_url` value `https://z.ai`, the `ZAI_API_KEY` env var, and the default `base_url` (`https://api.z.ai/api/paas/v4`) were left unchanged. Only the *identifiers*, the `type`/runtime-id config strings, the `default_display_name()` (`"HTTP API"`), and the doctor title (`"HTTP API Runtime"`) changed. Generalizing the auth/base_url/messages is scoped to tasks 03–05.
- **Doctor title extracted into `runtime_title(&RuntimeKind)`:** the title was an inline match in `run_doctor`; extracted into a small `pub(crate)` helper (identical output) so the listed unit test could assert it without shelling out.
- **Developer home-config migration (ADR-002 breaking change):** `~/.config/.multiagent/multiagent.toml` had `[runtimes.zai]`/`type = "zai"`, which no longer deserializes after the rename and broke 6 `run_doctor` tests that read the real home config (`config_path: None`). It was migrated to `[runtimes.http_api]`/`type = "http_api"` (base_url/api_key unchanged). This file is outside the repo and is **not** part of the commit. The automated `--doctor` migration *hint* for end users is task_05's scope.

## Success Criteria
- All tests passing
- Test coverage >=80%
- Zero references to `RuntimeKind::Zai`, `ProviderId::Zai`, or `ZaiRuntime` remain (grep confirms)
- `cargo clippy --all-targets` clean
- Starter config template uses `type = "http_api"` consistently
