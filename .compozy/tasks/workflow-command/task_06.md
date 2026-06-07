---
status: pending
title: Extend Fake Runtime Workflow Fixtures And Integration Coverage
type: test
complexity: medium
dependencies:
  - task_04
---

# Task 06: Extend Fake Runtime Workflow Fixtures And Integration Coverage

## Overview
Add deterministic fake-runtime workflow scenarios and app-level integration coverage for the end-to-end command. This task ensures workflow behavior can be verified without real Codex, Claude, Cursor, or Z.ai runtimes.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- Fake-runtime behavior MUST support workflow prompt envelopes without losing existing parallel test triggers.
- Integration tests MUST cover a workflow happy path that writes deterministic scoped files.
- Integration tests MUST cover completed-with-issues from approval denial.
- Integration tests MUST cover completed-with-issues from parse error or failed child result.
- Integration tests MUST cover disabled prerequisite failure before Run history starts.
- Integration tests MUST verify normal non-workflow parallel behavior remains unchanged.
- Parse-error or failed-child workflow coverage MUST use a write-scoped child so failed target accounting is exercised.
</requirements>

## Subtasks
- [ ] 6.1 Extend fake-runtime prompt matching only where needed for workflow prompt envelopes.
- [ ] 6.2 Add app tests for workflow happy-path completion and target counts.
- [ ] 6.3 Add app tests for workflow completed-with-issues from approval denial.
- [ ] 6.4 Add app tests for workflow completed-with-issues from parse error or failed child.
- [ ] 6.5 Add app tests for disabled feature and zero parallel limit preflight.
- [ ] 6.6 Add non-workflow regression tests or extend existing tests to prove behavior remains unchanged.
- [ ] 6.7 Add or adapt a write-scoped fake child fixture for parse-error target accounting.

## Implementation Details
Use existing fake-runtime patterns in `src/runtime/fake.rs` and existing app integration tests in `src/app/mod.rs`. Reference TechSpec "Testing Approach" and "Development Sequencing" for the exact cases required.

### Relevant Files
- `src/runtime/fake.rs` - owns deterministic Orchestrator decisions, parallel group branches, and fake agent results.
- `src/app/mod.rs` - contains fake config helpers and integration tests around parallel groups, approvals, parse errors, interrupts, and slash commands.
- `src/orchestrator/mod.rs` - defines fake decision and result data structures used by tests.
- `.compozy/tasks/workflow-command/_techspec.md` - lists required unit and integration test scenarios.

### Dependent Files
- `src/app/chat/projection.rs` - may be exercised indirectly when workflow events sync Chat state.
- `.multiagent/` test session output under temporary directories - fake app tests should keep writes isolated to temp dirs.
- `Cargo.toml` - defines test dependencies already used by the app test suite.

### Related ADRs
- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - requires workflow behavior to reuse normal Run infrastructure.
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - requires execution and validation evidence.
- [ADR-003: App-Owned Workflow Target Ledger](adrs/adr-003.md) - requires target-count verification from declared write scopes.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - requires completed-with-issues coverage without global RunState change.

## Deliverables
- Fake-runtime fixture support for workflow-mode prompts.
- App integration tests for happy path and completed-with-issues paths.
- Regression coverage for disabled prerequisite and non-workflow prompts.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for fake workflow execution **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Fake prompt matching recognizes workflow envelopes containing `parallel scoped write action`.
  - [ ] Fake prompt matching still recognizes existing non-workflow `parallel` prompts.
- Integration tests:
  - [ ] `/workflow parallel scoped write action create a feature` writes both scoped fake output files.
  - [ ] The happy-path workflow records `workflow_started` and `workflow_completed.status = completed`.
  - [ ] `/workflow parallel approval action create a feature` records `workflow_completed.status = completed_with_issues`.
  - [ ] Workflow parse-error scenario with a write-scoped child records `workflow_completed.status = completed_with_issues` and a failed target count.
  - [ ] Disabled Parallel Step Groups rejects workflow before `run_started`.
  - [ ] Existing `parallel scoped write action create a feature` non-workflow test still passes without workflow events.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Workflow command behavior is covered end to end using fake runtime.
- Existing fake parallel tests remain deterministic and unchanged in behavior.
