# Plan: OpenAI Codex API Runtime

Status: Draft
Date: 2026-06-03

## Summary

The current `codex` runtime launches the Codex CLI as a short-lived child
process. This document plans an optional direct OpenAI API runtime for coding
models so the harness can run Codex-like agent steps without spawning `codex`.

Important caveat: the official public docs consulted on 2026-06-03 do not
document a stable "Codex subscription HTTP endpoint" that can reuse ChatGPT
Plus/Pro subscription auth directly. The documented direct API path is the
OpenAI API, especially the Responses API, using `OPENAI_API_KEY`. The documented
programmatic Codex path is the Codex SDK, which controls local Codex agents.

Therefore this plan adds an API-keyed `openai_responses` runtime. Codex also
documents enterprise access tokens and SDK/app-server flows for trusted Codex
automation, but those are Codex workflows, not general OpenAI API calls from
this harness. If the desired target is a private or OpenCode-specific
`openai/codex` provider endpoint, this plan should be revisited after the exact
endpoint contract is confirmed.

## Official docs basis

OpenAI docs used for this plan:

- Responses API migration guide:
  https://developers.openai.com/api/docs/guides/migrate-to-responses
- Streaming Responses guide:
  https://developers.openai.com/api/docs/guides/streaming-responses
- Background mode guide:
  https://developers.openai.com/api/docs/guides/background
- Structured Outputs guide:
  https://developers.openai.com/api/docs/guides/structured-outputs
- Function calling guide:
  https://developers.openai.com/api/docs/guides/function-calling
- Apply Patch tool guide:
  https://developers.openai.com/api/docs/guides/tools-apply-patch
- Shell and local shell tool guides:
  https://developers.openai.com/api/docs/guides/tools-shell and
  https://developers.openai.com/api/docs/guides/tools-local-shell
- Codex SDK docs:
  https://developers.openai.com/codex/sdk
- Codex authentication docs:
  https://developers.openai.com/codex/auth
- Codex models docs:
  https://developers.openai.com/codex/models
- GPT-5.2-Codex model page:
  https://developers.openai.com/api/docs/models/gpt-5.2-codex

Key implications:

- Responses is recommended for new agentic integrations.
- Responses supports multi-turn state, tools, structured outputs, streaming,
  and background polling.
- GPT-5.2-Codex is documented as optimized for agentic coding and supports the
  Responses API, streaming, function calling, and structured outputs.
- Codex SDK is a programmatic control surface for local Codex, not a replacement
  for API-keyed Responses calls in this Rust binary.
- Codex CLI and IDE integrations support ChatGPT login and API-key auth, but a
  direct Platform API runtime should use Platform API keys and must not read or
  copy local Codex login files.
- Model guidance can change. The runtime should require an explicit model or
  default only to the currently documented Codex recommendation after a fresh
  docs check.
- Hosted `shell` and `apply_patch` tools exist, but `multiagent` should keep
  local command/file mutation under the harness action policy.

## Current runtime behavior

`src/runtime/codex.rs`:

- Resolves and checks a configured `codex` command.
- Builds a JSON prompt envelope.
- Runs `codex exec --skip-git-repo-check --color never`.
- Writes the envelope to stdin.
- Waits for full stdout/stderr completion.
- Parses the final stdout as an action request, orchestrator decision, agent
  result, or parse error.
- Emits one final `RuntimeStreamDelta`.

Benefits:

- Reuses a user's existing Codex CLI setup.
- Avoids handling OpenAI API keys in this project.
- Keeps local actions owned by the harness prompt contract.

Limitations:

- Process startup per step is slow.
- stdout is not streamed into app state while running.
- It depends on CLI output and installed CLI behavior.
- The harness cannot use native Responses tool calls, structured outputs,
  background mode, or response IDs.

## Target design

Add a new runtime kind:

```toml
[runtimes.openai]
type = "openai_responses"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"

[agents.fixer]
runtime = "openai"
# Example only. Keep this configurable and refresh official model guidance
# before choosing a built-in default.
model = "gpt-5.2-codex"
effort = "high"
```

Do not replace the existing `codex` runtime immediately. Keep both:

- `codex`: CLI/subscription-backed local runtime.
- `openai_responses`: direct API runtime.

This preserves the current ADR while allowing an explicit migration path. Once
the API runtime is stable, add a new ADR that either supersedes or narrows
`docs/adr/0001-codex-runtime-uses-cli-subscription.md`.

## Configuration changes

Add to `RuntimeKind`:

```rust
OpenAiResponses
```

Add or reuse fields in `RuntimeConfig`:

- `base_url`: default `https://api.openai.com/v1`
- `api_key_env`: default `OPENAI_API_KEY`
- `args`: unused for this runtime
- `prompt_mode`: can remain `stdin` until removed or generalized

Potential future fields:

- `store = false`: explicit no-storage option for stateless mode only; do not
  combine with `previous_response_id` or background recovery.
- `background = false`: opt into background mode for long-running steps.
- `stream = true`: default once stream-mode plumbing exists.
- `response_state = "stateless" | "previous_response_id"`: controls whether
  response IDs are chained.

Validation:

- Reject raw API keys in config.
- Validate `api_key_env` with existing env-reference rules.
- `doctor` should report whether the env var is set, not the key value.

## Auth and storage choices

Keep auth choices explicit:

- `openai_responses` uses `OPENAI_API_KEY` or another configured Platform API
  key env var.
- `codex` continues to use the installed Codex CLI and whatever login method the
  CLI already owns.
- Do not read `~/.codex/auth.json`, `CODEX_ACCESS_TOKEN`, or ChatGPT session
  state from this runtime.

Keep storage choices explicit:

- Phase 1 should not use server-side state: send the full prompt envelope and do
  not use `previous_response_id`.
- If the chosen privacy default is no server-side response retention, send
  `store = false` explicitly. Do not rely on omitting `store` to mean stateless.
- If the runtime uses `previous_response_id`, background mode, crash recovery,
  or hosted response retrieval, it must allow server-side response storage and
  document that behavior.
- The config validator should reject impossible combinations such as
  `store = false` with `response_state = "previous_response_id"`.

## Request mapping

### Phase 1: compatible schema-only mode

The lowest-risk first version mirrors `ZaiRuntime` but uses
`POST /v1/responses`:

- `model`: `request.agent_profile.model`
- `instructions`: agent instructions plus the runtime contract
- `input`: serialized prompt envelope
- `text.format`: JSON Schema for either `orchestrator_decision` or
  `agent_result`
- `reasoning.effort`: map from `AgentEffort` when supported
- `stream`: false until `docs/stream-mode.md` is implemented
- `store`: send explicit false only in the no-storage stateless mode described
  above

In this phase, action requests remain text/JSON contracts. This gets the runtime
working without changing the app action loop.

Drawback: native function calling is not used yet, so malformed output handling
still matters.

### Phase 2: native function tools for harness actions

Expose harness actions as strict function tools:

- `read_file`
- `list_files`
- `search_text`
- `run_command`
- `apply_patch`
- `write_file`
- `record_note`

The runtime loop becomes:

1. Call Responses with input and function tools.
2. If the response includes `function_call` items, convert each to the existing
   `ActionRequest` shape.
3. Return `RuntimeOutput::ActionRequest` to the app.
4. After the app executes the action, send `function_call_output` items back to
   Responses.
5. Preserve model output items needed for the next call, including reasoning
   items when present.
6. Continue until the response contains a final structured decision/result.

This should replace delimiter parsing for action requests but can still use
structured outputs for final results.

### Phase 3: response state

Initial implementation can stay stateless by sending the full prompt envelope
and relevant history every call. Later:

- Store `response.id` in run/step history.
- Use `previous_response_id` to continue within a run or agent thread.
- Decide whether response IDs survive process restarts.
- Allow `store = true` for stateful/background modes.
- Keep `store = false` available for users who do not want server-side response
  storage, while noting that stateful features require storage.

## Tool policy

Do not grant OpenAI hosted tools by default.

Reasons:

- The harness already owns local file and command effects.
- Hosted shell runs elsewhere and changes the security model.
- Local shell changes the trust boundary and should be evaluated separately from
  custom harness function tools.
- The official apply-patch tool is useful, but the harness already has an
  `apply_patch` action that can enforce local path and approval policy.

Recommended first implementation:

- Use custom function tools that represent harness actions.
- Let the harness execute those actions exactly as it does today.
- Consider the official `apply_patch` tool only after comparing its response
  shape with the current `ActionRequest::ApplyPatch`.

## Runtime events and streaming

This runtime should be designed around the future streaming event contract in
`docs/stream-mode.md`.

Non-streaming first version:

- Parse the final response.
- Emit one final delta with `response.output_text` or a redacted diagnostic.

Streaming version:

- Use `stream: true`.
- Map `response.output_text.delta` to `RuntimeStreamDelta`.
- Map `response.created`, `response.completed`, and `error` to status/error
  runtime events.
- Buffer enough output to parse the final structured result.

Background mode:

- Optional for long-running steps only.
- Use `background: true`, then poll `responses.retrieve`.
- Support cancellation through `responses.cancel`.
- Persist the response ID so a TUI restart or app crash can show recoverable
  status later.

## Error handling

Classify errors into:

- Configuration: missing API key env var, invalid base URL.
- Authentication: 401/403.
- Rate limit: 429, retryable with backoff or fallback model.
- Timeout/network: retryable if idempotent.
- API validation: request schema bug, not retryable.
- Tool mismatch: model asked for an unknown/disallowed function.
- Structured output parse failure: one repair attempt, matching existing app
  behavior.

Redaction:

- Never persist Authorization headers.
- Redact API response bodies conservatively before history/debug events.
- Store large raw model output as artifacts only when needed for parse errors.

## Implementation phases

### Phase 1: API runtime skeleton

- Add `OpenAiResponses` config enum and validation.
- Add `src/runtime/openai_responses.rs`.
- Add availability check based on API key env presence.
- Add mock HTTP tests for request body and response parsing.
- Add README/runtime docs showing config.

### Phase 2: schema-only Responses call

- Implement non-streaming `responses.create`.
- Map final output to current `RuntimeOutput`.
- Support `text.format` JSON Schema for final decision/result if practical.
- Keep action-request delimiter compatibility as fallback.

### Phase 3: native function action loop

- Add function tool definitions for harness actions.
- Convert Responses `function_call` output into `ActionRequest`.
- Convert `ActionResult` into `function_call_output`.
- Preserve reasoning and tool-call items across the loop.
- Add tests for read/search/command/approval-required actions.

### Phase 4: streaming and background

- Implement SSE streaming after the app-level stream sink exists.
- Add optional background mode for long steps.
- Add cancellation support wired to TUI interrupt.

### Phase 5: migration from CLI runtime

- Add a migration doc:
  - when to use `codex`
  - when to use `openai_responses`
  - how auth differs
  - how costs differ
- Add an ADR superseding or extending the CLI subscription ADR.

## Acceptance criteria

- `cargo test` passes without network or API credentials.
- A mock Responses server verifies request shape, auth header, structured result
  parsing, and error redaction.
- `--doctor --json` reports `openai_responses` readiness without leaking keys.
- A fake or mock tool-call response executes through the existing harness action
  approval path.
- Existing `codex`, `zai`, and `fake` runtimes continue to work.
- Docs clearly state that API-key auth is not the same as ChatGPT subscription
  auth.
- No `openai_responses` code path reads local Codex login files or Codex access
  tokens.

## Open questions

- Does the project want the runtime name to be `openai`, `openai_responses`, or
  `codex_api` in user config?
- Should the runtime require every agent to specify a model, or should it ship a
  default that is refreshed whenever official Codex model guidance changes?
- Should response storage be disabled by default with `store = false`, or should
  the runtime use Responses state features by default?
- Is the user specifically targeting an OpenCode provider id such as
  `openai/<model>`, or a currently undocumented Codex subscription endpoint?
