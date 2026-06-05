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
Runtime Events and extracts the completed assistant/result text for the existing
harness contract parser.

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
- force `base_url = None`;
- force `api_key_env = None`;
- reject `api_key_env` if it is set on a Claude runtime;
- reject protected Claude args in `[runtimes.claude].args`.

Protected args include:

- `-p`, `--print`
- `--output-format`
- `--include-partial-messages`
- `--no-session-persistence`
- `--tools`, `--allowedTools`, `--allowed-tools`,
  `--disallowedTools`, `--disallowed-tools`
- `--continue`, `-c`, `--resume`, `-r`, `--session-id`
- `--mcp-config`, `--strict-mcp-config`
- `--plugin-dir`, `--plugin-url`
- `--setting-sources`
- dangerous permission bypass flags

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
5. Return `Unknown` when the command exists and version succeeds, because auth
   cannot be proven without model work or interactive login.

Do not run:

- `claude -p` probes;
- interactive login flows;
- `claude doctor` in the Working Directory.

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

Do not include `--max-turns 1` initially.

Runtime construction should defensively verify that the final arg list still
contains the protected defaults and no protected override before spawning
Claude.

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
- completed assistant/result frames for final contract text;
- result error frames.

Partial assistant text maps to coalesced Runtime Events. Final contract parsing
must use only completed assistant/result text.

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
- Claude result frame with `is_error = true`;
- protected arg violation;
- missing auth;
- invalid model.

Retryable provider errors:

- rate limits;
- overload or capacity failures;
- timeouts;
- temporary service failures.

Contract parse errors:

- missing final harness contract;
- malformed final harness contract;
- output that cannot parse as Action Request, Orchestrator Decision, or Agent
  Result.

Contract parse errors return `RuntimeOutput::ParseError` and must not trigger
model fallback.

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

Raw frames may appear only in explicit debug artifacts or fixture files.

## 12. Test Plan

Config tests:

- `type = "claude"` deserializes to `RuntimeKind::Claude`;
- default command is `claude`;
- starter config includes `[runtimes.claude]`;
- no Built-in Profile moves to Claude;
- protected args in Claude runtime config fail validation;
- `api_key_env` in Claude runtime config fails validation.

Runtime tests:

- fake `claude --version` success returns `Unknown`;
- missing command returns `Unavailable`;
- version failure returns `Unavailable`;
- version timeout returns `Unknown` or `Unavailable` with clear remediation;
- fake process captures synthesized args and stdin;
- `--model` is appended only when model is not `default`;
- user args cannot override protected defaults.

Parser fixtures:

- init frame with session metadata;
- partial assistant messages;
- completed assistant/result contract;
- action request contract;
- orchestrator decision contract;
- agent result contract;
- result error frame;
- malformed JSONL;
- unknown mandatory frame;
- non-zero process exit with stderr.

Doctor/print-config tests:

- doctor summarizes protected defaults without full synthetic command line;
- doctor output excludes credentials and local Claude config paths;
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

## Current Question

Which Runtime Event stream names should Claude use for assistant text and
diagnostics?

Recommended answer: **Use `message` for assistant content.** Map partial
assistant text to `events.delta("message", content)`, matching Z.ai's model-text
stream. Treat Claude stderr as `events.diagnostic("stderr", content)`, and use
`events.status(...)` for concise lifecycle/provider summaries. Do not expose raw
Claude stdout as `stdout`, because stdout carries provider JSONL protocol frames,
not user-facing text.
