---
status: pending
title: Emit Workflow Completion Evidence
type: backend
complexity: medium
dependencies:
  - task_03
---

# Task 04: Emit Workflow Completion Evidence

## Overview
Convert workflow ledger state into terminal workflow evidence and persist it as `workflow_completed`. This task makes completed-with-issues explicit without changing the global Run state machine.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- The app MUST update workflow target statuses from parallel child terminal results.
- `Completed` and `NoChanges` child results MUST mark that child's planned write targets completed.
- `Blocked` and `ApprovalDenied` child results MUST mark that child's planned write targets blocked with a reason.
- `Failed`, `ParseError`, `LimitReached`, and `Cancelled` child results MUST mark that child's planned write targets failed with a diagnostic.
- The app MUST NOT infer `Skipped` from free-form model summary text; skipped targets require explicit app-owned evidence.
- The app MUST emit `workflow_completed` with status, target counts, unfinished targets, verification, skipped checks, and residual risks.
- `completed_with_issues` MUST keep `RunState::Completed` when all unfinished targets are accounted for.
- The app MUST record `workflow_completed` before generic `run_completed` for workflow-mode Runs.
</requirements>

## Subtasks
- [ ] 4.1 Add helpers that map child terminal results to workflow target terminal statuses.
- [ ] 4.2 Update ledger targets when parallel child/group results are recorded.
- [ ] 4.3 Aggregate verification commands, skipped-check reasons, and residual risks into workflow context where available.
- [ ] 4.4 Derive workflow completion status from target counts and interrupted/unaccounted states.
- [ ] 4.5 Record `workflow_completed` before or near generic terminal run completion.
- [ ] 4.6 Add explicit ordering coverage for `workflow_completed` before `run_completed`.
- [ ] 4.7 Add tests for every target-status mapping and completion status.

## Implementation Details
Implement completion accounting in `src/app/mod.rs` around parallel child result recording, group join synthesis, and `DecisionStatus::Complete` handling. Reference TechSpec "Data Models", "Event Payloads", and ADR-004 for terminal event behavior.

### Relevant Files
- `src/app/mod.rs` - owns parallel child result recording, `synthesize_parallel_group_result`, Run completion, and history recording.
- `src/orchestrator/mod.rs` - defines `AgentResultStatus`, `ParallelGroupResult`, and status variants used by mapping rules.
- `src/history/mod.rs` - persists the `workflow_completed` payload as a generic history event.
- `.compozy/tasks/workflow-command/_techspec.md` - defines status mapping and completion rules.

### Dependent Files
- `src/app/chat/projection.rs` - later renders `workflow_completed` status and evidence.
- `src/runtime/fake.rs` - later integration coverage must produce deterministic completed and completed-with-issues cases.
- `src/tui/mod.rs` - help text can describe a command that now has terminal evidence.

### Related ADRs
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - requires final evidence, unfinished target status, skipped checks, and residual risks.
- [ADR-003: App-Owned Workflow Target Ledger](adrs/adr-003.md) - defines the ledger source of truth.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - keeps `completed_with_issues` out of global `RunState`.

## Deliverables
- Workflow target status mapper for all terminal child result variants.
- `workflow_completed` history event with target counts and evidence.
- Completion-status derivation for completed, completed-with-issues, and failed.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for workflow completion evidence **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `AgentResultStatus::Completed` maps planned targets to completed.
  - [ ] `AgentResultStatus::NoChanges` maps planned targets to completed.
  - [ ] `AgentResultStatus::Blocked` maps planned targets to blocked with blocker text.
  - [ ] `AgentResultStatus::ApprovalDenied` maps planned targets to blocked with approval denial reason.
  - [ ] `AgentResultStatus::Failed`, `ParseError`, `LimitReached`, and `Cancelled` map planned targets to failed.
  - [ ] Mixed completed and blocked targets derive `completed_with_issues`.
  - [ ] Unaccounted planned targets derive `failed`.
  - [ ] Workflow completion payload includes unfinished target reasons for blocked and failed targets.
- Integration tests:
  - [ ] `/workflow parallel scoped write action create a feature` records `workflow_completed.status = completed`.
  - [ ] `/workflow parallel approval action create a feature` records `workflow_completed.status = completed_with_issues`.
  - [ ] Workflow `workflow_completed` event appears before generic `run_completed`.
  - [ ] Interrupted workflow records failed or interrupted workflow evidence without hiding unfinished targets.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Workflow completion status is derived from app-owned target evidence.
- Completed-with-issues is visible in workflow payload without changing generic Run completion semantics.
