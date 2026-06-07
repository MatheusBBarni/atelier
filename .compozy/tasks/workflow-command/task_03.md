---
status: pending
title: Add App-Owned Workflow Target Ledger
type: backend
complexity: high
dependencies:
  - task_02
---

# Task 03: Add App-Owned Workflow Target Ledger

## Overview
Add the workflow target ledger that makes planned file-edit targets inspectable and enforceable at the app layer. The ledger must be derived from declared parallel child `write_files`, not from post-hoc changed files or model summary text.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- `RunDriveContext` MUST carry optional workflow state for workflow-mode Runs.
- Workflow planned targets MUST be keyed by normalized workspace-relative file paths.
- Planned targets MUST be derived from `ParallelChildStepPlan.file_scope.write_files` when a Parallel Step Group starts.
- Parallel children with empty `write_files` MUST NOT create file-edit targets.
- The ledger MUST record source group, source step identity, step label, initial status, and reason fields needed by the TechSpec.
- Repeated planned targets for the same path across later groups MUST be represented without silently overwriting source evidence.
- Any `RunDriveContext` construction sites introduced before this task MUST explicitly set workflow state to `None` when not in workflow mode.
</requirements>

## Subtasks
- [ ] 3.1 Add workflow context, target, target status, and completion status types near `RunDriveContext`.
- [ ] 3.2 Attach optional workflow context to workflow-mode `RunDriveContext` construction.
- [ ] 3.3 Add target-key normalization consistent with existing parallel scope validation.
- [ ] 3.4 Create planned targets when `run_parallel_group` starts and child specs are available.
- [ ] 3.5 Ensure read-only reviewer children and empty `write_files` do not add ledger entries.
- [ ] 3.6 Update normal Run, subtask Run, and test-only `RunDriveContext` constructors for the new workflow field.
- [ ] 3.7 Add focused unit tests for target derivation, duplicate path handling, and normalization.

## Implementation Details
Keep the ledger app-owned in `src/app/mod.rs` as described in the TechSpec "Core Interfaces" and "Data Models" sections. Do not move ownership into the Orchestrator or derive workflow targets from `ParallelGroupResult.changed_files`.

### Relevant Files
- `src/app/mod.rs` - owns `RunDriveContext`, `ParallelChildRuntimeState`, `prepare_parallel_children`, and `run_parallel_group`.
- `src/orchestrator/mod.rs` - defines `ParallelChildStepPlan`, `ParallelFileScope`, and existing validation semantics for safe exact write scopes.
- `src/actions/mod.rs` - enforces parallel action scope at execution time; ledger behavior must remain consistent with exact write-scope enforcement.
- `.compozy/tasks/workflow-command/_techspec.md` - defines the workflow target ledger model and target derivation rules.

### Dependent Files
- `src/app/mod.rs` completion handling - later completion evidence depends on terminal ledger state.
- `src/app/chat/projection.rs` - later workflow completion projection depends on target-count payloads.
- `src/runtime/fake.rs` - later fake integration tests rely on deterministic `write_files`.

### Related ADRs
- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - preserves app-owned execution boundaries.
- [ADR-003: App-Owned Workflow Target Ledger](adrs/adr-003.md) - requires planned target accounting from declared write scopes.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - depends on run-level workflow evidence.

## Deliverables
- Workflow context and target ledger data structures.
- Ledger initialization from parallel child `write_files`.
- Normalized target keys and source metadata.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for target derivation from Parallel Step Groups **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A child with `write_files = ["parallel-output/fixer-a.txt"]` creates one planned target for that path.
  - [ ] A reviewer child with `write_files = []` and read roots only creates no target.
  - [ ] Multiple write files in one child create separate targets with the same source step metadata.
  - [ ] Target keys are normalized consistently for workspace-relative paths accepted by existing parallel scope validation.
  - [ ] Repeated target paths across separate groups retain source evidence instead of silently replacing a prior ledger entry.
- Integration tests:
  - [ ] `/workflow parallel create a feature` records planned targets from the fake parallel group.
  - [ ] `/workflow parallel create a feature` does not count the fake reviewer read-only scope as a planned file-edit target.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Workflow target evidence is app-owned and independent from model final summaries.
- Read-only parallel children do not inflate workflow file-edit target counts.
