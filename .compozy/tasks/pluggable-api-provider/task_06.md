---
status: pending
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
- [ ] 06.1 Update `src/runtime/http_api.rs` inline tests: `RuntimeKind::Zai` → `HttpApi` (lines 575, 632, 689, 730, 807)
- [ ] 06.2 Update `src/runtime/status.rs` inline tests: `ProviderId::Zai` → `HttpApi`, `"Z.ai"` → `"HTTP API"` (lines 1390-1487)
- [ ] 06.3 Update `src/config/mod.rs` tests: `"zai"` → `"http_api"` in test assertions (lines 3871, 4226)
- [ ] 06.4 Update `src/app/mod.rs` tests: `fake_with_zai_config()` helper with `"zai"` → `"http_api"` (lines 14082-14229)
- [ ] 06.5 Update `tests/runtime_integration.rs`: import path and `RuntimeKind::Zai` → `HttpApi` (lines 9, 63, 65)
- [ ] 06.6 Update `tests/provider_status_service.rs`: `RuntimeKind::Zai` → `HttpApi`, `ProviderId::Zai` → `HttpApi` (lines 58, 66)
- [ ] 06.7 Update `tests/provider_status_verification.rs`: `"Z.ai"` → `"HTTP API"` (lines 113-130)
- [ ] 06.8 Add config roundtrip test for `auth_header_name` and `auth_header_prefix` fields
- [ ] 06.9 Add unit tests for `HttpApiRuntime::build_auth_header()` with default, Verboo, and custom auth schemes
- [ ] 06.10 Run `cargo test` (full suite) and verify all tests pass
- [ ] 06.11 Run `cargo clippy --all-targets` and verify no warnings

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
  - [ ] Config roundtrip: `auth_header_name = "api-key"` survives serialize → deserialize
  - [ ] Config roundtrip: missing auth fields default to `None`
  - [ ] `build_auth_header()` with defaults returns `("Authorization", "Bearer <key>")`
  - [ ] `build_auth_header()` with `("api-key", "")` returns `("api-key", "<key>")`
  - [ ] `build_auth_header()` with `("X-Key", "Token")` returns `("X-Key", "Token <key>")`
- Integration tests:
  - [ ] `cargo test` passes (full suite)
  - [ ] `cargo clippy --all-targets` passes
  - [ ] No references to `RuntimeKind::Zai`, `ProviderId::Zai`, or `ZaiRuntime` remain
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Zero references to old names (`Zai`, `Z.ai`, `ZaiRuntime`) in test files
- New auth header tests pass
- `cargo clippy --all-targets` clean
- Full test suite green
