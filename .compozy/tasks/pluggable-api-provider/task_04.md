---
status: pending
title: "Generalize HttpApiRuntime auth header construction"
type: backend
complexity: medium
dependencies:
  - task_03
---

# Task 04: Generalize HttpApiRuntime auth header construction

## Overview

The `HttpApiRuntime` (renamed from `ZaiRuntime`) currently hardcodes `.bearer_auth(api_key)` for API key authentication. This task reads the `auth_header_name` and `auth_header_prefix` fields from `RuntimeConfig` and constructs the auth header dynamically. This enables providers like Verboo (which uses `api-key: <key>`) to work alongside standard Bearer-auth providers (OpenRouter, DeepSeek, etc.).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC "Core Interfaces" section for the `build_auth_header()` pattern
- FOCUS ON "WHAT" — read auth config from RuntimeConfig and construct the header dynamically
- MINIMIZE CODE — the change is in the HTTP request construction, not a rewrite of the streaming logic
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST read `auth_header_name` and `auth_header_prefix` from `self.config` in `HttpApiRuntime`
- MUST default to `Authorization` / `Bearer` when the fields are `None`
- MUST handle empty `auth_header_prefix` (e.g., Verboo uses just `api-key: <key>` with no prefix)
- MUST construct the HTTP header as `{header_name}: {prefix} {api_key}` (or `{header_name}: {api_key}` when prefix is empty)
- MUST update `check_availability()` to work with any HTTP runtime (not just Zai-specific checks)
- MUST NOT change the SSE streaming, non-streaming fallback, or error classification logic
- MUST add unit tests for auth header construction with various combinations
</requirements>

## Subtasks
- [ ] 04.1 Add a `build_auth_header(&self) -> (String, String)` method to `HttpApiRuntime` that reads config fields and constructs the header name/value pair
- [ ] 04.2 Update `stream_step()` to use `build_auth_header()` instead of `.bearer_auth(api_key)`
- [ ] 04.3 Update `stream_or_fallback()` HTTP request construction to use dynamic header
- [ ] 04.4 Update `run_non_streaming_completion()` to use dynamic header
- [ ] 04.5 Verify `check_availability()` works generically for any `HttpApi` runtime (should already work since it only checks `api_key_env`)
- [ ] 04.6 Add unit test: `build_auth_header()` with default values (None/None) returns `("Authorization", "Bearer <key>")`
- [ ] 04.7 Add unit test: `build_auth_header()` with `auth_header_name = "api-key"`, `auth_header_prefix = ""` returns `("api-key", "<key>")`
- [ ] 04.8 Add unit test: `build_auth_header()` with custom prefix returns `("X-Custom", "Token <key>")`

## Implementation Details

The HTTP request construction is in `src/runtime/http_api.rs` (renamed from `zai.rs`):
- `stream_step()` (line 55): reads api_key, builds client, calls `stream_or_fallback()`
- `stream_or_fallback()` (line 92): constructs the POST request with `.bearer_auth(api_key)` (line ~233)
- `run_non_streaming_completion()` (line 155): same pattern

The change is to replace `.bearer_auth(api_key)` with `.header(&header_name, &header_value)` where the header is constructed from config fields.

See TechSpec "Core Interfaces" section for the `build_auth_header()` pattern.

### Relevant Files
- `src/runtime/http_api.rs` — `stream_step()`, `stream_or_fallback()`, `run_non_streaming_completion()`, `check_availability()`

### Dependent Files
- `src/config/mod.rs` — `RuntimeConfig` provides the auth fields (updated in Task 03)

### Related ADRs
- [ADR-001: Generalize ZaiRuntime into Configurable HTTP API Runtime](adrs/adr-001.md) — Configurable auth is the core of this generalization

## Deliverables
- Updated `HttpApiRuntime` with `build_auth_header()` method
- Updated HTTP request construction using dynamic auth headers
- Unit tests for auth header construction
- Unit tests with 80%+ coverage **(REQUIRED)**
- `cargo test --lib` passes **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Default auth: `build_auth_header()` with `None` fields returns `("Authorization", "Bearer <key>")`
  - [ ] Verboo auth: `build_auth_header()` with `("api-key", "")` returns `("api-key", "<key>")`
  - [ ] Custom auth: `build_auth_header()` with `("X-API-Key", "Token")` returns `("X-API-Key", "Token <key>")`
  - [ ] HTTP request includes the correct header (mock server assertion)
- Integration tests:
  - [ ] `cargo test --lib` passes
  - [ ] `cargo clippy --all-targets` passes
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `build_auth_header()` correctly constructs headers for all auth schemes
- HTTP requests use the dynamic header instead of hardcoded `.bearer_auth()`
- SSE streaming and non-streaming fallback still work correctly
