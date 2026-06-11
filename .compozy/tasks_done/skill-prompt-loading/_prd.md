# Skill Prompt Loading PRD

## Overview

Skill Prompt Loading makes `/skill:<skill_name>` a deterministic user action in Atelier. Existing Harness users can reference one or more skills in a normal prompt and trust that the selected skill instructions are loaded before the run starts.

V1 focuses on the deterministic common flow: every `/skill:name` occurrence invokes a skill, project skills take precedence over personal skills, `.agents/skills` takes precedence over `.claude/skills`, frontmatter `name` and directory name both work as aliases, duplicate skills load once, and the run/history surface records what loaded.

## Goals

- Make explicit skill invocation reliable for existing Harness users.
- Ensure invalid skill references fail before run creation.
- Give users concise feedback showing which skills loaded and from which source.
- Preserve runtime compatibility by treating skill loading as prompt behavior, not a runtime-specific command.
- Reduce failed runs caused by missing skill context.

## User Stories

- As an existing Harness user, I want `/skill:reviewer inspect README` to load the reviewer skill so the run follows the intended workflow.
- As a power user, I want multiple skill references in one prompt so I can combine reusable workflows.
- As a user with project and personal skills, I want predictable precedence so I know which skill will apply.
- As a user debugging a run, I want history to show loaded skill names and sources so I can explain the model context.
- As a user with a typo or missing skill, I want a clear diagnostic before the run starts so I do not waste a model run.

## Core Features

| Priority | Feature | Requirement |
| --- | --- | --- |
| Critical | Explicit invocation | Every `/skill:name` occurrence in a submitted prompt invokes a skill. |
| Critical | Skill resolution | Project skills beat personal skills; within a tier, `.agents/skills` beats `.claude/skills`. |
| Critical | Alias support | Users can invoke a skill by frontmatter `name` or directory name. |
| Critical | Duplicate handling | Repeated references to the same resolved skill load once, preserving first-use order. |
| Critical | Fail-closed diagnostics | Unknown, unreadable, invalid, or ambiguous skills stop the run before creation and show suggested matches. |
| High | Success feedback | Skill-backed runs record a concise note listing loaded skill names and sources. |
| High | Derived prompt continuity | Child prompts, council prompts, and subtasks retain the explicitly loaded skill context. |

## User Experience

1. The user enters a prompt containing one or more `/skill:name` references.
2. Atelier resolves all skill references before starting the run.
3. If every reference resolves, Atelier starts the run and records a concise loaded-skills note in run/history.
4. If any reference fails, Atelier does not create the run and shows a diagnostic with suggested matching skills.
5. If the same resolved skill appears multiple times, Atelier loads it once and records it once.
6. If a project and personal skill share a name, the project skill wins.
7. If `.agents/skills` and `.claude/skills` both contain the same skill in the same tier, `.agents/skills` wins.

## High-Level Technical Constraints

- Skill loading must apply to all prompt submission paths, not only TUI autocomplete.
- Skill content must not grant new action authority. Harness Actions remain the enforcement boundary.
- Runtime adapters must not need to interpret `/skill:` syntax for V1.
- Skill loading must preserve enough provenance for users to audit which skill source affected a run.
- V1 must not execute scripts, shell substitutions, or dynamic commands from skill files.

## Non-Goals

- Implicit skill matching based on prompt meaning.
- Skill registry, trust dashboard, or expanded preview UI.
- Skill-granted capabilities or tool permissions.
- Runtime-native skill APIs.
- Shell expansion, dynamic resources, or executable skill workflows.
- Escaping or literal mention ergonomics beyond the selected V1 rule that every `/skill:name` occurrence invokes a skill.

## Phased Rollout Plan

### MVP (Phase 1)

- Resolve every `/skill:name` occurrence.
- Support frontmatter `name` and directory name aliases.
- Apply project-first and `.agents/skills`-first precedence.
- Dedupe repeated resolved skills.
- Fail closed before run creation with suggestions.
- Record concise success feedback in run/history.
- Propagate explicit skill context into derived prompts.

Success criteria: all core user flows work deterministically, and invalid references never start a run.

### Phase 2

- Improve discoverability around why a skill resolved to a specific source.
- Add richer diagnostics for ambiguous or conflicting skill names.
- Consider a literal mention escape affordance if user friction appears.

Success criteria: users can resolve confusion without inspecting files manually.

### Phase 3

- Explore registry, trust, preview, implicit matching, and runtime-native skill rendering.

Success criteria: broader skill-management features have clear demand beyond explicit invocation.

## Success Metrics

| Metric | Target |
| --- | --- |
| Resolution accuracy | `>= 99%` in resolver scenarios and dogfood prompts |
| Invalid skill run creation | `0` runs created with unresolved required skills |
| Duplicate skill injection | `0` duplicate loaded-skill entries per run |
| Success feedback coverage | `100%` of skill-backed runs record loaded skill names and sources |
| Missing-context regressions | Reduce by `>= 80%` in dogfood/manual QA |
| Derived prompt continuity | `100%` of child/council/subtask prompts retain explicit skill context |

## Risks And Mitigations

- **Users may unintentionally invoke skills when mentioning `/skill:name` literally.** Mitigation: document the V1 behavior and revisit escaping in Phase 2 if it causes friction.
- **Strict failure behavior may block users who expected best-effort runs.** Mitigation: show clear diagnostics and suggested matches.
- **Same-name skills may surprise users.** Mitigation: record the loaded source and apply explicit precedence.
- **Skill text may include unsafe instructions.** Mitigation: do not let skill content grant capabilities; keep Harness Actions as the enforcement boundary.
- **Users may expect preview UI.** Mitigation: keep V1 feedback concise and defer registry/preview surfaces.

## Architecture Decision Records

- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - App-owned skill prompt loading with generic runtime delivery.
- [ADR-002: Select Deterministic Common Flow For PRD](adrs/adr-002.md) - PRD scope uses the confirmed deterministic common-flow behavior.

## Open Questions

- None blocking for V1.
- Future consideration: should Phase 2 add an escape syntax for literal `/skill:name` mentions?

## Sources

- [OpenAI Codex skills](https://developers.openai.com/codex/skills)
- [Codex CLI slash commands](https://developers.openai.com/codex/cli/slash-commands)
- [Claude Code slash commands](https://code.claude.com/docs/en/slash-commands)
- [Cursor 2.4 skills changelog](https://cursor.com/changelog/2-4)
- [SKILL.md supply-chain research](https://arxiv.org/abs/2605.11418)
