---
status: completed
title: "Update all test files and run full test suite"
type: test
complexity: high
dependencies:
  - task_02
  - task_03
  - task_04
---

# Task 06: Update all test files and run full test suite

## Overview

The rename from `Zai` to `HttpApi` and the addition of auth header fields affect test assertions across multiple files. This task updates all test references, adds new tests for the auth header config fields and construction logic, and runs the full test suite to verify zero regressions.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC "Testing Approach" section for the test strategy
- FOCUS ON "WHAT" — update test assertions and add new tests
- MINIMIZE CODE — update references, add targeted tests
- TESTS REQUIRED — this IS the testing task
</critical>

<requirements>
- MUST update all `RuntimeKind::Zai` references in test files to `RuntimeKind::HttpApi`
- MUST update all `ProviderId::Zai` references in test files to `ProviderId::HttpApi`
- MUST update all `ZaiRuntime` references in test files to `HttpApiRuntime`
- MUST update all `"zai"` string literals in test TOML to `"http_api"`
- MUST update all `"Z.ai"` display string assertions to `"HTTP API"`
- MUST update import paths (`multiagent::runtime::zai::ZaiRuntime` → `multiagent::runtime::http_api::HttpApiRuntime`)
- MUST add config roundtrip tests for `auth_header_name` and `auth_header_prefix`
- MUST add unit tests for `build_auth_header()` with various auth schemes
- MUST run the full test suite (`cargo test`) and verify all tests pass
- MUST run `cargo clippy --all-targets` and verify no warnings
</requirements>

## Subtasks
- [x] 06.1 `src/runtime/http_api.rs` inline tests: `RuntimeKind::Zai` → `HttpApi` (done in task_02)
- [x] 06.2 `src/runtime/status.rs` inline tests: `ProviderId::Zai` → `HttpApi`, display `"Z.ai"` → `"HTTP API"` (done in task_02)
- [x] 06.3 `src/config/mod.rs` tests: `"zai"` → `"http_api"` (done in task_02)
- [x] 06.4 `src/app/mod.rs` tests: `fake_with_zai_config()` TOML `"zai"` → `"http_api"` (done in task_02; comment de-branded in this task)
- [x] 06.5 `tests/runtime_integration.rs`: import path → `multiagent::runtime::http_api::HttpApiRuntime` and `RuntimeKind::Zai` → `HttpApi` (done in task_02)
- [x] 06.6 `tests/provider_status_service.rs`: `RuntimeKind::Zai`/`ProviderId::Zai` → `HttpApi` (done in task_02/03)
- [x] 06.7 `tests/provider_status_verification.rs`: display `"Z.ai"` → `"HTTP API"` (done in task_02)
- [x] 06.8 Config roundtrip test for `auth_header_name`/`auth_header_prefix` (added in task_03: `runtime_config_auth_header_fields_roundtrip`)
- [x] 06.9 Unit tests for `HttpApiRuntime::build_auth_header()` — default, Verboo, custom (added in task_04)
- [x] 06.10 Ran full `cargo test`: all suites pass except the 12-test external skill-discovery env baseline (unchanged)
- [x] 06.11 Ran `cargo clippy --all-targets` (clean) and `cargo fmt --check` (clean)

## Implementation Details

The test files to update are:
- `src/runtime/http_api.rs` (inline `#[cfg(test)]` module) — 5 references
- `src/runtime/status.rs` (inline tests) — 4 references
- `src/config/mod.rs` (inline tests) — 2 references
- `src/app/mod.rs` (inline tests) — 3 references
- `tests/runtime_integration.rs` — 3 references (import + 2 usage)
- `tests/provider_status_service.rs` — 2 references
- `tests/provider_status_verification.rs` — 2 references

New tests to add:
- Config roundtrip: TOML with auth fields → deserialize → serialize → verify
- Auth header construction: test `build_auth_header()` with 3 auth schemes

See TechSpec "Testing Approach" section for the full test strategy.

### Relevant Files
- `src/runtime/http_api.rs` — inline tests
- `src/runtime/status.rs` — inline tests
- `src/config/mod.rs` — inline tests
- `src/app/mod.rs` — inline tests
- `tests/runtime_integration.rs` — integration test
- `tests/provider_status_service.rs` — integration test
- `tests/provider_status_verification.rs` — integration test

### Dependent Files
- All files modified in Tasks 02-04

### Related ADRs
- [ADR-002: Rename RuntimeKind::Zai to RuntimeKind::HttpApi](adrs/adr-002.md) — Test assertions must match the rename
- [ADR-001: Generalize ZaiRuntime into Configurable HTTP API Runtime](adrs/adr-001.md) — New auth tests validate the generalization

## Deliverables
- All test files updated with renamed references
- New config roundtrip test for auth header fields
- New unit tests for `build_auth_header()`
- Full test suite passing
- `cargo clippy --all-targets` clean
- Test coverage >=80% **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Config roundtrip: `auth_header_name = "api-key"` survives serialize → deserialize (`runtime_config_auth_header_fields_roundtrip`, task_03)
  - [x] Config roundtrip: missing auth fields default to `None` (`http_api_runtime_without_auth_headers_defaults_to_none`, task_03)
  - [x] `build_auth_header()` with defaults returns `("Authorization", "Bearer <key>")` (task_04)
  - [x] `build_auth_header()` with `("api-key", "")` returns `("api-key", "<key>")` (task_04)
  - [x] `build_auth_header()` with `("X-API-Key", "Token")` returns `("X-API-Key", "Token <key>")` (task_04)
- Integration tests:
  - [x] `cargo test` passes (full suite) — every suite green except the 12 external skill-discovery env failures (unchanged baseline; an occasional 13th is an env-sensitive CLI test that passes in isolation)
  - [x] `cargo clippy --all-targets` passes; `cargo fmt --check` clean
  - [x] No references to `RuntimeKind::Zai`, `ProviderId::Zai`, or `ZaiRuntime` remain (final repo-wide grep returns none; also zero `Z.ai`, `[runtimes.zai]`, `pub mod zai`)
- Test coverage target: >=80%
- All tests must pass

## Follow-up Notes
- **Most rename/auth test updates landed in tasks 02–04**, not here: task_02 renamed every identifier + display assertion and the `provider_status_*` tests; task_03 added the auth config fields to every `RuntimeConfig` literal + roundtrip tests; task_04 added the `build_auth_header` tests. Task_06's net new work was the full-suite verification plus a provider-neutral message cleanup.
- **Provider-neutral message cleanup (this task):** the runtime's user-facing `Z.ai` error/log strings in `http_api.rs` and `status.rs`, and a stale `[runtimes.zai].api_key_env` remediation, were changed to generic `HTTP API` / `[runtimes.http_api]`. This completes ADR-002's "grep for zai stragglers" and fixes a real wart (a generic runtime emitting "Z.ai" errors for an OpenRouter/Verboo user). `tests/provider_status_render.rs` sample data and a couple of test comments/prose were updated to match.
- **Intentional keeps (not "old name" references):** the `ZAI_API_KEY` default env var, the `https://api.z.ai/api/paas/v4` default endpoint, the `zai-` secret prefix in redaction tests, the `MULTIAGENT_*_ZAI_*` test env-var names, internal test fn/helper names (`zai_adapter_*`, `spawn_mock_zai_sequence_server`, `zai_status_error`), arbitrary `id = "zai"` test instance ids, and the README `type = "zai"` migration note.
- **Environmental baseline (per CLAUDE.md):** full `cargo test --lib` has 12 pre-existing skill-discovery failures from an external malformed personal skill (`~/.claude/skills/cy-archive-tasks/SKILL.md`); codex/cursor/claude CLI availability + process-timeout tests flake under parallel load and pass in isolation. None are regressions from this PRD.

## Success Criteria
- All tests passing
- Test coverage >=80%
- Zero references to old names (`Zai`, `Z.ai`, `ZaiRuntime`) in test files
- New auth header tests pass
- `cargo clippy --all-targets` clean
- Full test suite green
