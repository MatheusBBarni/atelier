# Multiagent Harness Initial PRD

## Status

Initial product requirements document.

This document defines the v1 product for `multiagent`, a Rust-based terminal UI for orchestrating multiple specialized agents from a global CLI command.

## Background

Developers already use terminal-native agent CLIs such as Codex, Claude, and opencode for software work. Those tools are powerful, but a single agent often has to switch between planning, exploration, implementation, review, and critique. This project should create a focused multiagent harness that makes those roles explicit, visible, configurable, and resumable from the terminal.

The desired shape is similar in spirit to `oh-my-opencode-slim`: a lightweight, local-first harness around named agent roles. The first version should not build a cloud service or generic plugin marketplace. It should build the core local orchestration loop.

## Product Summary

`multiagent` is a globally installed command. When the user runs `multiagent` in any folder, it opens a terminal UI with an input composer. The user enters a prompt and presses Enter. The prompt goes first to the Orchestrator. The Orchestrator creates or updates a run plan, chooses the next specialized agent, and the harness executes that agent through its configured runtime.

The initial specialized agents are:

- Orchestrator
- Explorer
- Oracle
- Consul
- Fixer
- Reviewer

Each agent is defined by an agent profile. A profile controls the role instructions, model assignment, runtime, and capabilities for that agent. Users can override built-in profiles and add custom agents through `multiagent.toml`.

## Goals

- Provide a global `multiagent` command that can be launched from any working directory.
- Open an interactive Rust TUI with an agent roster, event stream, and input composer.
- Route every user prompt through the Orchestrator before specialized agents run.
- Support Codex subscription auth through a child CLI process.
- Support Z.ai through API-key authenticated HTTP calls.
- Allow each agent profile to use a different runtime and model.
- Persist project-local session history under `.multiagent/`.
- Enforce per-agent capabilities and approval prompts for high-impact actions.
- Provide built-in profiles so the first launch does not require handwritten config.
- Provide setup and diagnostic commands for config, runtime readiness, and merged configuration inspection.

## Non-Goals For V1

- Parallel active runs.
- Direct agent-to-agent delegation.
- In-TUI editing of agent profiles.
- Long-lived Codex child sessions.
- Exact process-level resume of external runtime state.
- Non-interactive `multiagent run` mode.
- Remote server or cloud sync.
- Plugin marketplace.
- Voice, image, or multimodal UI.
- Automatic commits or pushes without explicit user request.
- Fine-grained OS sandboxing beyond harness-level action, command, and path enforcement.
- Building the Rust scaffold as part of this PRD pass.

## Users

The primary user is a developer working in a local repository from a terminal. They want to delegate software work to specialized agents while retaining visibility into routing, commands, edits, review findings, and run history.

The first version assumes the user is comfortable configuring a TOML file, setting environment variables, and running a globally installed CLI.

## Core Concepts

- Harness: the interactive system that receives prompts, coordinates agents, and presents progress in a terminal UI.
- Harness Session: one period of interactive TUI use.
- Prompt: one user-submitted instruction.
- Run: one attempt to satisfy one prompt.
- Run Plan: the Orchestrator-owned sequence of steps for a run.
- Agent Profile: the configurable definition of a specialized agent.
- Execution Runtime: the backend used by an agent profile to perform model work.
- Agent Capability: an allowed action class for an agent profile.
- Session History: persisted session, run, event, command, diff, and verification records.

`CONTEXT.md` is the source of truth for canonical domain language.

## Launch Behavior

Running `multiagent` without arguments must launch the TUI in the current working directory.

The command must not require the current folder to contain a Rust project, config file, git repo, or prior `.multiagent/` directory. If no config exists, the harness uses built-in profiles and marks unavailable runtimes clearly.

The working directory is where the run reads, edits, runs commands, and stores project-local session history by default.

## TUI Requirements

The first TUI should use a three-zone layout:

- Agent Roster: shows agents, runtime, model assignment, runtime availability, capabilities summary, and current status.
- Event Stream: shows prompts, run plans, routing decisions, agent output, diffs, command results, approval prompts, blockers, review findings, and final summaries.
- Input Composer: accepts the initial prompt, clarifying answers, follow-up instructions, and run interrupts.

The harness supports one active run at a time in v1. While a run is active, the Input Composer controls that run. It does not start a new parallel run.

When the Orchestrator is uncertain or a specialized agent is blocked, only the Orchestrator asks the user a clarifying question. The run pauses until the user answers.

## Agent Roles

### Orchestrator

Owns the run plan, chooses agents, tracks progress, asks clarifying questions, applies run limits, and decides when to stop. The Orchestrator is not just a router.

### Explorer

Reads code, documentation, repository state, and session context without making changes. Explorer returns structured findings.

### Oracle

Answers design or implementation questions from gathered context. Oracle may produce prose, but it must return it inside a typed result envelope.

### Consul

Challenges plans, architecture, and domain decisions before work proceeds. Consul is used for ambiguous, architecture-heavy, or high-risk work.

### Fixer

Edits files and runs targeted verification. Fixer owns file modification work.

### Reviewer

Reviews changes for bugs, regressions, and missing tests. Reviewer can inspect diffs and run verification, but it does not edit files.

## Default Routing Policy

Routing classification lives in the built-in Orchestrator instructions, not hardcoded Rust rules. Rust enforces state transitions, capabilities, approvals, and limits.

Default examples:

- Typo or obvious tiny edit: Orchestrator may route directly to Fixer.
- Non-trivial code change: Explorer, then Fixer.
- Normal code-change loop: Explorer, Fixer, Reviewer, optional Fixer, final Orchestrator summary.
- Ambiguous or architecture-heavy work: Consul before Fixer.
- Question-answering work: Oracle when implementation is not required.

Specialized agents must not directly call other specialized agents. They return agent results to the Orchestrator, and the Orchestrator decides the next step.

## Runtime Support

### Runtime Adapter Boundary

The harness should define an execution runtime boundary from the start. Agent profiles select a runtime by name. V1 implements Codex and Z.ai first.

Runtime/model selection happens per agent profile. The Orchestrator may choose a different agent, but it must not override a selected agent's model or runtime for a specific step in v1.

### Codex Runtime

Codex Runtime launches Codex as a child CLI process using the user's existing Codex subscription authentication. It must not be implemented as a direct OpenAI API client.

V1 Codex invocations should be short-lived per agent step. Long-lived per-agent Codex sessions are future work.

For each Codex step, the harness sends a prompt envelope containing:

- Agent role and instructions.
- Current run plan.
- Relevant prior session events.
- Capability constraints.
- Working directory.
- Requested output schema.

Codex output should stream into the Event Stream where possible.

### Z.ai Runtime

Z.ai Runtime calls Z.ai through API-key authenticated HTTP requests. The API key is not stored in config. Config stores the environment variable name that contains the key.

Z.ai is reasoning/text generation only in v1. It can produce structured output or proposed actions, but file reads, file edits, command execution, verification, and approvals are harness actions owned by the Rust process.

Z.ai streaming should be supported by the event model. If HTTP streaming is deferred in the first implementation, the adapter may emit a single completed event while preserving the streaming-capable internal model.

## Built-In Defaults

The binary must include built-in profiles. The user should be able to install the binary, run `multiagent`, type a prompt, and see the TUI without first writing config.

Default model assignment policy:

- Orchestrator: use Z.ai `glm-5.1` if available, otherwise Codex default.
- Consul: use Z.ai `glm-5.1` or the strongest configured reasoning model.
- Explorer: use Codex default.
- Fixer: use Codex default.
- Reviewer: use Codex default or a stronger configured review model.
- Oracle: use Z.ai `glm-5.1` if available, otherwise Codex default.

Exact model names are config values. Built-in defaults may include names, but the rest of the system should treat model assignment as configurable data.

If no runtime is usable on first launch, the TUI still opens. Agents are shown as unavailable with actionable setup information.

## Configuration

### Config Locations

Home-scoped harness configuration:

```text
~/.config/multiagent/multiagent.toml
```

Environment override:

```text
MULTIAGENT_CONFIG=/path/to/multiagent.toml
```

Optional working-directory local configuration:

```text
./multiagent.toml
```

Local configuration overrides project-specific behavior for the current working directory.

### Effective Configuration

The harness builds effective configuration in this order:

1. Built-in profiles and defaults.
2. Home harness configuration.
3. Local working-directory configuration.
4. Command-line overrides.

Tables merge deeply by key. For example, `[agents.fixer]` in local config overrides only specified fields from the built-in or home Fixer profile.

Arrays replace rather than append. This is especially important for `capabilities`; local config must not accidentally grant extra capabilities through array concatenation.

### Secrets

Raw secrets must not be stored in `multiagent.toml`.

Z.ai config stores a credential reference:

```toml
[runtimes.zai]
type = "zai"
api_key_env = "ZAI_API_KEY"
```

Local config may override the env var name, for example `api_key_env = "PROJECT_ZAI_API_KEY"`, but it must not store the API key value.

### Instruction Sources

Agent instructions may be inline strings or file references.

File paths resolve relative to the config file that declares them:

- Home config instruction files resolve relative to `~/.config/multiagent/`.
- Local config instruction files resolve relative to the working directory.

Example:

```toml
[agents.fixer]
instructions_file = "agents/fixer.md"
```

### Example Config Shape

```toml
[runtimes.codex]
type = "codex"
command = "codex"

[runtimes.zai]
type = "zai"
api_key_env = "ZAI_API_KEY"

[limits]
max_agent_steps = 12
max_wall_clock_minutes = 30
max_command_minutes = 10
max_review_fix_cycles = 2

[agents.orchestrator]
runtime = "zai"
model = "glm-5.1"
capabilities = ["plan"]
instructions_file = "agents/orchestrator.md"

[agents.fixer]
runtime = "codex"
model = "default"
capabilities = ["read", "edit", "command", "verify"]
instructions = "Apply scoped changes and run targeted verification."

[agents.reviewer]
runtime = "codex"
model = "default"
capabilities = ["read", "command", "verify", "review"]
instructions_file = "agents/reviewer.md"
```

Custom agents are allowed through additional `[agents.<name>]` entries. Built-in profiles can be overridden by using the same name.

## Config Validation

Before launching the TUI, the harness validates enough to render a truthful UI.

Block launch for malformed configuration:

- Invalid TOML.
- Unknown or duplicate required fields.
- Missing referenced instruction files.
- Invalid capability names.
- Invalid limit values.
- Agents that point at undefined runtimes.

Do not block launch for runtime unavailability:

- Missing Codex command.
- Codex not authenticated.
- Missing Z.ai env var.
- Z.ai API/network failure.

Unavailable runtimes should appear in the Agent Roster and in `--doctor` output.

## Run Limits

Runs must have configurable limits:

- `max_agent_steps`
- `max_wall_clock_minutes`
- `max_command_minutes`
- `max_review_fix_cycles`

Default values:

```toml
[limits]
max_agent_steps = 12
max_wall_clock_minutes = 30
max_command_minutes = 10
max_review_fix_cycles = 2
```

Each limit accepts either a positive integer or the explicit string `"unlimited"`.

Omitted values use defaults. Numeric `0` must not mean unlimited.

When a limit is reached, the Orchestrator stops the run and writes a clear summary to the Event Stream.

## Structured Outputs

The Orchestrator must return structured decisions. The harness should not scrape prose to decide what to run.

Minimum Orchestrator decision fields:

```json
{
  "plan": [],
  "next_agent": "explorer",
  "reason": "Need repository context before editing.",
  "required_capabilities": ["read"],
  "stop_condition": "Repository context gathered or blocker reported."
}
```

Specialized agents return typed agent results.

Minimum agent result fields:

```json
{
  "agent": "explorer",
  "status": "completed",
  "summary": "Found the relevant modules.",
  "findings": [],
  "changed_files": [],
  "commands": [],
  "verification": [],
  "blocker": null
}
```

Role-specific fields are allowed, but all agent results must be stored in a consistent envelope for session history and Orchestrator input.

## Capability Enforcement

Agent capabilities are declared in agent profiles and enforced by the harness where possible.

Expected initial capabilities:

- `plan`
- `read`
- `answer`
- `challenge`
- `edit`
- `command`
- `verify`
- `review`

Default capability expectations:

- Explorer: read.
- Oracle: read and answer.
- Consul: read and challenge.
- Fixer: read, edit, command, verify.
- Reviewer: read, command, verify, review.
- Orchestrator: plan.

Reviewer must not edit files. Explorer must not edit files. Orchestrator coordinates and should not directly mutate files except by routing through an agent profile with the required capability.

If an external runtime cannot expose fine-grained tool calls, the harness still enforces at the boundaries it controls: command allow/deny behavior, writable paths, approval prompts, and which runtime step is launched.

## Action Approval

High-impact harness actions require user approval even when the agent has the relevant capability.

Approval is required for:

- Destructive shell commands.
- VCS actions such as commit, branch creation, reset, or push.
- Writes outside the working directory.
- Credential-related changes.
- Any action classified as high-impact by built-in policy.

Normal read commands, safe verification, and allowed working-directory file edits may proceed under capability enforcement without modal approval.

VCS actions require an explicit user request. The harness may inspect git status and diffs by default, but it must not commit or push unless the user asks for that action.

## Session History

Session history is stored project-locally under:

```text
.multiagent/
```

Recommended v1 structure:

```text
.multiagent/
  sessions/
    <session-id>/
      events.jsonl
      metadata.json
      artifacts/
        <artifact-id>.txt
        <artifact-id>.diff
  runs/
    <run-id>.json
```

Events should be append-only JSONL. The event model should support streaming output by appending incremental events.

Large outputs should not bloat `events.jsonl`. Store summaries and metadata in events, and spill large command output, model output, or diffs into artifact files. Event records should reference artifact paths.

V1 supports history inspection and context resume. Context resume starts a new run with previous session history available as context. It does not promise exact restoration of old child CLI processes or provider sessions.

## CLI Surface

V1 commands and flags:

```text
multiagent
multiagent --config <path>
multiagent --cwd <path>
multiagent --doctor
multiagent --print-config
multiagent --init-config
```

`multiagent` launches the TUI in the current directory.

`--config <path>` uses an alternate home configuration file for this invocation. `MULTIAGENT_CONFIG` provides the same override through the environment.

`--cwd <path>` launches the TUI with a specific working directory.

`--doctor` checks setup and runtime readiness.

`--print-config` prints the effective configuration with secrets redacted.

`--init-config` creates starter home config and instruction files only when missing. It must not overwrite existing files in v1.

No `run` subcommand is required in v1.

## Doctor Checks

### Codex Checks

`--doctor` should verify:

- `codex` command is available on `PATH` or at the configured command path.
- Basic invocation works without launching the full TUI.
- Codex appears authenticated enough to start a session.
- Configured Codex model is accepted locally or passed through without local rejection.
- Working directory is readable.
- `.multiagent/` can be created or written.

Doctor must not perform real file edits or consume significant tokens.

### Z.ai Checks

`--doctor` should verify:

- `api_key_env` is configured.
- The named environment variable exists and is non-empty.
- A low-cost API request succeeds.
- Configured model names are syntactically valid locally where possible.
- Provider-side model errors are reported clearly.
- Network/API errors are actionable.
- The API key value is never printed.

## Packaging

The project should be a Rust binary crate named `multiagent`.

Development installation:

```text
cargo install --path .
```

Future distribution may use:

```text
cargo install multiagent
```

or a Homebrew formula.

The binary must not rely on being run from the source repository. No daemon, shell integration, or project scaffold is required for first use.

## Rust Implementation Stack

Recommended v1 stack:

- `ratatui` for TUI rendering.
- `crossterm` for terminal backend/input.
- `tokio` for async process/API streaming.
- `serde` for structured data.
- `toml` for config parsing.
- `reqwest` for Z.ai HTTP.

These choices are implementation guidance for the first Rust scaffold, not domain terminology.

## Testing Requirements

V1 should include focused tests for:

- Config parsing.
- Layered config merge behavior.
- Array replacement semantics.
- Instruction path resolution.
- Limit parsing, including `"unlimited"`.
- Runtime availability checks with mocked commands/env.
- Orchestrator decision parsing.
- Agent result parsing.
- Session history append/read behavior.
- Capability enforcement.
- Approval classification.

TUI rendering may start with smoke tests. Full terminal snapshot tests can come later.

## V1 Completion Criteria

V1 is complete when these workflows work end to end:

- Install `multiagent` globally and launch it from any folder.
- Load built-in profiles, home config, and optional local config.
- Show the TUI with Agent Roster, Event Stream, and Input Composer.
- Accept a prompt and route it through the Orchestrator.
- Execute at least one Codex Runtime agent step when Codex is configured.
- Execute at least one Z.ai Runtime agent step when Z.ai is configured.
- Run the normal code-change loop: Explorer, Fixer, Reviewer, optional Fixer, final Orchestrator summary.
- Persist session history under `.multiagent/`.
- Spill large outputs to artifacts and reference them from events.
- Enforce capabilities.
- Require approvals for high-impact actions.
- Stop runs when configured limits are reached.
- Support explicit `"unlimited"` run limits.
- Provide `--doctor`, `--print-config`, and `--init-config`.

## Future Work

- Non-interactive run mode.
- Parallel runs.
- In-TUI config/profile editing.
- Long-lived per-agent runtime sessions.
- Exact process-level resume where supported by runtime.
- Direct OpenAI API runtime distinct from Codex subscription runtime.
- OS keychain support for credential references.
- Rich search/indexing over session history.
- Additional runtimes.
- Plugin system for custom runtime adapters.
- More advanced sandboxing.
