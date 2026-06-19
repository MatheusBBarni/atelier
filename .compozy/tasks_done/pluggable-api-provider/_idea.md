# Idea: Pluggable API Provider Integration (BYOK)

## Overview

Atelier is locked to CLI-owned providers (Codex, Claude, Cursor) and one HTTP-based runtime (Zai) for API access. Users who already pay for API keys — through OpenRouter, Verboo, DeepSeek, Groq, or direct OpenAI — cannot use them without workarounds. This feature generalizes the existing `ZaiRuntime` into a configurable HTTP API runtime that accepts any OpenAI-compatible endpoint, with user-provided API keys configured via `atelier.toml`. V1 ships OpenRouter + Verboo as the first BYOK providers, with manual switching via config. The docs seed a model-agnostic task routing vision for V2.

**Target user:** Developers who already have API keys and want to use them with atelier, cost-conscious users who want access to cheap/free models (DeepSeek, Ollama, free OpenRouter tier), and privacy-focused users who need inference on specific infrastructure (Verboo, self-hosted).

**V1 ambition:** Low-risk refactor that unblocks the entire OpenAI-compatible provider ecosystem through config-only additions. Zero new runtime code — just generalize what exists.

## Problem

Atelier's current architecture binds each agent to a fixed runtime via `RuntimeKind` enum. Adding a new provider requires a new Rust variant, new dispatch match arms, and new config validation — even when the HTTP protocol is identical. Users who bring their own API keys face three problems:

1. **Discovery gap.** The config template does not surface BYOK as an option. Users must already know that `ZaiRuntime` can be pointed at arbitrary OpenAI-compatible endpoints. Without documentation or a first-run prompt, the feature is invisible.

2. **Auth rigidity.** The hardcoded `.bearer_auth()` scheme in `ZaiRuntime` breaks providers that use non-standard auth headers. Verboo, for example, requires `api-key: xxx` instead of `Authorization: Bearer xxx`. There is no config knob to override this.

3. **Duplicated code.** Redaction logic (~80 lines) and `parse_runtime_output` (three copies) are duplicated between `zai.rs` and `mod.rs`. Adding more HTTP-based providers multiplies this debt.

The result: users who already pay for API access cannot use atelier without switching to a different tool, while competitors (Cline, Cursor, Roo Code) treat BYOK as a headline feature.

### Market Data

- **OpenRouter**: 400+ models from 60+ providers, 5.5% platform fee, 1M free BYOK requests/month. Raised $113M Series B. The dominant BYOK surface for AI developers.
- **Verboo**: Brazilian AI platform running open-source models (mimo-v2.5, deepseek-v4-flash, qwen3.6-27b) on dedicated GPUs. OpenAI-compatible API with conversation-based pricing (~$0.11/conversation on Growth plan). Key differentiator: no data leaves their infrastructure.
- **OpenAI-compatible ecosystem**: Mistral, DeepSeek, Groq, Together AI, Fireworks, Ollama, vLLM all expose OpenAI Chat Completions format. Anthropic is the major exception (own Messages API).
- **BYOK adoption**: Cline (58K+ GitHub stars), Cursor, Roo Code, Warp, and Replit all support BYOK. It is table stakes in the AI coding tool category.
- **Cost spectrum**: From free (OpenRouter free tier, Ollama local) to $0.14/1M input (DeepSeek) to $5.00/1M input (Claude Opus). Users want the ability to choose based on task complexity.

## Core Features

| #   | Feature                          | Priority  | Description                                                                                     |
| --- | -------------------------------- | --------- | ----------------------------------------------------------------------------------------------- |
| F1  | Configurable HTTP API runtime    | Critical  | Generalize `ZaiRuntime` into `HttpApi` with configurable `auth_header_name` and `auth_header_prefix` fields in `RuntimeConfig`. Every OpenAI-compatible provider works via TOML config. |
| F2  | Extract shared HTTP utilities    | Critical  | Consolidate duplicated `redact_sensitive_text`, `redact_bearer_tokens`, and `parse_runtime_output` into `src/runtime/http_util.rs`. |
| F3  | OpenRouter provider preset       | High      | Document a ready-to-copy TOML config block for OpenRouter: `base_url = "https://openrouter.ai/api/v1"`, `api_key_env = "OPENROUTER_API_KEY"`. Include model examples. |
| F4  | Verboo provider preset           | High      | Document a ready-to-copy TOML config block for Verboo with `auth_header_name = "api-key"` and `auth_header_prefix = ""`. |
| F5  | Config template example          | High      | Add a commented-out HTTP provider example to the starter `atelier.toml` template so BYOK is discoverable out of the box. |
| F6  | `atelier --doctor` custom runtime validation | Medium | Minimal status check for custom HTTP runtimes: verify env var exists, report availability. One-day addition. |
| F7  | Model-routing docs seed          | Medium    | Frame BYOK docs around "use any model for any agent" rather than just "bring your own key." Seeds the V2 model-agnostic task routing vision without building it. |

## KPIs

| KPI                      | Target                          | How to Measure                                   |
| ------------------------ | ------------------------------- | ------------------------------------------------ |
| BYOK adoption rate       | >25% of active users within 90d | Count users with non-default `http_api` runtime configured |
| Time to first BYOK call  | <5 minutes from config to run   | Track config-save to first successful runtime step (manual testing) |
| Provider diversity       | >5 distinct `base_url` values   | Count unique `base_url` entries across user configs |
| BYOK error rate          | <10% failure rate               | Track `RuntimeProviderError` vs total steps on `http_api` runtimes |
| Config discoverability   | >80% of users see BYOK example  | Track if starter config template includes HTTP example (proxy metric) |
| Duplication debt         | 0 duplicated redaction/parsing functions | Grep for duplicate `redact_` and `parse_runtime_output` definitions |

## Feature Assessment

| Criteria            | Question                                            | Score   |
| ------------------- | --------------------------------------------------- | ------- |
| **Impact**          | How much more valuable does this make the product?  | Must do |
| **Reach**           | What % of users would this affect?                  | Strong  |
| **Frequency**       | How often would users encounter this value?         | Strong  |
| **Differentiation** | Does this set us apart or just match competitors?   | Maybe   |
| **Defensibility**   | Is this easy to copy or does it compound over time? | Pass    |
| **Feasibility**     | Can we actually build this?                         | Must do |

**Leverage type:** Quick Win — existing architecture already supports this pattern; the refactor is config-driven with zero new runtime code.

## Council Insights

- **Recommended approach:** Generalize `ZaiRuntime` into a configurable `HttpApi` runtime kind with parameterized auth headers. Extract duplicated code into a shared module. Ship OpenRouter + Verboo presets as config-only additions.
- **Key trade-offs:**
  - Generalizing the runtime is low-risk but opens a door to provider-support burden (each deviation from OpenAI spec becomes a bug report)
  - Manual switching in a terminal-native tool creates UX friction that GUI competitors don't have — acceptable for V1, but the config schema must support future routing
  - BYOK is the right user-facing term (searchable, familiar) but the config section should be named `http_api` to leave room for model-routing semantics
- **Risks identified:**
  - Provider deviation flood after V1 ships — mitigate with documented "OpenAI-compatible only" scope and clear error messages
  - Duplication refactor breaks existing behavior — mitigate with same test coverage and full test suite before merge
  - Users confuse `http_api` with CLI-based runtimes — mitigate with clear documentation distinguishing the two
- **Stretch goal (V2+):** Model-agnostic task routing — route different agents to different providers based on cost, capability, and privacy constraints. The `model` field on `AgentProfile` already works with any runtime; the missing piece is a routing policy engine (perhaps a `[routing]` config section).

## Out of Scope (V1)

- **Automatic provider fallback** — V1 uses manual switching only; auto-fallback across providers is a V2 feature that requires a routing policy engine
- **Provider-specific adapters** — Anthropic Messages API, Google Vertex AI, and AWS Bedrock have non-OpenAI formats; these require dedicated adapter code and are out of V1 scope (route through OpenRouter for access)
- **Interactive provider picker / `/provider` slash command** — GUI-style provider switching is a V2 concern; V1 is config-file-based
- **Per-request cost attribution** — OpenRouter's 5.5% fee and provider-specific pricing will generate demand for cost tracking, but this is a separate feature
- **Provider marketplace / managed partnerships** — Business development with OpenRouter, Verboo, etc. for built-in zero-config access is worth exploring but shouldn't block V1 engineering
- **API key vault / secure storage** — V1 uses `api_key_env` (shell environment variables); a proper key vault with encryption is a V2 security enhancement

## Architecture Decision Records

- [ADR-001: Generalize ZaiRuntime into Configurable HTTP API Runtime](adrs/adr-001.md) — Decides to generalize ZaiRuntime with configurable auth headers rather than creating new runtime variants

## Open Questions

- Should `atelier --doctor` validate custom HTTP runtimes in V1, or is that V2 scope? The council recommended including it (one-day addition), but the devil's advocate pushed back.
- What is the exact Verboo API endpoint format? Research found multiple endpoints (chat-api, generative-api, admin-api) but could not confirm which one exposes OpenAI-compatible chat completions. Needs hands-on testing.
- Should the `RuntimeKind::Zai` variant be renamed to `HttpApi` (breaking change) or should `HttpApi` be added as a new variant with `Zai` deprecated? The rename is cleaner but requires config migration.
- How should non-OpenAI-compatible endpoints be handled? Validate at config time (reject early) or at first request (clear error message)?

## Integration with Existing Features

| Integration Point              | How                                                                                |
| ------------------------------ | ---------------------------------------------------------------------------------- |
| `ZaiRuntime` (`src/runtime/zai.rs`) | Generalized into `HttpApi`; auth reads from config instead of hardcoding `.bearer_auth()` |
| `RuntimeConfig` (`src/config/mod.rs`) | Extended with `auth_header_name` and `auth_header_prefix` optional fields           |
| `RuntimeKind` enum (`src/config/mod.rs`) | Renamed or aliased: `Zai` → `HttpApi`                                              |
| `execute_runtime_step_streaming` (`src/runtime/mod.rs`) | Dispatch updated to use `HttpApi` kind; no logic changes                            |
| `atelier --doctor` (`src/cli.rs`) | Optional: add custom runtime status check                                         |
| Starter config template         | Commented-out HTTP provider example added                                          |
