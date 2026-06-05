# Technical Specification: Claude CLI Runtime

Status: Draft
Version: 1.0
Date: 2026-06-05
Source PRD: `docs/claude-integration/prd.md`

## Executive Summary

Add a first-class `claude` Execution Runtime backed by the installed Claude CLI.
The integration is CLI-backed, opt-in, and strict harness-action mode: Claude
can reason and emit harness contracts, but local reads, edits, commands, and VCS
operations remain Harness Actions.

The first implementation uses `claude -p --output-format stream-json` with
partial messages enabled. The runtime parses Claude JSONL frames into normalized
Runtime Events and extracts the successful final `result` frame text for the
existing harness contract parser.

## References

- Product requirements: `docs/claude-integration/prd.md`
- Domain glossary: `CONTEXT.md`
- Strict harness-action ADR: `docs/adr/0002-claude-runtime-uses-harness-actions.md`
- Existing Codex runtime: `src/runtime/codex.rs`
- Runtime dispatch: `src/runtime/mod.rs`
- Runtime config and starter config: `src/config/mod.rs`
- Doctor output: `src/doctor/mod.rs`
- Claude CLI reference: https://docs.anthropic.com/en/docs/claude-code/cli-usage
- Claude Agent SDK overview: https://docs.anthropic.com/en/docs/claude-code/sdk

## 1. Goals

- Add runtime kind `claude`.
- Keep Claude use explicit through Agent Profile configuration.
- Keep Claude credentials owned by the Claude CLI and user environment.
- Disable Claude Code tool use by default.
- Use day-zero `stream-json` with partial message support.
- Preserve existing `RuntimeOutput` semantics for Action Requests,
  Orchestrator Decisions, Agent Results, and Parse Errors.
- Keep normal tests offline and credential-free.

## 2. Non-Goals

- Direct Anthropic Messages API support.
- Reading Claude CLI credential files.
- Persisting Anthropic API keys or Claude auth material in harness config.
- Enabling Claude Code tools, MCP tools, hooks, plugins, or project-local Claude
  settings by default.
- Moving Built-in Profiles to Claude.
- Replacing the Codex or Z.ai runtimes.

## 3. Configuration Changes

### Runtime Kind

Add `Claude` to `RuntimeKind`:

```rust
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Codex,
    Claude,
    Zai,
    Fake,
}
```

`type = "claude"` is the TOML spelling.

### Built-In Runtime

Add a built-in runtime entry to `MergedConfig::builtin`:

```toml
[runtimes.claude]
type = "claude"
command = "claude"
args = []
prompt_mode = "stdin"
```

No Built-in Profile changes runtime assignment. Users opt in by setting
`runtime = "claude"` on an Agent Profile or Custom Agent.

### Effective Config Validation

In `MergedConfig::into_effective`, add a `RuntimeKind::Claude` arm:

- default `command` to `"claude"`;
- preserve user-authored `args`;
- default `prompt_mode` to `stdin`;
- reject any future non-stdin `prompt_mode` value for Claude rather than
  silently normalizing it;
- force `base_url = None`;
- force `api_key_env = None`;
- reject `api_key_env` if it is set on a Claude runtime;
- do not add or accept per-runtime environment-variable configuration in the
  first implementation;
- reject protected Claude args in `[runtimes.claude].args`.

Protected args include:

- `-p`, `--print`
- `--output-format`
- `--include-partial-messages`
- `--no-session-persistence`
- `--tools`, `--allowedTools`, `--allowed-tools`,
  `--disallowedTools`, `--disallowed-tools`
- `--system-prompt`, `--system-prompt-file`,
  `--append-system-prompt`, `--append-system-prompt-file`
- `--model`, `--fallback-model`
- `--max-turns`, `--max-budget-usd`
- `--continue`, `-c`, `--resume`, `-r`, `--session-id`
- `--mcp-config`, `--strict-mcp-config`
- `--plugin-dir`, `--plugin-url`
- `--setting-sources`
- dangerous permission bypass flags

Use a conservative high-impact denylist for v1. Reject flags in these
categories unless a future explicit policy allows them:

- local access and process surface, such as `--add-dir`, `--worktree`, `-w`,
  `--exec`, `--bg`, `--remote`, `--remote-control`, `--rc`, `--chrome`, and
  `--no-chrome`;
- permission behavior, such as `--permission-mode`,
  `--permission-prompt-tool`, and dangerous permission bypass flags;
- settings and config sources, such as `--settings` and `--setting-sources`;
- hooks, such as `--init`, `--init-only`, `--maintenance`, and
  `--include-hook-events`;
- MCP and plugins, such as `--mcp-config`, `--strict-mcp-config`,
  `--plugin-dir`, `--plugin-url`, and `--channels`;
- prompt transport and persona changes, such as system-prompt flags,
  `--agent`, and `--agents`;
- input/output protocol and structured output, such as `--input-format`,
  `--output-format`, `--replay-user-messages`, `--prompt-suggestions`, and
  `--json-schema`;
- debug/log paths, such as `--debug-file`;
- model routing and fallback behavior, such as `--model` and
  `--fallback-model`;
- turn and budget limits, such as `--max-turns` and `--max-budget-usd`;
- session persistence and resume behavior, such as `--continue`, `--resume`,
  `--session-id`, `--fork-session`, `--from-pr`, and `--teleport`.

Validation should fail with a targeted error before `--doctor`,
`--print-config`, or runtime execution proceeds.

### Starter Config

Update `starter_config_text()` to include `[runtimes.claude]` with `args = []`.

Do not place synthesized protected flags in starter TOML.

### Print Config

`to_redacted_toml` should print authored Claude runtime configuration only.
`args` remains the user-authored list. Synthesized protected flags must not be
expanded into printable TOML.

`prompt_mode` should be printed for Claude, matching Codex behavior.

## 4. CLI Init Flow

After `--init-config` creates or skips files, CLI output should:

- list created files;
- list skipped files;
- tell the user to review the generated config before running agents;
- include the config path;
- recommend `--doctor` after review.

Example:

```text
created /home/user/.config/multiagent/multiagent.toml
review /home/user/.config/multiagent/multiagent.toml before running agents
then run multiagent --doctor to check runtime setup
```

## 5. Runtime Module

Add `src/runtime/claude.rs` and wire it into `src/runtime/mod.rs`:

- `pub mod claude;`
- `RuntimeKind::Claude` in `check_runtime_availability`
- `RuntimeKind::Claude` in `execute_runtime_step_once`

Start with a separate `ClaudeRuntime` implementation. Reuse small helpers from
Codex only when straightforward, but do not introduce a generalized CLI runtime
abstraction in this PR.

## 6. Availability

`ClaudeRuntime::check_availability` should:

1. Require configured command.
2. Resolve the command.
3. Run `<command> --version` with a short timeout.
4. Return `Unavailable` when command resolution or version execution fails.
5. If version succeeds, run `<command> --help` with a short timeout.
6. Verify the help text contains required non-negotiable flags:
   `--output-format`, `--include-partial-messages`,
   `--no-session-persistence`, `--tools`, and `--setting-sources`.
7. Return `Unavailable` with upgrade remediation when required flags are
   missing.
8. Return `Unknown` when the command exists, version succeeds, and required
   flags are present, because auth cannot be proven without model work or
   interactive login.
9. Return `Unknown` with remediation when help output cannot be read or parsed.

Do not run:

- `claude -p` probes;
- interactive login flows;
- `claude doctor` in the Working Directory.

Runtime execution should not repeat the `<command> --help` capability check
before every step. Share the required-flag constants between availability checks
and tests. If execution later receives a nonzero exit because Claude rejects a
required protected flag, classify it as a nonretryable provider/runtime error
with upgrade remediation.

Doctor may summarize protected defaults as non-editable diagnostics:

- tools disabled;
- session persistence disabled;
- stream-json enabled;
- partial messages enabled;
- project/local settings minimized.

## 7. Execution Command

The runtime synthesizes protected defaults internally and writes the prompt
envelope to stdin.

Default invocation:

```sh
claude -p --output-format stream-json --include-partial-messages --no-session-persistence --tools "" --setting-sources user
```

Append `--model <model>` only when the Agent Profile model is not `default`.

Do not include or allow `--max-turns` or `--max-budget-usd` initially. Harness
Run Limits and council timeouts remain authoritative. Claude-side limits can
stop a run before a final `result` frame or valid harness contract and should
remain future explicit policy.

Runtime construction should defensively verify that the final arg list still
contains the protected defaults and no protected override before spawning
Claude.

The child process should run in the harness Working Directory, use piped stdin,
stdout, and stderr, and set `kill_on_drop(true)`, matching the Codex Runtime
process-management baseline.

The child process inherits the harness process environment by default. Do not
call `env_clear`, do not inject Claude-specific environment variables, and do
not inspect, print, or persist inherited environment values. The first
implementation does not add per-runtime `env` config.

## 8. Prompt Protocol

Build a Claude-specific prompt text equivalent to the Codex prompt protocol:

- Use the `RuntimeRequest` JSON envelope as the only task input.
- Do not edit files, run commands, inspect the repository, or use Claude Code
  tools directly.
- Return one Harness Action Request when local data or mutation is needed.
- Return one structured output contract when the step is complete.
- Return no prose outside contract delimiters.

The protocol should use existing `prompt_envelope_json` and existing contract
delimiters.

Do not use `--system-prompt`, `--system-prompt-file`,
`--append-system-prompt`, or `--append-system-prompt-file` in the initial
implementation. Those flags are protected args because they can change the
harness-owned contract instructions behind the Agent Profile. A future
prompt-transport option must be explicit, tested, and redacted in
`--print-config`.

## 9. Stream-JSON Parser

Parse stdout as newline-delimited JSON frames while the child process is
running. Stderr remains diagnostic stream content.

Keep the initial parser implementation in `src/runtime/claude.rs`. Split parser
helpers into a submodule later only if the runtime module becomes hard to scan.

Use a hybrid parser:

1. Parse each line into `serde_json::Value`.
2. Inspect `type` and `subtype`.
3. Deserialize known mandatory frames into typed structs.
4. Ignore unknown optional frames by default and track a count for future debug
   diagnostics.
5. Treat unknown mandatory or final frames as provider errors.

The parser should recognize at least:

- `system`/`init` frames for safe metadata such as `session_id` and model;
- partial assistant message frames for live Runtime Events;
- completed assistant message frames for optional diagnostics;
- final `result` frames for final contract text;
- result error frames.

Partial assistant text maps to coalesced Runtime Events. Final contract parsing
must use only successful final `result` text. Completed assistant messages may
be retained as diagnostic context, but they are not authoritative final contract
sources.

Partial assistant deltas are transient live events. Do not persist raw or
reconstructed partial Claude text as canonical Session History or as a separate
Claude transcript artifact. Normal history persistence should contain the parsed
final harness contract, safe aggregate metadata, and existing redacted
diagnostics according to the shared history pipeline.

Completed assistant message frames are provider diagnostics only. Do not retain
them on successful runs, do not treat them as final contract sources, and do not
persist them as canonical Session History. On provider failures, a capped and
redacted diagnostic summary may include useful completed-message context, but
raw completed message frames must not be persisted or written as a separate
Claude transcript artifact in v1.

If any Claude stream frame requests tool use or local action execution, fail
closed with a nonretryable provider/runtime error. This includes requests for
Claude built-in tools, MCP tools, Bash, file reads/edits, browser/worktree
actions, or any other local-action surface. The runtime must never execute these
frames or translate them into Harness Actions. Kill the child if needed and emit
a capped/redacted diagnostic that the Claude CLI violated the harness-action
boundary.

Require exactly one final `result` frame. Zero final result frames are provider
errors, one final result frame is authoritative, and multiple final result
frames are ambiguous provider output. Do not concatenate result text or choose
first/last.

Continue draining stdout and stderr until the Claude process exits before
returning success. The adapter may remember the single final `result` text as it
streams, but it must validate the complete stream and exit status first. Frames
after the first final `result` can still invalidate the run when they include a
second final `result`, malformed mandatory output, a tool-use/local-action
frame, or a nonzero process exit. Later optional diagnostic frames may be ignored
or emitted as capped/redacted diagnostics.

When a late frame or exit status invalidates a run after live Runtime Events were
already emitted, leave those events visible as transient run events, emit a final
error/status event that names the invalidating condition, and return the failed
attempt. Do not retract already-streamed UI events, and do not promote transient
partial text into canonical Session History. Persist only failed-attempt metadata
and capped/redacted diagnostics through the shared history pipeline.

After the Claude adapter extracts the single final result text, pass it through
the existing shared harness contract parser. Do not add Claude-specific
stricter validation for multiple contracts, trailing prose, or alternate
contract extraction in v1; any stricter contract validation should be a shared
parser change.

Runtime event stream names:

- assistant text: `events.delta("message", content)`;
- Claude stderr: `events.diagnostic("stderr", content)`;
- lifecycle/provider summaries: `events.status(...)`.

Do not expose raw Claude stdout as `stdout`; stdout is the JSONL protocol stream.

Redact and cap diagnostic text before emitting Runtime Events. Extract the
existing Z.ai bearer-token/raw-secret redaction and concise diagnostic behavior
into a shared runtime helper, then apply it to:

- Claude stderr;
- provider error text;
- nonzero-exit summaries;
- malformed-frame diagnostics.

After redaction, existing history compaction handles persisted stream payloads.
Do not persist unredacted Claude stderr or provider diagnostics outside
explicit fixture/debug artifacts.

Provider-specific frame names should stay inside the Claude adapter. The app
sees normalized Runtime Events and final `RuntimeOutput` only.

Unknown optional frames should not create Chat noise or fail a run. Track an
internal count so a future debug mode can report a concise diagnostic, such as
`ignored 3 unrecognized Claude stream frames`.

## 10. Error Mapping

Provider/runtime errors:

- invalid JSONL;
- unknown mandatory frame shapes;
- non-zero process exit;
- non-zero process exit even if stdout contained a valid final `result` frame;
- missing final `result` frame;
- multiple final `result` frames;
- final `result` frame without result text;
- Claude result frame with `is_error = true`;
- Claude tool-use or local-action frame;
- protected arg violation;
- missing auth;
- invalid model.

Retryable provider errors:

- rate limits;
- overload or capacity failures;
- timeouts;
- temporary service failures.

Contract parse errors:

- malformed final harness contract;
- output that cannot parse as Action Request, Orchestrator Decision, or Agent
  Result.

Contract parse errors return `RuntimeOutput::ParseError` and must not trigger
model fallback. This applies when Claude returns final result text, but that
text cannot be parsed as a harness contract.

Nonzero process exit is evaluated before final contract parsing. If Claude
emits a valid-looking final result but exits nonzero, return a provider/runtime
error with redacted/capped stderr and exit diagnostics. Classify retryability
from the error text when possible.

Stderr does not override a successful process status. If Claude exits zero,
emits exactly one successful final `result` frame, and the result text contains
a valid harness contract, the run succeeds even when stderr contains warnings.
Stderr remains redacted diagnostic Runtime Events unless the final `result`
frame has `is_error = true` or the final contract is invalid.

Claude's native `--fallback-model` is protected in v1. Agent Profile
`model_fallbacks` remain authoritative so the harness owns retry decisions,
Runtime Events, and model-change observability. Do not allow double fallback
behavior where Claude silently changes models inside one runtime attempt.

Treat each retry or model fallback as a distinct runtime attempt with its own
status boundary. If a Claude attempt emits live events and then fails with a
retryable provider error, keep those events tied to that failed attempt, emit a
redacted failure/fallback status before the next model starts, and make only the
successful fallback attempt's final parsed contract canonical. Session History
may retain concise failed-attempt diagnostics, but must not merge failed-attempt
partial text into the successful attempt transcript.

Fallback attempts should replay the same `RuntimeRequest` with only
`agent_profile.model` changed by the existing model chain. Do not inject
failed-attempt Claude partial text, completed-message diagnostics, stderr, or
provider error text into the next prompt. Those details are observability data
for Runtime Events and Session History, not fallback prompt inputs.

Preserve existing Agent Profile model-chain semantics for `default`. For Claude,
`default` means omit `--model` for that attempt, including fallback attempts.
Allow `default` as a fallback when the primary model is explicit, reject it only
when it duplicates the primary model or another existing validation fails, and
let invalid explicit Claude model names surface from the Claude CLI as
nonretryable provider errors.

Claude final `result` frames with `is_error = true` are provider/runtime errors.
Do not parse any result text from an error frame as a harness contract. Classify
retryability from Claude's error subtype or message when possible: rate limits,
overload/capacity, timeouts, and temporary service failures are retryable; auth
failures, invalid model, protected-arg/config problems, malformed protocol
output, and contract parse failures are nonretryable.

Partial-message parse failures can be diagnostics only when a later complete
result frame still yields a valid final contract.

## 11. Metadata

Persist safe execution metadata only:

- Claude `session_id`;
- resolved model;
- duration;
- turn count;
- total cost;
- final result subtype.

Do not persist:

- raw provider JSONL frames;
- raw `system init` payloads;
- credential material;
- local Claude config paths.

Raw frames may appear only in fixture files in v1. Do not implement live raw
Claude provider debug artifact capture in the first runtime. A future live
capture mode must be explicit opt-in, warn the user, write private artifacts,
and mark artifact metadata with `redaction_status = "unredacted"`.

Usage and cost metadata are best-effort optional diagnostics. Parse known
numeric fields from the final `result` frame when present and ignore missing or
unrecognized optional usage fields. Malformed optional usage fields may emit a
diagnostic but must not fail a run when the final result contract is valid.
Mandatory control fields such as final `type`, `subtype`, `is_error`, and
result text remain strict.

The final Claude `result` frame is authoritative for final execution metadata.
`system`/`init` frames may seed early or fallback metadata, but they do not
override final result metadata. If `init` and final `result` metadata disagree,
emit one redacted diagnostic and prefer the final result values; the mismatch
must not fail a run whose final result and harness contract are otherwise valid.

Missing optional metadata fields should be omitted from Session History rather
than stored as `null`, `unknown`, or synthetic defaults. Use `system`/`init`
fallback only when Claude reports `session_id` or resolved model in `init` and
omits that field from the final `result`. Duration, turn count, total cost, and
result subtype should be persisted only when reported by the final `result`.

## 12. Test Plan

Config tests:

- `type = "claude"` deserializes to `RuntimeKind::Claude`;
- default command is `claude`;
- starter config includes `[runtimes.claude]`;
- no Built-in Profile moves to Claude;
- Claude accepts `prompt_mode = "stdin"` and rejects any future non-stdin
  `prompt_mode` value;
- protected args in Claude runtime config fail validation;
- protected-arg validation covers each high-impact category;
- `--model` and `--fallback-model` in Claude runtime args fail validation;
- `--max-turns` and `--max-budget-usd` in Claude runtime args fail validation;
- `api_key_env` in Claude runtime config fails validation.

Runtime tests:

- fake `claude --version` success returns `Unknown`;
- missing command returns `Unavailable`;
- version failure returns `Unavailable`;
- version timeout returns `Unknown` or `Unavailable` with clear remediation;
- fake `claude --help` success with all required flags returns `Unknown`;
- missing required help flags returns `Unavailable` with upgrade remediation;
- help failure or timeout returns `Unknown` with clear remediation;
- fake process captures synthesized args and stdin;
- fake process execution does not run a per-step `claude --help` preflight;
- fake process runs in the harness Working Directory and uses `kill_on_drop`;
- cancellation kills the Claude child process and returns a runtime-cancelled
  error;
- fake process execution does not inject Claude-specific environment variables;
- fake process execution does not synthesize Claude system-prompt flags;
- Claude tool-use or local-action frames fail closed, never execute local
  actions, never become Harness Actions, kill the child if needed, and classify
  as nonretryable provider/runtime errors;
- partial assistant deltas emit normalized live Runtime Events but do not create
  raw or reconstructed Claude transcript artifacts in Session History;
- completed assistant message frames are not retained on successful runs and
  only contribute capped, redacted diagnostic summaries on provider failures;
- Claude stderr and provider diagnostics redact bearer tokens/raw secrets before
  Runtime Events;
- nonzero exit wins over a valid final result frame and returns a
  provider/runtime error without parsing the harness contract;
- zero exit with stderr warnings and a valid final result succeeds while
  emitting redacted diagnostic Runtime Events;
- adapter drains stdout/stderr until process exit and does not return success
  early after a final `result`;
- late invalidation after live Runtime Events emits a final error/status event
  and does not promote transient events into canonical Session History;
- retryable Claude provider errors after live events surface a failed-attempt
  status boundary, start a distinct fallback attempt, make only the successful
  fallback final contract canonical, and do not merge failed-attempt partial text
  into the successful attempt transcript;
- fallback attempts replay the same `RuntimeRequest` with only the model changed
  and do not inject failed-attempt Claude diagnostics, partial text, stderr, or
  provider errors into the next prompt;
- `default` fallback attempts omit `--model`, explicit fallback attempts pass
  `--model <fallback>`, primary/fallback duplicates fail existing config
  validation, and invalid explicit Claude model names are nonretryable provider
  errors reported by Claude;
- long Claude diagnostics are capped before Runtime Events;
- normal Claude runtime execution does not create raw provider debug artifacts;
- `--model` is appended only when model is not `default`;
- user args cannot override protected defaults.

Parser fixtures:

- init frame with session metadata;
- init and final result frames with matching metadata;
- final result metadata overriding init metadata;
- init/final result metadata mismatch emitting a redacted diagnostic while the
  valid run succeeds;
- final result missing optional metadata, with omitted history fields;
- final result missing `session_id` or resolved model while `init` fallback is
  available;
- result frame with usage and cost metadata;
- result frame with missing optional usage metadata;
- result frame with malformed optional usage metadata and valid final contract;
- result error frames classified as retryable and nonretryable;
- partial assistant messages;
- final `result` contract;
- final `result` followed by optional diagnostic frames;
- final `result` followed by a second final `result`;
- final `result` followed by malformed mandatory output;
- final `result` followed by a tool-use/local-action frame;
- missing final result frame;
- multiple final result frames;
- final result frame without result text;
- final result text using current shared parser forms: delimited contract, raw
  JSON object, and JSON code fence;
- action request contract;
- orchestrator decision contract;
- agent result contract;
- result error frame;
- tool-use/local-action frame;
- malformed JSONL;
- unknown mandatory frame;
- non-zero process exit with stderr.

Doctor/print-config tests:

- doctor summarizes protected defaults without full synthetic command line;
- doctor output excludes credentials and local Claude config paths;
- doctor output excludes inherited environment values;
- `--print-config` prints authored args only;
- `--init-config` tells the user to review the config path and run `--doctor`.

Live integration tests:

- ignored by default;
- gated behind `MULTIAGENT_TEST_CLAUDE=1`;
- run in a temporary directory;
- use a harmless prompt;
- assert stream-json parsing and final contract extraction.

## 13. Rollout Order

1. Add config kind, validation, starter config, and print-config behavior.
2. Add `ClaudeRuntime` availability checks and doctor summaries.
3. Add protected argument synthesis and fake-process execution tests.
4. Add Claude stream-json parser with fixtures.
5. Wire final contract parsing and retryable provider errors.
6. Add ignored live integration tests.
7. Run full unit test suite and targeted ignored live tests manually when
   credentials are available.
