---
status: completed
title: Add Workflow Command Parsing And Preflight
type: backend
complexity: medium
dependencies: []
---

# Task 01: Add Workflow Command Parsing And Preflight

## Overview
Add the `/workflow <prompt>` command entry point and prerequisite checks before normal prompt submission starts a Run. This task gives workflow mode a clear command contract and prevents disabled Parallel Step Group configurations from creating partial workflow history.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- The app MUST recognize `/workflow <prompt>` before `reject_unknown_slash_command` handles slash-prefixed input.
- The app MUST reject `/workflow` and whitespace-only workflow prompts with a clear usage error.
- The app MUST fail before `run_started`, `workflow_started`, and `prompt_submitted` when `features.parallel_step_groups` is false.
- The app MUST fail before `run_started`, `workflow_started`, and `prompt_submitted` when `limits.max_parallel_agent_steps` is 0.
- The app MUST preserve existing behavior for `/help`, `/goal`, `/config`, `/subtask`, `/agent:<name>`, `/skill:<name>`, and unknown slash commands.
- The parser MUST NOT treat `/workflowfoo` or other longer slash commands as `/workflow`.
- Workflow command handling SHOULD run after pending clarification answers are accepted so slash-prefixed clarification answers keep existing behavior.
</requirements>

## Subtasks
- [x] 1.1 Add a small parser/helper that extracts workflow prompt text from `/workflow <prompt>`.
- [x] 1.2 Add workflow prerequisite checks for Parallel Step Group availability.
- [x] 1.3 Wire the parser into `App::submit_prompt` before unknown slash-command rejection.
- [x] 1.4 Return workflow-specific diagnostic text for empty prompt and failed prerequisites.
- [x] 1.5 Update slash-command availability text so `/workflow <prompt>` is not reported as unknown.
- [x] 1.6 Add focused tests for command parsing, failed preflight, and non-workflow regression.

## Implementation Details
Implement the command parsing and prerequisite decision points in `src/app/mod.rs`. Reference the TechSpec "Integration Points" and "Testing Approach" sections for the expected placement and behavior.

### Relevant Files
- `src/app/mod.rs` - contains `App::submit_prompt`, slash-command handlers, `reject_unknown_slash_command`, app tests, and access to effective feature/limit config.
- `src/config/mod.rs` - defines `features.parallel_step_groups` and `limits.max_parallel_agent_steps` defaults used by workflow preflight.
- `src/orchestrator/mod.rs` - contains the existing Parallel Step Group feature-gate messages and validation semantics that workflow preflight must align with.
- `.compozy/tasks/workflow-command/_prd.md` - defines the critical workflow command and prerequisite behavior.
- `.compozy/tasks/workflow-command/_techspec.md` - defines where `handle_workflow_command` should be added and which prerequisite states fail.

### Dependent Files
- `src/runtime/fake.rs` - later workflow tests depend on successful command recognition.
- `src/tui/mod.rs` - later help text must describe the accepted command surface.
- `src/app/chat/projection.rs` - later workflow events depend on preflight ensuring only real workflow Runs emit workflow history.

### Related ADRs
- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - requires `/workflow` to enter one normal Run and fail clearly when parallel prerequisites are disabled.
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - defines executing workflow mode rather than a planning-only fallback.
- [ADR-004: Workflow Events Carry Workflow-Specific Completion Status](adrs/adr-004.md) - requires failed preflight to avoid recording workflow history.

## Deliverables
- `/workflow <prompt>` parser and usage validation.
- Parallel Step Group prerequisite checks before Run creation.
- Workflow-specific errors for disabled feature and zero parallel limit.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for workflow command preflight **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `/workflow parallel scoped write action create a feature` extracts `parallel scoped write action create a feature`.
  - [ ] `/workflow` returns a usage error that includes `/workflow <prompt>`.
  - [ ] `/workflow     ` returns a usage error that includes `/workflow <prompt>`.
  - [ ] `/workflowfoo create a feature` is not parsed as `/workflow`.
  - [ ] `/doctor` still returns the existing unknown-command behavior.
- Integration tests:
  - [ ] With `features.parallel_step_groups = false`, `/workflow create a feature` fails and records no `run_started`, `workflow_started`, or `prompt_submitted`.
  - [ ] With `limits.max_parallel_agent_steps = 0`, `/workflow create a feature` fails and records no `run_started`, `workflow_started`, or `prompt_submitted`.
  - [ ] Existing `/agent:fixer inspect README` and `/skill:reviewer inspect README` prompts still submit normally.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/workflow` is a known command with explicit usage and prerequisite failures.
- Failed workflow preflight leaves app state idle and does not create partial run history.
- Slash-prefixed clarification answers keep the existing clarification-answer flow.
