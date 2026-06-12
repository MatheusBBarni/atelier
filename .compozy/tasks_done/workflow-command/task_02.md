---
status: completed
title: Start Workflow Runs With Prompt Envelope And Start Event
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 02: Start Workflow Runs With Prompt Envelope And Start Event

## Overview
Start workflow-mode execution as one normal Run while keeping user-visible history auditable. This task records `workflow_started`, preserves the original `/workflow <prompt>` in `prompt_submitted`, and passes workflow-specific guidance to the runtime path.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- Workflow command handling MUST start exactly one normal app Run after prerequisite checks pass.
- The visible `prompt_submitted` event MUST preserve the original `/workflow <prompt>` command text.
- The runtime prompt used by the Orchestrator MUST include workflow-specific instructions to decompose, execute, validate, and account for planned targets.
- The app MUST record a `workflow_started` event with run ID, original command, extracted user prompt, mode, and preflight details.
- Existing active-run and waiting-for-user guards MUST still apply to workflow prompts.
- Runtime prompt-envelope text MUST NOT appear in the user-visible `prompt_submitted` payload.
</requirements>

## Subtasks
- [x] 2.1 Add `App::handle_workflow_command` or equivalent flow after task 01 preflight.
- [x] 2.2 Add workflow prompt-envelope creation for the runtime prompt path.
- [x] 2.3 Record `workflow_started` after `run_started` and before or near `prompt_submitted`.
- [x] 2.4 Ensure `prompt_submitted` stores the original command text, not the runtime envelope.
- [x] 2.5 Ensure `RunDriveContext.prompt` carries the workflow runtime prompt while workflow metadata preserves the original command.
- [x] 2.6 Add tests for event ordering, payload content, and unchanged normal prompt submission.

## Implementation Details
Implement the workflow start path in `src/app/mod.rs` near existing command handlers and Run creation. Reference TechSpec "Event Payloads", "Development Sequencing", and ADR-004 for the exact history contract.

### Relevant Files
- `src/app/mod.rs` - owns Run creation, command handling, history recording, and `RunDriveContext` construction.
- `src/history/mod.rs` - defines the generic history event structure used for `workflow_started`.
- `src/runtime/mod.rs` - serializes `RuntimeRequest.prompt` into the runtime envelope consumed by fake and real runtimes.
- `src/runtime/fake.rs` - fake-runtime integration tests may need prompt-envelope-aware branches if the envelope changes prompt matching.
- `.compozy/tasks/workflow-command/_techspec.md` - defines the workflow prompt envelope and `workflow_started` payload.

### Dependent Files
- `src/app/chat/projection.rs` - later tasks project `workflow_started`.
- `src/app/mod.rs` parallel-group flow - later target ledger depends on workflow context being attached to the active Run.
- `src/tui/mod.rs` - user-facing help depends on this command becoming runnable.

### Related ADRs
- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - keeps workflow mode inside one normal Run.
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - requires execution, validation, and synthesis guidance.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - requires original command preservation and workflow-specific events.

## Deliverables
- Workflow Run startup path that reuses normal Run lifecycle state.
- Runtime prompt envelope for workflow-mode Orchestrator routing.
- `workflow_started` history event with preflight payload.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for workflow Run startup **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Workflow prompt envelope includes the extracted user prompt and workflow evidence requirements.
  - [x] Workflow start payload includes `mode = workflow`, `parallel_step_groups = true`, and configured `max_parallel_agent_steps`.
- Integration tests:
  - [x] `/workflow parallel create a feature` records `run_started`, `workflow_started`, and `prompt_submitted` in the expected order.
  - [x] `prompt_submitted.payload.prompt` remains `/workflow parallel create a feature`.
  - [x] Runtime-facing prompt content contains workflow-mode instructions while history preserves the raw command.
  - [x] The workflow runtime path still reaches the fake Orchestrator and completes a normal Run.
  - [x] A normal `parallel create a feature` prompt records no `workflow_started` event.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Workflow mode starts through the existing Run lifecycle.
- Session History preserves both the visible command and explicit workflow start evidence.
