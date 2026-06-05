# Technical Specification: Multiagent Harness V1

Status: Draft
Date: 2026-06-03

## Executive Summary

This specification defines the v1 technical design for `multiagent`, a globally installed Rust TUI that routes user prompts through an Orchestrator and specialized agent profiles. The implementation is a single Rust binary package with a testable library core, a typed app event loop, durable JSON/JSONL history, TOML configuration, capability-enforced harness actions, and two initial runtimes: Codex as a subscription-backed child process and Z.ai as an OpenAI-compatible HTTP adapter.

The key architectural decision is that model runtimes do not directly mutate files or run shell commands. Runtimes produce structured decisions, agent results, and action requests; the Rust harness validates capabilities, path scope, command policy, limits, and approval mode before executing actions.

## Background / Context

The product requirements are captured in [docs/initial-prd.md](./initial-prd.md). Canonical domain language is captured in [CONTEXT.md](../CONTEXT.md). The Codex subscription runtime decision is captured in [docs/adr/0001-codex-runtime-uses-cli-subscription.md](./adr/0001-codex-runtime-uses-cli-subscription.md).

The repository currently contains documentation only. There is no Rust scaffold or existing architecture to preserve.

## Goals

- Build a Rust binary crate named `multiagent` that installs globally with `cargo install --path .`.
- Keep the implementation as one Rust package with `src/lib.rs` and `src/main.rs`.
- Provide a TUI with Agent Roster, Chat, and Input Composer.
- Implement a typed app event loop that coordinates TUI input, runtime streaming, harness actions, approvals, history, and diagnostics.
- Resolve built-in, home, local, and CLI config into a validated `EffectiveConfig`.
- Persist meaningful session/run history as JSON/JSONL under `.multiagent/`.
- Enforce agent capabilities, path scope, command policy, run limits, and hard denies even when `approval_mode = "yolo"`.
- Support Codex Runtime through configurable child process invocation.
- Support Z.ai Runtime through an OpenAI-compatible chat-completions HTTP contract.
- Make external runtime integration tests opt-in while keeping default tests deterministic.

## Non-Goals

- Multi-crate workspace in v1.
- Parallel active runs.
- In-TUI profile/config editing.
- Non-interactive `multiagent run`.
- Long-lived runtime sessions beyond one agent step.
- Exact restoration of old external runtime process state.
- Remote telemetry or cloud sync.
- Direct OpenAI API runtime separate from Codex subscription runtime.
- Full OS sandboxing beyond harness-enforced action, command, and path policy.

## Requirements

### Functional Requirements

- `multiagent` launches the TUI in the current working directory.
- `multiagent --cwd <path>` launches the TUI in a specific working directory.
- `multiagent --config <path>` or `MULTIAGENT_CONFIG` selects an alternate home config.
- `multiagent --doctor` prints human-readable diagnostics.
- `multiagent --doctor --json` prints machine-readable diagnostics.
- `multiagent --print-config` prints redacted effective config as TOML only.
- `multiagent --init-config` creates missing starter home config/instruction files without overwriting existing files.
- `multiagent --clean-sessions` deletes project-local session/run history for the selected working directory after confirmation.
- `multiagent --clean-sessions --yes` skips cleanup confirmation.
- The TUI supports one active run at a time.
- Every prompt starts with the Orchestrator.
- Specialized agents never directly delegate to other specialized agents.
- Runtime output is normalized into typed decisions, action requests, and agent results.
- All file and shell actions are executed by the harness, not directly by model runtimes.

### Non-Functional Requirements

- Default tests must not require Codex subscription auth, Z.ai credentials, or network access.
- Persisted history must be inspectable with normal text tools.
- Config and history files should be private by default on Unix-like systems.
- The app must avoid panics for malformed config, malformed runtime output, missing credentials, missing child commands, and interrupted runtime steps.
- Runtime and action failures must surface as redacted diagnostics and, when run-relevant, durable history events.

### Success Metrics

- A user can install and launch `multiagent` from any folder.
- A fake-runtime run can complete through Orchestrator and at least one specialized agent.
- Codex and Z.ai adapters each execute one agent step in opt-in integration tests.
- Config merge, action policy, state transitions, and history append/read have deterministic unit tests.
- `--doctor` reports missing runtime setup without launching the TUI.

## Proposed Design

V1 uses one Rust package:

```text
Cargo.toml
src/
  main.rs
  lib.rs
  cli.rs
  config/
  app/
  tui/
  orchestrator/
  runtime/
  actions/
  history/
  doctor/
  diagnostics/
  ids.rs
```

`src/main.rs` handles process startup, CLI parsing, terminal lifecycle, and top-level error reporting. `src/lib.rs` exposes testable modules.

The core architecture is an async event loop:

- TUI input becomes typed `AppEvent`s.
- Runtime adapters stream output and action requests back as `AppEvent`s.
- The app core validates transitions, updates `AppState`, performs harness actions, and emits durable `HistoryEvent`s.
- The TUI renders immutable snapshots of `AppState`.
- The history store persists only durable history records.

The harness is the only executor of file reads, file writes, patches, shell commands, VCS actions, and session cleanup.

## Architecture / Components

### CLI

Responsibilities:

- Parse flags and commands.
- Resolve working directory.
- Load config.
- Dispatch to TUI, doctor, print-config, init-config, or clean-sessions.

Recommended crate: `clap`.

CLI surface:

```text
multiagent
multiagent --config <path>
multiagent --cwd <path>
multiagent --doctor
multiagent --doctor --json
multiagent --print-config
multiagent --init-config
multiagent --clean-sessions
multiagent --clean-sessions --yes
```

`--print-config` emits TOML only. `--doctor --json` is the machine-readable diagnostic path.

### Config

The config module owns:

- Built-in defaults.
- TOML parsing.
- Deep merge.
- Instruction loading.
- Path resolution.
- Limit parsing.
- Validation.
- Redaction and TOML rendering for `--print-config`.

Use two layers:

- `RawConfig`: TOML-shaped partial config with `Option<T>` fields.
- `EffectiveConfig`: fully merged, validated, executable config.

Merge order:

1. Built-in defaults.
2. Home config: `~/.config/multiagent/multiagent.toml`, or `MULTIAGENT_CONFIG`, or `--config`.
3. Local config: `<working-directory>/multiagent.toml`.
4. CLI overrides.

Merge rules:

- Tables deep-merge by key.
- Scalars replace.
- Arrays replace.
- `instructions` and `instructions_file` are mutually exclusive in the final profile.
- Runtime `type` cannot change after a runtime key is introduced.
- Agents can be disabled with `enabled = false`.
- Raw secret values are invalid in config.

Instruction file paths resolve relative to the config file that declares them.

### App Core

The app core owns `AppState`, state transitions, and side-effect orchestration.

Core inputs:

- TUI input events.
- Runtime events.
- Action results.
- Approval decisions.
- Cancellation requests.
- Timer/limit events.

Core outputs:

- State snapshots for TUI.
- Runtime task requests.
- Harness action executions.
- Durable history events.
- Diagnostics.

The core should use Tokio channels:

- `mpsc::Sender<AppEvent>` for event ingress.
- Spawned runtime/action tasks send events back to the app loop.
- Cancellation uses `tokio_util::sync::CancellationToken` or equivalent.

### TUI

Recommended stack:

- `ratatui`
- `crossterm`

The TUI must be side-effect-light:

- Rendering reads immutable `AppState`.
- Keyboard/input handling emits `AppEvent`.
- TUI code does not invoke runtimes.
- TUI code does not write history.
- TUI code does not execute harness actions.

Primary surfaces:

- Agent Roster.
- Chat.
- Input Composer.

### Orchestrator

V1 uses a hybrid Orchestrator:

- The Orchestrator agent handles semantic planning, routing, clarifying questions, and final summaries.
- Rust enforces one active run, limits, capabilities, runtime availability, valid transitions, cancellation, parse repair counts, and approval policy.

Every `OrchestratorDecision` is validated before dispatch.

Invalid Orchestrator decisions:

- Emit a durable error event.
- Trigger one repair retry if limits allow.
- Pause the run with a clear diagnostic if repair fails.

### Runtime Adapters

Runtime adapters normalize model/provider output into harness contracts.

Trait shape:

```rust
#[async_trait::async_trait]
pub trait Runtime: Send + Sync {
    async fn check_availability(&self) -> RuntimeAvailability;

    async fn stream_step(
        &self,
        request: RuntimeRequest,
        sink: RuntimeEventSink,
    ) -> RuntimeResult<AgentResult>;

    async fn doctor(&self) -> DoctorReport;
}
```

`RuntimeRequest` includes:

- Agent profile.
- Working directory.
- Prompt envelope.
- Relevant history.
- Output schema.
- Capability constraints.
- Limits.
- Step transcript.

`RuntimeEventSink` emits streaming chunks, action requests, parse diagnostics, and progress updates into the app event loop.

### Codex Runtime

Codex Runtime launches a configurable child process for each agent step.

Config shape:

```toml
[runtimes.codex]
type = "codex"
command = "codex"
args = []
prompt_mode = "stdin"
```

Invocation:

- Build command as `command + args`.
- Set current directory to the working directory.
- Send the prompt envelope to stdin.
- Capture stdout and stderr.
- Keep the process alive only for the current agent step.
- Require a delimited JSON block for final `AgentResult` or `OrchestratorDecision`.

If the JSON block is missing or malformed:

- Store raw output as an artifact.
- Return `status = "parse_error"`.
- Allow one repair retry through the Orchestrator if limits allow.

The exact Codex CLI subcommand is not hardcoded in v1 beyond defaults; users can adjust `command` and `args`.

### Z.ai Runtime

Z.ai Runtime uses an OpenAI-compatible chat completions contract.

Config shape:

```toml
[runtimes.zai]
type = "zai"
base_url = "https://api.z.ai/api/paas/v4"
api_key_env = "ZAI_API_KEY"
```

Request:

```text
POST {base_url}/chat/completions
Authorization: Bearer <api-key>
Content-Type: application/json
```

Body includes:

- `model`
- `messages`
- optional `stream`
- runtime-specific supported options

The adapter parses non-streaming responses and streaming deltas into runtime events, then normalizes final content into an `AgentResult` or `OrchestratorDecision`.

The API key value is never logged, persisted, or printed.

### Actions

Runtimes request work through typed `ActionRequest`s. The harness returns typed `ActionResult`s.

V1 action set:

- `read_file`
- `list_files`
- `search_text`
- `run_command`
- `apply_patch`
- `write_file`
- `record_note`

`write_file` is only for newly created files or full generated artifacts. Existing source edits should use `apply_patch`.

`apply_patch` uses validated unified diffs:

- Reject absolute paths.
- Reject path traversal.
- Reject files outside allowed write roots.
- Verify context matches current file contents.
- Apply atomically per file.
- Reject binary patches in v1.
- Emit changed-file summaries.

### Command Policy

Commands are classified before execution:

- `allow`: safe read/verification commands.
- `approve`: high-impact commands, VCS mutations, package installs, writes outside normal build/cache paths.
- `deny`: destructive, privilege-escalating, or credential-exfiltration patterns.

`approval_mode = "yolo"` is the default. In yolo mode:

- Skip prompts for `approve` commands.
- Still enforce capabilities.
- Still enforce path scope.
- Still enforce run/step limits.
- Still enforce hard denies.

`approval_mode = "normal"` prompts for `approve` actions.

The exact command string is always shown in event history for executed commands, with redaction applied where needed.

### History Store

History lives under the selected working directory:

```text
.multiagent/
  sessions/
    <session-id>/
      metadata.json
      events.jsonl
      artifacts/
        <artifact-id>.<ext>
  runs/
    <run-id>.json
  debug.log
```

Use ULIDs for:

- `session_id`
- `run_id`
- `step_id`
- `event_id`
- `artifact_id`

Persist only `HistoryEvent`, not internal terminal/UI noise.

Each persisted record includes `schema_version = 1`.

Artifact metadata lives in the referencing `HistoryEvent`:

- `artifact_id`
- relative path
- media type
- byte length
- sha256
- redaction status

No separate artifact manifest is required in v1.

### Doctor

Doctor checks return a `DoctorReport` containing checks with:

- id
- title
- status
- severity
- message
- remediation
- redacted context

Default output is human-readable. `--doctor --json` emits structured JSON.

Doctor warns, but does not block, for broad config/history file permissions.

## Data Model and Contracts

### IDs

Use ULIDs encoded as strings.

### Run State

```rust
pub enum RunState {
    Idle,
    Planning,
    Running,
    WaitingForUser,
    Interrupted,
    Completed,
    Failed,
    LimitReached,
}
```

### Step State

```rust
pub enum StepState {
    Queued,
    Starting,
    Running,
    WaitingForAction,
    WaitingForApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
}
```

### Agent Result Status

```rust
pub enum AgentResultStatus {
    Completed,
    Blocked,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
    ApprovalDenied,
    NoChanges,
}
```

### Effective Config

Conceptual shape:

```rust
pub struct EffectiveConfig {
    pub schema_version: u32,
    pub working_directory: PathBuf,
    pub approval_mode: ApprovalMode,
    pub workspace: WorkspacePolicy,
    pub limits: Limits,
    pub runtimes: BTreeMap<RuntimeId, RuntimeConfig>,
    pub agents: BTreeMap<AgentId, AgentProfile>,
}
```

Limits:

```rust
pub struct Limits {
    pub max_agent_steps: Limit<u32>,
    pub max_step_actions: Limit<u32>,
    pub max_wall_clock_minutes: Limit<u32>,
    pub max_step_minutes: Limit<u32>,
    pub max_command_minutes: Limit<u32>,
    pub max_review_fix_cycles: Limit<u32>,
}

pub enum Limit<T> {
    Value(T),
    Unlimited,
}
```

Default limits:

```toml
[limits]
max_agent_steps = 12
max_step_actions = 20
max_wall_clock_minutes = 30
max_step_minutes = 10
max_command_minutes = 10
max_review_fix_cycles = 2
```

### Workspace Policy

```toml
[workspace]
extra_read_roots = []
extra_write_roots = []
```

Agent-visible reads and writes are scoped to the working directory by default. Extra roots are opt-in.

The harness may read its own config and instruction files outside the working directory for operation, but those files are not automatically exposed to model-visible actions.

### Orchestrator Decision

Minimum contract:

```json
{
  "schema_version": 1,
  "decision_id": "01...",
  "run_id": "01...",
  "status": "continue",
  "plan": [],
  "next_agent": "explorer",
  "reason": "Need repository context before editing.",
  "required_capabilities": ["read"],
  "stop_condition": "Repository context gathered or blocker reported.",
  "clarifying_question": null,
  "final_summary": null
}
```

Valid statuses:

- `continue`
- `waiting_for_user`
- `complete`
- `failed`

### Agent Result

Minimum contract:

```json
{
  "schema_version": 1,
  "agent": "explorer",
  "step_id": "01...",
  "status": "completed",
  "summary": "Found the relevant modules.",
  "findings": [],
  "changed_files": [],
  "commands": [],
  "verification": [],
  "blocker": null,
  "artifacts": []
}
```

### Action Request

Conceptual tagged enum:

```json
{
  "schema_version": 1,
  "action_id": "01...",
  "step_id": "01...",
  "kind": "read_file",
  "params": {
    "path": "src/lib.rs"
  }
}
```

All action requests are validated against:

- selected agent capabilities
- workspace path policy
- command policy
- approval mode
- run and step limits
- cancellation state

### Action Result

```json
{
  "schema_version": 1,
  "action_id": "01...",
  "status": "completed",
  "summary": "Read 120 lines.",
  "content": null,
  "artifact": null,
  "diagnostic": null
}
```

Large content should be stored as an artifact and referenced from the result.

### History Event

Conceptual tagged enum:

```json
{
  "schema_version": 1,
  "event_id": "01...",
  "session_id": "01...",
  "run_id": "01...",
  "step_id": "01...",
  "timestamp": "2026-06-03T13:00:00Z",
  "kind": "agent_result",
  "payload": {}
}
```

V1 event kinds:

- `session_started`
- `session_ended`
- `run_started`
- `prompt_submitted`
- `orchestrator_decision`
- `orchestrator_decision_invalid`
- `agent_step_started`
- `runtime_stream_delta`
- `action_requested`
- `action_completed`
- `approval_requested`
- `approval_resolved`
- `agent_result`
- `command_started`
- `command_completed`
- `file_edit_applied`
- `artifact_written`
- `diagnostic`
- `blocker_reported`
- `run_limit_reached`
- `step_limit_reached`
- `step_cancel_requested`
- `step_cancelled`
- `run_interrupted`
- `run_completed`
- `run_failed`

## APIs / Events

### AppEvent

`AppEvent` is internal and may include terminal-only events:

- input changed
- prompt submitted
- terminal resized
- tick
- runtime stream delta
- action requested
- approval answered
- interrupt requested
- runtime task completed
- diagnostic emitted

`AppEvent` is not persisted directly.

### HistoryEvent

`HistoryEvent` is durable and append-only. It excludes terminal-only noise.

### Runtime Tool Loop

Each agent step is a bounded interaction loop:

1. App core builds `RuntimeRequest`.
2. Runtime streams text/events and may emit `ActionRequest`.
3. Harness validates and executes the action.
4. Harness sends `ActionResult` back to the runtime step.
5. Runtime continues until it emits final result or hits a limit/cancellation.

Z.ai implements this with repeated chat-completion calls and appended action results.

Codex may keep the child process alive for one step or re-invoke with accumulated transcript, but it must not outlive the step.

## Security and Privacy

### Capability Enforcement

Capabilities are enforced in Rust, not just prompts.

Initial capabilities:

- `plan`
- `read`
- `answer`
- `challenge`
- `edit`
- `command`
- `verify`
- `review`

### Path Scope

Default allowed read roots:

- working directory
- configured `extra_read_roots`

Default allowed write roots:

- working directory
- configured `extra_write_roots`

Reject absolute paths and path traversal in model-requested actions unless they resolve inside allowed roots.

### Approval Mode

Default:

```toml
approval_mode = "yolo"
```

Yolo skips approval prompts for actions classified as `approve`, but it does not bypass capabilities, path scope, run limits, step limits, private file permissions, or hard denies.

Session cleanup confirmation is independent of yolo and still prompts unless `--yes` is passed.

### Redaction

Redaction is source-based:

- Never print or persist API key values.
- Store credential references as env var names only.
- Redact Authorization headers.
- Redact configured secret env vars in diagnostics/logs.
- Do not automatically redact ordinary prompt/file content from history.
- Mark artifacts with `redaction_status`.

Artifact redaction statuses:

- `not_redacted`
- `redacted`
- `contains_user_content`

### File Permissions

On Unix-like systems:

- Create config/history directories with `0700` where possible.
- Create config/history files with `0600` where possible.
- Warn in `--doctor` when existing permissions are broader.
- Do not automatically chmod existing files in v1.

On non-Unix platforms, use best-effort private defaults.

### Abuse Cases

- Agent asks to edit outside working directory: deny unless configured extra write root allows it.
- Agent asks to read secrets outside working directory: deny unless configured extra read root allows it.
- Agent emits destructive command in yolo: hard deny if classified as deny; otherwise allow approve-classified actions only under yolo semantics.
- Runtime returns malformed control output: persist raw output artifact, mark parse error, allow one repair retry.
- Local config tries to store raw secret: config validation fails.
- Local config silently changes runtime type: validation fails.

## Performance and Reliability

### Limits

Default limits:

- max agent steps: 12
- max step actions: 20
- max wall-clock minutes: 30
- max step minutes: 10
- max command minutes: 10
- max review/fix cycles: 2

All limits accept positive integers or `"unlimited"`. Omission uses defaults. Numeric `0` is invalid.

### Cancellation

Interrupt flow:

1. TUI emits `RunInterruptRequested`.
2. App core records cancel-requested event.
3. Active runtime/action task receives a cancellation token.
4. The task gets a short graceful shutdown window.
5. Child process is killed or HTTP request is dropped if needed.
6. Step moves to `cancelled`.
7. Run moves to `interrupted`.
8. Orchestrator does not continue automatically.

### Atomicity

- History appends should flush after each durable event.
- Patch application should be atomic per file.
- Failed patches must not partially modify a file.
- Session cleanup should delete only `.multiagent/sessions/` and `.multiagent/runs/` in the selected working directory.

### Failure Modes

- Missing runtime credential: runtime unavailable, TUI still launches.
- Missing child command: runtime unavailable, TUI still launches.
- Invalid config: block launch or command execution with diagnostic.
- History write failure: show diagnostic and fail the active run if history is required.
- Terminal setup failure: exit cleanly with diagnostic and restore terminal if partially initialized.

## Observability

V1 observability is local-only.

Sources:

- `.multiagent/sessions/<session-id>/events.jsonl`
- `.multiagent/sessions/<session-id>/artifacts/`
- optional `.multiagent/debug.log`
- `--doctor`

Enable debug log with:

```text
MULTIAGENT_LOG=debug
multiagent --debug
```

Debug logs are opt-in because they may contain prompt, code, and command context.

No remote telemetry is included in v1.

Debug records should include:

- event IDs
- session/run/step IDs
- runtime name
- command path
- exit status
- durations
- artifact references
- diagnostics

All logs and diagnostics use redaction rules.

## Migration and Rollout

There is no existing crate or persisted schema to migrate.

Implementation milestones:

1. Crate skeleton, CLI parsing, config pipeline, built-in profiles.
2. History store, app event loop, run/step state machine.
3. TUI shell rendering Agent Roster, Chat, and Input Composer with fake runtime.
4. Action protocol, capability/path/command policy, approvals/yolo.
5. Codex and Z.ai runtime adapters, doctor checks, final acceptance loop.

Feature flags are not required for v1, but runtime adapters should be independently testable and disabled when unavailable.

Backwards compatibility begins with persisted `schema_version = 1`. Unknown future major versions should be rejected with a clear diagnostic. Unknown fields in same-version records should be tolerated where possible.

## Testing Strategy

Default tests must be deterministic and not require external services.

Unit tests:

- `RawConfig` TOML parsing.
- Config merge order and conflict rules.
- Array replacement.
- Runtime type immutability.
- Agent disabling.
- Instruction path resolution.
- Limit parsing including `"unlimited"` and invalid `0`.
- Effective config validation.
- Orchestrator decision parsing and validation.
- Agent result parsing.
- Run/step state transitions.
- AppEvent to HistoryEvent mapping.
- History append/read.
- Artifact metadata and sha256 calculation.
- Path scope validation.
- Unified diff validation and application.
- Command classification.
- Approval mode behavior.
- Redaction.
- Doctor report construction.

Runtime tests:

- Codex adapter with a fake process runner.
- Codex malformed output behavior.
- Z.ai adapter with an HTTP mock server.
- Z.ai streaming delta parsing.
- Action loop behavior with fake runtime.

TUI tests:

- Rendering smoke tests for empty state, unavailable runtimes, active run, approval prompt, and completed run.
- Input handling tests that verify key events become `AppEvent`s.

Integration tests:

- Fake runtime end-to-end prompt to completed run.
- Session cleanup deletes only selected working-directory history.
- `--print-config` emits redacted TOML.
- `--doctor --json` emits valid report JSON.

Opt-in real runtime tests:

- `MULTIAGENT_TEST_CODEX=1` enables real Codex checks.
- `MULTIAGENT_TEST_ZAI=1` enables real Z.ai checks.

Real runtime tests are ignored by default.

## Alternatives Considered

### Multi-Crate Workspace

Rejected for v1. A workspace would create cleaner long-term boundaries, but one package with internal modules is simpler to install and refactor while the architecture is still settling.

### Let Codex Edit Files Directly

Rejected. It would make capability enforcement mostly prompt-based and would conflict with Reviewer/Explorer restrictions. The harness must execute actions itself.

### Persist AppEvent Directly

Rejected. Internal events include terminal noise and transient state. Durable `HistoryEvent`s provide a cleaner replay and history contract.

### SQLite History

Rejected for v1. JSON/JSONL is easier to inspect, stream, append, and repair manually.

### `--print-config --json`

Rejected for v1. `--print-config` prints redacted TOML only because config is authored as TOML. Machine-readable diagnostics are handled by `--doctor --json`.

### Default Normal Approval Mode

Rejected by product decision. Default is yolo for speed, with capability enforcement, path scope, limits, and hard denies still active.

## Risks and Mitigations

- Risk: Codex CLI invocation differs across installed versions.
  Mitigation: Make `command`, `args`, and stdin prompt mode configurable; avoid hardcoding a specific subcommand beyond defaults.

- Risk: Model output does not follow JSON contracts.
  Mitigation: Delimited JSON blocks, parse-error agent results, raw output artifacts, and one repair retry.

- Risk: Yolo mode allows unintended actions.
  Mitigation: Yolo skips prompts only; capabilities, path scope, limits, hard denies, and session cleanup confirmation remain enforced.

- Risk: Command classifier misses dangerous shell behavior.
  Mitigation: Conservative built-in deny patterns, exact command display, redaction, and future config allow/deny extensions.

- Risk: History leaks sensitive workspace content.
  Mitigation: Working-directory scoped reads by default, private file permissions, opt-in debug logs, and source-based secret redaction.

- Risk: Runtime/action loops become runaway.
  Mitigation: run limits, step limits, command limits, cancellation tokens, and explicit limit-reached states.

- Risk: Config merge behavior surprises users.
  Mitigation: `--print-config` redacted TOML, deterministic merge rules, and validation errors for ambiguous conflicts.

## Open Questions

- Exact Codex CLI default `args` may need adjustment after testing against the installed Codex CLI version.
- Exact Z.ai streaming behavior should be verified during adapter implementation, though the spec assumes OpenAI-compatible chat completions.
- Command classifier deny/approve patterns will need empirical tightening once real workflows run through the harness.

## Acceptance Criteria

- `cargo install --path .` installs a `multiagent` binary.
- `multiagent` launches a TUI from an arbitrary folder.
- TUI renders Agent Roster, Chat, and Input Composer from `AppState`.
- Built-in config resolves into `EffectiveConfig` without user config.
- Home and local config merge with documented conflict rules.
- `--print-config` prints redacted effective TOML.
- Invalid config blocks launch with a typed diagnostic.
- Missing Codex/Z.ai setup marks runtimes unavailable without blocking TUI launch.
- `--doctor` prints human-readable checks.
- `--doctor --json` prints valid structured diagnostics.
- A fake runtime can complete a prompt through Orchestrator and one specialized agent.
- App core persists JSONL history with `schema_version = 1`.
- Large outputs are stored as artifacts with metadata in referencing events.
- ULIDs are used for persisted session/run/step/event/artifact IDs.
- Runtime action requests are validated before execution.
- Reads and writes are scoped to working directory by default.
- Unified diff patches are validated and applied atomically per file.
- Commands are classified as allow, approve, or deny.
- Default `approval_mode = "yolo"` skips approval prompts but preserves capabilities, path scope, limits, and hard denies.
- Interrupting an active run cancels runtime/action work and moves the run to `interrupted`.
- Malformed runtime output is persisted as an artifact and represented as `parse_error`.
- `--clean-sessions` deletes only selected working-directory session/run history and prompts unless `--yes` is passed.
- Default test suite passes without external runtime credentials.
