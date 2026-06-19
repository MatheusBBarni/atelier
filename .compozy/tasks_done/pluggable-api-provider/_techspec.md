# TechSpec: Pluggable API Provider Integration (BYOK)

## Executive Summary

Generalize the existing `ZaiRuntime` into a configurable HTTP API runtime (`RuntimeKind::HttpApi`) that accepts any OpenAI-compatible endpoint through TOML configuration. The implementation renames `RuntimeKind::Zai` to `RuntimeKind::HttpApi`, adds `auth_header_name` and `auth_header_prefix` fields to `RuntimeConfig`, extracts 6 duplicated utility functions into a new `http_util.rs` module, expands `SECRET_PREFIXES` for new providers, and adds a commented-out HTTP provider example to the starter config template.

**Primary trade-off:** A breaking config change (`type = "zai"` → `type = "http_api"`)换取 clean naming and zero deprecation maintenance. Existing users must update their configs, but `--doctor` provides migration guidance.

**Implementation strategy:** Mechanical refactor — rename, extract, generalize. No new runtime code beyond config-driven auth header selection. The existing SSE streaming, non-streaming fallback, and error classification logic is preserved unchanged.

## System Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────┐
│                    Config Layer                      │
│  RawRuntimeConfig → MergedRuntimeConfig → RuntimeConfig  │
│  + auth_header_name    + auth_header_prefix          │
│  + base_url            + api_key_env                 │
└──────────────────────┬──────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────┐
│                 Runtime Dispatch                     │
│  RuntimeKind::HttpApi → http_api::HttpApiRuntime     │
│  (renamed from Zai)                                  │
└──────────────────────┬──────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────┐
│              HTTP Execution Layer                    │
│  HttpApiRuntime::stream_step()                       │
│  ├── Reads auth config from RuntimeConfig            │
│  ├── Constructs header: {auth_header_name}:          │
│  │   {auth_header_prefix}{api_key}                   │
│  ├── POST {base_url}/chat/completions                │
│  ├── SSE streaming with non-streaming fallback       │
│  └── Error classification (retryable/non-retryable)  │
└──────────────────────┬──────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────┐
│              Shared Utilities                        │
│  http_util.rs                                        │
│  ├── redact_sensitive_text (pub(crate))              │
│  ├── redact_bearer_tokens                            │
│  ├── redact_raw_secret_tokens                        │
│  ├── next_raw_secret_prefix (SECRET_PREFIXES: 4)     │
│  ├── is_secret_token_character                       │
│  └── parse_runtime_output (pub(crate))               │
└─────────────────────────────────────────────────────┘
```

### Data Flow

1. User configures `[runtimes.openrouter]` in `atelier.toml` with `type = "http_api"`, `base_url`, `api_key_env`, and optional auth header fields
2. Config merge pipeline (`apply_runtime` → `into_effective`) produces a `RuntimeConfig` with the new fields populated
3. `execute_runtime_step_streaming` dispatches to `HttpApiRuntime::new(config)`
4. `stream_step` reads the API key from the env var, constructs the auth header, and sends the request
5. SSE streaming response is parsed; fallback to non-streaming on rejection
6. Response is parsed via `parse_runtime_output` (from `http_util.rs`) into `RuntimeOutput`

### External System Interactions

- **OpenAI-compatible APIs**: POST `{base_url}/chat/completions` with Bearer or custom auth
- **Environment variables**: API keys read via `env::var(api_key_env)`
- No other external dependencies beyond existing `reqwest` crate

## Implementation Design

### Core Interfaces

**RuntimeConfig — new fields:**

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub id: String,
    pub kind: RuntimeKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub prompt_mode: PromptMode,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub auth_header_name: Option<String>,   // NEW: default "Authorization"
    pub auth_header_prefix: Option<String>, // NEW: default "Bearer"
}
```

**HttpApiRuntime — auth header construction:**

```rust
fn build_auth_header(&self) -> (String, String) {
    let name = self.config.auth_header_name.as_deref()
        .unwrap_or("Authorization");
    let prefix = self.config.auth_header_prefix.as_deref()
        .unwrap_or("Bearer");
    let api_key = env::var(
        self.config.api_key_env.as_deref().unwrap_or_default()
    ).expect("api_key_env validated at config load");
    if prefix.is_empty() {
        (name.to_string(), api_key)
    } else {
        (name.to_string(), format!("{prefix} {api_key}"))
    }
}
```

**HttpApiRuntime — HTTP request construction:**

```rust
let (header_name, header_value) = self.build_auth_header();
let response = client
    .post(format!("{base_url}/chat/completions"))
    .header(&header_name, &header_value)
    .json(&body)
    .send()
    .await?;
```

### Data Models

**RawRuntimeConfig — new fields:**

```rust
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRuntimeConfig {
    #[serde(rename = "type")]
    runtime_type: Option<RuntimeKind>,
    command: Option<String>,
    args: Option<Vec<String>>,
    prompt_mode: Option<PromptMode>,
    base_url: Option<String>,
    api_key_env: Option<String>,
    auth_header_name: Option<String>,   // NEW
    auth_header_prefix: Option<String>, // NEW
}
```

**MergedRuntimeConfig — new fields:**

```rust
#[derive(Clone, Debug)]
struct MergedRuntimeConfig {
    kind: Option<RuntimeKind>,
    command: Option<String>,
    args: Option<Vec<String>>,
    prompt_mode: Option<PromptMode>,
    base_url: Option<String>,
    api_key_env: Option<String>,
    auth_header_name: Option<String>,   // NEW
    auth_header_prefix: Option<String>, // NEW
}
```

### API Endpoints

No new API endpoints. The existing OpenAI-compatible chat completions endpoint (`{base_url}/chat/completions`) is the integration point. The change is in how the auth header is constructed for that endpoint.

## Integration Points

| Integration | Approach |
|-------------|----------|
| **OpenAI-compatible APIs** (OpenRouter, Verboo, DeepSeek, etc.) | POST `{base_url}/chat/completions` with configurable auth header |
| **Config merge pipeline** | New fields flow through `apply_runtime()` → `into_effective()` with same merge semantics |
| **Doctor runtime check** | New `RuntimeKind::HttpApi` match arm; checks `api_key_env` presence |
| **TUI agent roster** | No change — already shows `runtime/model` per agent |
| **Secret redaction** | Expanded `SECRET_PREFIXES` in `http_util.rs`; all output surfaces redact consistently |
| **Hooks system** | Imports `redact_sensitive_text` from `runtime::http_util` (pub(crate)) |

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|-----------|-------------|---------------------|-----------------|
| `src/runtime/zai.rs` | renamed | Renamed to `http_api.rs`; struct renamed `ZaiRuntime` → `HttpApiRuntime` | Rename file, update struct, generalize auth |
| `src/runtime/mod.rs` | modified | Remove duplicated functions (6), update `RuntimeKind` enum, update dispatch match arms | Import from `http_util`, rename `Zai` → `HttpApi` |
| `src/runtime/http_util.rs` | new | Shared HTTP utilities: redaction + parsing | Create module, move canonical function copies |
| `src/runtime/codex.rs` | modified | Remove private `parse_runtime_output` copy, import from `http_util` | Update import |
| `src/config/mod.rs` | modified | Add fields to `RuntimeConfig`/`RawRuntimeConfig`/`MergedRuntimeConfig`, rename `Zai` → `HttpApi` in `into_effective()`, update starter template | Add fields, update validation, update template |
| `src/runtime/status.rs` | modified | Rename `ProviderId::Zai` → `HttpApi`, update dispatch | Rename variant, update match arms |
| `src/doctor/mod.rs` | modified | Add `HttpApi` match arm for display title | Add arm |
| `src/hooks/follow.rs` | modified | Update import path for `redact_sensitive_text` | Update import |
| `src/hooks/dispatch.rs` | modified | Update import path for `redact_sensitive_text` | Update import |
| Tests (`tests/*.rs`) | modified | Update all `RuntimeKind::Zai` references to `HttpApi` | Update test assertions |

## Testing Approach

### Unit Tests

- **Config roundtrip**: Serialize `RuntimeConfig` with new fields → deserialize → verify equality
- **Auth header construction**: Test `build_auth_header()` with Bearer, empty prefix, custom name
- **Config validation**: Test `into_effective()` with missing `base_url`, missing `api_key_env`, valid config
- **Redaction**: Test `redact_sensitive_text` with `or-` and `ov-` prefixed tokens
- **parse_runtime_output**: Verify shared copy produces identical results to previous per-module copies
- **Legacy type detection**: Test that `type = "zai"` is rejected (or warns) in config validation

### Integration Tests

- **Full runtime roundtrip** (using `FakeRuntime`): Verify that renaming doesn't break the orchestrator loop
- **Config merge**: Verify that `http_api` runtimes merge correctly across config layers
- **Doctor check**: Verify that `atelier --doctor` reports availability for `http_api` runtimes
- **Existing runtimes**: Run full test suite to verify zero regression for Codex, Claude, Cursor, Fake

### Test Data Requirements

- Mock HTTP server returning OpenAI-compatible responses (SSE and non-streaming)
- Test configs with `type = "http_api"` and various auth header combinations
- Test configs with legacy `type = "zai"` for migration testing

## Development Sequencing

### Build Order

1. **Create `src/runtime/http_util.rs`** — no dependencies; extract all 6 duplicated functions
2. **Rename `src/runtime/zai.rs` → `src/runtime/http_api.rs`** — no dependencies on step 1 (can be parallel)
3. **Update `src/runtime/mod.rs`** — depends on step 1 (import from `http_util`), rename `RuntimeKind::Zai` → `HttpApi`, remove duplicated functions
4. **Update `src/config/mod.rs`** — depends on step 3 (new `RuntimeKind` variant name); add `auth_header_name`/`auth_header_prefix` to all config structs, update `into_effective()` validation, expand `SECRET_PREFIXES`, update starter template
5. **Update `src/runtime/http_api.rs`** — depends on steps 1 and 3; update struct name, import from `http_util`, generalize auth header construction
6. **Update `src/runtime/codex.rs`** — depends on step 1; remove private `parse_runtime_output`, import from `http_util`
7. **Update `src/runtime/status.rs`** — depends on step 3; rename `ProviderId::Zai` → `HttpApi`
8. **Update `src/doctor/mod.rs`** — depends on step 3; add `HttpApi` match arm
9. **Update `src/hooks/follow.rs` and `src/hooks/dispatch.rs`** — depend on step 1; update import paths
10. **Update all test files** — depends on steps 3-8; update `RuntimeKind::Zai` references
11. **Run full test suite** — depends on all previous steps

### Technical Dependencies

- No new crate dependencies required — `reqwest` is already in `Cargo.toml`
- No infrastructure changes — all changes are within the existing Rust codebase
- `SECRET_PREFIXES` expansion depends on confirming OpenRouter and Verboo key prefixes (currently assumed `or-` and `ov-`)

## Monitoring and Observability

- **Log redaction**: Expanded `SECRET_PREFIXES` ensures new provider keys are redacted in all log surfaces
- **Doctor output**: Custom HTTP runtimes appear in `atelier --doctor` with availability status
- **Error messages**: Non-OpenAI-compatible responses produce clear error messages (not silent failures)
- **`atelier --print-config`**: Shows custom runtimes with secrets redacted; users can verify configuration

## Technical Considerations

### Key Decisions

| Decision | Rationale | Trade-offs |
|----------|-----------|------------|
| Two auth fields (`auth_header_name` + `auth_header_prefix`) | Handles all known auth schemes: Bearer, custom header with no prefix (Verboo), API key header | Slightly more verbose than a single parsed field |
| Rename `Zai` → `HttpApi` (breaking change) | Clean naming, no deprecation burden | Existing users must update configs |
| New `http_util.rs` module | Separation of concerns; single source of truth for security-critical code | New module to maintain |
| Config-time validation for `base_url` | Fail early on misconfiguration | May reject future-compatible endpoints |
| Expand `SECRET_PREFIXES` to 4 | Prevents key leaks for OpenRouter and Verboo | Slightly larger redaction pattern set |

### Known Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| OpenRouter/Verboo key prefix assumption is wrong (`or-`/`ov-`) | Medium | Verify with actual API keys before merging; fallback: use generic detection |
| `parse_runtime_output` behavior divergence between old and new copies | Low | Mechanical extraction; run identical test assertions on both copies |
| Starter config template change breaks `--init-config` snapshot tests | Medium | Update snapshot assertions after template change |
| `#[serde(deny_unknown_fields)]` on `RawRuntimeConfig` rejects new fields before upgrade | Low | New fields are `Option<T>` and deserialized correctly by serde; no rename needed |

## Architecture Decision Records

- [ADR-001: Generalize ZaiRuntime into Configurable HTTP API Runtime](adrs/adr-001.md) — Generalize ZaiRuntime with configurable auth headers rather than creating new runtime variants
- [ADR-002: Rename RuntimeKind::Zai to RuntimeKind::HttpApi](adrs/adr-002.md) — Clean rename with breaking change rather than maintaining a deprecated alias
- [ADR-003: Extract Shared HTTP Utilities into http_util.rs](adrs/adr-003.md) — Consolidate duplicated redaction and parsing code into a single module
