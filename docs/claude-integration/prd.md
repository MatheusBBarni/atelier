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
- Put all harness instructions, role context, and the `RuntimeRequest` envelope
  in the stdin prompt text. Do not use Claude system-prompt flags in the first
  implementation.
- Force Claude prompt transport to stdin in v1. If the config model gains any
  future non-stdin `prompt_mode` value, effective config validation should
  reject that value for Claude rather than silently normalizing it.
- Use `claude -p --output-format stream-json` from the first implementation.
  The Claude Runtime must parse Claude JSONL events while the child process is
  running instead of waiting for one final text blob.
- Run the first Claude integration in strict harness-action mode: Claude may
  reason and return structured contracts, but local reads, edits, commands, and
  VCS operations must be requested as Harness Actions.
- Pass `--tools ""` by default so Claude Code built-in tools, MCP tools, Bash,
  Read, Edit, and related local tool surfaces are unavailable during harness
  runs.
- Fail closed if Claude emits any tool-use or local-action frame despite
  `--tools ""`. The Claude Runtime must never execute Claude tool-use frames or
  silently translate them into Harness Actions. If a frame requests built-in
  tools, MCP tools, Bash, file reads/edits, browser/worktree actions, or another
  local-action surface, stop treating the run as valid output, kill the child if
  needed, emit a capped/redacted diagnostic, and return a nonretryable
  provider/runtime error for violating the harness-action boundary.
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
- Use the successful final Claude `result` frame as the authoritative source
  for final harness contract parsing. Partial assistant frames are live progress
  only, and completed assistant message frames may be diagnostic context only.
  If the final `result` frame is missing or has no result text, treat the run as
  a provider/runtime error. If result text exists but does not contain a valid
  harness contract, return `RuntimeOutput::ParseError`.
- Treat Claude partial assistant deltas as transient live Runtime Events, not as
  canonical Session History content. Persist the parsed final harness contract,
  safe aggregate metadata, and normal redacted diagnostics through the existing
  history pipeline. Do not persist raw or reconstructed partial Claude text as a
  separate transcript artifact in v1.
- Do not retain completed Claude assistant message frames after successful runs.
  Treat them as provider diagnostics only, never as final contract sources and
  never as canonical Session History. On provider failures, the runtime may
  include a capped, redacted diagnostic summary derived from completed assistant
  frames when it helps explain the failure, but it must not persist raw completed
  message frames or create a second Claude transcript artifact in v1.
- For `--doctor`, resolve the configured `claude` command and run
  `claude --version`. If version succeeds, run `claude --help` with a short
  timeout and verify required non-negotiable flags appear: `--output-format`,
  `--include-partial-messages`, `--no-session-persistence`, `--tools`, and
  `--setting-sources`. Do not run paid model requests, interactive login flows,
  or `claude doctor` in the Working Directory. If required flags are missing,
  report Claude Runtime Availability as `Unavailable` with an upgrade
  remediation. If help cannot be read or parsed, report Availability as
  `Unknown` with remediation instead of running an authenticated probe.
- Do not repeat the `claude --help` capability check before every runtime step.
  Keep the capability probe in `--doctor` / runtime availability, share the
  required-flag constants with tests, and let execution spawn the synthesized
  command directly. If Claude rejects a required protected flag at runtime,
  classify the nonzero exit as a nonretryable provider/runtime error with an
  upgrade remediation.
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

- Run the Claude child process in the harness Working Directory, with piped
  stdin/stdout/stderr, `kill_on_drop(true)`, and cancellation-token process
  killing matching the Codex Runtime. On app cancellation or council timeout,
  kill the Claude process, stop stdout and stderr readers, and return a normal
  runtime-cancelled error.
- Inherit the harness process environment when spawning Claude. Do not add,
  inspect, print, or persist Claude-related environment variables, and do not
  add per-runtime `env` configuration in the first implementation. This keeps
  normal Claude CLI auth, proxy, certificate, and platform behavior outside
  `multiagent.toml`.
- Add `--model <model>` only when the Agent Profile model is not `default`.
- Treat `-p`, `--print`, `--output-format`, `--include-partial-messages`,
  `--no-session-persistence`, `--tools`, `--setting-sources`, `--continue`,
  `--resume`, `--session-id`, `--model`, `--fallback-model`, `--max-turns`,
  `--max-budget-usd`, `--mcp-config`, `--system-prompt`,
  `--system-prompt-file`, `--append-system-prompt`,
  `--append-system-prompt-file`, plugin flags, session flags,
  prompt-transport flags, turn/budget flags, and tool flags as protected
  runtime arguments that user-provided `args` cannot override.
- Use a conservative high-impact protected-arg denylist for v1. Reject Claude
  flags that can alter local access, permission behavior, settings/config
  sources, hooks, MCP/plugins, prompt transport, input/output protocol,
  structured-output mode, debug/log paths, model routing/fallbacks,
  turn/budget limits, background/remote/browser/worktree execution, or session
  persistence. Keep `args` available only for narrow, reviewed compatibility
  flags that do not change the harness boundary.
- Include `--include-partial-messages` from day one. Partial assistant text is
  live progress only: map it into coalesced Runtime Events, but parse the final
  harness contract only from the successful final `result` text.
- Do not include `--max-turns 1` in the first default invocation. Harness step
  boundaries, final contract parsing, Harness Actions, and Run Limits already
  bound each runtime step, and Claude's own turn limit may truncate the stream
  before a final result frame is emitted.
- Reject Claude-native `--max-turns` and `--max-budget-usd` in v1. Keep harness
  Run Limits and council timeouts authoritative, because Claude-side limits can
  stop a run before a final `result` frame or valid harness contract and turn a
  predictable harness limit into a provider-stream failure.
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
  missing final `result` frames, multiple final `result` frames, final `result`
  frames without result text, and Claude `result` frames with `is_error = true`
  are runtime/provider errors. Malformed harness contracts inside final result
  text become `RuntimeOutput::ParseError`, matching Codex behavior.
  Partial-message parse failures may be diagnostic-only when a later final
  result frame still yields a valid final contract.
- Treat unexpected Claude tool-use or local-action frames as nonretryable
  provider/runtime errors, even if a valid-looking final `result` appears later.
  These frames indicate the CLI crossed the harness-action boundary.
- Nonzero Claude process exit wins over any valid-looking final result text.
  Do not parse or act on a final harness contract from a process that exited
  unsuccessfully. Include redacted/capped stderr and exit diagnostics in the
  provider error, classify retryability from the error text when possible, and
  require zero exit plus exactly one successful final `result` frame before
  contract parsing.
- Claude stderr does not override a successful process status. If Claude exits
  zero, emits exactly one successful final `result` frame, and that result text
  contains a valid harness contract, the run succeeds even when stderr contains
  warnings. Treat stderr as redacted diagnostic Runtime Events only unless the
  final Claude `result` frame reports `is_error = true` or the final contract is
  invalid.
- Require exactly one final Claude `result` frame. Zero final result frames are
  missing provider output, one final result frame is authoritative, and multiple
  final result frames are ambiguous provider output. Do not concatenate result
  text or choose first/last.
- Drain Claude stdout and stderr until process exit before returning success.
  The runtime may extract the single final `result` text while reading, but it
  must validate the whole stream and exit status before parsing the contract as
  successful output. Frames after the first final `result` can still invalidate
  the run when they include a second final `result`, malformed mandatory output,
  a tool-use/local-action frame, or a nonzero process exit. Later optional
  diagnostic frames may be ignored or emitted as capped/redacted diagnostics.
- If a later Claude frame or exit status invalidates a run after live Runtime
  Events have already been emitted, keep those events visible as transient run
  events, emit a final error/status event explaining the invalidation, and mark
  the attempt failed. Do not try to retract UI events that were already streamed,
  and do not promote partial text or other transient events into canonical
  Session History. Persist only failed-attempt metadata and capped/redacted
  diagnostics through the normal history pipeline.
- Claude provider errors should participate in existing Agent Profile
  `model_fallbacks` retry behavior. Map rate limits, overload/capacity
  failures, timeouts, and temporary service failures to
  `RuntimeProviderError::retryable`. Treat missing authentication, invalid model
  names, protected-arg violations, malformed Claude JSONL, and final harness
  contract parse errors as non-retryable.
- Surface each retry or model fallback as a distinct runtime attempt with its
  own status boundary. If an earlier Claude attempt emitted live events before a
  retryable failure, keep those events tied to the failed attempt, emit a
  redacted failure/fallback status before the next attempt starts, and make only
  the successful fallback attempt's final parsed contract canonical. Session
  History may retain concise failed-attempt diagnostics for observability, but
  must not merge failed-attempt partial text into the successful attempt
  transcript.
- Replay the same `RuntimeRequest` for each retry or model fallback attempt,
  changing only the Agent Profile model according to the existing model chain.
  Do not include failed-attempt Claude partial text, completed-message
  diagnostics, stderr, or provider error text in the next attempt prompt.
  Failed-attempt diagnostics belong in status events and Session History
  observability, not in fallback prompt construction.
- Preserve existing Agent Profile model-chain semantics for `default` in Claude
  fallback attempts. For Claude, `default` means "do not pass `--model`" for
  that attempt, including fallback attempts. Allow `default` as a fallback when
  the primary model is explicit, reject it only when it duplicates the primary
  model or another existing validation fails, and let invalid explicit Claude
  model names surface from the Claude CLI as nonretryable provider errors.
- Treat Claude final `result` frames with `is_error = true` as
  provider/runtime errors and do not parse any result text as a harness
  contract. Classify retryability from Claude's error subtype or message when
  possible: rate limits, overload/capacity, timeouts, and temporary service
  failures are retryable; auth failures, invalid model, protected-arg/config
  problems, malformed protocol output, and contract parse failures are
  nonretryable.
- Reject Claude's native `--fallback-model` in v1 and keep Agent Profile
  `model_fallbacks` authoritative. The harness owns retries and model fallback
  selection so provider errors, Runtime Events, and model changes remain
  observable and consistent. Do not allow double fallback behavior where Claude
  silently changes models inside one runtime attempt.
- Persist safe Claude execution metadata only. Session History may record
  Claude `session_id`, resolved model, duration, turn count, total cost, and
  final result subtype as structured diagnostic metadata. Do not persist raw
  provider JSONL frames, raw `system init` payloads, credential material, or
  local Claude config paths outside test fixtures.
- Use the final Claude `result` frame as authoritative for safe final execution
  metadata. `system`/`init` metadata may be used only as early or fallback
  metadata. If `init` and final `result` metadata disagree, do not fail an
  otherwise valid run; emit a redacted diagnostic and prefer final result
  metadata because it describes the completed attempt.
- Omit missing optional Claude metadata fields from Session History rather than
  writing `null` or `unknown` placeholders. Use `system`/`init` fallback only
  for fields Claude reports there but omits from the final `result`, such as
  `session_id` or resolved model. For duration, turn count, total cost, and
  result subtype, persist only values actually reported by the final `result` so
  history can distinguish "not reported" from "reported as zero or empty."
- Do not support live raw Claude provider debug artifact capture in v1. Fixture
  files may contain representative raw Claude JSONL, but normal runtime
  execution should persist only redacted diagnostics and safe aggregate
  metadata. If raw live capture is needed later, add an explicit opt-in debug
  mode with a clear user-facing warning, private artifact permissions, and
  `redaction_status = "unredacted"` metadata.
- Treat Claude usage and cost metadata as best-effort optional diagnostics, not
  required protocol fields. Parse known numeric fields from the final `result`
  frame when present, store only safe aggregate values, and ignore missing or
  unrecognized optional usage fields. Malformed optional usage fields may emit a
  diagnostic but must not fail a run when the final result contract is valid.
  Mandatory identity/control fields such as final `type`, `subtype`,
  `is_error`, and result text still use strict parsing.
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
- Add Claude capability tests for `claude --help` success with required flags,
  missing required flags, help failure, and help timeout. Missing required flags
  should be `Unavailable`; help failure or timeout should be `Unknown` with
  remediation.
- Add doctor tests proving Claude Runtime availability and protected-default
  summaries appear without credential material or full synthetic command lines.
- Add doctor, print-config, metadata, and history tests proving environment
  values are never displayed or persisted by the Claude Runtime.
- Add config tests proving Claude accepts `prompt_mode = "stdin"` and rejects
  any future non-stdin `prompt_mode` value rather than silently normalizing it.
- Add fake-process runtime tests that capture Claude arguments and stdin,
  proving protected defaults are synthesized, user args cannot override them,
  `--model` is added only when configured, and the prompt envelope is written to
  stdin.
- Add fake-process runtime tests proving execution does not run a per-step
  `claude --help` preflight before the synthesized command.
- Add fake-process runtime tests proving Claude system-prompt flags are never
  synthesized and config validation rejects user-provided system-prompt flags.
- Add fake-process runtime tests proving Claude runs in the harness Working
  Directory and cancellation kills the child process cleanly.
- Add fake-process runtime tests proving nonzero exit wins over a valid final
  result frame and returns a provider/runtime error without parsing the harness
  contract.
- Add fake-process runtime tests proving zero exit with stderr warnings and a
  valid final result succeeds while still emitting redacted diagnostic Runtime
  Events.
- Add JSONL parser fixture tests for Claude `init`, partial assistant messages,
  final `result` output, result error frames, malformed JSONL, and unknown
  mandatory frame shapes.
- Add parser/runtime tests proving Claude tool-use or local-action frames fail
  closed, never execute local actions, never become Harness Actions, kill the
  child if needed, and classify as nonretryable provider/runtime errors.
- Add runtime/history tests proving partial assistant deltas emit normalized live
  Runtime Events but do not create raw or reconstructed Claude transcript
  artifacts in Session History.
- Add runtime/history tests proving completed assistant message frames are not
  retained on successful runs and only contribute capped, redacted diagnostic
  summaries on provider failures.
- Add parser fixture tests proving `is_error = true` frames are provider/runtime
  errors, never parsed as harness contracts, and classified as retryable or
  nonretryable from error subtype/message when possible.
- Add parser fixture tests for missing final result frames, multiple final
  result frames, and final result frames without result text.
- Add runtime/parser tests proving the adapter drains stdout/stderr until
  process exit, does not return success early after a final `result`, and fails
  if later frames or exit status invalidate the run.
- Add runtime/history tests proving already-emitted live Runtime Events remain
  visible as transient run events when a later frame invalidates the run, the
  attempt receives a final error/status event, and those transient events are
  not promoted into canonical Session History.
- Add runtime/fallback tests proving retryable Claude provider errors after live
  events surface a failed-attempt status boundary, start a distinct fallback
  attempt, make only the successful fallback final contract canonical, and do not
  merge failed-attempt partial text into the successful attempt transcript.
- Add runtime/fallback tests proving fallback attempts replay the same
  `RuntimeRequest` with only the model changed and do not inject failed-attempt
  Claude diagnostics, partial text, stderr, or provider errors into the next
  prompt.
- Add runtime/fallback tests proving `default` fallback attempts omit `--model`,
  explicit fallback attempts pass `--model <fallback>`, primary/fallback
  duplicates still fail existing config validation, and invalid explicit Claude
  model names are nonretryable provider errors reported by Claude.
- Add runtime output tests proving action requests, orchestrator decisions,
  agent results, and final contract parse errors follow existing harness
  semantics.
- Add metadata tests proving only safe Claude execution metadata is persisted.
- Add metadata tests proving matching `init`/final `result` metadata persists
  cleanly, conflicting final `result` metadata overrides `init` metadata,
  mismatches emit a redacted diagnostic without failing a valid run, and final
  `result` metadata remains authoritative.
- Add metadata tests proving missing optional final metadata is omitted, valid
  runs do not fail when optional metadata is absent, and `init` fallback applies
  only to final-result omissions for `session_id` or resolved model.
- Add diagnostic tests proving Claude stderr, provider errors, nonzero-exit
  summaries, and malformed-frame diagnostics redact bearer tokens/raw secrets
  and cap long content before Runtime Events or history persistence.
- Add tests proving normal Claude runtime execution does not create raw
  provider debug artifacts.
- Add parser fixture tests for result frames with usage/cost metadata, missing
  optional usage/cost metadata, and malformed optional usage/cost metadata
  alongside a valid final contract.
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
  `--output-format`, `--tools`, `--model`, `--fallback-model`, `--continue`,
  `--resume`, `--session-id`, `--max-turns`, `--max-budget-usd`,
  `--mcp-config`, plugin flags, session flags, turn/budget flags, or tool
  flags, config loading should fail with a targeted error before `--doctor`,
  `--print-config`, or runtime execution. Runtime construction should also
  defensively check the final arg list before spawning Claude.
- Add category-based protected-arg validation tests for local access,
  permissions, settings/config sources, hooks, MCP/plugins, prompt transport,
  input/output protocol, structured output, debug/log paths,
  background/remote/browser/worktree execution, session persistence, model
  routing/fallbacks, and turn/budget limits.
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
- Map Claude assistant text to `events.delta("message", content)`, matching the
  existing model-text stream used by Z.ai. Treat Claude stderr as
  `events.diagnostic("stderr", content)` and use `events.status(...)` for
  concise lifecycle or provider summaries. Do not expose raw Claude stdout as
  `stdout`, because stdout carries provider JSONL protocol frames.
- Redact and cap Claude diagnostics before emitting any diagnostic Runtime
  Event. Reuse or extract the existing Z.ai-style bearer-token/raw-secret
  redaction into a shared runtime helper, apply it to Claude stderr, provider
  error text, nonzero-exit summaries, and malformed-frame diagnostics, then
  rely on existing history compaction for persistence. Do not persist unredacted
  Claude stderr or provider diagnostics outside explicit fixture/debug
  artifacts.
- Treat the successful final Claude `result` frame as authoritative for final
  harness contract parsing. Partial frames are never final contract sources.
  Completed assistant messages may be accumulated for diagnostics, but the
  parser extracts the harness contract from final `result` text.
- Pass the single final `result` text through the existing shared harness
  contract parser and preserve current contract parsing semantics for v1. Do
  not add Claude-specific stricter validation for multiple contracts, trailing
  prose, or alternate extraction. If stricter contract validation becomes
  necessary, change the shared parser so Codex, Z.ai, and Claude remain
  consistent.

## Open Decisions

None at this time. Reopen this section only for materially new implementation
ambiguities, not for behavior already implied by accepted model-chain, history,
or harness-action decisions.

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
