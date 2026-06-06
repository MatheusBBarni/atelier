# Multi-Agent Run PRD

## Status

Product requirements document for parallel specialized agent steps inside one
active `atelier` run.

## Problem Statement

The harness already models a run as one prompt handled by the Orchestrator and
then delegated to one or more Specialized Agents over time. In practice, the
current execution contract is sequential: an Orchestrator Decision names one
`next_agent`, the app runs one agent step, records one Agent Result, and then
asks the Orchestrator what to do next.

That sequential model is safe, but it leaves obvious throughput on the table
when work can be split across independent files. A prompt may need multiple
Fixers or Reviewers to work on disjoint file scopes, and the user should be
able to see those agents working in Chat at the same time without starting
multiple runs or losing the existing Orchestrator-owned control loop.

## Solution

Add Parallel Step Groups inside one active Run. The Orchestrator may choose a
Parallel Step Group when independent file scopes can move forward concurrently.
The Harness still has at most one active Run in the Harness Session; parallelism
happens inside that Run as concurrent specialized agent steps.

Each parallel step has its own step ID, agent profile, scoped instruction,
required capabilities, and Parallel File Scope. The Harness validates that file
scopes are disjoint before the group starts and enforces those scopes while the
group runs. The group joins only after every child step finishes, fails, blocks,
or is cancelled. The Orchestrator then receives a joined Parallel Group Result
plus the child Agent Results and decides the next step.

## Goals

- Allow the Orchestrator to start multiple specialized agent steps concurrently
  inside one active Run.
- Keep the one prompt, one Run, one Orchestrator-owned Run Plan model.
- Support parallel Fixer and Reviewer work when file scopes are disjoint.
- Allow multiple concurrent steps using the same Agent Profile, such as two
  Fixer steps on different files.
- Preserve Harness-owned Capability Enforcement, Harness Actions, Action
  Approval, run limits, cancellation, and Session History.
- Show each active parallel Specialized Agent as working in Chat.
- Bound concurrency through Harness Configuration.
- Store enough history to replay chronological parallel activity and understand
  the joined group outcome.

## Non-Goals

- Multiple active Runs in one Harness Session.
- Specialized Agents delegating directly to other Specialized Agents.
- Dynamic DAG scheduling where partial parallel results start new work before
  the current group joins.
- Isolated worktrees or per-agent sandboxes in the first implementation.
- Agents expanding their own file scope while a parallel group is running.
- Parallel execution of mutation-capable or project-wide commands such as
  formatters, dependency installs, code generation, migrations, or whole-suite
  tests.
- Per-agent interrupt controls inside a parallel group.
- Automatic commits, pushes, branches, or other VCS Actions.

## Users

The primary user is a developer running `atelier` in a local repository. They
want the Harness to split independent work across Specialized Agents when that
improves throughput, while still keeping routing, edits, review findings,
approvals, and verification understandable in the terminal.

## Core Concepts

`CONTEXT.md` is the source of truth for canonical domain language.

- Parallel Step Group: a set of specialized agent steps that the Orchestrator
  starts concurrently inside one Run.
- Parallel File Scope: the file paths assigned to one specialized agent step
  inside a Parallel Step Group.
- Parallel Group Result: the typed output envelope returned after a Parallel
  Step Group joins and summarizes its child Agent Results.

## Product Behavior

### Run Model

A Prompt still creates exactly one Run. The Harness still has at most one active
Run at a time. Parallel Step Groups do not create parallel Runs, nested Runs, or
sub-runs.

The Orchestrator owns the Run Plan and chooses whether the next step is a
single Specialized Agent step or a Parallel Step Group. Parallelism is optional:
the Orchestrator may choose sequential execution when ordering, safety, review
quality, or user clarity matters more than throughput.

### Scope Selection

Every parallel step must have a Parallel File Scope before the group starts.
For non-trivial code-change work, Explorer should identify likely scopes first
unless the user provides explicit file scopes in the prompt.

The Orchestrator may infer file scopes from Explorer findings when file
boundaries are clear. If scopes are missing, overlapping, or high-risk, the
Orchestrator must ask a Clarifying Question before starting the Parallel Step
Group.

The Harness must validate that no file path belongs to more than one Parallel
File Scope in the same group. Invalid or overlapping scopes fail before any
parallel step starts.

### Parallel Agents

A Parallel Step Group may contain built-in or custom enabled Agent Profiles.
The same Agent Profile may appear more than once in a group, as long as each
step has a distinct step ID and disjoint Parallel File Scope.

Each parallel step receives:

- Shared run context.
- Relevant prior Session History and previous results.
- The user Prompt.
- The step's Agent Profile.
- A narrowed per-step instruction.
- The step's Parallel File Scope.
- Capability constraints derived from its profile and scope.
- The requested output schema.

Parallel Fixer steps may edit files only inside their assigned file scopes.
Parallel Reviewer steps review only their assigned file scopes. A combined-diff
Reviewer pass after the group is not automatic; the Orchestrator chooses it as
the next step when needed.

### Harness Actions And Approvals

Parallel agents use the existing Harness Action path. They do not read, edit,
run commands, or perform verification outside Harness-owned enforcement.

If a parallel agent requests a file action outside its Parallel File Scope, the
Harness blocks the action. The agent result should report `Blocked` with the
requested path and reason. The Orchestrator handles scope changes after the
group joins by widening scopes, asking a Clarifying Question, scheduling a new
step, or stopping.

Parallel agents may run scoped read-only verification commands for their file
scope. Mutation-capable or project-wide commands must run outside the Parallel
Step Group after the group joins.

High-impact Action Approval requests are handled one at a time by the Harness.
The parallel step waiting for approval pauses while unrelated parallel steps may
continue. If approval is denied, only the requesting step receives
`ApprovalDenied` unless the denial invalidates the whole group.

### Failure And Join Semantics

A Parallel Step Group is a barrier in the first implementation. The Orchestrator
does not consume partial results or schedule new work until every child step has
reached a terminal state.

A blocked or failed step does not cancel other independent steps by default.
Other steps continue unless the failure invalidates shared assumptions,
violates safety, reaches a Run Limit, or triggers run-level cancellation.

When the group joins, the Harness records a Parallel Group Result that includes:

- Group ID.
- Run ID.
- Child step IDs.
- Agent IDs and display labels.
- Parallel File Scopes.
- Per-step status.
- Child Agent Result references.
- Group start and end timestamps.
- Join summary.
- Whether follow-up Orchestrator action is required.

Child Agent Results remain normal typed results. The Parallel Group Result
summarizes and references them; it does not replace them.

### Limits And Runtime Behavior

Harness Configuration must include a concurrency bound for parallel agent
steps, such as `max_parallel_agent_steps`. Built-in defaults should be
conservative, with `2` as the recommended default.

The Harness enforces the concurrency bound across initial steps and runtime
fallback retries. Runtime and model fallback happens independently for each
parallel step according to that step's Agent Profile. A failure in one runtime
does not pause unrelated steps unless the Harness detects a shared runtime
availability problem that would make the rest predictably fail.

Run Limits continue to apply to the whole Run. Step limits apply to each
parallel step. Review-fix cycle limits should count parallel Fixer and Reviewer
results in a way that prevents unbounded loops after a group joins.

### Cancellation

Interrupt remains run-level in the first implementation. If the user interrupts
while a Parallel Step Group is running, the Harness cancels every active child
step in that group and records coherent cancellation events.

Per-agent cancellation inside a Parallel Step Group is future work.

## Orchestrator Contract

The current Orchestrator Decision contract names one `next_agent`. This feature
requires a schema evolution so the Orchestrator can explicitly select either a
single agent step or a Parallel Step Group.

The contract should move toward a next-step shape such as:

```json
{
  "schema_version": 2,
  "decision_id": "decision-id",
  "run_id": "run-id",
  "status": "continue",
  "plan": ["..."],
  "next_step": {
    "kind": "parallel_group",
    "group_id": "group-id",
    "reason": "These file scopes can move independently.",
    "steps": [
      {
        "step_label": "fix src/runtime",
        "agent": "fixer",
        "instruction": "Apply the runtime changes in this scope.",
        "required_capabilities": ["read_files", "edit_files", "run_commands"],
        "file_scope": ["src/runtime/mod.rs", "src/runtime/fake.rs"]
      },
      {
        "step_label": "review src/app",
        "agent": "reviewer",
        "instruction": "Review this scope for regressions and missing tests.",
        "required_capabilities": ["read_files", "run_commands"],
        "file_scope": ["src/app/mod.rs"]
      }
    ]
  },
  "stop_condition": "All parallel steps have terminal results.",
  "clarifying_question": null,
  "final_summary": null
}
```

A single-agent decision can use the same `next_step` envelope with
`kind = "single_agent"` and one agent payload. During migration, the Harness may
support schema version 1 for sequential decisions, but Parallel Step Groups
must use the new explicit structure.

Validation must reject:

- Unknown, disabled, or unavailable agents.
- Missing file scopes.
- Overlapping file scopes.
- Required capabilities not granted by the selected Agent Profile.
- Parallel group sizes above the configured concurrency limit.
- Parallel write steps without disjoint file scopes.
- Parallel commands that are mutation-capable or project-wide.

## TUI Requirements

Chat must show each active Specialized Agent as working during a Parallel Step
Group. The user should be able to tell which agent profile is running, which
file scope it owns, and whether it is streaming output, waiting for approval,
blocked, failed, completed, or cancelled.

The first UI can render one group-level Chat item with child live blocks, or
separate adjacent agent-working items, as long as each active agent is visible
and streams do not collapse into one ambiguous transcript.

The Agent Roster should show parallel active state for every participating
agent. When the same Agent Profile appears more than once, labels should include
the step label or file scope so two concurrent Fixer steps are distinguishable.

The Input Composer controls the active Run. It does not start a second Run while
the Parallel Step Group is running. Interrupt cancels the active Run and all
current parallel steps.

## Session History

Session History must preserve chronological parallel events with enough identity
to reconstruct the run:

- Run ID.
- Group ID.
- Step ID.
- Agent ID.
- Step label.
- Parallel File Scope.
- Event kind and timestamp.
- Action IDs and approval IDs where applicable.

The Harness also records group-start and group-joined events. The joined event
contains or references the Parallel Group Result.

Context Resume should treat a completed Parallel Step Group as prior run
history. It does not resume child runtime processes exactly.

## Configuration

Add a concurrency limit under Harness Configuration. Exact TOML naming belongs
in the technical specification, but the product behavior should support:

```toml
[limits]
max_parallel_agent_steps = 2
```

The limit applies after built-in profiles, Harness Configuration, Local
Configuration, and command-line overrides are merged into the Effective
Configuration.

If the configured value is missing, use a conservative built-in default. If the
value is zero, the Harness should disable Parallel Step Groups and require
sequential execution.

## User Stories

1. As a developer, I want the Orchestrator to run independent agent steps in parallel, so that disjoint file work completes faster.
2. As a developer, I want the Harness to keep one active Run while agents work in parallel, so that the TUI remains understandable.
3. As a developer, I want parallel Fixers to work on different files, so that independent edits do not need to wait for each other.
4. As a developer, I want parallel Reviewers to review their assigned file scopes, so that review work can be split safely.
5. As a developer, I want each parallel agent's file scope to be visible, so that I know what it is allowed to touch.
6. As a developer, I want Chat to show each agent working, so that parallel execution is visible rather than hidden behind one spinner.
7. As a developer, I want the same Agent Profile to run more than once in parallel when scopes are disjoint, so that two Fixers can handle separate slices.
8. As a developer, I want the Harness to block out-of-scope file actions, so that parallel agents cannot collide.
9. As a developer, I want scoped read-only verification commands to run in parallel, so that independent checks can finish sooner.
10. As a developer, I want project-wide mutation commands to wait until after the group joins, so that shared state is not changed unpredictably.
11. As a developer, I want approval requests to stay clear even when multiple agents are active, so that I can make one decision at a time.
12. As a developer, I want unrelated parallel agents to continue while one agent waits for approval, so that the whole group does not stall unnecessarily.
13. As a developer, I want blocked parallel steps to report their requested path and reason, so that the Orchestrator can decide whether to widen scope or retry.
14. As a developer, I want a failed parallel step not to cancel independent work by default, so that useful results are preserved.
15. As a developer, I want interrupt to cancel the whole active Run, so that I have one clear emergency stop.
16. As a developer, I want the Orchestrator to ask a Clarifying Question when file scopes are unclear, so that unsafe parallel work does not start.
17. As a developer, I want a final joined summary for the parallel group, so that I can understand what happened after all agents finish.
18. As a maintainer, I want parallel group history to preserve chronological events and group identity, so that replay and debugging remain accurate.
19. As a maintainer, I want the Orchestrator contract to explicitly represent a parallel group, so that the app does not infer concurrency from prose.
20. As a maintainer, I want concurrency bounded by configuration, so that parallel runtime calls do not overwhelm the user's machine or provider limits.
21. As a maintainer, I want child Agent Results to remain normal typed results, so that existing review, history, and Orchestrator inputs can evolve incrementally.
22. As a maintainer, I want the first implementation to avoid worktree merging, so that scope enforcement can be proven before adding merge complexity.

## Implementation Decisions

- Add a first-class Parallel Step Group concept inside the Run drive loop.
- Preserve one active Run per Harness Session.
- Evolve Orchestrator Decision from `next_agent` to an explicit `next_step`
  structure that supports `single_agent` and `parallel_group`.
- Keep schema version 1 sequential decisions compatible during migration if
  practical, but require the new schema for parallel groups.
- Add typed structures for parallel group member steps, Parallel File Scopes,
  and Parallel Group Results.
- Validate parallel group shape before any child step starts.
- Enforce file scope at the Harness Action layer, not only through prompt text.
- Allow parallel Fixer edits only when file scopes are disjoint.
- Allow parallel Reviewers as slice reviewers for their assigned scopes.
- Keep combined-diff review as a follow-up Orchestrator decision.
- Run parallel steps in the same Working Directory for the first
  implementation.
- Do not add isolated worktrees, merge queues, or patch reconciliation in this
  PRD.
- Treat a Parallel Step Group as a barrier: the Orchestrator receives the joined
  result after every child step reaches a terminal state.
- Keep child Agent Results independently persisted and parseable.
- Record a joined Parallel Group Result that references child results.
- Keep Action Approval serialized through the Harness while allowing unrelated
  parallel steps to continue.
- Apply runtime/model fallback independently per child step.
- Enforce `max_parallel_agent_steps` across active child steps and retries.
- Keep interrupt run-level and cancel all active child steps together.
- Render every active child agent as working in Chat.
- Include group ID and step ID in runtime stream, action, approval, result, and
  history events associated with parallel work.

## Testing Decisions

- Test Orchestrator Decision parsing and validation for `single_agent` and
  `parallel_group` next-step shapes.
- Test validation rejects overlapping file scopes before any child runtime is
  started.
- Test validation rejects group sizes above `max_parallel_agent_steps`.
- Test capability validation per child step.
- Test Harness Action enforcement blocks out-of-scope reads, edits, and scoped
  commands.
- Test two Fixer steps can edit disjoint files and produce separate Agent
  Results.
- Test two steps using the same Agent Profile remain distinguishable by step ID
  and file scope.
- Test a blocked child step does not cancel independent children by default.
- Test group join waits for all terminal child states.
- Test the joined Parallel Group Result references all child Agent Results.
- Test approval handling pauses only the requesting child step when unrelated
  steps can continue.
- Test run-level interrupt cancels all active child steps and records coherent
  cancellation events.
- Test Chat projection renders each active parallel agent as working.
- Test Agent Roster state when the same agent profile has multiple active
  parallel steps.
- Test chronological Session History contains group ID, step ID, agent ID, and
  file scope for parallel events.
- Test Context Resume includes prior Parallel Group Results as history rather
  than attempting to resume child runtime processes.
- Test runtime/model fallback remains per child step while respecting the global
  concurrency limit.
- Use the fake runtime first for deterministic parallel execution tests before
  relying on Codex, Claude, Cursor, or Z.ai runtimes.

## Out of Scope

- Replacing the existing Agent Result contract for child steps.
- Letting runtime adapters bypass Harness Actions for file edits or commands.
- Letting Specialized Agents ask users Clarifying Questions directly.
- Adding per-agent pause, resume, or cancel controls.
- Automatically resolving merge conflicts between parallel write scopes.
- Parallelizing VCS Actions.
- Running full-suite verification inside the parallel group when it mutates or
  depends on shared state.
- Provider-specific parallel scheduling beyond the runtime boundary.

## Further Notes

The important product rule is that parallelism improves throughput without
changing ownership. The Orchestrator owns the Run Plan, the Harness owns action
execution and policy enforcement, and Specialized Agents return typed results.

The first valuable milestone is a fake-runtime run where the Orchestrator
selects a Parallel Step Group, Chat shows two agents working, each child result
is recorded, and a joined Parallel Group Result returns control to the
Orchestrator.
