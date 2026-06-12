---
status: completed
title: "Integrate Normal Prompt Skill Loading In App"
type: backend
complexity: high
dependencies:
  - task_02
---

# Task 03: Integrate Normal Prompt Skill Loading In App

## Overview
Wire shared skill prompt compilation into app-owned run creation for normal prompts and subtask prompts. This task establishes the run lifecycle contract: resolve skills before a run is created, carry normalized prompt plus skill context in memory, and emit metadata-only `skills_loaded` evidence after successful resolution.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST call shared prompt compilation before creating a normal run for prompts containing skill references.
- MUST fail unknown, empty, ambiguous, unreadable, or invalid skill references before `run_started`, `prompt_submitted`, `skills_loaded`, or run-record creation.
- MUST extend `RunDriveContext` to carry submitted prompt, normalized user prompt, and optional `SkillPromptContext`.
- MUST keep `RunDriveContext.prompt` or its successor as normalized user-facing text, not full rendered skill text.
- MUST record `skills_loaded` after `run_started` and `prompt_submitted`, before runtime work starts.
- MUST record only loaded skill metadata in `skills_loaded`; full `SKILL.md` bodies must not be persisted.
- MUST resolve `/skill:` references in `/subtask <agent> <task>` task text before `subtask_started`.
- MUST keep pending approval and clarification-answer handling ahead of skill compilation.
- MUST preserve V1 behavior that clarification answers containing `/skill:x` are not newly resolved as skill invocations.
- MUST replace the existing raw `/skill:` pass-through test with explicit load and fail-closed coverage.
</requirements>

## Subtasks
- [x] 3.1 Extend run context data to carry submitted prompt, normalized prompt, and skill context.
- [x] 3.2 Integrate prompt compilation into normal prompt submission before run creation.
- [x] 3.3 Integrate prompt compilation into `/subtask` task text before subtask run creation.
- [x] 3.4 Emit metadata-only `skills_loaded` after successful run creation and prompt submission.
- [x] 3.5 Add fail-closed app diagnostics for skill load failures.
- [x] 3.6 Preserve clarification, pending approval, and unknown slash-command ordering.
- [x] 3.7 Replace raw `/skill:` app tests with loading and failure tests.

## Implementation Details
Use the TechSpec "Run lifecycle" and "Integration Points" sections. `src/app/mod.rs` should consume `skills::compile_prompt` rather than duplicating resolver logic. Runtime prompt rendering is finalized in task 04, but this task should leave enough context on the run for task 04 to render later without reparsing prompt text.

### Relevant Files
- `src/app/mod.rs` - Main integration point for `App::submit_prompt`, `/subtask`, `RunDriveContext`, event ordering, run records, and app tests.
- `src/skills/mod.rs` - Provides `compile_prompt`, `SkillPromptContext`, and loaded skill metadata from task 02.
- `src/history/mod.rs` - Persists event payloads and debug payloads, so `skills_loaded` must be metadata-only before it reaches history.
- `src/runtime/mod.rs` - Downstream runtime requests will receive rendered skill sections in task 04.
- `src/app/chat/projection.rs` - Downstream projection will display `skills_loaded` in task 06.

### Dependent Files
- `src/tui/mod.rs` - TUI suggestions remain advisory and must not be treated as authoritative app resolution.
- `src/runtime/fake.rs` - App integration tests use fake runtime behavior and may need fixtures that avoid accidental trigger words in skill bodies.
- `.multiagent/runs/<run_id>.json` output shape - Run records must not contain rendered full skill text.

### Related ADRs
- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - Requires fail-closed validation before runtime invocation.
- [ADR-002: Select Deterministic Common Flow For PRD](adrs/adr-002.md) - Requires deterministic success and failure behavior.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Requires normalized prompt plus in-memory skill context and metadata-only events.

## Deliverables
- App run creation path that compiles skill references before creating normal runs.
- `/subtask` path that compiles skill references before creating subtask runs.
- Metadata-only `skills_loaded` event emission.
- Updated run-record payloads that preserve prompt provenance without full skill bodies.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- App integration tests for skill load success, fail-closed errors, event order, and persistence safety **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `RunDriveContext` construction stores submitted prompt, normalized prompt, and skill context without rendered skill sections.
  - [x] `skills_loaded` payload serialization includes names, canonical IDs, source origins, source paths, and requested names only.
  - [x] Skill-load errors map to user-facing app diagnostics with suggestions when provided by the resolver.
- Integration tests:
  - [x] `/skill:reviewer inspect README` resolves before run creation and emits `run_started`, `prompt_submitted`, then `skills_loaded`.
  - [x] Unknown `/skill:missing inspect README` records no `run_started`, no `prompt_submitted`, no `skills_loaded`, and no run record.
  - [x] Empty `/skill:` reports a skill-load diagnostic instead of generic unknown-command behavior.
  - [x] Mid-prompt `please use /skill:reviewer here` resolves as a skill invocation.
  - [x] Duplicate references like `/skill:a do x /skill:a` emit one loaded skill entry with requested alias metadata.
  - [x] `prompt_submitted`, history JSONL, debug log, and run record JSON do not contain a sentinel full skill body.
  - [x] Clarification answer `/tmp/project` remains a clarification answer.
  - [x] Clarification answer `/skill:reviewer` does not load a new skill in V1 and emits no `skills_loaded`.
  - [x] Pending approval still rejects normal prompt answers before any skill compilation.
  - [x] `/subtask explorer /skill:reviewer inspect README` resolves before `subtask_started`.
  - [x] Failed `/subtask explorer /skill:missing inspect README` creates no subtask run.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Skill resolution succeeds or fails before model runtime work starts.
- App history and run records persist metadata only.
- Existing non-skill slash-command, clarification, and approval flows keep their current behavior.
