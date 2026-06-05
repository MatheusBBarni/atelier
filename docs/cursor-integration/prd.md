# Cursor Agent CLI Runtime PRD

## Problem Statement

Developers want harness agents to use Cursor through the installed Cursor Agent
CLI command, the same way the Codex Runtime uses the installed
`codex` command and the planned Claude Runtime uses the installed `claude`
command.

The goal is not a generic Cursor HTTP API integration. The goal is to let an
agent profile select a Cursor-backed execution runtime without changing the
harness action model, configuration layering, runtime availability reporting, or
session history contract.

The current harness supports `codex`, `zai`, and `fake` runtime kinds. Cursor is
closest to Codex and the planned Claude runtime because it is a local child CLI
process with its own authentication and CLI-specific output formats. Cursor is
not installed in this local environment yet, so implementation details must be
validated with fake command fixtures and the official CLI contract before any
live integration test is enabled.

## Proposed Direction

Add a first-class `cursor` Execution Runtime that launches the local Cursor
Agent CLI in non-interactive print mode.

The runtime should preserve the existing harness boundary:

- The harness owns file reads, file edits, command execution, VCS actions,
  approval policy, session history, and final output parsing.
- The Cursor CLI owns Cursor authentication, model access, Cursor-local CLI
  settings, and any Cursor-specific session metadata.
- Agent profiles continue to select runtime and model declaratively through
  effective configuration.
- Normal tests remain offline and credential-free.

This mirrors the Codex Runtime decision while keeping a separate runtime kind
because Cursor has Cursor-specific auth commands, permission files, print-mode
semantics, stream JSON events, tool-call events, and failure modes.

## User Stories

1. As a developer, I want an agent profile to select runtime `cursor`, so that
   Explorer, Fixer, Reviewer, Consul, or a custom agent can use Cursor without
   changing its role definition.
2. As a developer, I want the harness to use my installed Cursor Agent CLI
   command, so that setup remains local and familiar.
3. As a developer, I want the harness to avoid storing Cursor secrets in
   `multiagent.toml`, so that credential ownership stays with Cursor CLI or the
   user environment.
4. As a developer, I want `multiagent --doctor` to report whether the Cursor
   runtime command is installed and authenticated, so setup failures are visible
   before a run.
5. As a developer, I want missing Cursor authentication to point me to
   `agent login`, `agent status`, or `CURSOR_API_KEY` setup, so I
   can fix auth through supported Cursor flows.
6. As a developer, I want Cursor output parsed through the same
   action-request, orchestrator-decision, and agent-result contracts as other
   runtimes, so orchestration behavior does not fork by runtime.
7. As a developer, I want Cursor progress to appear in the TUI, so long steps do
   not look frozen.
8. As a harness maintainer, I want Cursor runtime tests to use fake process
   fixtures, so CI does not require Cursor credentials.
9. As a harness maintainer, I want Cursor-specific CLI args isolated in the
   Cursor Runtime, so Codex and Z.ai behavior does not regress.

## Initial Implementation Decisions

- Add `RuntimeKind::Cursor` and a `CursorRuntime` module instead of overloading
  the existing `CodexRuntime`.
- Use `cursor` as the runtime kind and built-in runtime id. The local command is
  configurable, with `agent` as the default Cursor Agent CLI executable.
- Add a built-in runtime entry:

  ```toml
  [runtimes.cursor]
  type = "cursor"
  command = "agent"
  args = []
  prompt_mode = "stdin"
  ```

- Do not add Cursor credential configuration for the first CLI integration.
  Cursor-owned saved login through `agent login` is the primary setup
  path. `CURSOR_API_KEY` may be used as an optional environment-owned automation
  fallback, but the harness should not require or persist a Cursor Credential
  Reference.
- Run the first phase in strict harness-action mode. Cursor can reason and
  return structured contracts, but file reads, file writes, shell commands, and
  VCS actions must still be requested as Harness Actions and executed by the
  harness.
- Keep Built-in Profiles on their existing runtimes by default. Adding the
  Cursor Runtime only makes `cursor` available for explicit selection in Agent
  Profiles, Custom Agents, Harness Configuration, or Local Configuration.
- Use a prompt envelope equivalent to the Codex Runtime envelope, adjusted only
  where Cursor-specific wording is necessary.
- Use `agent --print --output-format stream-json` as the first execution
  path. Parse safe progress from NDJSON events and parse only the terminal
  `result` event through the existing harness-owned structured output contract.
- For `--doctor`, resolve the configured Cursor command, run `<command>
  --version`, and run `<command> status` with short timeouts.
  Do not run paid model prompts or live print-mode smoke tests from doctor.
- Do not preserve or resume Cursor CLI sessions by default. Each Cursor runtime
  step starts a fresh non-interactive print run using the harness prompt envelope
  plus harness-owned Session History for context.
- Preserve Cursor `session_id` values from stream events only as diagnostic
  metadata.
- Do not generate or mutate `.cursor/cli.json` permission files in the first
  phase. Document how Cursor permissions interact with harness capabilities, but
  keep Agent Capabilities and Action Approval as the authoritative policy.
- Allow normal Cursor project context by default. Cursor project rules and
  ambient instruction files may be loaded by the Cursor command, but Agent Profile
  instructions, Capability Enforcement, Action Approval, and the structured
  output contract remain higher priority.
- Map the Agent Profile model field to Cursor's `--model` flag when the model is
  not `default`.
- Keep CLI args configurable for advanced users, but reject args that bypass
  harness policy, leak secrets, or conflict with runtime-owned execution flags.
  Protected args include `--force`, `-f`, `--api-key`, `-a`, `--api-key=<value>`,
  `resume`, `--resume`, session resume identifiers, `--print`,
  `--output-format`, and `--model`.
- Treat Cursor `stream-json` tool-call events as adapter input, not trusted
  Harness Actions. Mutating Cursor tool calls such as file writes or shell
  commands fail the step as a policy violation. Read-only tool events may be
  surfaced as diagnostics only in the first phase.
- Keep normal tests fake and credential-free. Unit tests should use fake Cursor
  Agent CLI commands and recorded NDJSON fixtures. Real Cursor smoke tests,
  if added, must be ignored by default and gated behind explicit environment
  variables.
- Setup remediation should point to Cursor-owned setup commands. Missing command
  remediation should tell users to install Cursor Agent CLI from official Cursor
  docs or set `[runtimes.cursor].command`. Authentication remediation should
  tell users to run `agent login` and verify with `agent status`.
  Mention `CURSOR_API_KEY` only as an optional automation fallback.
- Pass the harness prompt envelope to the Cursor command over stdin by default. The
  envelope may be large and may include Session History, so stdin avoids argv
  length limits, shell quoting problems, and prompt leakage through process
  arguments. If live Cursor smoke testing proves `agent --print` does not accept
  stdin input, add a narrow fallback to pass the envelope with `-p` through
  `Command::arg`, never through a shell string.

## Open Decisions

None.

## Out of Scope

- Direct Cursor HTTP/API integration.
- Reading or mutating Cursor CLI credential files.
- Storing `CURSOR_API_KEY` or any Cursor credential value in harness
  configuration.
- Exposing Cursor credentials through `--print-config`.
- Letting Cursor CLI tools bypass Harness Actions before an explicit policy
  decision.
- Replacing the Codex or Z.ai runtimes.

## References Consulted

- Existing glossary and runtime language: `CONTEXT.md`.
- Existing Codex CLI runtime precedent: `docs/codex-api/prd.md`.
- Planned Claude CLI runtime precedent: `docs/claude-integration/prd.md`.
- Runtime configuration and current supported runtime kinds:
  `src/config/mod.rs`.
- Current Codex child-process runtime implementation: `src/runtime/codex.rs`.
- Cursor CLI overview: `https://docs.cursor.com/en/cli/overview`.
- Cursor CLI usage: `https://docs.cursor.com/en/cli/using`.
- Cursor headless mode: `https://docs.cursor.com/en/cli/headless`.
- Cursor CLI parameters: `https://docs.cursor.com/en/cli/reference/parameters`.
- Cursor CLI authentication:
  `https://docs.cursor.com/en/cli/reference/authentication`.
- Cursor CLI output format:
  `https://docs.cursor.com/en/cli/reference/output-format`.
- Cursor CLI permissions:
  `https://docs.cursor.com/cli/reference/permissions`.

## Final Decision

Build a first-class `cursor` Execution Runtime backed by the installed Cursor
Agent CLI command, defaulting to `agent`. The runtime should use Cursor-owned saved
login, strict harness-action mode, stdin prompt envelopes, stream JSON parsing,
and harness-owned Session History. Cursor-native tools, session resume, project
permission mutation, direct API integration, and stored Cursor credentials remain
out of scope for phase one.
