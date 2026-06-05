# OpenAI Responses Runtime PRD

## Problem Statement

Developers using the harness can currently run agent steps through a Codex CLI execution runtime or a Z.ai HTTP execution runtime. The Codex CLI runtime is useful because it reuses a user's existing Codex setup, but each step launches a short-lived child process, waits for complete stdout, and depends on CLI output behavior. That makes agent steps slower than they need to be and prevents the harness from using direct Responses API capabilities such as structured outputs, response IDs, native function calling, streaming, and background execution.

The user needs an explicit OpenAI API-keyed execution runtime for coding models that can run Codex-like agent steps inside the existing harness loop without replacing the current Codex CLI runtime or weakening harness-owned action policy. The runtime must also make the auth difference clear: OpenAI Platform API-key auth is not ChatGPT subscription auth, and the harness must not read local Codex login files or access tokens.

## Solution

Add an optional `openai_responses` execution runtime that calls the OpenAI Responses API through an environment-variable credential reference. The runtime will initially mirror the existing HTTP runtime shape: it sends the harness prompt envelope, receives a final structured result, maps it into the existing runtime output contract, and emits a final runtime stream delta. Later phases will add native function tools for harness actions, response state, streaming, and background mode.

The existing `codex` execution runtime remains available as the CLI/subscription-backed path. The new runtime is selected only when an agent profile explicitly points to it. This preserves the current architecture while giving users a direct API runtime for OpenAI coding models.

## User Stories

1. As a developer, I want to configure an OpenAI API-keyed execution runtime, so that I can run harness agent steps without spawning the Codex CLI.
2. As a developer, I want the existing Codex CLI runtime to keep working, so that I can continue using subscription-backed Codex sessions when that is the right auth path.
3. As a developer, I want the OpenAI runtime to use an environment variable for credentials, so that secrets are not stored in harness configuration.
4. As a developer, I want doctor output to report whether the configured OpenAI credential reference is present, so that I can diagnose setup without leaking the key.
5. As a developer, I want docs to clearly distinguish Platform API-key auth from ChatGPT subscription auth, so that I do not expect the direct runtime to reuse my Codex CLI login.
6. As a developer, I want the OpenAI runtime to reject raw API keys in configuration, so that accidental secret persistence is blocked.
7. As a developer, I want to configure the OpenAI base URL, so that I can use the official endpoint or a compatible endpoint when explicitly intended.
8. As a developer, I want every agent profile to keep selecting its own runtime and model assignment, so that the orchestrator does not silently override runtime choice.
9. As a developer, I want the runtime to use the agent profile's model assignment, so that model selection stays declarative and configurable.
10. As a developer, I want the runtime to map agent effort into the OpenAI request when supported, so that high-effort agent profiles can request stronger reasoning behavior.
11. As a developer, I want the first implementation to be stateless by default, so that it can work without relying on server-side response continuation.
12. As a developer, I want response storage behavior to be explicit, so that privacy-sensitive runs do not accidentally depend on retained server-side state.
13. As a developer, I want invalid storage/state combinations to be rejected, so that configuration errors fail before a run behaves unpredictably.
14. As a developer, I want the runtime to send the existing harness prompt envelope, so that existing orchestrator and specialized agent contracts continue to work.
15. As a developer, I want the runtime to parse orchestrator decisions, so that the orchestrator can continue producing run plans and routing decisions.
16. As a developer, I want the runtime to parse agent results, so that specialized agents can complete steps using the current result envelope.
17. As a developer, I want the runtime to parse action requests during the compatibility phase, so that models can still ask the harness to read files, edit files, or run commands through existing policy.
18. As a developer, I want malformed model output to produce a parse error, so that bad responses are visible and recoverable instead of silently ignored.
19. As a developer, I want provider errors to be classified, so that retryable failures can use model fallbacks while non-retryable request bugs fail clearly.
20. As a developer, I want authorization headers and secret-bearing diagnostics redacted, so that session history and debug output do not expose credentials.
21. As a developer, I want existing `codex`, `zai`, and `fake` runtimes to continue passing tests, so that adding the OpenAI runtime does not regress current behavior.
22. As a developer, I want OpenAI runtime tests to use mock HTTP servers, so that normal test runs do not require network access or API keys.
23. As a developer, I want mock tests to verify request shape, so that the runtime sends the correct endpoint, auth header, model, instructions, input, structured-output settings, and storage settings.
24. As a developer, I want mock tests to verify response parsing, so that orchestrator decisions, agent results, action requests, and parse errors are mapped correctly.
25. As a developer, I want mock tests to verify error redaction, so that sensitive API response content is not persisted or displayed.
26. As a developer, I want doctor JSON to include the OpenAI runtime type and credential reference name, so that automation can validate readiness without inspecting secrets.
27. As a developer, I want README/runtime docs to show a working OpenAI runtime configuration, so that I can enable the runtime without reverse-engineering the config schema.
28. As a developer, I want the docs to recommend refreshing official model guidance before hardcoding a built-in default Codex model, so that stale model names do not become part of the product contract.
29. As a harness maintainer, I want the first runtime phase to avoid native hosted tools, so that local file and command effects remain owned by harness actions.
30. As a harness maintainer, I want custom function tools to map to harness actions in a later phase, so that native Responses tool calls can reuse existing action validation and approval behavior.
31. As a harness maintainer, I want unknown or disallowed function calls to be rejected, so that the model cannot bypass configured agent capabilities or tool allowlists.
32. As a harness maintainer, I want function-call outputs to be sent back to Responses after harness action execution, so that the model can continue the same reasoning loop after receiving action results.
33. As a harness maintainer, I want reasoning and tool-call items needed for continuation preserved when native function tools are used, so that multi-step Responses interactions remain coherent.
34. As a harness maintainer, I want streaming support to integrate with the future runtime event sink contract, so that active agent output can appear incrementally in the TUI.
35. As a harness maintainer, I want streaming output buffered for final parsing, so that live text does not compromise the final structured runtime output.
36. As a harness maintainer, I want background mode to be optional and explicit, so that long-running Responses jobs can be recovered without changing the normal step behavior.
37. As a harness maintainer, I want background response IDs persisted only when the runtime is configured for stateful/background behavior, so that crash recovery has a clear storage contract.
38. As a harness maintainer, I want cancellation to be wired through Responses cancellation for background jobs, so that TUI interrupts can stop long-running OpenAI work.
39. As a harness maintainer, I want a migration note comparing `codex` and `openai_responses`, so that users can choose between CLI/subscription and direct API execution deliberately.
40. As a harness maintainer, I want an ADR to extend or supersede the current Codex CLI auth decision once the API runtime is stable, so that architecture history remains accurate.
41. As an orchestrator user, I want routing decisions generated by OpenAI models to behave like routing decisions from other runtimes, so that the run plan flow stays consistent.
42. As a fixer user, I want edit and command actions proposed by OpenAI models to pass through the same approval path, so that the runtime change does not alter local mutation safety.
43. As a reviewer user, I want OpenAI-backed review agents to run verification through harness actions, so that review evidence is recorded in session history.
44. As a user running without OpenAI credentials, I want the TUI to continue opening and mark the runtime unavailable, so that missing credentials do not make the whole harness unusable.
45. As a user with multiple agent profiles, I want only profiles configured for the OpenAI runtime to use it, so that introducing the runtime does not unexpectedly change other agents.
46. As a user reading session history, I want OpenAI runtime events to use the same durable event vocabulary as other runtimes, so that history remains understandable across runtime types.

## Implementation Decisions

- Add a new execution runtime kind named `openai_responses`.
- Keep the existing `codex` execution runtime as the CLI/subscription-backed runtime.
- Treat the new runtime as an OpenAI Platform API-key runtime, not as a Codex subscription-auth runtime.
- Do not read local Codex auth files, Codex access tokens, ChatGPT session state, or Codex CLI credentials from the OpenAI runtime.
- Use a credential reference field that names an environment variable. The default credential reference should be `OPENAI_API_KEY`.
- Use `https://api.openai.com/v1` as the default base URL for the official OpenAI endpoint.
- Reject raw API key values in configuration and keep diagnostics limited to the configured environment variable name.
- Add runtime availability behavior that reports whether the configured credential environment variable is set while deferring network/API validation until a step runs.
- Add doctor rendering for the new runtime type with redacted context and no key material.
- The first implementation should use non-streaming `responses.create` behavior and emit one final runtime stream delta.
- The first implementation should send the current prompt envelope as the primary input so the existing harness contract remains usable.
- The request should include the agent profile's model assignment.
- The request should include agent instructions plus the runtime contract.
- The request should map agent effort into the provider request when the selected model and API support that field.
- The request should prefer structured output for final orchestrator decisions and agent results where practical.
- Compatibility mode should still parse delimiter-wrapped action-request contracts while native function tools are not yet implemented.
- The runtime should map final output into the existing runtime output variants: orchestrator decision, agent result, action request, or parse error.
- Parse failures should include a concise diagnostic and preserve enough raw output for troubleshooting without overexposing large model responses.
- Provider errors should be classified into configuration, authentication, rate limit, timeout/network, API validation, tool mismatch, and structured-output parse categories.
- Retryable provider errors should participate in existing model fallback behavior.
- Non-retryable request/schema bugs should fail directly instead of falling through model fallbacks.
- API response bodies and headers should be redacted before being placed in diagnostics, events, history, or artifacts.
- The initial mode should be stateless: send the needed prompt envelope and history each call without using `previous_response_id`.
- If stateless no-storage mode is selected, send explicit no-storage configuration rather than relying on omitted fields.
- If response continuation, background mode, or hosted response retrieval is enabled later, require explicit configuration that allows server-side response state.
- Reject impossible combinations, such as no-storage mode with response continuation.
- Do not grant OpenAI hosted shell, local shell, or hosted apply-patch tools by default.
- Use custom function tools in a later phase to represent harness actions rather than letting the provider directly mutate local files or run local commands.
- Native function tool definitions should cover the existing harness action vocabulary: reading files, listing files, searching text, running commands, applying patches, writing files, and recording notes.
- Function-call output from Responses should be converted into the existing action-request shape, then executed by the app through the existing capability and approval path.
- Harness action results should be converted back into provider function-call output items when native tool calling is enabled.
- Preserve required reasoning and tool-call items across Responses turns when native function calling or response state is used.
- Streaming support should wait for the app-level stream sink/event contract so runtime adapters can publish live deltas before returning the final parsed runtime output.
- Streaming Responses events should map output-text deltas to runtime stream deltas and status/error provider events to runtime status/error events.
- Background mode should be a later opt-in for long-running steps, with response retrieval, cancellation, and persisted response IDs.
- Runtime docs should include example configuration and explain when to use `codex` versus `openai_responses`.
- Once the OpenAI runtime is implemented and stable, add an ADR that extends or supersedes the existing CLI-auth runtime decision.

## Testing Decisions

- Good tests should verify external behavior at the runtime boundary, config boundary, doctor boundary, and harness action boundary. Tests should not lock onto private helper structure when the observable request, response, or policy behavior is what matters.
- Add config tests proving that `openai_responses` deserializes, merges, validates, and prints redacted configuration correctly.
- Add config tests proving that raw API keys are rejected and environment-variable credential references are accepted.
- Add doctor tests proving that the OpenAI runtime reports available/unknown/unavailable status from credential-reference state without leaking the key value.
- Add runtime mock HTTP tests proving that `responses.create` uses the expected endpoint, authorization header, model, instructions, input, structured output settings, stream flag, and storage flag.
- Add runtime mock HTTP tests proving that successful orchestrator decision responses parse into orchestrator decisions.
- Add runtime mock HTTP tests proving that successful specialized-agent responses parse into agent results.
- Add runtime mock HTTP tests proving that compatibility action-request output parses into action requests.
- Add runtime mock HTTP tests proving that malformed output produces a parse error instead of a panic or unrelated provider failure.
- Add runtime mock HTTP tests for authentication failures, rate limits, server failures, API validation failures, timeout/network failures, and conservative diagnostic redaction.
- Add model fallback tests showing retryable provider failures can fall back to the next model assignment while non-retryable schema bugs do not.
- Add native function-tool tests in the later phase proving that Responses function calls are converted into harness action requests and that unknown/disallowed tool calls are rejected.
- Add action-loop tests in the later phase proving that read/search/command/edit actions still pass through existing capability checks, tool allowlists, and action approval.
- Add streaming tests only after the stream sink contract exists. They should verify live deltas arrive before final output, final output remains parseable, and tiny chunks are coalesced before durable history writes.
- Add background-mode tests only after background mode exists. They should verify response ID persistence, retrieval, cancellation, and clear storage configuration requirements.
- Continue running the existing runtime tests for `codex`, `zai`, and `fake` so the new runtime does not regress current behavior.
- Keep normal `cargo test` fully offline and credential-free. Real OpenAI integration tests, if added, should be ignored by default and gated behind explicit environment variables.
- Existing prior art includes mock HTTP tests for the current HTTP runtime, runtime integration smoke tests that are ignored by default, config merge/validation tests, doctor report tests, and action policy tests.

## Out of Scope

- Replacing the Codex CLI runtime.
- Reusing ChatGPT Plus/Pro subscription auth through an undocumented direct HTTP endpoint.
- Reading local Codex auth files, Codex access tokens, or ChatGPT session state.
- Adding OpenAI hosted shell, local shell, or hosted apply-patch tools by default.
- Implementing native function tools in the first skeleton phase.
- Implementing streaming before the app-level stream sink/event contract exists.
- Implementing background mode in the first skeleton phase.
- Changing orchestrator routing policy or hardcoding runtime selection inside the orchestrator.
- Allowing the provider to directly perform local file edits, command execution, VCS actions, or verification outside harness actions.
- Building a remote server, cloud sync flow, or web UI for this runtime.
- Automatically choosing a long-term built-in Codex model without refreshing official model guidance.

## Further Notes

- The source plan was written against official OpenAI docs consulted on 2026-06-03. Model names, supported fields, and recommended Codex models may change, so implementation should re-check official docs before hardcoding a model default.
- The product-facing runtime name should be `openai_responses` unless the team deliberately chooses a shorter alias. The important requirement is that the name does not imply subscription-backed Codex auth.
- The initial implementation can be valuable even without native function tools because it removes Codex CLI process startup from configured agents and establishes the direct Responses API runtime boundary.
- Native function tools become more important after the skeleton works because they reduce delimiter parsing for action requests while keeping harness-owned policy enforcement.
- Response state should be treated as a privacy-sensitive feature, not as an invisible optimization.
