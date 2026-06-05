# Plan: Codex Subscription Integration

Status: Draft
Date: 2026-06-04

## Summary

The Codex integration should use Codex local surfaces, not a direct OpenAI
Responses API runtime. When the user wants to use an OpenAI Codex subscription,
the supported path is the installed Codex CLI or app-server with Codex-owned
authentication.

The current harness already has the right runtime name, `codex`. This plan
hardens that runtime instead of adding `openai_responses`.

## Official Docs Basis

OpenAI Codex docs used for this plan:

- Codex authentication: https://developers.openai.com/codex/auth
- Codex non-interactive mode: https://developers.openai.com/codex/noninteractive
- Codex app-server: https://developers.openai.com/codex/app-server
- Codex access tokens: https://developers.openai.com/codex/enterprise/access-tokens
- Codex CLI reference: https://developers.openai.com/codex/cli/reference

Key implications:

- Codex supports ChatGPT sign-in for subscription access and API-key sign-in for usage-based access.
- The Codex CLI reuses cached Codex login state for local runs.
- `codex exec` is the documented non-interactive CLI path.
- `codex exec --json` emits machine-readable JSONL events.
- `codex app-server` is the documented rich-client integration surface for auth, conversation history, approvals, and streamed agent events.
- Codex access tokens are for trusted Business/Enterprise automation that specifically needs ChatGPT workspace identity.
- For general OpenAI API calls, Platform API keys remain the correct credential type.

## Current Runtime Behavior

`src/runtime/codex.rs`:

- Resolves and checks a configured `codex` command.
- Builds a JSON prompt envelope.
- Runs `codex exec --skip-git-repo-check --color never`.
- Writes the envelope to stdin.
- Waits for full stdout/stderr completion.
- Parses final stdout as an action request, orchestrator decision, agent result, or parse error.
- Emits one final `RuntimeStreamDelta`.

Benefits:

- Reuses the user's installed Codex CLI and Codex-owned authentication.
- Keeps ChatGPT subscription auth separate from Platform API auth.
- Keeps local file and command effects under Harness Actions.

Gaps:

- Availability currently checks only `codex --version`.
- Doctor does not report whether Codex is logged in.
- Progress is not streamed while `codex exec` is running.
- The runtime does not yet consume `codex exec --json` events.
- The runtime does not use app-server thread/turn APIs.

## Target Design

Keep runtime selection unchanged:

```toml
[runtimes.codex]
type = "codex"
command = "codex"

[agents.fixer]
runtime = "codex"
model = "default"
```

Do not add an `OPENAI_API_KEY` requirement for this path. Codex subscription
auth is established through Codex itself:

```bash
codex login
codex login status
```

For headless or remote environments:

```bash
codex login --device-auth
```

For trusted Business/Enterprise automation when enabled by the workspace:

```bash
printf '%s' "$CODEX_ACCESS_TOKEN" | codex login --with-access-token
```

For single non-interactive automation runs, Codex also supports exec-scoped
environment credentials:

```bash
CODEX_API_KEY=... codex exec --json "triage open bug reports"
CODEX_ACCESS_TOKEN=... codex exec --json "review this repository"
```

`CODEX_API_KEY` is supported only by `codex exec`, so `codex login status` can
report no saved login even though `codex exec` would authenticate successfully.

## Phase 1: Availability and Doctor

Enhance `CodexRuntime::check_availability`:

1. Resolve the configured command.
2. Run `codex --version`.
3. Run `codex login status`.
4. Return `Available` when command and saved login status succeed.
5. Return `Unknown`, not `Unavailable`, when login status fails but a supported
   exec-scoped auth environment variable is present.
6. Return a clear remediation when login is missing or status cannot be determined.

Doctor should show the runtime id, runtime type, command, version, and login
status summary. It must not inspect or print Codex credential contents.

## Phase 2: JSONL Events

Use `codex exec --json` to improve progress streaming after the app's stream
event plumbing is stable:

- parse stdout as JSONL events;
- map item lifecycle and agent-message deltas into runtime stream deltas;
- parse the final agent message through the existing harness contract;
- keep stderr diagnostic-only.

## Phase 3: App-Server Driver

Use `codex app-server` when the harness needs rich local integration:

- spawn app-server over stdio by default;
- send `initialize` and `initialized`;
- start or resume a thread;
- send `turn/start`;
- read item and turn notifications;
- extract the final agent message;
- preserve Harness Action policy until Codex app-server approvals and tool actions are explicitly mapped.

Do not expose app-server on public or unauthenticated network transports.

## Auth Boundary

The harness may call Codex commands, but it must not:

- parse `~/.codex/auth.json`;
- copy Codex auth files;
- persist `CODEX_ACCESS_TOKEN`;
- treat ChatGPT subscription auth as `OPENAI_API_KEY`;
- call undocumented subscription endpoints.

The direct OpenAI Platform API path can be proposed separately if the product
also needs API-keyed execution. It should not be described as the Codex
subscription integration.

## Acceptance Criteria

- `multiagent --doctor` reports Codex command availability and login status.
- Missing Codex login produces a clear remediation, not an opaque runtime failure.
- Runtime `codex` continues to execute through the installed Codex CLI.
- No code path reads local Codex credential files.
- No `openai_responses` runtime is introduced for the subscription feature.
- Docs explain the difference between Codex subscription auth and Platform API-key auth.

## Open Questions

- Should the TUI gain `/login`, or is doctor/remediation enough for the first phase?
- Should `codex exec --json` become the default after stream-mode lands?
- Should app-server be a mode of `codex` or a separate runtime kind if policy behavior diverges?
