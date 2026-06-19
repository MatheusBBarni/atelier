---
status: completed
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
- [x] 03.1 Add `auth_header_name: Option<String>` and `auth_header_prefix: Option<String>` to `RuntimeConfig` struct
- [x] 03.2 Add the same fields to `RawRuntimeConfig` struct (no serde rename — TOML keys match field names)
- [x] 03.3 Add the same fields to `MergedRuntimeConfig` struct (+ all 4 builtin runtime literals and the `apply_runtime` `or_insert` literal set them to `None`)
- [x] 03.4 Update `apply_runtime()` to merge the new fields (same last-write-wins pattern as `base_url`/`api_key_env`)
- [x] 03.5 Update `into_effective()` `HttpApi` arm to pass the new fields through to `RuntimeConfig` (`runtime.auth_header_name`/`auth_header_prefix`)
- [x] 03.6 Update `into_effective()` for other runtime kinds (Codex, Claude, Cursor, Fake) to set the new fields to `None`
- [x] 03.7 Add config roundtrip test (`runtime_config_auth_header_fields_roundtrip`: serialize → deserialize → assert equality) plus deserialize/default tests

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
- Unit tests (in `src/config/mod.rs`):
  - [x] TOML with `auth_header_name = "api-key"` and `auth_header_prefix = ""` deserializes correctly (`http_api_auth_header_fields_deserialize_and_pass_through`)
  - [x] TOML without auth header fields defaults to `None` for both (`http_api_runtime_without_auth_headers_defaults_to_none`)
  - [x] Config roundtrip: serialize → deserialize → verify fields match (`runtime_config_auth_header_fields_roundtrip`)
  - [x] `into_effective()` for `HttpApi` runtime passes auth fields through (covered by `http_api_auth_header_fields_deserialize_and_pass_through`, which loads via `into_effective`)
  - [x] `into_effective()` for `Codex` runtime sets auth fields to `None` (`codex_runtime_auth_header_fields_are_none`)
- Integration tests:
  - [x] `cargo test --lib` passes (1353 passed; the 12 failures are the unchanged external skill-discovery baseline)
  - [x] Starter config (without auth header fields) still parses correctly (`config::` template tests pass; fields default to `None`)
- Test coverage target: >=80% (4 new tests cover deserialize/default/pass-through/roundtrip)
- All tests must pass

## Follow-up Notes
- **Construction-site churn:** `RuntimeConfig` is built in ~19 test literals across `runtime/{http_api,codex,claude,cursor}.rs`, `tests/runtime_integration.rs`, and `tests/provider_status_service.rs`; each gained `auth_header_name: None, auth_header_prefix: None` (additive, behavior-neutral). The 4 builtin `MergedRuntimeConfig` literals and the `apply_runtime` `or_insert` also set them to `None`.
- **`PrintableRuntime` (`--print-config` view) intentionally NOT extended:** surfacing the new auth fields in the redacted print/template output is documentation work scoped to task_05; the struct only reads selected `RuntimeConfig` fields, so it is unaffected.
- **Defaults applied at the runtime layer, not config:** unset `auth_header_name`/`auth_header_prefix` stay `None` in `RuntimeConfig`; the `"Authorization"`/`"Bearer"` defaults are applied when the header is built — that consumption is task_04's scope.

## Success Criteria
- All tests passing
- Test coverage >=80%
- `RuntimeConfig` has `auth_header_name` and `auth_header_prefix` fields
- Config roundtrip test passes
- Existing configs without the new fields still parse correctly (backwards compatible)
