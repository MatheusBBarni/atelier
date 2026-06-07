# Workflow Command TechSpec

## Executive Summary

Implement `/workflow <prompt>` as a thin app-owned workflow mode inside the existing `Run` loop. The app parses the slash command, preflights Parallel Step Group availability, records `workflow_started`, passes a workflow prompt envelope to the Orchestrator, tracks planned file-edit targets in a `WorkflowRunContext`, and emits `workflow_completed` with `completed`, `completed_with_issues`, or `failed`.

Primary trade-off: this adds a small app-owned target ledger instead of trusting the Orchestrator final summary. That increases app state and tests, but gives the PRD's trustworthy-completion requirement enforceable evidence.

## System Architecture

### Component Overview

| Component | Responsibility |
| --------- | -------------- |
| `App::handle_workflow_command` | Parse `/workflow <prompt>`, preflight workflow prerequisites, and start a workflow-mode Run. |
| `WorkflowRunContext` | Store original command, runtime prompt, target ledger, and workflow evidence while the Run executes. |
| Orchestrator prompt envelope | Tell the Orchestrator to decompose, execute, validate, and account for planned targets. |
| Parallel Step Group path | Existing app-owned concurrent execution and joined result flow. |
| Workflow target ledger | Derive planned targets from `ParallelChildStepPlan.file_scope.write_files` and update target outcomes from child results. |
| History events | Persist `workflow_started` and `workflow_completed`. |
| Chat projection | Render workflow start/completion, including completed-with-issues as warning. |
| Fake runtime tests | Deterministic coverage for workflow happy path, failure path, target accounting, and projection. |

Data flow:

```text
/workflow <prompt>
  -> handle_workflow_command
  -> preflight parallel_step_groups
  -> run_started + workflow_started + prompt_submitted
  -> RunDriveContext { workflow: Some(...) }
  -> Orchestrator receives workflow envelope
  -> Parallel Step Groups execute
  -> WorkflowRunContext records targets and outcomes
  -> workflow_completed + run_completed
```

## Implementation Design

### Core Interfaces

The project is Rust; the core implementation types should live near `RunDriveContext` in `src/app/mod.rs`.

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowRunContext {
    original_command: String,
    user_prompt: String,
    target_ledger: BTreeMap<String, WorkflowTarget>,
    verification: Vec<String>,
    skipped_checks: Vec<String>,
    residual_risks: Vec<String>,
}
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct WorkflowTarget {
    path: String,
    source_group_id: String,
    source_step_id: Option<String>,
    source_step_label: String,
    status: WorkflowTargetStatus,
    reason: Option<String>,
}
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WorkflowTargetStatus {
    Planned,
    Completed,
    Skipped,
    Blocked,
    Failed,
}
```

### Data Models

Add `workflow: Option<WorkflowRunContext>` to `RunDriveContext`.

`WorkflowTarget` keys should use normalized workspace-relative paths. Initial targets come from each parallel child `file_scope.write_files`. Read-only children with empty `write_files` do not create targets.

Target status mapping:

| Source | Target status |
| ------ | ------------- |
| Child `Completed` or `NoChanges` | `Completed` for that child's write targets. |
| Child `Blocked` | `Blocked` with blocker or summary. |
| Child `ApprovalDenied` | `Blocked` with approval denial reason. |
| Child `Failed`, `ParseError`, `LimitReached`, `Cancelled` | `Failed` with diagnostic. |
| Explicit Orchestrator final evidence for not doing a target | `Skipped`. |

Add `WorkflowCompletionStatus` for event payloads:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WorkflowCompletionStatus {
    Completed,
    CompletedWithIssues,
    Failed,
}
```

Completion rules:

- `completed`: every planned target is `Completed`.
- `completed_with_issues`: at least one target is `Skipped`, `Blocked`, or `Failed`, and every target has a terminal status.
- `failed`: workflow cannot produce valid target evidence, is interrupted, or cannot account for planned targets.

### Event Payloads

`workflow_started`:

```json
{
  "run_id": "...",
  "original_command": "/workflow migrate auth module",
  "user_prompt": "migrate auth module",
  "mode": "workflow",
  "preflight": {
    "parallel_step_groups": true,
    "max_parallel_agent_steps": 2
  }
}
```

`workflow_completed`:

```json
{
  "run_id": "...",
  "status": "completed_with_issues",
  "target_counts": {
    "completed": 2,
    "skipped": 0,
    "blocked": 1,
    "failed": 0
  },
  "unfinished_targets": [],
  "verification": ["cargo test -p app"],
  "skipped_checks": [],
  "residual_risks": ["Reviewer did not inspect docs scope."]
}
```

## Integration Points

- [src/app/mod.rs](/Users/matheusbbarni/projects/multiagent-harness/src/app/mod.rs:510): add `handle_workflow_command` before unknown slash-command rejection.
- [src/app/mod.rs](/Users/matheusbbarni/projects/multiagent-harness/src/app/mod.rs:229): extend `RunDriveContext`.
- [src/app/mod.rs](/Users/matheusbbarni/projects/multiagent-harness/src/app/mod.rs:1051): update workflow ledger when parallel groups start/join.
- [src/orchestrator/mod.rs](/Users/matheusbbarni/projects/multiagent-harness/src/orchestrator/mod.rs:50): reuse existing `DecisionNextStep`, `ParallelGroupPlan`, and `ParallelFileScope`.
- [src/app/chat/projection.rs](/Users/matheusbbarni/projects/multiagent-harness/src/app/chat/projection.rs:48): project `workflow_started` and `workflow_completed`.
- [src/runtime/fake.rs](/Users/matheusbbarni/projects/multiagent-harness/src/runtime/fake.rs:174): extend deterministic workflow fixtures.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
| --------- | ----------- | -------------------- | --------------- |
| `src/app/mod.rs` | Modified | Adds command parsing, workflow context, target accounting, completion event. Medium risk. | Implement workflow path and ledger helpers. |
| `src/orchestrator/mod.rs` | Modified | May need workflow prompt guidance or helper text. Low risk. | Add workflow envelope instructions without changing schema. |
| `src/app/chat/projection.rs` | Modified | Needs warning rendering for completed-with-issues. Low risk. | Add workflow event projection tests. |
| `src/runtime/fake.rs` | Modified | Needs deterministic workflow cases. Low risk. | Add fake prompt branches for workflow mode. |
| `src/tui/mod.rs` | Modified | Help text only. Low risk. | Add `/workflow <prompt>` to help. |
| `.compozy/tasks/workflow-command` | New docs | Planning artifacts only. Low risk. | Keep PRD/TechSpec/ADRs aligned. |

## Testing Approach

### Unit Tests

- Parse `/workflow <prompt>` and reject `/workflow` with no prompt.
- Preflight rejects when `features.parallel_step_groups = false`.
- Preflight rejects when `max_parallel_agent_steps = 0`.
- Ledger derives targets only from `write_files`.
- Ledger ignores read-only reviewer children.
- Target status mapping covers completed, no-changes, blocked, approval-denied, failed, parse-error, limit-reached, and cancelled.
- Completion status derives from target counts.

### Integration Tests

Use fake runtime app tests.

- `/workflow parallel scoped write action create a feature` records `workflow_started`.
- Happy path records completed targets and `workflow_completed.status = completed`.
- Approval denial or parse error records `workflow_completed.status = completed_with_issues`.
- Disabled Parallel Step Groups fails before `run_started`.
- ChatProjection renders `workflow_completed.completed_with_issues` with warning severity.
- Existing non-workflow prompts remain unchanged.

No real Codex, Claude, Cursor, or Z.ai runtime smoke tests are required for V1.

## Development Sequencing

### Build Order

1. Add workflow data types and parser helpers - no dependencies.
2. Add `handle_workflow_command` and prerequisite failure path - depends on step 1.
3. Add workflow prompt envelope and `workflow_started` event - depends on step 2.
4. Add `WorkflowRunContext` to `RunDriveContext` - depends on step 1.
5. Derive planned targets when Parallel Step Groups start - depends on steps 3 and 4.
6. Update target outcomes when child/group results join - depends on step 5.
7. Emit `workflow_completed` before generic run completion finalization - depends on step 6.
8. Add ChatProjection support for workflow events - depends on step 7.
9. Extend fake runtime workflow fixtures - depends on steps 3 and 5.
10. Add unit and integration tests - depends on steps 1-9.
11. Update TUI help text and docs references - depends on step 2.

### Technical Dependencies

- Parallel Step Groups must be enabled for workflow execution.
- Existing Harness Action enforcement remains authoritative.
- Existing fake runtime remains the primary deterministic test runtime.

## Monitoring and Observability

- History events: `workflow_started`, `workflow_completed`.
- Workflow completion payload includes status, target counts, unfinished targets, verification, skipped checks, residual risks.
- Chat should show workflow completed-with-issues as warning.
- No external telemetry in V1.

## Technical Considerations

### Key Decisions

- **Decision:** App-owned target ledger.
  **Rationale:** Completion evidence must be enforceable, not only a model summary.
  **Trade-off:** More app state, stronger trust.

- **Decision:** Preserve original command in history and use runtime prompt envelope internally.
  **Rationale:** History stays auditable while the Orchestrator gets workflow-specific instructions.
  **Trade-off:** App must carry both visible and runtime prompt forms.

- **Decision:** Dedicated `workflow_completed` event with `completed_with_issues`.
  **Rationale:** Avoid broad `RunState` change while rendering workflow status clearly.
  **Trade-off:** Adds a workflow-specific terminal event.

### Known Risks

- Target accounting can drift if targets are derived from changed files; derive from planned `write_files`.
- `NoChanges` may be ambiguous; treat it as completed only when the child's scoped instruction covered that target.
- Chat can show duplicate completion messages; make `workflow_completed` the evidence-rich item and keep `run_completed` generic.
- Prompt envelope behavior can be brittle; fake runtime tests must prove app-side completion accounting independently.

## Architecture Decision Records

- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - Keeps `/workflow` inside one normal Run and requires workflow evidence.
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - Selects executing workflow mode over plan-gated or investigation-only variants.
- [ADR-003: App-Owned Workflow Target Ledger](adrs/adr-003.md) - Tracks planned file-edit target outcomes in app-owned run state.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - Uses `workflow_started` and `workflow_completed` while keeping `RunState::Completed`.
