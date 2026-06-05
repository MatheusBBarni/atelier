# Technical Specification: Codex Subscription Runtime

Status: Draft
Version: 1.0
Date: 2026-06-04
Source PRD: `docs/codex-api/prd.md`

## Executive Summary

This specification replaces the direct OpenAI Responses runtime plan with a
subscription-backed Codex local integration plan. The harness should use the
installed Codex CLI and its supported auth/session surfaces. It should not
create a direct Platform API runtime when the user asks to use a Codex
subscription.

Phase 1 improves the existing `codex` runtime: availability checks include
login status, doctor output explains the active Codex setup, and execution
continues through `codex exec`. Later phases add machine-readable `codex exec
--json` event parsing and a `codex app-server` JSON-RPC driver.

## References

- Product requirements: `docs/codex-api/prd.md`
- Source planning doc: `docs/codex-api.md`
- Domain glossary: `CONTEXT.md`
- Current Codex runtime ADR: `docs/adr/0001-codex-runtime-uses-cli-subscription.md`
- Codex authentication: https://developers.openai.com/codex/auth
- Codex non-interactive mode: https://developers.openai.com/codex/noninteractive
- Codex app-server: https://developers.openai.com/codex/app-server
- Codex access tokens: https://developers.openai.com/codex/enterprise/access-tokens

## 1. Background

`multiagent` currently supports:

- `codex`: launches the Codex CLI as a child process.
- `zai`: posts to Z.ai with API-key auth.
- `fake`: deterministic local runtime for tests and simulation.

The `codex` runtime is already the correct subscription-auth boundary because
Codex owns ChatGPT login, API-key login, token refresh, cached credentials, and
workspace policy. The gap is not "add a direct OpenAI API provider"; the gap is
"make the Codex local integration first-class enough for this harness."

## 2. Goals

- Keep runtime kind `codex` as the subscription-backed Codex path.
- Verify both Codex CLI installation and Codex login status.
- Keep the harness independent from Codex credential storage internals.
- Preserve harness-owned action policy for file edits and command execution.
- Make future streaming and structured event work use documented Codex local surfaces.
- Keep normal tests offline and credential-free.

## 3. Non-Goals

- Adding `openai_responses` for this feature.
- Calling private or undocumented subscription HTTP endpoints.
- Reading `~/.codex/auth.json` from harness code.
- Persisting Codex access tokens in harness config.
- Replacing the existing runtime/action contract in phase 1.

## 4. Current Runtime Contract

The runtime boundary remains:

```rust
#[async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;
    async fn stream_step(&self, request: RuntimeRequest) -> Result<RuntimeStepResult>;
}
```

`RuntimeRequest` carries the prompt envelope and policy context. The `codex`
runtime writes the prompt protocol to `codex exec` stdin, waits for stdout, and
parses the final stdout into one of the existing `RuntimeOutput` variants.

## 5. Phase 1 Target Architecture: Harden `codex exec`

### Availability

`CodexRuntime::check_availability` should:

1. Resolve the configured command.
2. Run `<command> --version` with a short timeout.
3. Run `<command> login status` with a short timeout.
4. Return `Available` only when the command is present and login status succeeds.
5. Return `Unknown` when the command exists but `CODEX_API_KEY` or
   `CODEX_ACCESS_TOKEN` is present and login status reports no saved login.
   These variables are consumed by `codex exec`, so the login-status probe must
   not block execution before `exec` can inherit them.
6. Return `Unavailable` when the command is missing or login status reports a known non-success state that needs user action and no exec-scoped auth environment is present.
7. Return `Unknown` when the command exists but login status cannot be determined, such as a timeout or probe execution failure.

The status message should include the version and a concise login-status
summary. Remediation should point to `codex login`, and can mention
`codex login --device-auth` for remote/headless environments.

### Execution

Continue using the current compatibility command:

```text
codex exec --skip-git-repo-check --color never
```

Model assignment remains appended through `--model <model>` unless the user
already supplied a model flag or the agent model is `default`.

The prompt protocol continues to instruct Codex not to directly edit files,
run commands, or inspect the repository outside the harness contract. If the
agent needs local data or mutation, it must return one Harness Action request.

### Login Workflow

Phase 1 does not need to implement interactive login inside the TUI. The
doctor and runtime availability messages should provide exact external commands:

- `codex login`
- `codex login --device-auth`
- `printf '%s' "$CODEX_ACCESS_TOKEN" | codex login --with-access-token`

For single non-interactive runs, `CODEX_API_KEY` is supported only by
`codex exec`; `codex login status` does not prove whether that env-only path
will work. Availability should surface the saved-login failure as `Unknown` and
let the runtime attempt `codex exec` when the env var is present.

A later TUI `/login` command can suspend raw-mode UI, run a selected Codex
login command in the user's terminal, and refresh runtime availability after
the process exits.

## 6. Phase 2: `codex exec --json`

`codex exec --json` emits JSONL events on stdout. Add this only after the
runtime can distinguish progress events from the final contract cleanly.

Implementation outline:

1. Add an opt-in Codex runtime mode or internal capability probe.
2. Spawn `codex exec --json` with the current prompt protocol.
3. Parse each stdout JSONL frame.
4. Map agent-message deltas and item lifecycle events into runtime stream deltas.
5. Extract the final agent message for existing contract parsing.
6. Preserve stderr as diagnostic-only output.

This phase improves progress visibility without changing runtime selection,
auth, or action policy.

## 7. Phase 3: `codex app-server`

Use `codex app-server` when the harness needs a deep Codex integration:
authentication, Codex-owned thread history, approvals, streamed events, and
turn lifecycle control.

Protocol outline:

1. Spawn `codex app-server` with stdio transport by default.
2. Send `initialize`, then `initialized`.
3. Start or resume a thread with the configured model and working directory.
4. Send `turn/start` with the harness prompt protocol.
5. Read notifications until `turn/completed`.
6. Map item events into harness runtime stream events.
7. Extract the final agent message and parse the existing harness contract.

Security constraints:

- Default to stdio or local Unix socket transports.
- Do not use unauthenticated non-loopback WebSocket listeners.
- Do not let app-server mutate local files independently until the harness has
  an explicit policy mapping for Codex approvals and Harness Actions.

## 8. Configuration Design

No new runtime kind is required for phase 1. Existing config remains:

```toml
[runtimes.codex]
type = "codex"
command = "codex"
args = ["exec", "--skip-git-repo-check", "--color", "never"]

[agents.fixer]
runtime = "codex"
model = "default"
```

Future app-server support can be modeled as either:

- additional fields on the `codex` runtime, such as `mode = "exec" | "app_server"`; or
- a separate runtime kind only if the lifecycle and policy behavior diverges enough to justify it.

Do not add credential fields for Codex subscription auth. Codex already owns
credentials through its CLI configuration, credential store, and supported
environment variables.

Do not persist `CODEX_API_KEY` or `CODEX_ACCESS_TOKEN` in harness config.
Automation can provide those variables in the process environment for the
specific `codex exec` invocation.

## 9. Doctor Output

Doctor should include:

- runtime id;
- runtime type `codex`;
- command path or configured command;
- Codex version output;
- login status summary;
- remediation when login is missing or unknown.

Doctor must not include credential contents, auth file paths beyond generic
documentation references, or access-token values.

## 10. Test Plan

Unit tests:

- missing command returns `Unavailable`;
- command with successful `--version` and `login status` returns `Available`;
- command with successful `--version` and failed `login status` returns `Unavailable` with login remediation;
- command with failed `login status` and exec-scoped env auth returns `Unknown`
  so execution can attempt `codex exec`;
- `login status` timeout returns `Unknown`;
- older fake command without `login status` returns `Unknown` rather than panicking;
- doctor JSON includes command and runtime type without credential material.

Existing runtime tests should continue to cover:

- prompt protocol content;
- default and explicit model args;
- stdout-only contract parsing;
- stderr progress ignored for parsing;
- malformed output becoming `ParseError`;
- nonzero child process exit becoming a runtime error.

Future tests:

- JSONL fixtures for `codex exec --json`;
- fake app-server JSON-RPC lifecycle tests;
- TUI `/login` state transition tests if login is integrated into the app.

## 11. Rollout Plan

1. Remove the direct Responses runtime implementation from this branch.
2. Update PRD, techspec, README, and glossary to describe Codex local/subscription behavior.
3. Add Codex login-status availability checks.
4. Run config, doctor, and runtime unit tests.
5. Revisit `codex exec --json` after stream-mode plumbing is stable.
6. Design app-server policy mapping before enabling app-server-driven local mutations.

## 12. Risks

- `codex login status` output can change. Keep parsing conservative and rely primarily on process success.
- Older Codex CLI versions may not support every command. Treat this as `Unknown` with remediation instead of a hard crash.
- Running interactive login from inside the TUI can corrupt terminal state. Prefer external command guidance until raw-mode suspension is implemented.
- App-server can expose richer local actions than the harness currently models. Do not enable those actions without an explicit policy design.
