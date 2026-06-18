# PRD: Pluggable API Provider Integration (BYOK)

## Overview

Atelier currently requires users to rely on CLI-owned providers (Codex, Claude, Cursor) or a single hardcoded HTTP runtime (Zai) for API access. Users who already pay for API keys through OpenRouter, Verboo, or other OpenAI-compatible providers cannot use them without workarounds. This feature lets users bring their own API keys and point atelier at any OpenAI-compatible endpoint, with configuration through `atelier.toml`.

**Who it is for:** Developers who already have API keys and want to use them with atelier. Cost-conscious users who want access to cheap or free models. Privacy-focused users who need inference on specific infrastructure.

**Why it is valuable:** BYOK is table stakes in the AI coding tool category — Cline, Cursor, Roo Code, JetBrains, VS Code, and Warp all support it. Without BYOK, atelier loses a segment of power users who have existing API investments. The generalized runtime architecture also unblocks every future provider addition as a config-only change.

## Goals

| Goal | Target | Measurement |
|------|--------|-------------|
| Enable BYOK for any OpenAI-compatible provider | Users can configure OpenRouter, Verboo, or custom endpoints via TOML | Config round-trip test: starter config → effective config |
| Reduce time to first BYOK run | <5 minutes from reading docs to first successful completion | Manual testing with fresh install |
| Maintain zero degradation for existing users | No breaking changes to existing runtimes (Codex, Claude, Cursor) | Full test suite passes; starter config works unchanged |
| Clean up duplicated code | 0 duplicated redaction/parsing functions | Grep for duplicate definitions |
| Surface BYOK as discoverable | >80% of users encounter the BYOK example | Starter config template includes HTTP provider block |

## User Stories

### Primary: Cost-Conscious Developer

As a developer who already pays for OpenAI or DeepSeek API access, I want to use my existing API key with atelier so that I don't pay twice for the same inference.

**Flow:** User adds a `[runtimes.openrouter]` block to `atelier.toml` with their `base_url` and `api_key_env`, points an agent's `runtime = "openrouter"`, runs `atelier --doctor` to confirm the key is valid, and starts working. The agent roster in the TUI shows the custom runtime name and model.

### Primary: Privacy-Focused Enterprise User

As a developer working with sensitive code, I want to route inference through a provider that keeps my data within their infrastructure (e.g., Verboo's dedicated GPUs) so that my prompts never leave a controlled environment.

**Flow:** User configures a Verboo runtime with `auth_header_name = "api-key"` (non-standard auth), sets their API key environment variable, and routes the orchestrator through Verboo. The doctor check confirms the endpoint is reachable.

### Secondary: Multi-Project Developer

As a developer working on multiple projects, I want different projects to use different providers and models based on cost and capability needs, without reconfiguring the system globally.

**Flow:** User has project A using OpenRouter (broad model access) and project B using a local Ollama instance (zero cost, air-gapped). Each project has its own `atelier.toml` with provider-specific runtime blocks.

### Secondary: New User Evaluating Atelier

As a developer evaluating atelier, I want to try it with my existing API key without signing up for a new service, so that I can assess the tool before committing.

**Flow:** User runs `atelier --init-config`, sees the commented-out OpenRouter example in the starter config, copies their key into an environment variable, and runs atelier with their own model. The first successful completion is the "aha moment."

## Core Features

### F1: Configurable HTTP API Runtime (Critical)

Generalize the existing ZaiRuntime into a configurable HTTP API runtime that accepts any OpenAI-compatible endpoint. The runtime is parameterized by config: `base_url` for the endpoint, `api_key_env` for the key, `auth_header_name` and `auth_header_prefix` for the auth scheme. Every OpenAI-compatible provider (OpenRouter, Verboo, DeepSeek, Groq, Mistral, Ollama, etc.) works through this single runtime kind.

**User-visible behavior:** Users add a `[runtimes.<name>]` block to `atelier.toml` with `type = "http_api"` and provider-specific settings. The TUI roster shows the custom runtime name alongside the model. The existing streaming-with-fallback behavior and error classification are preserved.

**Functional requirements:**
- Configurable auth header name and prefix (defaults: `Authorization` / `Bearer`)
- SSE streaming with automatic fallback to non-streaming on rejection
- Retryable error detection for rate limits, overloads, and server errors
- Model fallback chains work across models within the same runtime
- Secrets redacted in logs and `--print-config` output

### F2: Extract Shared HTTP Utilities (Critical)

Consolidate duplicated code between the existing HTTP runtime and the dispatch layer into a shared module. This reduces maintenance burden and prevents security-relevant code (secret redaction) from diverging.

**User-visible behavior:** No visible change. Existing behavior is preserved. Future providers benefit from consistent error handling and redaction.

### F3: OpenRouter Provider Preset (High)

Document a ready-to-copy TOML configuration block for OpenRouter. The preset uses the standard `Authorization: Bearer` auth scheme and points at `https://openrouter.ai/api/v1`. Include model examples showing how to use OpenRouter's model ID format (`provider/model-name`).

**User-visible behavior:** Users copy a 4-line config block from the docs, paste their OpenRouter API key into an environment variable, and have access to 400+ models.

### F4: Verboo Provider Preset (High)

Document a ready-to-copy TOML configuration block for Verboo. The preset uses Verboo's custom `api-key` auth header (non-standard: `auth_header_name = "api-key"`, `auth_header_prefix = ""`) and points at the Verboo generative API endpoint.

**User-visible behavior:** Users copy a 5-line config block, set their Verboo API key, and route inference through Verboo's dedicated GPU infrastructure.

### F5: Config Template Example (High)

Add a commented-out HTTP provider example to the starter `atelier.toml` template. The example includes the `type = "http_api"` block with inline comments explaining each field, and a note pointing to the full provider documentation.

**User-visible behavior:** When a user runs `atelier --init-config` or opens the starter config, they see a clearly documented example of how to add a custom provider.

### F6: Doctor Validation for Custom Runtimes (Medium)

Extend `atelier --doctor` to validate custom HTTP runtimes. The check verifies that the configured environment variable exists and contains a non-empty value, then reports availability. For legacy configs using `type = "zai"`, doctor suggests migrating to `type = "http_api"`.

**User-visible behavior:** After configuring a custom provider, running `atelier --doctor` shows whether the API key is set and the endpoint is reachable. If using the old `type = "zai"` name, a migration hint appears.

### F7: Model-Routing Documentation Seed (Medium)

Frame the BYOK documentation around "use any model for any agent" rather than just "bring your own key." The docs explain that each agent's `model` field works with any HTTP API runtime, and seed the concept that different agents can use different providers based on cost and capability needs.

**User-visible behavior:** Documentation reads as "configure which model each agent uses" rather than "configure API keys." This sets expectations for the V2 model-agnostic task routing feature.

## User Experience

### Primary Flow: First-Time BYOK Setup

1. User runs `atelier --init-config` and opens the starter config
2. Sees the commented-out HTTP provider example with inline documentation
3. Copies the example block, sets their API key in an environment variable
4. Points an agent's `runtime` field to the new runtime name
5. Runs `atelier --doctor` — sees the custom runtime validated with green status
6. Runs atelier — the TUI roster shows the custom runtime name and model
7. First successful completion confirms the setup works

### Primary Flow: Switching Providers

1. User edits `atelier.toml` to change an agent's `runtime` field
2. Optionally updates the `model` field for the new provider
3. Runs `atelier --doctor` to validate the new configuration
4. Starts a new session — the roster reflects the updated runtime

### Onboarding and Discoverability

- **Starter config template:** Commented-out HTTP provider example visible on first init
- **Doctor output:** Validates custom runtimes and surfaces availability; suggests migration for legacy `type = "zai"` configs
- **Documentation:** Frames BYOK as "use any model for any agent" with ready-to-copy config blocks for OpenRouter and Verboo
- **No TUI-level provider picker:** V1 is config-file-based; in-TUI switching is a V2 feature

### UI/UX Considerations

- The TUI agent roster already shows `runtime/model` per agent — custom HTTP runtimes appear with their configured name and model
- No global "current provider" indicator is needed; the per-agent roster provides sufficient visibility
- `atelier --print-config` shows custom runtimes with secrets redacted, giving users a way to verify their configuration
- `--doctor` provides a clear success/failure signal for each configured runtime

## High-Level Technical Constraints

- **OpenAI-compatible only:** V1 supports providers that expose the OpenAI Chat Completions API format (`/v1/chat/completions`). Providers with non-OpenAI formats (Anthropic Messages API, Google Vertex AI, AWS Bedrock) are out of scope — users can route through OpenRouter for access.
- **No breaking changes to existing runtimes:** Codex, Claude, Cursor, and Fake runtimes must continue to work identically. The rename from `Zai` to `HttpApi` only affects the config type name, not runtime behavior.
- **API key security:** Keys are stored as environment variables (existing `api_key_env` pattern). No key vault or encrypted storage in V1. Secrets are redacted in all output surfaces (logs, `--print-config`, error messages).
- **Config merge order preserved:** Custom HTTP runtimes follow the same merge semantics as existing runtimes: built-in defaults → home config → local override → CLI flags.

## Non-Goals (Out of Scope)

- **Automatic provider fallback across providers:** V1 uses manual switching only. Auto-fallback (e.g., try OpenRouter, then fall back to Verboo) requires a routing policy engine and is a V2 feature.
- **Provider-specific adapters for non-OpenAI formats:** Anthropic Messages API, Google Vertex AI, and AWS Bedrock have incompatible API formats. These require dedicated adapter code and are out of V1 scope.
- **Interactive provider picker or `/provider` slash command:** GUI-style provider switching is a V2 concern. V1 is config-file-based.
- **Per-request cost attribution:** OpenRouter's 5.5% fee and provider-specific pricing will generate demand for cost tracking, but this is a separate feature.
- **Provider marketplace or managed partnerships:** Business development with OpenRouter, Verboo, etc. for built-in zero-config access is worth exploring but shouldn't block V1 engineering.
- **API key vault or secure storage:** V1 uses `api_key_env` (shell environment variables). A proper key vault with encryption is a V2 security enhancement.
- **Multi-key management:** Switching between personal and work keys, or between multiple accounts on the same provider, is a V2 feature.
- **Model-agnostic task routing:** Automatically routing different agents to different providers based on cost, capability, and privacy constraints is the V2 stretch goal. V1 seeds this concept in documentation only.

## Phased Rollout Plan

### MVP (Phase 1)

**Included:**
- F1: Configurable HTTP API runtime (generalized from ZaiRuntime)
- F2: Extract shared HTTP utilities
- F3: OpenRouter provider preset (docs)
- F4: Verboo provider preset (docs)
- F5: Config template example

**Success criteria:**
- User can configure OpenRouter or Verboo in `atelier.toml` and run a successful completion
- `atelier --doctor` validates custom runtimes (env var check)
- Existing runtimes (Codex, Claude, Cursor) are unaffected
- Full test suite passes

### Phase 2

**Included:**
- F6: Doctor validation for custom runtimes (env var check + availability)
- F7: Model-routing documentation seed
- `--doctor` migration hint for legacy `type = "zai"` configs

**Success criteria:**
- >25% of active users have a non-default HTTP API runtime configured within 90 days
- BYOK error rate <10% on custom runtimes
- Provider diversity >5 distinct `base_url` values across user configs

### Phase 3

**Included:**
- Model-agnostic task routing (V2 `[routing]` config section)
- Interactive provider picker or `/provider` slash command
- Per-request cost attribution

**Success criteria:**
- Users can declare routing rules (e.g., "use cheapest model for refactoring, smartest for architecture decisions")
- Cost tracking shows per-agent spend across providers

## Success Metrics

| Metric | Target | How to Measure |
|--------|--------|----------------|
| BYOK adoption rate | >25% of active users within 90 days | Count users with non-default `http_api` runtime configured |
| Time to first BYOK call | <5 minutes from config to run | Manual testing with fresh install |
| BYOK error rate | <10% failure rate | `RuntimeProviderError` count vs total steps on `http_api` runtimes |
| Provider diversity | >5 distinct `base_url` values | Count unique `base_url` entries across user configs |
| Config discoverability | >80% of users see BYOK example | Starter config template includes HTTP provider block (proxy metric) |
| Zero regression for existing users | 0 new failures on existing runtimes | Full test suite passes; starter config works unchanged |

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Users break on upgrade due to `type = "zai"` rename | High — existing configs stop working | `--doctor` detects legacy `type = "zai"` and suggests migration; changelog entry with clear instructions |
| Provider deviations from OpenAI spec flood in as bug reports | Medium — support burden | Document "OpenAI-compatible only" scope prominently; return clear error for incompatible responses |
| Users confuse `http_api` with CLI-based runtimes | Low — documentation clarity | Document clearly: `http_api` is for API-key-based providers; `codex`/`claude`/`cursor` are CLI-based |
| BYOK discoverability is insufficient | Medium — feature goes unused | Config template comment + `--doctor` hint; documentation frames BYOK as a primary setup path |
| Duplication refactor breaks existing behavior | Medium — regression | Consolidate into shared module with same test coverage; run full test suite before merge |

## Architecture Decision Records

- [ADR-001: Generalize ZaiRuntime into Configurable HTTP API Runtime](adrs/adr-001.md) — Generalize ZaiRuntime with configurable auth headers rather than creating new runtime variants
- [ADR-002: Rename RuntimeKind::Zai to RuntimeKind::HttpApi](adrs/adr-002.md) — Clean rename with breaking change rather than maintaining a deprecated alias

## Open Questions

- **Verboo API endpoint:** Research found multiple endpoints (chat-api, generative-api, admin-api) but could not confirm which one exposes OpenAI-compatible chat completions. Needs hands-on testing with a Verboo Growth plan key.
- **Migration helper scope:** Should `--doctor` offer an automatic config migration for `type = "zai"` → `type = "http_api"`, or just print a suggestion? Auto-migration is more helpful but adds complexity.
- **Config validation timing:** Should non-OpenAI-compatible endpoints be rejected at config load time (fail early) or at first request (clear error message)? Config-time validation is safer but may reject future-compatible endpoints.
- **OpenRouter model discovery:** Should the docs include a curated list of recommended models (e.g., `openai/gpt-4.1` for quality, `deepseek/deepseek-v3` for cost, `:free` variants for zero-cost), or just link to OpenRouter's model catalog?
