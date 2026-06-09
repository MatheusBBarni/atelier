---
status: completed
title: Project Workflow Events In Chat
type: backend
complexity: medium
dependencies:
  - task_04
---

# Task 05: Project Workflow Events In Chat

## Overview
Render workflow-specific lifecycle events in Chat so users can distinguish workflow start, completion, and completed-with-issues from generic run success. This task keeps the evidence-rich workflow summary visible while avoiding noisy duplicate terminal messages.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- `ChatProjection` MUST handle `workflow_started` as a workflow lifecycle item.
- `ChatProjection` MUST handle `workflow_completed` as the evidence-rich workflow terminal item.
- `workflow_completed.status = completed_with_issues` MUST render with warning severity.
- `workflow_completed.status = completed` MUST render with success severity.
- `workflow_completed.status = failed` MUST render with error severity.
- Projection MUST show target counts and unfinished target evidence when present.
- Generic `run_completed` rendering MUST remain stable for non-workflow Runs.
- Workflow completion projection MUST use a lifecycle key that cannot be overwritten by generic `run_completed`.
- Projection MUST tolerate missing optional workflow payload fields from older history events.
</requirements>

## Subtasks
- [x] 5.1 Add `workflow_started` handling in `ChatProjection::apply_history_event`.
- [x] 5.2 Add `workflow_completed` handling with severity derived from workflow status.
- [x] 5.3 Render target counts, unfinished targets, verification, skipped checks, and residual risks from the event payload.
- [x] 5.4 Keep generic `run_completed` projection unchanged for non-workflow prompts.
- [x] 5.5 Use a distinct workflow lifecycle key strategy so `run_completed` cannot replace the workflow evidence item.
- [x] 5.6 Add focused projection tests for workflow start and each terminal status class.

## Implementation Details
Implement projection logic in `src/app/chat/projection.rs` alongside existing run and parallel group summary handlers. Reference TechSpec "Chat projection" and ADR-004 for the warning treatment and duplicate-completion expectations.

### Relevant Files
- `src/app/chat/projection.rs` - owns history-to-chat projection and tests for severity/status rendering.
- `src/app/mod.rs` - emits the workflow events and defines their payload shape.
- `src/app/chat/mod.rs` - defines Chat item kinds, statuses, severities, and line views consumed by projection.
- `.compozy/tasks/workflow-command/_techspec.md` - defines workflow event fields and projection expectations.

### Dependent Files
- `src/tui/mod.rs` - displays projected Chat items in the TUI.
- `src/history/mod.rs` - stores the events that projection rebuilds from.
- `src/runtime/fake.rs` - app integration tests produce workflow history events used by projection tests if needed.

### Related ADRs
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - requires evidence-first final answers.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - requires dedicated workflow projection and warning for completed-with-issues.

## Deliverables
- Chat projection support for `workflow_started`.
- Chat projection support for `workflow_completed`.
- Warning rendering for completed-with-issues.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for workflow Chat projection **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Rebuilding projection with `workflow_started` creates a workflow lifecycle item.
  - [x] Rebuilding projection with `workflow_completed.status = completed` creates a success item.
  - [x] Rebuilding projection with `workflow_completed.status = completed_with_issues` creates a warning item.
  - [x] Rebuilding projection with `workflow_completed.status = failed` creates an error item.
  - [x] `workflow_completed` body includes target counts and unfinished targets when payload includes them.
  - [x] `workflow_completed` remains a separate item after a later `run_completed` event for the same run.
  - [x] A `workflow_completed` event missing optional arrays still renders without panic.
- Integration tests:
  - [x] A fake workflow completed-with-issues run surfaces a warning Chat item.
  - [x] A non-workflow completed run still renders the existing generic completion behavior.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Chat makes workflow completed-with-issues visibly different from ordinary success.
- Existing non-workflow Chat summaries do not regress.
