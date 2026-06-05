# Codex Subscription Runtime PRD

## Problem Statement

Developers want `multiagent` agents to use the same Codex access they already
have through the Codex CLI and ChatGPT subscription login. The previous
OpenAI Responses runtime plan solved a different problem: direct OpenAI
Platform API execution with `OPENAI_API_KEY`. That path is useful only when the
user intentionally wants API billing and Responses API semantics; it does not
reuse ChatGPT/Codex subscription entitlements.

The harness already has a `codex` execution runtime, but it treats Codex as a
plain child process and only checks `codex --version`. It does not surface
whether Codex is logged in, does not provide a first-class login workflow, and
does not yet use Codex's richer local integration surfaces for structured
events, conversation history, approvals, or streaming.

## Solution

Make the existing `codex` runtime the subscription-backed OpenAI path. The
runtime should rely on Codex-owned authentication and supported local Codex
interfaces instead of calling undocumented ChatGPT subscription endpoints or
reading Codex credential files.

The first implementation phase should harden the current `codex exec` path:
doctor reports the Codex CLI version and login status, users get clear
remediation when login is missing, and the runtime continues to execute through
the installed `codex` command. Later phases can add `codex exec --json` parsing
and a `codex app-server` JSON-RPC driver for richer product integration.

## User Stories

1. As a developer, I want agents using runtime `codex` to reuse my existing Codex CLI login, so that my ChatGPT/Codex subscription can back local harness work.
2. As a developer, I want the harness to work without `OPENAI_API_KEY` when I am using the `codex` runtime, so that subscription-backed usage is not confused with Platform API usage.
3. As a developer, I want `multiagent --doctor` to show whether Codex is installed and logged in, so that setup problems are obvious before a run starts.
4. As a developer, I want missing Codex login to point me to `codex login`, `codex login --device-auth`, or access-token setup when appropriate, so that I can fix auth using supported Codex flows.
5. As a developer, I want the harness to avoid reading `~/.codex/auth.json`, so that Codex credentials remain owned by Codex.
6. As a developer, I want trusted enterprise automation to be able to use `CODEX_ACCESS_TOKEN` or `codex login --with-access-token`, so that Codex workspace identity can be used without browser login when allowed.
7. As a developer, I want the runtime docs to explain that ChatGPT subscription auth and OpenAI Platform API-key auth are different billing and policy paths.
8. As a developer, I want `CODEX_API_KEY` to work for exec-scoped automation even when no saved Codex login exists, so that CI/headless runs are not blocked by a login-status preflight.
9. As a developer, I want custom agent profiles to keep selecting runtime `codex` explicitly, so that no agent silently switches to API-key execution.
10. As a developer, I want the current `codex exec` prompt contract to remain harness-owned, so that file edits and commands still pass through Harness Actions.
11. As a developer, I want model assignment to continue flowing through Codex CLI arguments, so that agent profiles remain declarative.
12. As a developer, I want Codex progress to become streamable in a future phase, so that long-running steps are visible in the TUI.
13. As a developer, I want machine-readable Codex events where available, so that parsing does not depend on human CLI prose.
14. As a harness maintainer, I want app-server integration to use Codex's documented JSON-RPC protocol, so that rich integration does not rely on private endpoints.
15. As a harness maintainer, I want app-server authentication, thread state, approvals, and streamed events evaluated against the existing harness action boundary before enabling local mutations through Codex.
16. As a harness maintainer, I want normal tests to stay offline and credential-free, so that CI does not need a Codex account.
17. As a user without a Codex login, I want the TUI to open and explain the unavailable runtime rather than fail with a cryptic process error.

## Implementation Decisions

- Keep the user-facing runtime kind as `codex`.
- Do not add an `openai_responses` runtime as part of this subscription feature.
- Treat `codex` as a local Codex integration that delegates authentication to the installed Codex CLI.
- Use `codex login status` in availability checks when the installed CLI supports it.
- Report command availability and login status separately in doctor output.
- Never parse, copy, or redact the contents of `~/.codex/auth.json`; the harness should only call documented Codex commands.
- Do not persist `CODEX_ACCESS_TOKEN` in harness config. Users or automation may provide it in the environment, and Codex owns how it is consumed.
- Do not block `codex exec` when `CODEX_API_KEY` or `CODEX_ACCESS_TOKEN` is set but `codex login status` reports no saved login.
- Keep `codex exec --skip-git-repo-check --color never` as the compatibility execution path until JSONL event parsing is implemented.
- Consider `codex exec --json` as the next incremental improvement for machine-readable events while keeping the final output contract harness-owned.
- Consider `codex exec --output-schema` only after the harness can generate stable per-step JSON Schema files without weakening action-request handling.
- Use `codex app-server` for a later rich-client driver when the harness needs persistent thread state, streamed agent events, approvals, and deeper integration.
- App-server integration must be local by default. WebSocket listeners need explicit authentication before non-loopback use.
- Keep direct OpenAI Responses/API-key execution as a separate future proposal, not as the subscription-backed Codex path.

## Testing Decisions

- Add unit tests for Codex availability with fake `codex` commands that model installed, missing, logged-in, logged-out, and timeout states.
- Add a regression test proving exec-scoped environment auth leaves availability `Unknown` instead of `Unavailable` when saved login is missing.
- Add doctor tests proving Codex login status appears without exposing credential material.
- Keep real Codex integration tests ignored and gated behind explicit environment variables.
- Preserve existing runtime parsing tests for action requests, orchestrator decisions, agent results, and parse errors.
- When `codex exec --json` is added, test event parsing with recorded JSONL fixtures rather than live Codex.
- When app-server is added, test JSON-RPC lifecycle handling with a fake app-server process.

## Out of Scope

- Calling undocumented ChatGPT subscription HTTP endpoints.
- Treating ChatGPT Plus/Pro subscription auth as an OpenAI Platform API key.
- Adding `openai_responses` as the implementation for this feature.
- Reading local Codex auth files or storing access tokens in harness config.
- Exposing `codex app-server` over public or unauthenticated network transports.
- Letting Codex bypass Harness Actions for local file edits or command execution without a separate policy decision.

## Further Notes

Official Codex docs consulted for this plan describe ChatGPT sign-in for
subscription access, API-key sign-in for usage-based access, `codex exec` for
non-interactive local runs, `codex exec --json` for machine-readable events,
`codex app-server` for rich client integrations, and Codex access tokens for
trusted Business/Enterprise automation. Those supported local surfaces are the
right basis for this feature.
