---
status: pending
title: "Render Skill Context For Runtime And Derived Prompts"
type: backend
complexity: high
dependencies:
  - task_03
---

# Task 04: Render Skill Context For Runtime And Derived Prompts

## Overview
Render loaded skill context into the prompt text used by runtime requests while keeping runtime adapters generic. This task ensures orchestrator, agent, parallel child, council, and subtask prompts all carry the selected skill context exactly once and do not leak rendered skill bodies into persisted app data or action authorization checks.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render skill sections only while constructing `RuntimeRequest` values.
- MUST keep `RuntimeRequest` shape unchanged for V1; rendered text flows through the existing `prompt` field.
- MUST NOT render skill sections into `RunDriveContext.prompt`, derived prompt helper strings, history events, debug events, chat projection payloads, run records, or session history payloads.
- MUST ensure orchestrator, specialized agent, parallel child, council member, and subtask runtime requests include loaded skill context exactly once.
- MUST ensure derived prompt helpers receive normalized prompt text and reuse `SkillPromptContext` rather than reparsing `/skill:` syntax.
- MUST ensure action authorization uses normalized user prompt text, not rendered skill body text.
- MUST preserve runtime adapter generic behavior; Codex, Claude, Cursor, Z.ai, and Fake runtimes must not parse `/skill:` syntax.
- MUST add regression coverage for fake runtime trigger words appearing in skill bodies or otherwise control fixture text to avoid false routing.
</requirements>

## Subtasks
- [ ] 4.1 Update app runtime request construction to render skill context at the final boundary.
- [ ] 4.2 Thread existing `SkillPromptContext` through orchestrator and specialized agent requests.
- [ ] 4.3 Thread existing `SkillPromptContext` through parallel child prompts.
- [ ] 4.4 Thread existing `SkillPromptContext` through council prompts.
- [ ] 4.5 Thread existing `SkillPromptContext` through subtask runtime prompts.
- [ ] 4.6 Keep action authorization and VCS explicit-request checks based on normalized user prompt text.
- [ ] 4.7 Add runtime envelope, derived prompt, double-rendering, and leakage tests.

## Implementation Details
Use the TechSpec "Prompt Rendering", "Integration Points", and ADR-003. `runtime_request` in `src/app/mod.rs` is the intended render point. Do not render in `parallel_child_prompt`, `council_member_prompt`, or `subtask_prompt`; those helpers should operate on normalized user prompt text and let the final runtime request boundary render the skill sections.

### Relevant Files
- `src/app/mod.rs` - Main target for `runtime_request`, `RunDriveContext`, orchestrator, specialized agent, parallel child, council, subtask, and action-loop integration.
- `src/runtime/mod.rs` - Owns `RuntimeRequest` and `prompt_envelope_json`; tests should assert rendered text appears only in `prompt`.
- `src/skills/mod.rs` - Provides `render_runtime_prompt` and `SkillPromptContext`.
- `src/actions/mod.rs` - `ActionExecutionContext.user_prompt` must not receive rendered skill body text.
- `src/runtime/fake.rs` - Fake runtime prompt substring matching can be affected by rendered skill bodies.

### Dependent Files
- `src/runtime/codex.rs` - Consumes `prompt_envelope_json` and should remain adapter-generic.
- `src/runtime/claude.rs` - Consumes `prompt_envelope_json` and should remain adapter-generic.
- `src/runtime/cursor.rs` - Consumes `prompt_envelope_json` and should remain adapter-generic.
- `src/runtime/zai.rs` - Consumes `prompt_envelope_json` and should remain adapter-generic.
- `src/history/mod.rs` - Session history events must not receive full rendered skill bodies.
- `src/app/chat/projection.rs` - Chat projection must not display full skill bodies.

### Related ADRs
- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - Requires runtime adapters to receive composed prompt text instead of interpreting skill syntax.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Requires rendering only at runtime request construction and preventing double rendering.

## Deliverables
- Runtime request construction that renders loaded skill context exactly once.
- Derived prompt flows that carry skill context without reparsing or nesting rendered sections.
- Action authorization path that continues to use normalized user intent text.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for runtime envelope rendering and derived prompt propagation **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `prompt_envelope_json` output contains rendered `<Skill: ...>` sections only inside the existing `prompt` field.
  - [ ] Rendering one loaded skill referenced multiple times produces one skill section per `RuntimeRequest`.
  - [ ] Derived prompt helper strings do not contain `<Skill:` sections.
  - [ ] `ActionExecutionContext.user_prompt` receives normalized user prompt text, not rendered skill body text.
- Integration tests:
  - [ ] Orchestrator request contains the skill section exactly once.
  - [ ] Specialized agent request contains the skill section exactly once.
  - [ ] Parallel child request contains the skill section exactly once.
  - [ ] Council member request contains the skill section exactly once.
  - [ ] `/subtask explorer /skill:x inspect` runtime request contains subtask guard text plus the skill section exactly once.
  - [ ] A clarification resume preserves existing skill context once and does not resolve a new `/skill:new` in the clarification answer.
  - [ ] Skill body text containing `commit` does not authorize a git commit action when the normalized prompt did not request it.
  - [ ] Skill body text containing fake runtime routing trigger words does not cause unintended route selection, or test fixtures document and avoid those trigger words.
  - [ ] History events, debug events, chat projection data, run records, and session history payloads do not contain a sentinel full skill body.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Every runtime request path receives skill context exactly once.
- Runtime adapters remain unaware of `/skill:` syntax.
- Rendered skill bodies do not leak into persistence or action authorization.
