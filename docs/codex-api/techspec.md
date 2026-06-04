# Technical Specification: OpenAI Responses Runtime

Status: Draft
Version: 1.0
Date: 2026-06-04
Source PRD: `docs/prd/codex-api-prd.md`

## Executive Summary

This specification defines the implementation plan for an optional `openai_responses` execution runtime in `multiagent`. The runtime will call the OpenAI Responses API with an environment-variable credential reference, map results into the existing runtime output contract, and keep file edits, shell commands, approvals, and history persistence owned by the Rust harness.

The first implementation is a schema-compatible, non-streaming HTTP adapter modeled after the existing Z.ai runtime. Native Responses function tools, response state, streaming, background mode, and migration ADR work are deferred into explicit later phases.

## References

- Product requirements: `docs/prd/codex-api-prd.md`
- Source planning doc: `docs/codex-api.md`
- Stream-mode planning doc: `docs/stream-mode.md`
- Domain glossary: `CONTEXT.md`
- Current Codex runtime ADR: `docs/adr/0001-codex-runtime-uses-cli-subscription.md`
- OpenAI Structured Outputs: https://developers.openai.com/api/docs/guides/structured-outputs
- OpenAI Function Calling: https://developers.openai.com/api/docs/guides/function-calling
- OpenAI Streaming Responses: https://developers.openai.com/api/docs/guides/streaming-responses
- OpenAI GPT-5.2-Codex model page: https://developers.openai.com/api/docs/models/gpt-5.2-codex

## 1. Background

`multiagent` currently supports three execution runtimes:

- `codex`: launches the Codex CLI as a child process.
- `zai`: posts to a Z.ai chat-completions HTTP endpoint with API-key auth.
- `fake`: deterministic local runtime for tests and simulation.

The `codex` runtime keeps subscription-backed Codex behavior isolated from direct OpenAI Platform API calls. It also has costs: each agent step starts a child process, waits for complete stdout/stderr, and cannot directly use Responses API features such as structured outputs, response IDs, function calling, streaming, or background mode.

The new runtime is not a replacement for the Codex CLI runtime. It is a separate OpenAI Platform API-key runtime for users who explicitly configure agent profiles to use it.

## 2. Goals

- Add `openai_responses` as a first-class execution runtime type.
- Keep `codex`, `zai`, and `fake` behavior unchanged.
- Use OpenAI Platform API keys through `api_key_env`, defaulting to `OPENAI_API_KEY`.
- Do not read Codex CLI auth files, Codex access tokens, or ChatGPT session state.
- Map OpenAI Responses output into existing `RuntimeOutput` variants.
- Keep harness actions centralized in the app/action layer.
- Keep default tests offline and credential-free.
- Add docs and diagnostics that distinguish Platform API-key auth from Codex CLI subscription auth.

## 3. Non-Goals

- Replacing or renaming the `codex` runtime.
- Implementing a private Codex subscription HTTP endpoint.
- Reading `~/.codex/auth.json`, `CODEX_ACCESS_TOKEN`, or ChatGPT login state.
- Enabling hosted shell, local shell, or hosted apply-patch tools by default.
- Implementing native function tools in phase 1.
- Implementing app-level streaming before the stream sink contract exists.
- Implementing background mode in phase 1.
- Hardcoding a long-term built-in Codex model without refreshing official docs.

## 4. Current Architecture

### Runtime Contract

The runtime boundary is:

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;
    async fn stream_step(&self, request: RuntimeRequest) -> Result<RuntimeStepResult>;
}
```

`RuntimeRequest` carries the prompt, session goal, working directory, agent profile, session history, recent context, previous agent results, action results, output schema, capability constraints, and run limits.

`RuntimeStepResult` returns:

- one `RuntimeOutput`;
- zero or more `RuntimeStreamDelta` records.

`RuntimeOutput` already supports:

- `OrchestratorDecision`
- `AgentResult`
- `ActionRequest`
- `ParseError`

### Config Contract

Runtime config is represented by `RuntimeKind`, `RuntimeConfig`, `RawRuntimeConfig`, and `MergedRuntimeConfig`.

Existing credential handling uses `api_key_env` plus `validate_env_reference`, which rejects empty values, malformed environment variable names, and raw-looking secrets such as strings beginning with `sk-` or `zai-`.

### HTTP Runtime Prior Art

`ZaiRuntime` already provides the closest implementation template:

- read `api_key_env` from config;
- check credential presence in `check_availability`;
- build a JSON request from `RuntimeRequest`;
- use `reqwest` with a timeout;
- send bearer auth;
- classify retryable HTTP statuses;
- parse final text into action requests, orchestrator decisions, agent results, or parse errors;
- emit one final stream delta.

### Harness Action Boundary

The action module owns `ActionRequest`, `ActionResult`, capability checks, tool allowlists, path policy, command classification, VCS action gating, approval handling, and execution. The OpenAI runtime must not bypass this boundary.

## 5. Target Architecture

Add a new runtime adapter module:

```text
src/runtime/openai_responses.rs
```

The adapter will implement the existing `Runtime` trait and be dispatched from the same availability and execution paths as the other runtimes.

Phase 1 flow:

1. App constructs a `RuntimeRequest`.
2. Runtime dispatch selects `OpenAiResponsesRuntime`.
3. Adapter reads the configured API key environment variable.
4. Adapter builds a Responses API request from the prompt envelope and agent profile.
5. Adapter posts to `{base_url}/responses`.
6. Adapter extracts final output text from the response.
7. Adapter parses the text into the existing harness output contract.
8. App continues existing orchestration, action, approval, and history behavior.

No new app state machine behavior is required in phase 1.

## 6. Configuration Design

### Runtime Type

Add a `RuntimeKind` variant with an explicit serde rename:

```rust
#[serde(rename = "openai_responses")]
OpenAiResponses,
```

The explicit rename is required because the repo uses `rename_all = "snake_case"` and the PRD requires the user-facing value `openai_responses`, not `open_ai_responses`.

### Runtime Config

Phase 1 reuses the existing `RuntimeConfig` fields:

- `id`
- `kind`
- `base_url`
- `api_key_env`
- `args` unused and normalized to an empty vector
- `prompt_mode` normalized to `stdin`
- `command` unused and normalized to `None`

Example config:

```toml
[runtimes.openai]
type = "openai_responses"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"

[agents.fixer]
runtime = "openai"
model = "gpt-5.2-codex"
effort = "high"
```

`gpt-5.2-codex` is an example model assignment, not a permanent built-in default. Model guidance must be refreshed from official docs before adding any built-in OpenAI agent profile defaults.

### Built-In Defaults

Do not add a built-in `openai` runtime in phase 1. Users should opt in through home or local config. This avoids showing a new unavailable runtime warning to users who never configured OpenAI.

### Config Merge and Validation

Add `OpenAiResponses` handling in effective config construction:

- require `api_key_env`, defaulting to `OPENAI_API_KEY` if omitted;
- validate `api_key_env` with existing credential-reference validation;
- default `base_url` to `https://api.openai.com/v1`;
- trim trailing slashes from `base_url` using existing merge behavior;
- set `command = None`;
- set `args = Vec::new()`;
- set `prompt_mode = PromptMode::Stdin`.

Future config fields should not be added until their behavior exists:

- `store`
- `background`
- `stream`
- `response_state`

When these are added, validation must reject impossible combinations such as `store = false` with `response_state = "previous_response_id"`.

## 7. Runtime Request Design

### Endpoint

Phase 1 sends:

```text
POST {base_url}/responses
Authorization: Bearer <api-key>
Content-Type: application/json
```

With the default base URL, the full endpoint is:

```text
https://api.openai.com/v1/responses
```

### Request Body

The request body should be built with `serde_json::json!` and must include:

- `model`: `request.agent_profile.model`
- `instructions`: agent instructions plus the runtime contract
- `input`: serialized prompt envelope from `prompt_envelope_json`
- `stream`: `false`
- `store`: `false` for phase 1 stateless no-storage mode, if supported by the current API contract
- `reasoning.effort`: mapped from `AgentEffort` when supported by the selected model/API
- `text.format`: JSON Schema for final result output when practical

The structured-output field should be verified against current OpenAI docs during implementation. Current docs describe Responses structured output through `text.format` with `type = "json_schema"`, `strict = true`, and a schema body.

### Output Schema Selection

`request.output_schema` determines the expected final schema:

- `orchestrator_decision` for orchestrator steps;
- `agent_result` for specialized agent steps.

Action requests remain compatibility text contracts in phase 1. The runtime first attempts to parse an action request contract, matching current `ZaiRuntime` behavior.

### Prompt Contract

The runtime instructions must preserve the current harness policy:

- Return only a structured contract.
- Do not claim to edit files, run commands, or inspect the repository directly.
- If repository data or mutation is needed, return one action request.
- If the current step is complete, return the requested output schema.

The OpenAI runtime can share a helper with the Codex/Z.ai prompt contract only if doing so reduces duplication without changing existing behavior. Otherwise, keep a local helper in the new adapter.

## 8. Response Parsing Design

### Content Extraction

Implement a helper that extracts final text from common Responses output shapes. It should accept the stable shape documented by OpenAI and be defensive around missing fields.

Preferred extraction order:

1. Top-level `output_text` string, if present.
2. Concatenation of text items inside `output[]`, when item/content shapes expose text.
3. Provider diagnostic failure if no text can be found.

The helper should return a non-retryable provider error for unexpected successful response JSON that does not contain usable text.

### Runtime Output Mapping

The adapter maps content using this order:

1. `parse_contract::<ActionRequest>(content)`
2. `parse_orchestrator_decision(content)` if `agent_profile.id == "orchestrator"`
3. `parse_agent_result(content)` otherwise
4. `RuntimeOutput::ParseError`

The parse-error path should mirror current runtime behavior: it is a runtime output, not a provider transport error, so model fallback should not trigger.

### Stream Delta

Phase 1 emits exactly one final delta:

```rust
RuntimeStreamDelta::final_delta(1, "message", content)
```

Do not attempt token streaming until the app-level stream sink contract from `docs/stream-mode.md` exists.

## 9. Error Handling

Add OpenAI-specific error messages, but reuse `RuntimeProviderError` for retry behavior.

### Retryable Provider Errors

Treat these as retryable:

- request timeout;
- network connection failure;
- HTTP 408;
- HTTP 429;
- HTTP 5xx.

Retryable errors can trigger existing model fallback behavior in `execute_runtime_step`.

### Non-Retryable Provider Errors

Treat these as non-retryable:

- missing `api_key_env`;
- unset or empty credential environment variable;
- invalid `base_url`;
- HTTP 400 request validation failure;
- HTTP 401 or 403 auth failure;
- successful response with no extractable output text;
- JSON response parse failure caused by provider shape mismatch.

### Redaction

Never include these in diagnostics, history, or artifacts:

- bearer token values;
- complete Authorization headers;
- raw API key environment values.

Diagnostic response bodies should be parsed as JSON when possible and redacted before formatting. Non-JSON bodies should be whitespace-normalized and truncated, following the current HTTP runtime style.

## 10. Doctor and Config Output

### Doctor

Add `OpenAiResponses` handling in doctor title rendering:

```text
OpenAI Responses Runtime
```

Doctor context may include:

- runtime id;
- runtime type;
- base URL;
- credential environment variable name.

Doctor context must not include:

- credential environment variable value;
- Authorization header;
- raw response bodies containing secrets.

Availability behavior:

- missing `api_key_env`: unavailable;
- unset env var: unavailable;
- set non-empty env var: unknown, because network/API validation is deferred until a step runs.

### Print Config

`to_redacted_toml` should render:

- `type = "openai_responses"`
- `base_url`
- `api_key_env`

It should not render command/prompt-mode fields for the OpenAI runtime unless those fields become meaningful later.

## 11. Native Function Tools, Deferred

Phase 1 does not send function tools.

Phase 2 should expose strict custom function tools that map to the existing harness actions:

- `read_file`
- `list_files`
- `search_text`
- `run_command`
- `apply_patch`
- `write_file`
- `record_note`

Responses function-call items should be converted into `ActionRequest` and returned as `RuntimeOutput::ActionRequest`. The app then executes the action through existing validation and approval behavior.

After an action result exists, the runtime should send `function_call_output` items back to Responses and continue until it receives a final structured result.

The implementation must reject:

- unknown function names;
- malformed arguments;
- tool calls outside the agent's effective tool list;
- multiple parallel tool calls unless the app loop supports them.

For deterministic phase 2 behavior, set provider tool choice to allow zero or one function call per turn where the current API supports that constraint.

## 12. Streaming and Background Mode, Deferred

### Streaming

Streaming depends on the app-level runtime event sink planned in `docs/stream-mode.md`.

When implemented:

- send `stream = true`;
- map `response.output_text.delta` events into runtime text deltas;
- map `response.created`, `response.completed`, and `error` events into status/error runtime events;
- buffer full content for final structured parsing;
- coalesce small chunks before durable history writes.

### Background Mode

Background mode is opt-in and should be introduced only after phase 1 is stable.

When implemented:

- send `background = true` for configured long-running steps;
- persist response IDs in run/step history;
- poll response retrieval for status;
- cancel active background responses on user interrupt when supported;
- require storage-enabled config for response ID recovery.

## 13. Security and Privacy

- Platform API-key auth is separate from Codex CLI subscription auth.
- The OpenAI runtime must not inspect Codex CLI auth state.
- The OpenAI runtime must not execute local actions directly.
- Hosted provider tools are disabled by default.
- Local file and command effects continue through harness actions.
- VCS actions remain gated by explicit user prompts.
- Raw secrets are rejected in config and redacted in diagnostics.
- `store = false` is the phase 1 privacy default when supported by the current API contract.
- Response state and background mode require explicit storage semantics before implementation.

## 14. Implementation Plan

### Phase 1: Runtime Skeleton and Config

- Add `OpenAiResponses` to `RuntimeKind` with explicit serde rename.
- Add `pub mod openai_responses`.
- Add runtime dispatch in availability and step execution.
- Add effective config construction for `openai_responses`.
- Add doctor title and context handling.
- Add README runtime docs and an example config.

### Phase 2: Non-Streaming Responses Call

- Implement `OpenAiResponsesRuntime`.
- Implement availability checks from `api_key_env`.
- Implement request body construction.
- Implement `POST {base_url}/responses`.
- Implement response text extraction.
- Implement output parsing into existing `RuntimeOutput`.
- Implement retryable/non-retryable provider error classification.
- Implement conservative redaction.

### Phase 3: Mock and Integration Testing

- Add unit tests for config merge, serde name, env validation, and redacted TOML.
- Add mock HTTP tests for request shape and response parsing.
- Add mock HTTP tests for auth failures, rate limits, server failures, missing output text, and redaction.
- Add ignored real OpenAI smoke test gated behind explicit environment variables.

Suggested integration gates:

```text
MULTIAGENT_RUN_OPENAI_INTEGRATION=1
MULTIAGENT_OPENAI_API_KEY_ENV=OPENAI_API_KEY
MULTIAGENT_OPENAI_BASE_URL=https://api.openai.com/v1
MULTIAGENT_OPENAI_MODEL=<model>
```

### Phase 4: Native Function Tools

- Add custom function tool schema builders.
- Map function calls to `ActionRequest`.
- Map `ActionResult` to `function_call_output`.
- Preserve response items required for continuation.
- Add tests for allowed and denied action calls.

### Phase 5: Streaming and Background

- Wait for runtime event sink contract.
- Implement SSE streaming parser.
- Add live-delta tests.
- Add background response polling, persistence, and cancellation.

### Phase 6: Migration Documentation and ADR

- Add docs comparing `codex`, `zai`, and `openai_responses`.
- Add an ADR extending or superseding the Codex CLI subscription-auth ADR.

## 15. Test Plan

### Unit Tests

- `RuntimeKind` deserializes `openai_responses`.
- Redacted TOML prints `type = "openai_responses"` and credential reference names only.
- Raw secret-looking `api_key_env` values are rejected.
- OpenAI runtime defaults are applied correctly.
- OpenAI runtime cannot change type across merged config sources.
- Doctor reports missing, unset, and set credential-reference states correctly.

### Runtime Mock Tests

Use a local `TcpListener` mock server, matching the existing Z.ai test style.

Verify successful requests include:

- `POST /responses`;
- bearer auth header;
- configured model;
- prompt envelope input;
- agent instructions;
- `stream = false`;
- expected storage flag;
- structured output settings when enabled.

Verify successful responses parse into:

- `RuntimeOutput::OrchestratorDecision`;
- `RuntimeOutput::AgentResult`;
- `RuntimeOutput::ActionRequest`;
- `RuntimeOutput::ParseError`.

Verify provider failures:

- 401/403 are non-retryable;
- 429 and 5xx are retryable;
- network errors are retryable;
- missing output text is non-retryable;
- diagnostics redact bearer tokens.

### Integration Tests

Add one ignored smoke test in `tests/runtime_integration.rs` after the adapter is implemented. It should mirror existing Codex and Z.ai integration tests and require explicit environment gates.

Default `cargo test` must not require OpenAI credentials or network access.

## 16. Acceptance Criteria

- `cargo test` passes without network access or API credentials.
- `type = "openai_responses"` is accepted in runtime config.
- A configured OpenAI runtime appears in `--doctor` output with no secret leakage.
- Missing `OPENAI_API_KEY` marks the runtime unavailable without breaking unrelated runtimes.
- A mock Responses server verifies request path, auth, body shape, and parsing.
- Successful mock orchestrator and agent responses produce the expected runtime outputs.
- Mock action-request output uses the existing action-request contract.
- Retryable provider failures can use model fallbacks.
- Non-retryable OpenAI request/auth/schema failures do not trigger model fallback.
- Existing `codex`, `zai`, and `fake` runtime tests continue to pass.
- README or runtime docs explain auth differences between Codex CLI and OpenAI Platform API.
- No OpenAI runtime path reads local Codex login files or Codex access tokens.

## 17. Risks and Mitigations

### API Shape Drift

OpenAI Responses fields and recommended models can change. Keep provider-specific request/response helpers isolated in the adapter and verify current docs before implementation.

### Auth Confusion

Users may expect ChatGPT subscription auth to work with the direct runtime. Mitigate with naming, docs, doctor messages, and no Codex auth file access.

### Provider Tool Boundary Drift

Native tools can blur local effect ownership. Mitigate by deferring hosted tools, using custom function tools, and converting every action through existing harness policy.

### Secret Leakage

HTTP diagnostics may include sensitive content. Mitigate with explicit redaction tests and conservative truncation.

### Stateful Response Privacy

Response IDs and background mode imply server-side state. Mitigate by keeping phase 1 stateless and requiring explicit config before stateful features.

## 18. Open Questions

- Should the docs prefer runtime id `openai` or `openai_responses` in examples?
- Should phase 1 require a model on every OpenAI-backed agent profile, or should docs suggest one current model after a fresh official-docs check?
- Should `store = false` become a new config field immediately, or remain a hardcoded phase 1 request setting?
- Should structured output be mandatory for final results, or best-effort with delimiter parsing fallback?
- Should phase 2 force at most one function call per Responses turn, or allow multiple calls after the app loop explicitly supports batching?
