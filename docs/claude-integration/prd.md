# Claude CLI Runtime PRD

## Problem Statement

Developers want harness agents to use Claude through the installed `claude`
command, the same way the Codex Runtime uses the installed `codex` command.
The goal is not a generic Anthropic HTTP API integration yet; the goal is to
let an agent profile select a Claude-backed execution runtime without changing
the harness action model, configuration layering, or session history contract.

The current harness supports `codex`, `zai`, and `fake` runtime kinds. Codex is
the closest precedent: it launches a child CLI process, delegates local
authentication to that CLI, streams stdout/stderr into runtime events, and
parses the final output through the harness-owned structured contract.

## Proposed Direction

Add a first-class `claude` Execution Runtime that launches the local `claude`
CLI in non-interactive mode.

The runtime should preserve the existing harness boundary:

- The harness owns file reads, file edits, command execution, VCS actions,
  approval policy, session history, and final output parsing.
- The Claude CLI owns Claude authentication, model access, local CLI settings,
  and any Anthropic-specific session metadata.
- Agent profiles continue to select runtime and model declaratively through
  effective configuration.
- Normal tests remain offline and credential-free.

This mirrors the Codex Runtime decision while keeping a separate runtime kind
because CLI flags, auth diagnostics, streaming formats, permissions, and
failure modes are Claude-specific.

## User Stories

1. As a developer, I want an agent profile to select runtime `claude`, so that
   Explorer, Fixer, Reviewer, or a custom agent can use Claude without changing
   its role definition.
2. As a developer, I want the harness to use my installed `claude` command, so
   that setup remains local and familiar.
3. As a developer, I want the harness to avoid storing Claude secrets in
   `multiagent.toml`, so that credential ownership stays with Claude CLI or the
   user environment.
4. As a developer, I want `multiagent --doctor` to report whether the Claude
   runtime command is installed and usable, so that setup failures are visible
   before a run.
5. As a developer, I want Claude output parsed through the same action-request,
   orchestrator-decision, and agent-result contracts as other runtimes, so that
   orchestration behavior does not fork by runtime.
6. As a developer, I want Claude process stdout/stderr to appear as runtime
   progress in the TUI, so that long steps do not look frozen.
7. As a harness maintainer, I want fake process fixtures for Claude runtime
   tests, so that CI does not require Claude credentials.
8. As a harness maintainer, I want Claude-specific CLI flags isolated in the
   Claude Runtime, so that Codex and Z.ai behavior does not regress.

## Initial Implementation Decisions

- Add `RuntimeKind::Claude` and a `ClaudeRuntime` module instead of overloading
  the existing `CodexRuntime`.
- Add a built-in runtime entry:

  ```toml
  [runtimes.claude]
  type = "claude"
  command = "claude"
  args = []
  prompt_mode = "stdin"
  ```

- Do not add `api_key_env` as a required Claude Runtime field for the first CLI
  integration.
- Use a prompt envelope equivalent to the Codex Runtime envelope, adjusted only
  where Claude-specific wording is necessary.
- Use `claude -p --output-format stream-json` from the first implementation.
  The Claude Runtime must parse Claude JSONL events while the child process is
  running instead of waiting for one final text blob.
- Run the first Claude integration in strict harness-action mode: Claude may
  reason and return structured contracts, but local reads, edits, commands, and
  VCS operations must be requested as Harness Actions.
- Pass `--tools ""` by default so Claude Code built-in tools, MCP tools, Bash,
  Read, Edit, and related local tool surfaces are unavailable during harness
  runs.
- Map the Agent Profile model field to Claude's `--model` flag when the model
  is not `default`.
- Keep CLI args configurable for advanced users, but make the default path safe
  and testable.
- Record the strict harness-action boundary in
  `docs/adr/0002-claude-runtime-uses-harness-actions.md`.
- Keep existing Built-in Profile runtime assignments unchanged. Users opt in to
  Claude by selecting `runtime = "claude"` in an Agent Profile or by adding a
  Custom Agent.
- Treat Claude `stream-json` frames as provider-specific adapter input. The app
  sees only normalized Runtime Events and the final parsed Runtime Output.
- Use fixture-backed JSONL parser tests for Claude `init`, assistant message,
  result, and error frames before enabling the runtime in normal flows.
- Accumulate the final assistant/result text from Claude stream events and pass
  that text through the existing action-request, orchestrator-decision, and
  agent-result contract parsers.
- For `--doctor`, resolve the configured `claude` command and run
  `claude --version`. Do not run paid model requests, interactive login flows,
  or `claude doctor` in the Working Directory. If only installation can be
  verified, report Claude Runtime Availability as `Unknown`.
- Invoke Claude print mode with `--no-session-persistence` by default. Do not
  pass `--continue`, `--resume`, or `--session-id` unless a future explicit
  opt-in maps harness runs to Claude sessions.
- Preserve Claude `session_id` values from `stream-json` only as diagnostic
  metadata; harness-owned Session History remains the source of cross-step and
  resume context.
- Do not allow generic runtime `args` to re-enable Claude Code tools. Any future
  tool delegation must be explicit policy, not accidental CLI flag override.
- Minimize ambient Claude context by default. Do not use `--bare` in the first
  phase because local Claude help says it changes auth behavior and disables
  OAuth/keychain reads. Instead, use normal print mode with explicit constraints:
  disable Claude tools, avoid project/local setting sources where supported, do
  not pass `--mcp-config`, and do not pass plugin flags.
- Treat CLAUDE.md and other Claude project-context inheritance as opt-in future
  work if the CLI exposes a way to disable it without changing authentication.
- Synthesize this protected default invocation and write the harness prompt
  envelope to stdin:

  ```sh
  claude -p --output-format stream-json --include-partial-messages --no-session-persistence --tools "" --setting-sources user
  ```

- Add `--model <model>` only when the Agent Profile model is not `default`.
- Treat `-p`, `--output-format`, `--no-session-persistence`, `--tools`,
  `--setting-sources`, `--continue`, `--resume`, `--session-id`,
  `--mcp-config`, plugin flags, session flags, and tool flags as protected
  runtime arguments that user-provided `args` cannot override.
- Include `--include-partial-messages` from day one. Partial assistant text is
  live progress only: map it into coalesced Runtime Events, but parse the final
  harness contract only from the completed assistant/result text.
- Do not include `--max-turns 1` in the first default invocation. Harness step
  boundaries, final contract parsing, Harness Actions, and Run Limits already
  bound each runtime step, and Claude's own turn limit may truncate the stream
  before a final result frame is emitted.
- Implement Claude first as a separate `ClaudeRuntime`. Reuse small helpers only
  where the duplication is obvious, such as command resolution,
  concise-process-output formatting, prompt-envelope construction, and final
  contract parsing. Do not introduce a generalized CLI runtime abstraction until
  Codex, Claude, and any later CLI runtimes have enough stable overlap.
- Include a `[runtimes.claude]` entry in generated starter configuration so
  Claude opt-in is discoverable, while keeping all Built-in Profile runtime
  assignments unchanged.
- Keep Claude protected default CLI flags synthesized inside `ClaudeRuntime`.
  Starter configuration should show `args = []` for Claude rather than exposing
  protected flags as ordinary editable args.
- After `--init-config` creates or skips starter files, the CLI should tell the
  user to review the generated config file before running agents. The message
  should include the config path.
- The `--init-config` completion message should also recommend running
  `--doctor` after the user reviews the generated config, so runtime setup
  problems are visible before the first agent run.
- Keep Claude provider/transport errors separate from final contract parse
  errors. Invalid JSONL, unknown mandatory frame shapes, non-zero process exits,
  and Claude `result` frames with `is_error = true` are runtime/provider
  errors. Missing or malformed final harness contracts become
  `RuntimeOutput::ParseError`, matching Codex behavior. Partial-message parse
  failures may be diagnostic-only when a later complete result frame still
  yields a valid final contract.
- Claude provider errors should participate in existing Agent Profile
  `model_fallbacks` retry behavior. Map rate limits, overload/capacity
  failures, timeouts, and temporary service failures to
  `RuntimeProviderError::retryable`. Treat missing authentication, invalid model
  names, protected-arg violations, malformed Claude JSONL, and final harness
  contract parse errors as non-retryable.
- Persist safe Claude execution metadata only. Session History may record
  Claude `session_id`, resolved model, duration, turn count, total cost, and
  final result subtype as structured diagnostic metadata. Do not persist raw
  provider JSONL frames, raw `system init` payloads, credential material, or
  local Claude config paths outside explicit debug artifacts or test fixtures.
- Do not add `api_key_env` to the CLI-backed Claude Runtime. Claude credentials
  remain owned by the Claude CLI and user environment, like Codex credentials.
  A future direct Anthropic API integration should be a separate API-keyed
  Execution Runtime.
- `--print-config` should print authored Claude runtime configuration only. It
  should not expand synthesized protected flags into `args`, because those flags
  are not ordinary editable configuration.
- `--doctor` should summarize Claude protected defaults as non-editable runtime
  diagnostics: tools disabled, session persistence disabled, stream-json
  enabled, partial messages enabled, and project/local settings minimized. It
  should not print the full synthetic command line unless a future verbose
  doctor mode exists.

## Testing Decisions

- Add Claude availability tests with fake `claude` commands for missing command,
  version success, version failure, and version timeout.
- Add doctor tests proving Claude Runtime availability and protected-default
  summaries appear without credential material or full synthetic command lines.
- Add fake-process runtime tests that capture Claude arguments and stdin,
  proving protected defaults are synthesized, user args cannot override them,
  `--model` is added only when configured, and the prompt envelope is written to
  stdin.
- Add JSONL parser fixture tests for Claude `init`, partial assistant messages,
  completed assistant/result output, result error frames, malformed JSONL, and
  unknown mandatory frame shapes.
- Add runtime output tests proving action requests, orchestrator decisions,
  agent results, and final contract parse errors follow existing harness
  semantics.
- Add metadata tests proving only safe Claude execution metadata is persisted.
- Keep live Claude integration tests ignored and gated behind explicit
  environment variables.
- Include one or two ignored live Claude tests in the first implementation PR,
  gated by `MULTIAGENT_TEST_CLAUDE=1`. They should run a harmless prompt in a
  temporary directory and verify stream-json parsing plus final harness contract
  extraction. Normal CI must remain fake-command and fixture-only.
- Deliver the Claude Runtime in one opt-in implementation PR if it remains
  fixture-backed and does not move any Built-in Profile to Claude. Internally
  stage the PR as config kind and starter config, runtime module, JSONL parser
  fixtures, doctor/print-config behavior, and opt-in execution.
- Reject protected Claude runtime args during effective config validation. If
  `[runtimes.claude].args` contains protected flags such as `-p`,
  `--output-format`, `--tools`, `--continue`, `--resume`, `--session-id`,
  `--mcp-config`, plugin flags, session flags, or tool flags, config loading
  should fail with a targeted error before `--doctor`, `--print-config`, or
  runtime execution. Runtime construction should also defensively check the
  final arg list before spawning Claude.
- Create `docs/claude-integration/techspec.md` before implementation. The
  techspec should translate this PRD into Rust module changes, config schema
  updates, JSONL parser structures, doctor/print-config behavior, tests, and
  rollout order.
- Keep the initial Claude JSONL parser inside `src/runtime/claude.rs`. Split it
  into a submodule later only if the parser and fixtures make the runtime module
  hard to scan.
- Use a hybrid JSONL parser: parse each line into `serde_json::Value`, inspect
  `type` and `subtype`, then deserialize known mandatory frames into typed
  structs. Unknown optional frames can be ignored or surfaced as diagnostics;
  unknown mandatory or final frames are provider errors.
- Ignore unknown optional Claude stream frames by default and track an internal
  count for future debug diagnostics. Unknown optional frames should not create
  Chat noise or fail a run. Unknown mandatory or final frames still fail as
  provider errors.

## Open Decisions

1. Which Runtime Event stream names should Claude use for assistant text and
   diagnostics?

## Out of Scope

- Direct Anthropic Messages API integration.
- Building against Claude Agent SDK libraries instead of the installed CLI.
- Reading or mutating Claude CLI credential files.
- Exposing Claude subscription or API credentials through `--print-config`.
- Allowing Claude CLI tools to bypass Harness Actions before an explicit policy
  decision.
- Replacing the Codex or Z.ai runtimes.

## References Consulted

- Existing glossary and runtime language: `CONTEXT.md`.
- Existing Codex CLI runtime precedent: `docs/codex-api/prd.md`.
- Current Codex implementation: `src/runtime/codex.rs`.
- Runtime configuration and doctor wiring: `src/config/mod.rs`,
  `src/runtime/mod.rs`, and `src/doctor/mod.rs`.
- Anthropic Claude Code CLI reference:
  `https://code.claude.com/docs/en/cli-usage`.
- Anthropic Claude Agent SDK overview:
  `https://code.claude.com/docs/en/agent-sdk/overview`.

## Current Question

Which Runtime Event stream names should Claude use for assistant text and
diagnostics?

Recommended answer: **Use `message` for assistant content.** Map partial
assistant text to `events.delta("message", content)`, matching Z.ai's model-text
stream. Treat Claude stderr as `events.diagnostic("stderr", content)`, and use
`events.status(...)` for concise lifecycle/provider summaries. Do not expose raw
Claude stdout as `stdout`, because stdout carries provider JSONL protocol frames,
not user-facing text.
