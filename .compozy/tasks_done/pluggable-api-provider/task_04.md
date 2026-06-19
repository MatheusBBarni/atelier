---
status: completed
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
- [x] 04.1 Add a `build_auth_header(&self, api_key: &str) -> (String, String)` method to `HttpApiRuntime` that reads config fields and constructs the header name/value pair (takes api_key param for testability — see notes)
- [x] 04.2 Update `stream_step()` to build the header via `build_auth_header()` and thread it down instead of `.bearer_auth(api_key)`
- [x] 04.3 Update `stream_or_fallback()` to take/forward the `(name, value)` auth header pair
- [x] 04.4 Update `run_non_streaming_completion()` (and the shared `send_chat_completion()` — the single `.bearer_auth` site) to use the dynamic header via `.header(name, value)`
- [x] 04.5 Verified `check_availability()` works generically for any `HttpApi` runtime (only checks `api_key_env`; unchanged)
- [x] 04.6 Unit test: `build_auth_header()` with default values (None/None) returns `("Authorization", "Bearer <key>")`
- [x] 04.7 Unit test: `build_auth_header()` with `auth_header_name = "api-key"`, `auth_header_prefix = ""` returns `("api-key", "<key>")`
- [x] 04.8 Unit test: `build_auth_header()` with custom prefix returns `("X-API-Key", "Token <key>")`

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
- Unit tests (in `src/runtime/http_api.rs`):
  - [x] Default auth: `build_auth_header()` with `None` fields returns `("Authorization", "Bearer <key>")`
  - [x] Verboo auth: `build_auth_header()` with `("api-key", "")` returns `("api-key", "<key>")`
  - [x] Custom auth: `build_auth_header()` with `("X-API-Key", "Token")` returns `("X-API-Key", "Token <key>")`
  - [x] HTTP request includes the correct header (mock server assertion): `http_api_adapter_sends_custom_auth_header_over_the_wire` asserts the custom `api-key: <key>` header (no `authorization:`); the existing `zai_adapter_streams_sse_chunks_and_parses_agent_result` asserts the default `authorization: Bearer <key>` over the wire
- Integration tests:
  - [x] `cargo test --lib` passes (1356 passed; the 12 skill-discovery + 1 environment-sensitive codex-CLI failures are unchanged baseline / flaky, all pass in isolation)
  - [x] `cargo clippy --all-targets` passes ("No issues found"); `cargo fmt --check` clean
- Test coverage target: >=80% (3 construction unit tests + 1 over-the-wire test, plus the default path covered by the existing SSE test)
- All tests must pass

## Follow-up Notes
- **`build_auth_header(&self, api_key: &str)` signature deviates slightly from the techspec's `build_auth_header(&self)`:** the techspec sketch read `api_key_env` from the environment inside the method (with `.expect`). Passing `api_key` in instead keeps `stream_step`'s single, properly error-handled env read (`with_context`), avoids a duplicate read and a potential panic, and makes the method pure and unit-testable without setting env vars. Header-field reading from `self.config` is unchanged.
- **Threaded the `(name, value)` pair, not the key:** `stream_or_fallback`, `run_non_streaming_completion`, and the shared `send_chat_completion` now take `auth_header: (&str, &str)` instead of `api_key: &str`; the single `.bearer_auth()` site became `.header(name, value)`. SSE streaming, non-streaming fallback, and error classification are untouched.
- **Internal "Z.ai" message strings retained:** `check_availability()` and error messages still say "Z.ai" (asserted by `tests/provider_status_render.rs`, kept since task_02); they are user-facing copy, not behavior. Cleaning them up belongs to task_05 (docs).

## Success Criteria
- All tests passing
- Test coverage >=80%
- `build_auth_header()` correctly constructs headers for all auth schemes
- HTTP requests use the dynamic header instead of hardcoded `.bearer_auth()`
- SSE streaming and non-streaming fallback still work correctly
