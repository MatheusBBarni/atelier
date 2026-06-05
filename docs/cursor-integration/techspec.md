# Technical Specification: Cursor Agent CLI Runtime

Status: Draft
Date: 2026-06-05
Source PRD: `docs/cursor-integration/prd.md`

## Executive Summary

This specification defines a first-class `cursor` Execution Runtime for the
harness. The runtime launches the installed Cursor Agent CLI as a child process,
uses Cursor-owned local authentication, parses Cursor stream JSON events into
provider-neutral Runtime Events, and preserves the existing harness boundary for
Harness Actions, Action Approval, and Session History.

Phase one is a CLI-backed integration only. It does not add a direct Cursor API
runtime, does not read Cursor credential files, does not generate Cursor
permission files, and does not allow Cursor-native tools to mutate the
workspace.

## Background / Context

The harness currently routes work through `RuntimeKind::Codex`,
`RuntimeKind::Zai`, and `RuntimeKind::Fake` in `src/config/mod.rs` and
`src/runtime/mod.rs`. The Codex runtime in `src/runtime/codex.rs` is the nearest
implementation precedent: it resolves a configured command, probes
availability, writes the harness prompt envelope to stdin, streams process
stdout/stderr into Runtime Events, waits for the child process, and parses the
final text into `RuntimeOutput`.

The Cursor PRD establishes the product boundary:

- runtime kind and built-in runtime id: `cursor`;
- primary setup path: existing Cursor Agent CLI login;
- optional automation fallback: `CURSOR_API_KEY` in the process environment;
- strict harness-action mode;
- stream JSON parsing from phase one;
- harness-owned Session History and Context Resume;
- fake fixtures for normal tests.

Current Cursor CLI docs consulted through Context7 describe the installed
command as `agent`, with examples such as `agent --version`, `agent status`, and
`agent -p "..." --model gpt-5.2`. The runtime therefore defaults to `agent`
while keeping `[runtimes.cursor].command` configurable for older or local
installations that expose a different executable name.

## Goals

- Add `RuntimeKind::Cursor` without changing existing runtime behavior.
- Let Agent Profiles and Custom Agents explicitly select `runtime = "cursor"`.
- Keep Built-in Profiles on their current runtimes unless user configuration
  changes them.
- Use Cursor-owned saved login as the primary authentication path.
- Parse Cursor stream JSON into normalized Runtime Events while keeping the final
  `RuntimeOutput` on the runtime return path.
- Preserve Capability Enforcement, Action Approval, Harness Actions, and Session
  History ownership in the harness.
- Keep normal tests offline, credential-free, and deterministic.

## Non-Goals

- Direct Cursor HTTP/API integration.
- Reading or mutating Cursor credential files.
- Storing Cursor credential values or Cursor API keys in `multiagent.toml`.
- Generating or mutating `.cursor/cli.json`.
- Allowing Cursor-native file writes, shell commands, or VCS operations to
  bypass Harness Actions.
- Resuming Cursor CLI sessions as harness Context Resume.
- Replacing Codex, Z.ai, or Fake runtimes.

## Requirements

### Functional Requirements

1. Config accepts a new runtime kind `cursor`.
2. The effective config includes a built-in `cursor` runtime entry.
3. The runtime command is configurable through `[runtimes.cursor].command`.
4. The runtime launches Cursor Agent CLI in non-interactive print mode.
5. The harness prompt envelope is written over stdin by default.
6. Non-default Agent Profile models map to Cursor's `--model` flag.
7. Cursor stream JSON is parsed while the process is running.
8. Cursor final result text is parsed through existing
   action-request, orchestrator-decision, agent-result, and parse-error
   contracts.
9. Cursor mutating tool-call events fail the runtime step as a policy violation.
10. Cursor `readToolCall` events are diagnostics only in phase one.
11. Every other Cursor tool-call family, including unknown future tools, fails
    closed as a non-retryable policy error.
12. `--doctor` reports Cursor command availability and authentication status.
13. Normal tests use fake commands and fixtures.

### Non-Functional Requirements

- Runtime startup and doctor probes use short timeouts.
- Process output parsing must tolerate unknown Cursor stream JSON fields.
- Diagnostic output must not expose Cursor credential values.
- Runtime event streaming must use the existing bounded `RuntimeEventSink`.
- Cancellation must kill the child process and return a clear runtime error.

## Proposed Design

Add a new `src/runtime/cursor.rs` module implementing the existing `Runtime`
trait:

```rust
pub struct CursorRuntime {
    config: RuntimeConfig,
}

#[async_trait]
impl Runtime for CursorRuntime {
    async fn check_availability(&self) -> RuntimeAvailability;

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        events: RuntimeEventSink,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput>;
}
```

The runtime should remain provider-specific rather than introducing a generic
CLI runtime abstraction in phase one. Small helpers can be shared later if Codex,
Claude, and Cursor converge around stable child-process behavior.

Cursor should keep its own runtime file and its own final contract parsing
helper in `src/runtime/cursor.rs`, even if that helper initially mirrors Codex
and Z.ai behavior. The duplication is intentional in phase one because Cursor's
stream/result contract may diverge from the other runtimes as the CLI
integration matures.

## Architecture / Components

### Configuration

Update `RuntimeKind`:

```rust
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Codex,
    Cursor,
    Zai,
    Fake,
}
```

Add a default runtime during config merge:

```toml
[runtimes.cursor]
type = "cursor"
command = "agent"
args = []
prompt_mode = "stdin"
```

For `RuntimeKind::Cursor`, effective config should set:

- `command`: configured value or `agent`;
- `args`: validated user-provided args, default empty;
- `prompt_mode`: stdin;
- `base_url`: none;
- `api_key_env`: none.

`to_redacted_toml` should include `prompt_mode` for Cursor, parallel with Codex,
because stdin prompt transport is part of the runtime contract.

Protected Cursor args must be rejected during effective config construction, not
deferred until runtime execution. Validation errors should identify the runtime
id and protected argument, for example:

```text
cursor runtime cursor uses protected arg --force
```

This prevents `--print-config`, `--doctor`, and the TUI from presenting an
unsafe Cursor runtime as valid.

### Runtime Dispatch

Update `check_runtime_availability` and `execute_runtime_step_once` in
`src/runtime/mod.rs` to instantiate `CursorRuntime` for `RuntimeKind::Cursor`.

### Doctor

Update `src/doctor/mod.rs` runtime title mapping:

```rust
RuntimeKind::Cursor => "Cursor Runtime"
```

Doctor context should include runtime id, runtime type, command, and any safe
summary fields already present. It must not include credential values.

### Prompt Envelope

Cursor should use a prompt protocol equivalent to Codex with Cursor-specific
wording:

- the model is a structured runtime adapter inside the harness;
- the JSON envelope is the only task input;
- the model must not use Cursor tools directly for reads, writes, shell
  commands, or VCS operations;
- local data and mutations must be requested as Harness Actions;
- final output must be one delimited harness JSON contract.

### Process Execution

Default invocation shape:

```text
agent --print --output-format stream-json
```

Append `--model <model>` when the Agent Profile model is not `default`.

Spawn rules:

- `current_dir` is `RuntimeRequest.working_directory`;
- `stdin` is piped and receives the full harness prompt envelope;
- if live Cursor smoke testing proves stdin is unsupported, the runtime may pass
  the same prompt envelope as a `-p` argument using `Command::arg`; it must not
  build a shell command string;
- `stdout` is piped and parsed as newline-delimited JSON;
- `stderr` is piped and emitted as diagnostics;
- `kill_on_drop(true)` is used;
- cancellation kills and waits for the child process.

## Data Model and Contracts

### Cursor Stream JSON Input

The adapter should parse each stdout line with a small typed top-level event
model. Known fields such as `type`, `subtype`, `message`, `is_error`, `result`,
`session_id`, and `request_id` should be represented in structs/enums with
`#[serde(default)]` where appropriate and unknown fields ignored. Nested
`tool_call` payloads should remain `serde_json::Value` so policy classification
can inspect tool-family keys without overfitting to every Cursor tool schema.

Known event families:

- `system` / `init`: diagnostic metadata such as session id, model, cwd, auth
  source, and permission mode;
- `assistant`: assistant text deltas or message fragments;
- `tool_call`: Cursor-native tool started/completed events;
- `result`: terminal success event containing final `result` text and
  `session_id`.

Unknown event types are diagnostics or ignored unless they are required to
extract final output.

### Failure Mapping

Cursor runtime failures map into existing harness outcomes by source:

- spawn failure, timeout, nonzero process exit, invalid required NDJSON, missing
  terminal `result`, and Cursor terminal `result` events with `is_error = true`
  are runtime/provider errors;
- `readToolCall` events are diagnostic-only in phase one;
- every other `tool_call` family, including `writeToolCall`, shell/terminal
  calls, and unknown future tool keys, is a non-retryable runtime policy error;
- a terminal `result.result` string that exists but does not contain a valid
  harness JSON contract becomes `RuntimeOutput::ParseError`, matching Codex
  behavior.
- accumulated assistant text is progress-only and must not be parsed as a final
  output fallback when the terminal successful `result` event is missing.

### Runtime Events Output

Map provider input into existing events:

- assistant progress -> `RuntimeEvent::Delta { stream: "stdout", ... }`;
- tool-call progress -> `RuntimeEvent::ToolCallProgress` or
  `RuntimeEvent::Diagnostic`, depending on policy;
- stderr chunks -> `RuntimeEvent::Diagnostic { stream: "stderr", ... }`;
- policy violations -> runtime error with diagnostic context.

The adapter must not emit final `RuntimeOutput` through `RuntimeEvent`.

### Final Runtime Output

The terminal successful Cursor `result.result` string is the only source for
final contract parsing. Accumulated assistant text from earlier stream events is
live progress only. It must not rescue a missing terminal `result`. The terminal
`result.result` text must be passed to the same parser behavior used by Codex:

- valid action contract -> `RuntimeOutput::ActionRequest`;
- valid orchestrator decision -> `RuntimeOutput::OrchestratorDecision`;
- valid agent result -> `RuntimeOutput::AgentResult`;
- malformed or missing contract -> `RuntimeOutput::ParseError`.

### Diagnostics Metadata

Cursor `session_id` and request ids can be preserved as diagnostic text or
internal parser metadata, but they do not become harness Session History ids and
are not used for Context Resume.

## APIs / Events

No external API is added.

Internal API changes:

- `RuntimeKind::Cursor`;
- `src/runtime/cursor.rs`;
- dispatch arms in `src/runtime/mod.rs`;
- doctor title mapping;
- starter config generation;
- redacted config rendering for Cursor prompt mode.

History events continue to use existing runtime stream and runtime result
records. No migration is required.

## Security and Privacy

- Reject user-provided runtime args that bypass policy or expose secrets:
  `--force`, `-f`, `--api-key`, `-a`, `--api-key=<value>`, `resume`,
  `--resume`, session resume identifiers, `--print`, `--output-format`, and
  `--model`. Reject them during config loading.
- Do not read Cursor credential files.
- Do not write Cursor permission files.
- Do not pass API keys through argv.
- Allow `CURSOR_API_KEY` only as an environment-owned optional fallback.
- Treat every Cursor tool event except `readToolCall` as a policy violation.
- Do not convert Cursor read tool output into trusted Session History file
  evidence.

## Performance and Reliability

- Availability probes use short timeouts, matching Codex availability behavior.
- Runtime execution uses the existing step timeout and cancellation path.
- Stream parsing should process lines incrementally rather than buffering the
  full stdout before emitting progress.
- Unknown Cursor stream fields are ignored to preserve compatibility with future
  Cursor CLI additions.
- Non-zero process exits are runtime/provider errors, not parse errors.
- Missing terminal result is a runtime/provider error.
- Malformed final harness contracts are parse errors only after Cursor has
  produced a terminal successful `result.result`.
- Model fallback should only retry Cursor errors explicitly wrapped as
  `RuntimeProviderError::retryable`. Cursor policy violations, protected-arg or
  config errors, `RuntimeOutput::ParseError`, nonzero exits not classified as
  retryable, and `result.is_error = true` frames should not retry by default.

## Observability

Doctor should show:

- runtime id;
- runtime type `cursor`;
- command name/path;
- version output;
- status output summary;
- remediation when missing or unauthenticated.

Doctor should continue checking every configured runtime, not only runtimes
referenced by enabled Agent Profiles. Cursor availability warnings remain
non-fatal when Cursor is configured but unused. The remediation should make clear
that Cursor setup is required only before assigning an Agent Profile to
`runtime = "cursor"`.

Runtime streaming should make long Cursor steps visible through existing TUI live
step rendering. Mutating tool-call policy violations should have clear diagnostic
messages that name the Cursor tool family and explain that Harness Actions are
required.

## Migration and Rollout

1. Add config/runtime enum support and starter config entry.
2. Add Cursor availability checks and doctor rendering.
3. Add Cursor process execution with protected arg validation.
4. Add Cursor stream JSON parser and fixtures.
5. Add policy checks for Cursor tool-call events.
6. Add prompt-envelope tests and final contract parsing tests.
7. Document setup in README/runtime docs.

Backwards compatibility:

- Existing configs without `[runtimes.cursor]` continue working.
- Existing Built-in Profiles keep their current runtimes.
- Users opt in by setting `runtime = "cursor"` on an Agent Profile or Custom
  Agent.
- `--init-config` should include an active `[runtimes.cursor]` entry with
  `command = "agent"`, `args = []`, and `prompt_mode = "stdin"` so Cursor
  opt-in is discoverable without changing any agent assignment.
- Cursor Runtime Availability warnings from `--doctor` are non-fatal when no
  enabled Agent Profile selects `runtime = "cursor"`.
- Doctor keeps the existing behavior of checking every configured runtime.

Feature flags are not required unless implementation lands incrementally behind
an internal disabled-by-default runtime entry.

## Testing Strategy

Unit tests:

- config parses and prints `RuntimeKind::Cursor`;
- default Cursor runtime is present with expected command and stdin prompt mode;
- starter config includes an active `[runtimes.cursor]` entry;
- protected args are rejected during config loading;
- model flag is synthesized from Agent Profile model assignment;
- explicit `--model` in runtime args is rejected;
- missing command returns `Unavailable`;
- successful version and status returns `Available`;
- failed status without `CURSOR_API_KEY` returns `Unavailable`;
- failed status with `CURSOR_API_KEY` returns `Unknown`;
- status timeout returns `Unknown`;
- stream JSON assistant/result fixtures parse final contract;
- unknown fields are ignored;
- typed top-level event parsing preserves known fields while ignoring unknown
  fields;
- nested tool-call payloads remain generic enough to classify tool-family keys;
- invalid required NDJSON returns a runtime/provider error;
- Cursor result frames with `is_error = true` return runtime/provider errors;
- missing terminal result returns a runtime/provider error;
- accumulated assistant text does not rescue a missing terminal result;
- `readToolCall` fixtures emit diagnostics only;
- `writeToolCall`, shell/terminal, and unknown tool-call fixtures fail with
  policy violation;
- malformed final contract becomes `RuntimeOutput::ParseError`;
- non-zero process exit becomes runtime error;
- model fallback retries explicitly retryable Cursor provider/runtime errors;
- model fallback does not retry policy violations, parse errors, or default
  `result.is_error = true` frames;
- cancellation kills fake child process.

Integration tests:

- fake Cursor Agent CLI command captures args and stdin prompt envelope;
- fake command emits recorded NDJSON and validates runtime output;
- doctor JSON contains runtime type and command without credential values.
- doctor reports Cursor availability for configured Cursor runtimes even before
  an enabled Agent Profile selects Cursor.

Live Cursor tests:

- ignored by default;
- gated behind `MULTIAGENT_CURSOR_LIVE=1`;
- may use `MULTIAGENT_CURSOR_COMMAND` to override the command path;
- require installed Cursor CLI and existing saved login by default;
- may run with `CURSOR_API_KEY` present, but the live gate must not require it;
- must not be part of normal CI.

## Alternatives Considered

- **Direct Cursor API runtime:** rejected for phase one because the product goal
  is to reuse the installed agent CLI and Cursor-owned login.
- **Generic CLI runtime abstraction:** deferred until Codex, Claude, and Cursor
  have enough proven overlap.
- **Shared final contract parser helper:** deferred. Cursor should keep a local
  parser helper in `src/runtime/cursor.rs` for now, even if it duplicates Codex
  and Z.ai, because Cursor may need runtime-specific parsing behavior later.
- **Cursor-native tools through permissions:** rejected for phase one because it
  competes with Harness Actions and Action Approval.
- **Cursor session resume:** rejected for phase one because harness-owned Session
  History remains the source of Context Resume.
- **Prompt as argv:** rejected as the default because the harness envelope can be
  large and sensitive. Allowed only as a narrow fallback if live Cursor testing
  proves stdin is unsupported.

## Risks and Mitigations

- **Cursor command name drift:** default to the current documented `agent`
  executable and keep the command configurable.
- **Cursor stream schema drift:** parse conservatively, ignore unknown fields,
  and test with recorded fixtures.
- **Tool-call leakage:** fail every tool-call family except `readToolCall` and
  keep `readToolCall` data diagnostic-only.
- **Auth ambiguity:** use status commands for saved login, return Unknown when
  env auth may still allow execution, and avoid live doctor prompts.
- **Ambient Cursor context conflicts:** harness prompt envelope and structured
  contract are higher priority; add opt-out later only if conflicts appear.

## Open Questions

None.

## Acceptance Criteria

- `runtime = "cursor"` can be selected by an Agent Profile.
- `--print-config` shows a safe Cursor runtime entry without credential values.
- `--doctor` reports Cursor command and auth status with actionable remediation.
- Cursor runtime steps parse stream JSON progress and final result contracts.
- Mutating Cursor tool events fail with policy diagnostics.
- Context Resume never passes Cursor resume arguments.
- Normal test suite passes without Cursor installed or authenticated.
