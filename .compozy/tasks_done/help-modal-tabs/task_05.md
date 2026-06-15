---
status: completed
title: Live and Getting Started tab builders
type: frontend
complexity: medium
dependencies:
  - task_01
  - task_02
---

# Task 05: Live and Getting Started tab builders

## Overview
Implement the three live/dynamic tab builders: `getting_started_lines` (the default front
door — routing mental model, then runnable example prompts, then a compact live agent
summary), `commands_tab_lines` (derived from the single command catalog so it never drifts),
and `skills_tab_lines` (the discovered skills from `ui_state.skill_suggestions`). These deliver
the feature's primary new-user value and the live differentiator.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `getting_started_lines(state, theme) -> Vec<Line>` that renders, in order: a one-line routing mental model (prompt → orchestrator → named agents; approvals gate writes), at least two runnable example prompts, then a compact agent summary built via `agent_roster_items(state.agents, RosterRowStyle::Compact, theme)`.
- MUST add `commands_tab_lines(filter, theme) -> Vec<Line>` derived from `slash_commands::catalog()` (reuse/extend `help_command_lines`); in MVP `filter` is always `""` (the Phase-2 filter lands in task_09). Every catalog command MUST appear exactly once.
- MUST add `skills_tab_lines(ui_state, theme) -> Vec<Line>` listing each `SkillSuggestion` (alias, source tag) plus the "skills are guidance, do not bypass approvals/permissions" disclaimer; MUST render an empty-state line when no skills are discovered.
- MUST keep Commands single-sourced (no hardcoded command list) and MUST NOT add a `category` field to `SlashCommandSpec`.
- MUST use only theme tokens (no inline `Color::`).
</requirements>

## Subtasks
- [x] 05.1 Implement `getting_started_lines` (model line + example prompts + compact agents).
- [x] 05.2 Implement `commands_tab_lines` from the catalog with a no-op `filter` param.
- [x] 05.3 Implement `skills_tab_lines` from `skill_suggestions`, including the disclaimer and empty state.
- [x] 05.4 Add unit tests for each builder (catalog coverage, skills listing, GS content + compact rows).

> **Implementation note (compact agent rows):** `getting_started_lines -> Vec<Line>` cannot
> consume `agent_roster_items -> Vec<ListItem>` (ratatui 0.29 `ListItem` content is not publicly
> readable). The shared compact-row logic was extracted into `agent_compact_line(index,
> &AgentView, &Theme) -> Line`; `agent_roster_items`' `Compact` arm wraps it and Getting Started
> reuses it directly, preserving the single data path the requirement intends.
> **Example prompts (PRD Open Question):** chose two runnable, read-first prompts — "Summarize
> what this project does and how the run loop works." and "Find where approval mode is enforced
> and add a test for it." — marked with a `> ` prefix.

## Implementation Details
Commands content already flows through `slash_commands::help_command_lines()` /
`catalog()`; reuse it and keep any formatting helper in `slash_commands.rs` for single-sourcing.
Getting Started consumes the `Compact` style from task_02's `agent_roster_items`. Skills read
the cached `ui_state.skill_suggestions` (`SkillSuggestion` fields: `alias`, `source_tag`,
`source_origin`). The two example prompts are an Open Question in the PRD — choose real,
runnable prompts and note the choice in the PR. See TechSpec "Core Interfaces".

### Relevant Files
- `src/slash_commands.rs` — `catalog()` / `help_command_lines()` source of truth for Commands.
- `src/tui/mod.rs` — `agent_roster_items` (task_02); `AgentView` fields; test helpers `agent_view` `:6534`, `test_skill_suggestions` `:6465`.
- `src/skills/mod.rs` — `SkillSuggestion` shape.

### Dependent Files
- task_06 (tabbed render) calls all three builders.
- task_09 (Phase 2) supplies a non-empty `filter` to `commands_tab_lines`.

### Related ADRs
- [ADR-001: V1 Scope for the Tabbed Help Modal](../adrs/adr-001.md) — live Agents folded into Getting Started; live Skills tab; Commands derived from the catalog.

## Deliverables
- `getting_started_lines`, `commands_tab_lines`, `skills_tab_lines` builders.
- Unit tests with 80%+ coverage **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `commands_tab_lines("", &theme)` contains every `slash_commands::catalog()` usage exactly once (mirror the existing `help_modal_command_rows_are_catalog_derived` assertion). — `commands_tab_lines_cover_every_catalog_command_once`
  - [x] `skills_tab_lines` with `test_skill_suggestions()` lists aliases `"project-alpha"` and `"personal-beta"` and includes the guidance disclaimer. — `skills_tab_lines_list_aliases_and_disclaimer`
  - [x] `skills_tab_lines` with an empty `skill_suggestions` renders a "no skills" empty-state line (no panic). — `skills_tab_lines_render_empty_state_without_panic`
  - [x] `getting_started_lines` with two agents contains the routing mental-model phrase, ≥2 example prompts, and exactly two compact agent rows (one per agent). — `getting_started_lines_render_model_examples_and_compact_agents`
- Integration tests:
  - [ ] Rendered via task_06 when each tab is active. — deferred to task_06 (builders not yet wired into `render_help_modal`).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Commands tab has zero drift (catalog-derived); Skills reflects live suggestions.
- `colors_live_only_in_theme_module` passes; `cargo clippy --all-targets` clean.
