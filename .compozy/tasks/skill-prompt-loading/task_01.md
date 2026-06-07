---
status: pending
title: "Add Shared Skill Discovery And Parsing Foundation"
type: backend
complexity: medium
dependencies: []
---

# Task 01: Add Shared Skill Discovery And Parsing Foundation

## Overview
Create the shared skills module that becomes the source of truth for skill roots, frontmatter parsing, metadata, and suggestion data. This task establishes the foundation for runtime loading and TUI reuse without changing app prompt submission behavior yet.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create `src/skills/mod.rs` with shared data models for skill roots, skill metadata, canonical skill identity, and TUI-compatible suggestion metadata.
- MUST expose the module from `src/lib.rs`.
- MUST add a Serde-compatible YAML frontmatter parser dependency, using the TechSpec-preferred `serde_norway` unless implementation evidence justifies a safer current alternative.
- MUST parse YAML frontmatter from `SKILL.md` files for at least `name` and `description`.
- MUST support both frontmatter `name` and directory name as aliases for a skill identity.
- MUST define deterministic root discovery order: project `.agents/skills`, project `.claude/skills`, personal `~/.agents/skills`, personal `~/.claude/skills`.
- MUST keep suggestion and cache-facing structs metadata-only and exclude full `SKILL.md` bodies.
- MUST NOT wire prompt compilation into `App::submit_prompt`, runtime adapters, history, or chat projection in this task.
</requirements>

## Subtasks
- [ ] 1.1 Create the shared `src/skills/mod.rs` module and export it.
- [ ] 1.2 Add YAML frontmatter parsing for skill metadata.
- [ ] 1.3 Add root discovery models with injectable project and home roots for test isolation.
- [ ] 1.4 Add canonical identity and alias metadata needed by later resolver work.
- [ ] 1.5 Add metadata-only suggestion data that can replace TUI-local discovery later.
- [ ] 1.6 Add focused unit tests for roots, metadata, aliases, and suggestion serialization.

## Implementation Details
Follow the TechSpec "Core Interfaces", "Parsing And Resolution", and "Technical Dependencies" sections for the shape of the shared module. The current TUI-only discovery code in `src/tui/mod.rs` is the main local behavior to preserve, but this task should not make the TUI consume the new module yet unless doing so is necessary to keep the module compileable.

### Relevant Files
- `src/skills/mod.rs` - New shared module for skill discovery foundations, metadata parsing, aliases, and suggestion data.
- `src/lib.rs` - Exports the new shared skills module.
- `Cargo.toml` - Adds the YAML parser dependency required for frontmatter parsing.
- `Cargo.lock` - Records the resolved YAML parser dependency.
- `src/tui/mod.rs` - Contains existing TUI-only `SkillSuggestion`, `SkillSourceTag`, roots, cache, fingerprint, and frontmatter helper logic that should guide the shared module.
- `.compozy/tasks/skill-prompt-loading/_techspec.md` - Defines shared module responsibilities and parser dependency constraints.

### Dependent Files
- `src/app/mod.rs` - Later tasks will compile prompts before run creation using this module.
- `src/runtime/mod.rs` - Later tasks will receive rendered prompt text through the existing runtime request path.
- `src/history/mod.rs` - Later tasks will persist metadata-only skill load events.
- `src/app/chat/projection.rs` - Later tasks will project metadata-only skill load events.
- `src/tui/mod.rs` - Later tasks will replace TUI-local skill discovery with shared suggestion APIs.

### Related ADRs
- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - Establishes app-owned skill loading and shared resolver direction.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Defines the shared module boundary and metadata-only persistence constraint.

## Deliverables
- `src/skills/mod.rs` with root, metadata, alias, canonical identity, and suggestion foundations.
- `src/lib.rs` export for the new module.
- YAML parser dependency recorded in `Cargo.toml` and `Cargo.lock`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Regression tests proving metadata-only suggestion data **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Root discovery returns project `.agents/skills`, project `.claude/skills`, personal `~/.agents/skills`, and personal `~/.claude/skills` in exact precedence order.
  - [ ] Root discovery tests use injected home/project paths and do not depend on the developer machine's real home directory.
  - [ ] Valid YAML frontmatter parses `name` and `description`.
  - [ ] Quoted YAML names parse correctly.
  - [ ] Missing frontmatter falls back to the skill directory name as an alias.
  - [ ] Invalid YAML frontmatter fails with a descriptive module-level error.
  - [ ] Directory name and frontmatter `name` can both be represented as aliases for one canonical skill identity.
  - [ ] Suggestion metadata contains source tag, source origin, display name, alias, and path data without full skill body content.
- Integration tests:
  - [ ] The new module is publicly importable from another crate module without requiring TUI state.
  - [ ] The shared suggestion shape can represent existing project and personal skill roots used by `src/tui/mod.rs`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `src/skills/mod.rs` is the shared place for root and metadata discovery foundations.
- Full `SKILL.md` bodies are not stored in cache-facing or suggestion-facing structs.
- App prompt submission, runtime request construction, and history behavior remain unchanged by this foundation task.
