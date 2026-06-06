# Technical Specification: Multi-Agent Run

Status: Draft
Date: 2026-06-06
Source PRD: `docs/multi-agent-run/prd.md`

## Executive Summary

This specification defines the implementation plan for Parallel Step Groups in
`atelier`. A Parallel Step Group lets the Orchestrator start multiple
Specialized Agent steps concurrently inside one active Run while preserving the
existing Harness-owned boundaries for runtime execution, Harness Actions,
Action Approval, Session History, TUI state, and run limits.

The core implementation is an app-owned `ParallelGroupDriver`. Runtime work runs
in child `RuntimeTask`s, but the app remains the only owner of state, history,
action execution, approval queues, cancellation, and group join behavior.
Parallel child steps are constrained by exact write file scopes, strict V1
parallel command policy, feature-gated rollout, and deterministic
Harness-synthesized `ParallelGroupResult`s.

## Background / Context

The current run driver is sequential. `OrchestratorDecision` schema version 1
uses `next_agent`, `handle_orchestrator_decision` calls one `run_agent_step`,
and `run_agent_step` uses `execute_runtime_step_with_actions` to drive one
runtime/action loop.

Current implementation constraints:

- `OrchestratorDecision` has `next_agent: Option<String>` and rejects non-v1
  schema versions.
- `AppState` has a single `live_step`.
- `App` has a single `pending_approval`.
- `PausedStep` supports only `Orchestrator` or one `Agent`.
- `RuntimeRequest.previous_results` is `Vec<AgentResult>`.
- `HistoryEvent` has first-class `run_id` and `step_id`, but no `group_id`.
- `validate_action_request` enforces capability and workspace path policy, but
  not per-parallel-child file scope.

Existing stream-mode work is the closest architectural prior art: runtime
adapters emit progress through `RuntimeEventSink`, while the app owns final
state, history, and output handling. Parallel groups should extend that
app-owned model rather than spawning multiple mutable app flows.

References:

- Product requirements: `docs/multi-agent-run/prd.md`
- Concurrency ADR:
  `docs/adr/0004-parallel-step-groups-inside-one-run.md`
- Domain glossary: `CONTEXT.md`
- Stream mode tech spec: `docs/stream-mode/techspec.md`
- Current app core: `src/app/mod.rs`
- Current orchestrator contract: `src/orchestrator/mod.rs`
- Current action boundary: `src/actions/mod.rs`
- Current runtime boundary: `src/runtime/mod.rs`

## Goals

- Add Parallel Step Groups inside one active Run.
- Keep the Orchestrator as the only owner of the Run Plan.
- Preserve one active Run per Harness Session.
- Preserve Harness-owned action execution, capability checks, path checks,
  approvals, history, cancellation, and limits.
- Allow parallel Fixer and Reviewer child steps on disjoint exact write scopes.
- Let the same Agent Profile run more than once concurrently when scopes are
  disjoint.
- Show every active child step as working in Chat.
- Persist chronological parallel events with run, group, step, and agent
  identity.
- Synthesize deterministic `ParallelGroupResult`s after child steps join.
- Keep schema v1 sequential decisions working during migration.
- Feature-gate parallel groups until fake-runtime and TUI behavior are proven.

## Non-Goals

- Multiple active Runs.
- Direct Specialized Agent delegation.
- Dynamic DAG scheduling before a group joins.
- Per-child worktrees or patch merge queues.
- Directory-prefix write scopes in V1.
- Per-child interrupt controls.
- In-group parse repair.
- Parallel build/test/fmt/package/codegen commands in V1.
- External telemetry or analytics.
- Replacing the Runtime API or Action API wholesale.

## Requirements

### Functional Requirements

- The Orchestrator may return a next step that is either one agent step or one
  Parallel Step Group.
- The app validates a Parallel Step Group before starting any child runtime.
- The app rejects parallel groups when `features.parallel_step_groups = false`.
- The app rejects groups larger than `limits.max_parallel_agent_steps`.
- Every parallel child step has a step ID, step label, agent ID, scoped
  instruction, required capabilities, and `ParallelFileScope`.
- Parallel child write scopes are exact file paths and must be disjoint.
- Out-of-scope file actions are denied by Harness policy.
- Parallel child runtime prompts include structured parallel context.
- Parallel child action execution uses the same Harness Action path as
  sequential steps.
- Approval prompts are shown one at a time while unrelated child steps continue.
- Child parse errors become terminal child results and do not trigger in-group
  parse repair.
- A group joins only after every child reaches a terminal state.
- The Harness synthesizes one `ParallelGroupResult` at join.
- Child `AgentResult`s and the joined `ParallelGroupResult` are passed to later
  Orchestrator steps through typed run history context.

### Non-Functional Requirements

- Default tests must remain offline and credential-free.
- Old `.multiagent` JSONL history without `group_id` must still deserialize.
- Parallel execution must not require `Clone` or concurrent mutable ownership of
  `App`.
- Parallel runtime events must remain bounded and coalesced like stream mode.
- Parallel action policy must fail closed on ambiguous paths, overlapping
  scopes, missing scopes, or unknown commands.
- Feature flag rollback must restore sequential-only orchestration without
  deleting old history.

### Success Metrics

- A fake-runtime run can execute two parallel child steps and join successfully.
- Chat shows two active child steps at the same time.
- A child action outside its file scope is denied before filesystem mutation.
- One child waiting for approval does not stop another child from completing.
- A child parse error is persisted and included in the joined group result.
- `git diff --check`, unit tests, and fake-runtime app tests pass after the
  implementation.

## Proposed Design

Add an explicit parallel orchestration layer inside `src/app`:

```text
Orchestrator step
  -> OrchestratorDecision::normalized_next_step()
  -> DecisionNextStep::ParallelGroup(plan)
  -> validate_parallel_group_plan(...)
  -> ParallelGroupDriver::run(...)
      -> spawn child RuntimeTask futures
      -> drain runtime events and outputs
      -> execute child Harness Actions through app-owned policy
      -> queue approvals
      -> update AppState.live_steps
      -> write Session History events
      -> synthesize ParallelGroupResult
  -> append RunStepResult::Agent(child) values
  -> append RunStepResult::ParallelGroup(group)
  -> return to Orchestrator loop
```

Sequential steps may keep using the existing `run_agent_step` path during the
first implementation. Shared lower-level runtime helpers can be extracted
incrementally from `drive_runtime_step_streaming`.

## Architecture / Components

### Orchestrator Contract

Keep one Rust `OrchestratorDecision` type during migration:

```rust
pub struct OrchestratorDecision {
    pub schema_version: u32,
    pub decision_id: String,
    pub run_id: String,
    pub status: DecisionStatus,
    pub plan: Vec<String>,

    // Legacy schema v1 field.
    pub next_agent: Option<String>,

    // Schema v2 field.
    pub next_step: Option<DecisionNextStep>,

    pub reason: String,
    pub required_capabilities: Vec<Capability>,
    pub stop_condition: String,
    pub clarifying_question: Option<String>,
    pub final_summary: Option<String>,
}
```

Add normalized access:

```rust
impl OrchestratorDecision {
    pub fn normalized_next_step(&self) -> Result<Option<DecisionNextStep>>;
}
```

Normalization rules:

- `schema_version = 1` may use `next_agent` and maps it to
  `DecisionNextStep::SingleAgent`.
- `schema_version = 2` must use `next_step`.
- A decision containing both `next_agent` and `next_step` is invalid.
- `parallel_group` decisions are invalid unless `schema_version = 2`.
- `WaitingForUser`, `Complete`, and `Failed` decisions must not contain a next
  step.

Decision next-step types:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DecisionNextStep {
    SingleAgent(SingleAgentStepPlan),
    ParallelGroup(ParallelGroupPlan),
}

pub struct SingleAgentStepPlan {
    pub agent: String,
    pub instruction: Option<String>,
    pub required_capabilities: Vec<Capability>,
}

pub struct ParallelGroupPlan {
    pub group_id: String,
    pub reason: String,
    pub steps: Vec<ParallelChildStepPlan>,
}

pub struct ParallelChildStepPlan {
    pub step_label: String,
    pub agent: String,
    pub instruction: String,
    pub required_capabilities: Vec<Capability>,
    pub file_scope: ParallelFileScope,
}
```

The Orchestrator prompt builder must include parallel-group instructions only
when the feature flag is enabled. When disabled, the prompt should explicitly
say to use sequential steps.

### Parallel Group Driver

Add an app-owned driver in `src/app`, either in `src/app/parallel.rs` or a
submodule under `src/app/mod.rs`:

```rust
struct ParallelGroupDriver {
    run: RunDriveContext,
    group: ParallelGroupPlan,
    children: BTreeMap<String, ParallelChildState>,
    approval_queue: VecDeque<PendingParallelApproval>,
    cancellation: CancellationToken,
    started_at: Instant,
}
```

The driver owns group execution state, but `App` remains the only component
allowed to:

- mutate `AppState`;
- append `HistoryEvent`s;
- execute `ActionRequest`s;
- expose pending approvals;
- synthesize the joined group result;
- write run records.

Child state:

```rust
struct ParallelChildState {
    group_id: String,
    step_id: String,
    step_label: String,
    agent_id: String,
    file_scope: ParallelFileScope,
    request: RuntimeRequest,
    status: ParallelChildStatus,
    started_at: Instant,
    next_runtime_sequence: u32,
    action_count: u32,
    result: Option<AgentResult>,
    cancellation: CancellationToken,
}

enum ParallelChildStatus {
    Queued,
    Starting,
    Running,
    Streaming,
    WaitingForAction,
    WaitingForApproval,
    Completed,
    Blocked,
    Failed,
    Cancelled,
    ParseError,
    LimitReached,
    ApprovalDenied,
}
```

The group driver is a barrier: it returns to the Orchestrator only after every
child has a terminal status.

### Runtime Task Layer

Extract a lower-level runtime task abstraction from the current streaming
driver. A `RuntimeTask` owns a child runtime future, event receiver, and
cancellation token, but does not mutate app state.

```rust
struct RuntimeTask {
    run_id: String,
    group_id: Option<String>,
    step_id: String,
    agent_id: String,
    receiver: mpsc::Receiver<RuntimeEvent>,
    cancellation: CancellationToken,
    output: JoinHandle<Result<RuntimeOutput>>,
}

enum RuntimeTaskEvent {
    RuntimeEvent {
        group_id: Option<String>,
        step_id: String,
        event: RuntimeEvent,
    },
    Output {
        group_id: Option<String>,
        step_id: String,
        output: RuntimeOutput,
    },
    Failed {
        group_id: Option<String>,
        step_id: String,
        diagnostic: String,
    },
}
```

The sequential path may continue to use `drive_runtime_step_streaming` first.
The parallel path should use `RuntimeTask` so multiple runtime futures can be
polled while app-owned code applies side effects.

### App State And Chat

Replace:

```rust
pub live_step: Option<LiveStepView>
```

with:

```rust
pub live_steps: Vec<LiveStepView>
```

Extend `LiveStepView`:

```rust
pub struct LiveStepView {
    pub run_id: String,
    pub group_id: Option<String>,
    pub step_id: String,
    pub step_label: Option<String>,
    pub file_scope: Option<ParallelFileScope>,
    pub agent: String,
    pub status: LiveStepStatus,
    pub streams: Vec<LiveStreamView>,
}
```

`ChatProjection` should render one transient progress item per live child step
using the existing step lifecycle key. Group-level events render durable group
summary items. When an Agent Profile appears more than once, titles should use
`step_label` or a concise file-scope summary.

Agent Roster status should support multiple active instances for the same Agent
Profile. V1 can show the profile as `running_parallel` with a count and rely on
Chat for per-step labels; a later TUI pass may add instance rows.

### Approval Queue

Keep the user-facing `AppState.pending_approval: Option<PendingApprovalView>`,
but replace the internal single pending value with a queue while a group is
active:

```rust
enum PendingApproval {
    Sequential(PendingSequentialApproval),
    Parallel(PendingParallelApproval),
}

struct PendingParallelApproval {
    run_id: String,
    group_id: String,
    step_id: String,
    action_request: ActionRequest,
    agent_profile: AgentProfile,
    context: ActionExecutionContext,
    reason: Option<String>,
}
```

The group driver exposes only the queue head to the TUI. When the user answers:

- approved: execute the action with approval mode forced to `Yolo`, push the
  `ActionResult` into that child `RuntimeRequest.action_results`, then resume
  only that child;
- denied: push `ActionResult::approval_denied`, resume only that child;
- unrelated children continue while the queue waits.

### Run Context

Introduce typed run step results:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RunStepResult {
    Agent { result: AgentResult },
    ParallelGroup { result: ParallelGroupResult },
}
```

Change:

- `RunDriveContext.previous_results: Vec<AgentResult>`
- `RuntimeRequest.previous_results: Vec<AgentResult>`
- `RunRecord.results: Vec<AgentResult>`

to use `Vec<RunStepResult>`.

Migration helpers:

- `agent_results(results: &[RunStepResult]) -> impl Iterator<Item = &AgentResult>`
- `last_agent_result(...)`
- `review_fix_cycle_count(...)` traverses child agent results and ignores group
  result summaries.

## Data Model and Contracts

### Parallel File Scope

V1 scope type:

```rust
pub struct ParallelFileScope {
    pub write_files: Vec<String>,
    pub read_roots: Vec<String>,
}
```

Rules:

- Paths are normalized through existing model-path validation rules.
- Relative paths are interpreted relative to the Working Directory.
- Absolute paths are allowed only if they are already allowed by workspace
  extra roots.
- `write_files` are exact file paths, not directories or prefixes.
- `apply_patch` may touch only files listed in `write_files`.
- `write_file` may create only files listed in `write_files`.
- `read_file` may target a write file or a path under `read_roots`.
- `list_files` and `search_text` may target only paths under `read_roots`.
- `.` is disallowed for a parallel child unless the child is the only group
  member and the scope is explicitly whole-repo read-only.
- A child cannot expand its own scope. Out-of-scope actions are denied and the
  child should return `Blocked`.

Group validation rejects:

- duplicate `write_files` inside one child;
- write files shared by two children;
- missing write files for an edit-capable child;
- non-normalized paths;
- unknown agents;
- disabled agents;
- required capabilities absent from the Agent Profile;
- group size above `max_parallel_agent_steps`;
- group size below two.

### Action Scope

Extend `ActionExecutionContext`:

```rust
pub enum ActionScope {
    Unrestricted,
    ParallelFileScope(ParallelFileScope),
}

pub struct ActionExecutionContext {
    pub working_directory: PathBuf,
    pub workspace: WorkspacePolicy,
    pub approval_mode: ApprovalMode,
    pub command_timeout: Option<Duration>,
    pub user_prompt: Option<String>,
    pub action_scope: ActionScope,
}
```

`validate_action_request` should perform scope validation after request schema
and capability/tool checks, and before returning `Allowed`.

Parallel command policy:

- Allow only read-only inspection commands in V1:
  `rg`, `git status`, `git diff`, `git log`, `git show`, `git grep`,
  `git blame`, `ls`, `pwd`, `sed -n`, `cat`, `grep`, `find`, `wc`,
  `atelier --doctor`, `atelier --print-config`, `atelier --help`, and
  `atelier --version`.
- Continue to reject shell control syntax.
- Deny or require follow-up outside the group for `cargo fmt`, `cargo test`,
  `cargo check`, `cargo clippy`, `cargo build`, installs, package-manager
  commands, codegen, migrations, and VCS mutations.
- Use the existing `Denied` action result shape with a diagnostic such as
  `command is not allowed inside a parallel step group; schedule after group
  join`.

### Runtime Request Parallel Context

Extend `RuntimeRequest`:

```rust
pub struct RuntimeRequest {
    // existing fields...
    pub previous_results: Vec<RunStepResult>,
    pub parallel_context: Option<ParallelRuntimeContext>,
}

pub struct ParallelRuntimeContext {
    pub group_id: String,
    pub step_label: String,
    pub file_scope: ParallelFileScope,
    pub parallel_siblings: Vec<ParallelSiblingContext>,
    pub scope_policy_summary: String,
}

pub struct ParallelSiblingContext {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
}
```

Every runtime prompt envelope should serialize `parallel_context` when present.
This is runtime guidance only; hard enforcement remains in `ActionScope`.

### Parallel Group Result

Harness-synthesized result:

```rust
pub struct ParallelGroupResult {
    pub schema_version: u32,
    pub group_id: String,
    pub run_id: String,
    pub status: ParallelGroupStatus,
    pub summary: String,
    pub children: Vec<ParallelChildResultRef>,
    pub counts: BTreeMap<String, u32>,
    pub changed_files: Vec<String>,
    pub blocked_scopes: Vec<ParallelBlockedScope>,
    pub failed_scopes: Vec<ParallelFailedScope>,
    pub approval_denials: Vec<String>,
    pub started_at: String,
    pub completed_at: String,
}

#[serde(rename_all = "snake_case")]
pub enum ParallelGroupStatus {
    Completed,
    CompletedWithIssues,
    Failed,
    Cancelled,
    LimitReached,
}

pub struct ParallelChildResultRef {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub status: AgentResultStatus,
    pub result_index: usize,
}

pub struct ParallelBlockedScope {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub blocker: String,
}

pub struct ParallelFailedScope {
    pub step_id: String,
    pub step_label: String,
    pub agent: String,
    pub file_scope: ParallelFileScope,
    pub diagnostic: String,
}
```

Group status mapping:

- `Completed`: all children are `Completed` or `NoChanges`.
- `CompletedWithIssues`: at least one child is `Blocked`, `ParseError`,
  `ApprovalDenied`, or `Failed`, but at least one useful child result completed.
- `Failed`: every child failed, blocked, or parse errored.
- `Cancelled`: run interrupt cancelled the group.
- `LimitReached`: wall-clock, step, action, or agent-step limit stopped the
  group.

### History Event Schema

Add first-class group identity:

```rust
pub struct HistoryEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub group_id: Option<String>,
    pub step_id: Option<String>,
    pub timestamp: String,
    pub kind: String,
    pub payload: Value,
    pub payload_truncated: bool,
}
```

Deserialization must default missing `group_id` to `None` for old JSONL
history. `HistoryEvent::new` can keep its current signature and call a new
`HistoryEvent::new_with_group` internally, or the code can add a builder to
avoid long argument lists.

## APIs / Events

Add explicit group lifecycle events:

- `parallel_group_started`
- `parallel_group_rejected`
- `parallel_child_started`
- `parallel_child_blocked`
- `parallel_child_completed`
- `parallel_child_failed`
- `parallel_group_joined`

Required common payload fields:

```json
{
  "group_id": "group-id",
  "step_id": "child-step-id",
  "agent": "fixer",
  "step_label": "fix runtime scope",
  "file_scope": {
    "write_files": ["src/runtime/mod.rs"],
    "read_roots": ["src/runtime"]
  },
  "status": "completed"
}
```

Event rules:

- `parallel_group_started` has `group_id` and no child `step_id`.
- `parallel_child_*` events have both `group_id` and `step_id`.
- Child runtime stream, action, approval, command, file edit, and result events
  include `group_id`.
- `parallel_group_joined` includes or references the `ParallelGroupResult`.
- `agent_result` is still emitted for each child result.
- `parallel_group_rejected` is emitted when validation rejects the group before
  starting any child runtime.

## Security and Privacy

- File-scope enforcement is a hard Harness Action policy, not a prompt rule.
- Parallel write scopes fail closed on ambiguous paths.
- Absolute paths remain blocked unless already allowed by workspace extra roots.
- VCS Actions remain denied unless explicitly requested by the user and are not
  allowed inside Parallel Step Groups in V1.
- Approval prompts remain one-at-a-time to avoid user confusion.
- Runtime prompt envelopes may include file paths and sibling scopes, but not
  file contents beyond existing runtime context behavior.
- Parallel events and group results may contain user file paths and summaries,
  so they follow existing Session History privacy behavior.
- No external telemetry is added.

## Performance and Reliability

- `max_parallel_agent_steps` bounds active child runtime tasks.
- Each child runtime keeps the existing bounded runtime event channel.
- The app coalesces runtime stream history as stream mode already does.
- Each child has its own cancellation token; the group has a parent token.
- Run interrupt cancels the parent token and all child tokens.
- Wall-clock run limit applies to the whole group.
- Step time limit applies per child.
- Step action limit applies per child.
- Every child step counts against `max_agent_steps`.
- Runtime/model fallback happens independently per child and counts against the
  same parallel concurrency bound.
- A child parse error, action denial, or runtime failure does not cancel sibling
  children unless a run-level limit, cancellation, or shared safety failure
  occurs.

## Observability

Parallel observability stays local:

- Session History records all group and child lifecycle events.
- Debug log mirrors those events when debug logging is enabled.
- Chat projection renders child progress and joined group summaries.
- Group result summaries include counts by status, changed files, blocked
  scopes, failed scopes, and approval denials.
- No analytics or remote telemetry are introduced.

The debug story should let a maintainer answer:

- Which Orchestrator decision started the group?
- Which child owned a file path?
- Which action was denied and why?
- Which child waited for approval?
- Which child produced parse errors?
- Why did the group status become `CompletedWithIssues` or `LimitReached`?

## Migration and Rollout

### Config

Add:

```toml
[features]
parallel_step_groups = false

[limits]
max_parallel_agent_steps = 2
```

`max_parallel_agent_steps` should be parsed as a non-negative integer rather
than the existing positive-only `Limit` type if `0` is used as an explicit
disable signal. The feature flag is the primary rollout control.

### Phases

1. **Contracts and config**
   - Add `features.parallel_step_groups`.
   - Add `max_parallel_agent_steps`.
   - Add `DecisionNextStep`, `ParallelFileScope`, `ParallelGroupResult`,
     `RunStepResult`, and `ParallelRuntimeContext`.
   - Keep v1 `next_agent` sequential decisions working.

2. **History and UI shape**
   - Add `HistoryEvent.group_id` with backward-compatible deserialization.
   - Replace `live_step` with `live_steps`.
   - Update Chat projection and TUI tests for multiple live steps.

3. **Action scope**
   - Add `ActionExecutionContext.action_scope`.
   - Enforce exact write scopes and read roots.
   - Add strict V1 parallel command policy.

4. **Runtime task extraction**
   - Extract lower-level `RuntimeTask`.
   - Keep sequential path working.
   - Add fake-runtime tests for multiple runtime tasks.

5. **Parallel group driver**
   - Validate group plans.
   - Start child runtime tasks.
   - Handle action requests, approvals, parse errors, cancellation, limits, and
     joins.
   - Synthesize `ParallelGroupResult`.

6. **Orchestrator prompt and feature enablement**
   - Teach `build_orchestrator_prompt` about `next_step`.
   - Include parallel routing rules only when the feature flag is enabled.
   - Start with fake runtime.
   - Flip built-in default only after full fake/TUI/regression coverage passes.

### Backward Compatibility

- Schema v1 sequential decisions remain accepted.
- Old JSONL history without `group_id` remains readable.
- Existing `AgentResult` JSON remains valid.
- Existing config without `[features]` or `max_parallel_agent_steps` remains
  valid.
- Feature disabled means Orchestrator prompt and validation stay
  sequential-only.

## Testing Strategy

### Unit Tests

- `OrchestratorDecision::normalized_next_step` maps v1 `next_agent`.
- v2 decisions with both `next_agent` and `next_step` are rejected.
- v2 parallel decisions are rejected when the feature flag is disabled.
- Group validation rejects unknown agents, disabled agents, missing scopes,
  overlapping write files, and group sizes above the limit.
- `ParallelFileScope` normalization rejects traversal, ambiguous absolute
  paths, directories in `write_files`, and duplicate write paths.
- `ActionScope::ParallelFileScope` allows exact write targets and rejects
  out-of-scope `write_file` and `apply_patch`.
- Parallel command policy allows inspection commands and rejects `cargo fmt`,
  `cargo test`, installs, and VCS mutations.
- `HistoryEvent` deserializes old entries with missing `group_id`.
- `RunStepResult` serialization remains stable and prompt-envelope JSON includes
  both child agent results and group results.

### App/Fake Runtime Tests

- Fake Orchestrator returns a parallel group and the app starts all children.
- Two fake child steps stream progress before group join.
- Chat state contains two live progress items.
- Two Fixer child steps edit disjoint files.
- Out-of-scope edit is denied and becomes a blocked child result.
- One child waiting for approval does not block another child from completing.
- Approval denial affects only the requesting child.
- Child parse error is persisted as a terminal child result.
- Group joins only after every child reaches terminal state.
- Harness synthesizes `ParallelGroupResult` with child references.
- Interrupt cancels all active children and writes group/child cancellation
  events.
- `max_agent_steps`, `max_parallel_agent_steps`, step action limits, step time
  limits, and wall-clock limits all stop or reject groups deterministically.

### Projection/TUI Tests

- Chat projection renders `parallel_group_started`.
- Chat projection renders each active live child separately.
- Chat projection renders `parallel_group_joined` with status counts.
- Same Agent Profile in two children is distinguishable by step label or file
  scope.
- Pending approval still renders one prompt at a time.

### Regression Tests

- Existing sequential fake-runtime run still passes.
- Existing v1 orchestrator decisions still parse and validate.
- Existing action policy tests still pass in unrestricted sequential context.
- Existing stream-mode tests still pass after `live_steps` migration.

## Alternatives Considered

- **Multiple active Runs**: rejected by the PRD and ADR because it changes TUI,
  history, and prompt-to-run semantics.
- **Spawn multiple `run_agent_step` flows**: rejected because `App` owns mutable
  state, history, approvals, and projections.
- **Payload-only group ID**: rejected because group correlation would become
  brittle across history, Chat, and debug tooling.
- **Directory-prefix write scopes**: rejected for V1 because overlap and merge
  safety become harder.
- **Parallel cargo test/check/fmt in V1**: rejected because these commands can
  mutate shared build/cache/source state and are hard to prove scoped.
- **Model-synthesized group result**: rejected because join artifacts should not
  introduce another parseable model failure point.
- **In-group parse repair**: rejected because it breaks the barrier model and
  complicates child state.

## Risks and Mitigations

- **Risk: app refactor touches broad TUI/state code.**
  Mitigation: migrate `live_step` to `live_steps` before implementing actual
  parallel runtime execution.

- **Risk: v2 decision parsing breaks v1 sequential runs.**
  Mitigation: use one version-tolerant type and `normalized_next_step`.

- **Risk: action scope misses paths inside patches.**
  Mitigation: validate parsed unified diff target paths before applying patches,
  and test multi-file patch denial.

- **Risk: approval queue creates confusing user flow.**
  Mitigation: show only the queue head in the existing pending approval surface
  and include child step label/file scope in the prompt.

- **Risk: parallel children overwhelm runtime providers.**
  Mitigation: keep feature disabled initially and enforce
  `max_parallel_agent_steps` across retries.

- **Risk: strict V1 command policy limits verification usefulness.**
  Mitigation: allow post-join verification steps and revisit scoped parallel
  verification after command-scope modeling improves.

- **Risk: history schema bump breaks old history reads.**
  Mitigation: default missing `group_id` to `None` and add fixtures for old
  events.

## Resolved Implementation Defaults

- Implement `ParallelGroupDriver` in `src/app/parallel.rs` by default.
- In V1, show duplicate active Agent Profiles in the Agent Roster as an
  aggregate `running_parallel` status with an active count; Chat remains the
  per-child source of step labels and file scopes.
- Keep scoped parallel `cargo test`, `cargo check`, and `cargo clippy` out of
  V1. Run those checks after group join until command scoping is explicit enough
  to prove safe parallel execution.

## Acceptance Criteria

- `docs/multi-agent-run/prd.md` and this tech spec agree on one active Run with
  Parallel Step Groups inside it.
- A feature-disabled config rejects parallel group decisions clearly.
- A feature-enabled fake-runtime run executes at least two parallel child steps.
- Every child has enforced exact write file scope.
- Out-of-scope actions are denied before filesystem mutation.
- Chat shows each active child as working.
- One pending approval is visible while unrelated children continue.
- Child parse errors join as terminal child results.
- The Harness writes `parallel_group_started`, child lifecycle events, and
  `parallel_group_joined`.
- `ParallelGroupResult` is synthesized by the Harness and passed to the next
  Orchestrator step through `RunStepResult`.
- Existing v1 sequential decisions and old history files remain compatible.
- Unit and fake-runtime tests cover validation, action scope, approval queue,
  cancellation, limits, history, and Chat projection.
