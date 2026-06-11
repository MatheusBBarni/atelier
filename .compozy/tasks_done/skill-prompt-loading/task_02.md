---
status: completed
title: "Add Skill Resolver, Dedupe, Diagnostics, And Renderer"
type: backend
complexity: high
dependencies:
  - task_01
---

# Task 02: Add Skill Resolver, Dedupe, Diagnostics, And Renderer

## Overview
Implement the authoritative resolver and renderer in the shared skills module. This task turns `/skill:<id>` references into normalized prompt data and loaded in-memory skill context, while preserving the TechSpec boundary that runtime adapters remain generic.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST parse every `/skill:<id>` occurrence anywhere in a submitted prompt.
- MUST fail empty `/skill:` references before any run creation can happen in later app integration.
- MUST stop skill identifiers at whitespace and common punctuation according to the TechSpec parser contract.
- MUST resolve aliases using both frontmatter `name` and directory name.
- MUST apply root precedence: project `.agents/skills`, project `.claude/skills`, personal `~/.agents/skills`, personal `~/.claude/skills`.
- MUST dedupe duplicate resolved skills by canonical identity while preserving first-use order and requested alias metadata.
- MUST produce fail-closed diagnostics for unknown, ambiguous, unreadable, or invalid skills, including useful suggestions when possible.
- MUST implement `compile_prompt` and `render_runtime_prompt` as described by the TechSpec.
- MUST strip skill references from the normalized user prompt without embedding full skill bodies in persistent data structures.
- MUST NOT modify `App::submit_prompt`, `RunDriveContext`, history, chat projection, or runtime adapters in this task.
</requirements>

## Subtasks
- [x] 2.1 Add `/skill:<id>` parsing and normalized prompt stripping.
- [x] 2.2 Build the alias index and precedence-aware resolver.
- [x] 2.3 Add canonical-identity dedupe with requested-name tracking.
- [x] 2.4 Add fail-closed resolver diagnostics and typo suggestions.
- [x] 2.5 Add in-memory skill content loading and validation.
- [x] 2.6 Add `compile_prompt` and `render_runtime_prompt` outputs for downstream app integration.
- [x] 2.7 Add parser, resolver, diagnostic, dedupe, and renderer tests.

## Implementation Details
Use the TechSpec "Core Interfaces", "Parsing And Resolution", and "Prompt Rendering" sections as the source of truth for public function names and rendering shape. Keep renderer output as text suitable for the existing `RuntimeRequest.prompt` field; do not add structured skill fields to `RuntimeRequest`.

### Relevant Files
- `src/skills/mod.rs` - Implements parser, resolver, diagnostics, `CompiledPrompt`, `SkillPromptContext`, and runtime prompt rendering.
- `src/tui/mod.rs` - Existing discovery behavior provides local examples for aliases and sources but should not remain the authoritative resolver.
- `.compozy/tasks/skill-prompt-loading/_prd.md` - Defines the user-visible V1 behavior and fail-closed contract.
- `.compozy/tasks/skill-prompt-loading/_techspec.md` - Defines resolver interfaces, prompt rendering, and testing requirements.
- `.compozy/tasks/skill-prompt-loading/adrs/adr-003.md` - Defines shared resolver and runtime-time rendering decisions.

### Dependent Files
- `src/app/mod.rs` - Later tasks will call `compile_prompt` before normal and subtask run creation.
- `src/runtime/mod.rs` - Later tasks will pass rendered prompt text through the existing runtime envelope.
- `src/history/mod.rs` - Later tasks will persist `LoadedSkillMetadata`, not full skill content.
- `src/app/chat/projection.rs` - Later tasks will display concise metadata only.
- `src/tui/mod.rs` - Later tasks will consume shared suggestions and avoid duplicate discovery logic.

### Related ADRs
- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - Requires every `/skill:name` occurrence to invoke and fail closed on bad references.
- [ADR-002: Select Deterministic Common Flow For PRD](adrs/adr-002.md) - Locks in deterministic common-flow product behavior.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Establishes resolver/rendering separation and metadata-only audit data.

## Deliverables
- Shared resolver APIs for prompt compilation and runtime prompt rendering.
- Fail-closed diagnostics with suggestions for unknown or ambiguous skill references.
- Deduped in-memory skill context with metadata suitable for later history events.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration-oriented renderer tests for existing runtime envelope consumption **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Parser detects multiple `/skill:name` occurrences anywhere in a prompt.
  - [x] Parser treats empty `/skill:` as invalid.
  - [x] Parser stops identifiers at whitespace and common punctuation.
  - [x] Normalized prompt removes skill references while preserving surrounding user text.
  - [x] Directory name and frontmatter `name` both resolve to the same canonical skill.
  - [x] Requesting both aliases for one canonical skill loads one skill and records all requested names.
  - [x] Project `.agents/skills` beats project `.claude/skills`.
  - [x] Project roots beat personal roots.
  - [x] Personal `.agents/skills` beats personal `.claude/skills`.
  - [x] Same-alias ambiguity in the same effective precedence tier fails with source details.
  - [x] Unknown typo diagnostics include close-match suggestions.
  - [x] Invalid YAML, unreadable `SKILL.md`, and missing skill content fail with descriptive errors.
  - [x] Renderer emits `<System Prompt>`, ordered `<Skill: ... source="...">`, and `<User Prompt>` sections once.
  - [x] Skill body text that resembles closing delimiters is safely framed according to the TechSpec.
- Integration tests:
  - [x] Rendering output can be placed in `RuntimeRequest.prompt` without requiring new runtime request fields.
  - [x] Metadata returned from compilation excludes full skill body content.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The resolver is deterministic for all selected V1 precedence and alias rules.
- Duplicate skill references load content once and preserve first-use order.
- Runtime prompt rendering is available without changing runtime adapter contracts.
