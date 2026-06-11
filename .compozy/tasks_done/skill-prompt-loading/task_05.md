---
status: completed
title: "Route TUI Skill Suggestions Through Shared Module"
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 05: Route TUI Skill Suggestions Through Shared Module

## Overview
Replace TUI-local skill discovery with the shared skills module while preserving the existing dropdown and reload user experience. This task aligns autocomplete with app resolution rules but keeps the TUI cache advisory only.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST route TUI skill suggestion discovery through shared `src/skills/mod.rs` APIs.
- MUST remove or retire duplicate root discovery and frontmatter parsing logic from `src/tui/mod.rs`.
- MUST preserve existing `/skill:` dropdown activation, filtering, navigation, rendering, and insertion behavior.
- MUST preserve `/reload:skills` as a TUI-local cache refresh command that does not send an app event.
- MUST keep `.multiagent/skills-cache.json` advisory and metadata-only.
- MUST include both frontmatter `name` aliases and directory-name aliases in suggestions.
- MUST reflect project-first and `.agents/skills`-first precedence in suggestion ordering and duplicate handling.
- MUST NOT make cached TUI suggestions authoritative for runtime skill resolution.
</requirements>

## Subtasks
- [x] 5.1 Replace TUI-local discovery calls with shared skills-module suggestion APIs.
- [x] 5.2 Preserve skill cache fingerprinting, loading, saving, and reload behavior.
- [x] 5.3 Preserve dropdown activation and filtering for prefix and mid-prompt `/skill:` references.
- [x] 5.4 Preserve dropdown keyboard selection and active-token replacement behavior.
- [x] 5.5 Preserve rendered source tags, origins, truncation, and max-visible-row behavior.
- [x] 5.6 Remove duplicate TUI-only frontmatter and root parsing logic.
- [x] 5.7 Update TUI tests for shared aliases, precedence, cache, reload, and dropdown behavior.

## Implementation Details
Use the TechSpec "TUI skill dropdown" and ADR-003 shared-discovery decision. The TUI should consume shared metadata/suggestions, while cache ownership, reload command handling, and rendering state remain in `src/tui/mod.rs`.

### Relevant Files
- `src/tui/mod.rs` - Contains current skill cache, discovery, reload, dropdown filtering, selection, and rendering logic.
- `src/skills/mod.rs` - Provides shared discovery and suggestion data from earlier tasks.
- `src/lib.rs` - Exposes the shared skills module.
- `.compozy/tasks/skill-prompt-loading/_techspec.md` - Requires TUI suggestions to come from shared discovery while cache remains advisory.
- `.compozy/tasks/skill-prompt-loading/adrs/adr-003.md` - Rejects separate TUI discovery behavior.

### Dependent Files
- `src/app/mod.rs` - App resolution remains authoritative and fresh; TUI suggestions must not alter app run creation semantics.
- `Cargo.toml` - Uses the YAML parser dependency added by earlier tasks; this task should not add a duplicate parser.
- `.multiagent/skills-cache.json` output shape - Cache data must stay metadata-only and may need a version bump if the suggestion shape changes.

### Related ADRs
- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - Requires aligning composer affordance with app behavior.
- [ADR-003: Shared Skill Resolver With Runtime-Time Prompt Rendering](adrs/adr-003.md) - Requires shared discovery to prevent resolver drift.

## Deliverables
- TUI skill suggestions sourced from `src/skills/mod.rs`.
- Preserved `/reload:skills` and dropdown behavior.
- Removed duplicate TUI-only discovery/frontmatter parsing where shared APIs replace it.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- TUI integration-style tests for cache, reload, filtering, selection, and rendering **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Shared discovery returns TUI suggestions for project `.agents/skills`, project `.claude/skills`, personal `~/.agents/skills`, and personal `~/.claude/skills`.
  - [x] Suggestions include frontmatter `name` aliases and directory-name aliases for the same skill identity.
  - [x] Project suggestions display ahead of personal suggestions.
  - [x] `.agents/skills` precedence is reflected when aliases collide with `.claude/skills`.
  - [x] Cached suggestions contain metadata only and no full skill body text.
- Integration tests:
  - [x] `load_skill_suggestions` uses cache only when the fingerprint matches.
  - [x] `/reload:skills` bypasses stale cache, writes fresh suggestions, clears input, resets `skill_selection_index`, and sends no app event.
  - [x] Dropdown renders for `/skill:` and mid-prompt `/skill:query`.
  - [x] Dropdown filters by typed query.
  - [x] Arrow keys cycle through visible skill suggestions.
  - [x] Enter replaces only the active token and preserves prompt suffix and trailing space/cursor behavior.
  - [x] Rendered dropdown preserves `Skills` title, `Project`/`Personal` tags, origin text, row-end tag alignment, narrow-row truncation, and max visible items.
  - [x] Deleting or mutating skill files after cache creation does not make cache authoritative for app-side resolution.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- TUI discovery and app resolution are based on the same shared rules.
- The TUI cache remains advisory, metadata-only, and reloadable.
- Existing dropdown interaction behavior remains stable.
