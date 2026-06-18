---
status: pending
title: "Add auth header config fields to RuntimeConfig"
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 03: Add auth header config fields to RuntimeConfig

## Overview

The existing `ZaiRuntime` (now `HttpApiRuntime`) hardcodes `.bearer_auth()` for API key authentication. Providers like Verboo use non-standard auth headers (`api-key: <key>` instead of `Authorization: Bearer <key>`). This task adds `auth_header_name` and `auth_header_prefix` optional fields to all three config structs (`RuntimeConfig`, `RawRuntimeConfig`, `MergedRuntimeConfig`) and updates the config validation pipeline to pass these fields through.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC "Core Interfaces" section for the field definitions
- FOCUS ON "WHAT" — add two optional config fields and wire them through the merge pipeline
- MINIMIZE CODE — add fields, update validation, done
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `auth_header_name: Option<String>` and `auth_header_prefix: Option<String>` to `RuntimeConfig` (line 324)
- MUST add the same fields to `RawRuntimeConfig` (line 663) with no serde rename (TOML keys match field names)
- MUST add the same fields to `MergedRuntimeConfig` (line 709)
- MUST update `apply_runtime()` (line 1452) to merge the new fields
- MUST update `into_effective()` (line 1670) to pass the new fields through for `HttpApi` runtimes
- MUST NOT require these fields — they remain `Option<String>` with `None` defaults
- MUST NOT change the existing `Zai`/`HttpApi` validation logic beyond passing through the new fields
- MUST add config roundtrip tests verifying the new fields serialize/deserialize correctly
</requirements>

## Subtasks
- [ ] 03.1 Add `auth_header_name: Option<String>` and `auth_header_prefix: Option<String>` to `RuntimeConfig` struct
- [ ] 03.2 Add the same fields to `RawRuntimeConfig` struct
- [ ] 03.3 Add the same fields to `MergedRuntimeConfig` struct
- [ ] 03.4 Update `apply_runtime()` to merge the new fields (same pattern as `base_url` and `api_key_env`)
- [ ] 03.5 Update `into_effective()` `HttpApi` arm to pass the new fields through to `RuntimeConfig`
- [ ] 03.6 Update `into_effective()` for other runtime kinds (Codex, Claude, Cursor, Fake) to set the new fields to `None`
- [ ] 03.7 Add config roundtrip test: TOML with `auth_header_name` and `auth_header_prefix` → deserialize → serialize → verify equality

## Implementation Details

The three config structs are in `src/config/mod.rs`:
- `RuntimeConfig` (line 324): public, used everywhere
- `RawRuntimeConfig` (line 663): deserialization target, `#[serde(deny_unknown_fields)]`
- `MergedRuntimeConfig` (line 709): intermediate merge target

The merge pipeline is `apply_runtime()` (line 1452) which does last-write-wins per field. The new fields follow the same pattern.

The `into_effective()` method (line 1593) builds the final `RuntimeConfig` per kind. The `HttpApi` arm (line 1670) currently sets `base_url` and `api_key_env` from the merged config. Add the new fields to this arm. Other arms (Codex, Claude, Cursor, Fake) should explicitly set them to `None`.

See TechSpec "Data Models" section for the field definitions.

### Relevant Files
- `src/config/mod.rs` — all three struct definitions, `apply_runtime()`, `into_effective()`

### Dependent Files
- `src/runtime/http_api.rs` — will read these fields in Task 04
- `src/runtime/mod.rs` — `RuntimeConfig` is used in `RuntimeRequest` construction

### Related ADRs
- [ADR-001: Generalize ZaiRuntime into Configurable HTTP API Runtime](adrs/adr-001.md) — The auth header fields are the core of this generalization

## Deliverables
- Updated `RuntimeConfig`, `RawRuntimeConfig`, `MergedRuntimeConfig` with new fields
- Updated `apply_runtime()` and `into_effective()` merge logic
- Config roundtrip test for new fields
- Unit tests with 80%+ coverage **(REQUIRED)**
- `cargo test --lib` passes **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] TOML with `auth_header_name = "api-key"` and `auth_header_prefix = ""` deserializes correctly
  - [ ] TOML without auth header fields defaults to `None` for both
  - [ ] Config roundtrip: serialize → deserialize → verify fields match
  - [ ] `into_effective()` for `HttpApi` runtime passes auth fields through
  - [ ] `into_effective()` for `Codex` runtime sets auth fields to `None`
- Integration tests:
  - [ ] `cargo test --lib` passes
  - [ ] Starter config (without auth header fields) still parses correctly
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `RuntimeConfig` has `auth_header_name` and `auth_header_prefix` fields
- Config roundtrip test passes
- Existing configs without the new fields still parse correctly (backwards compatible)
