# Skill Prompt Loading

## Overview

Implement deterministic prompt loading for `/skill:<skill_name>` in Atelier. Existing Harness users should be able to invoke one or more known skills in a normal prompt and trust that the selected `SKILL.md` content is loaded into the composed runtime prompt.

V1 covers the common flow: multiple skill references, alias resolution, duplicate handling, validated loading, structured prompt framing, project-first precedence, and propagation into child prompts, council prompts, and subtasks. The feature optimizes for correctness, not broad skill-platform behavior.

## Problem

Atelier already suggests `/skill:<name>` entries in the TUI, but the app currently records and dispatches the raw prompt unchanged. A user can type `/skill:reviewer inspect README`, yet the selected skill is not resolved, validated, or injected into the model context.

That mismatch makes skill invocation unreliable. Users see a command-like affordance, but the runtime only receives text that it may or may not interpret. This weakens repeatable workflows and makes failures hard to debug.

V1 should make `/skill:<name>` an explicit, deterministic instruction source. The app should resolve each skill before run creation, compile the prompt with clear section boundaries, and record which skills were applied.

### Market Data

OpenAI Codex, Claude Code, and Cursor all treat skills as reusable workflow units. Codex supports explicit skill use and inserts selected skill context for the next request. The Agent Skills spec recommends progressive disclosure: metadata first, full `SKILL.md` on activation.

A 2026 arXiv analysis of 40,285 public skills found skills emerging as agent infrastructure, especially for software engineering workflows. Security research on `SKILL.md` supply-chain attacks shows skill text can shape discovery, trust, and use, so Harness should treat loaded skill content as operational prompt code, not passive documentation.

## Summary / Differentiator

Atelier can differentiate by making skill invocation deterministic and auditable: resolve exact skills before runtime dispatch, inject each skill once with provenance, propagate explicit workflow context into derived prompts, and keep Harness Actions as the enforcement boundary.

## Core Features

| # | Feature | Priority | Description |
| --- | --- | --- | --- |
| F1 | Skill reference parsing | Critical | Parse multiple `/skill:<name>` references anywhere in submitted prompts across all submit paths. |
| F2 | Canonical resolution | Critical | Resolve aliases to canonical skill identities with project roots preferred over personal roots. |
| F3 | Duplicate handling | Critical | Dedupe by canonical skill identity while preserving first-use order. |
| F4 | Structured prompt framing | Critical | Compose `System Prompt`, ordered `Skill: <name>` sections, and `User Prompt`. |
| F5 | Provenance metadata | High | Record skill name, source/origin, canonical identity, and load reason in history or diagnostics. |
| F6 | Clear validation errors | High | Fail before run creation for unknown, ambiguous, unreadable, invalid, or unsupported skills. |
| F7 | Derived prompt propagation | High | Carry compiled skill context into child prompts, council prompts, and subtasks. |

## Integration With Existing Features

| Integration Point | How |
| --- | --- |
| `src/tui/mod.rs` skill discovery | Reuse discovery concepts, but move authoritative resolution into shared app infrastructure. |
| `src/app/mod.rs` prompt submission | Resolve and compile skill-backed prompts before run creation. |
| `src/runtime/mod.rs` prompt envelope | Keep runtime adapters generic; pass the composed prompt string through the existing envelope. |
| `.compozy/tasks/skill-prompt-loading/adrs/adr-001.md` | Captures the V1 boundary and alternatives. |

## KPIs

| KPI | Target | How to Measure |
| --- | --- | --- |
| Skill resolution accuracy | `>= 99%` | Table-driven resolver tests plus manual dogfood prompts. |
| Duplicate injection rate | `0` | Prompt snapshot tests for repeated skill aliases and IDs. |
| Missing-context regressions | `-80%` | Track dogfood failures where expected skill instructions were absent. |
| Prompt provenance coverage | `100%` | Assert skill-backed runs record skill name, origin, identity, and load reason. |
| Validation timing | `100% before runtime dispatch` | Tests verify invalid skills never create a run. |
| Propagation coverage | `100%` | Tests verify child, council, and subtask prompts retain explicit skill context. |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Strong |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe |
| **Feasibility** | Can we actually build this? | Strong |

Leverage type: Compounding Feature

## Council Insights

- **Recommended approach:** Build `/skill:` as app-owned prompt loading, not runtime-specific command handling.
- **Key trade-offs:** Common-flow support is broader than a one-skill MVP, but it matches the user-selected scope and avoids misleading partial behavior.
- **Risks identified:** Ambiguous skill identities, stale TUI cache, prompt injection, delimiter spoofing, literal `/skill:` parsing surprises, and future runtime-native skill APIs.
- **Stretch goal (V2+):** Introduce a normalized `SkillInvocation` contract with adapter-specific rendering when runtimes support first-class skills.

## Out of Scope (V1)

- **Implicit skill matching** - Explicit `/skill:` loading should work before automatic selection.
- **Shell expansion or dynamic commands** - This changes the trust model and needs separate approval design.
- **Skill-granted capabilities** - Harness Actions remain the only execution authority.
- **Runtime-native skill APIs** - V1 renders text through the existing prompt envelope.
- **Full skill registry UI** - Useful later, but not required to fix prompt loading.

## Architecture Decision Records

- [ADR-001: Scope Skill Prompt Loading V1](adrs/adr-001.md) - App-owned resolver and prompt compiler with generic runtime delivery.

## Open Questions

- What alias source should V1 support, if any beyond frontmatter names and directory names?

## Sources

- [OpenAI Codex skills](https://developers.openai.com/codex/skills)
- [Codex CLI slash commands](https://developers.openai.com/codex/cli/slash-commands)
- [Agent Skills specification](https://agentskills.io/specification)
- [Cursor skills changelog](https://cursor.com/changelog/2-4)
- [Agent Skills data-driven analysis](https://arxiv.org/abs/2602.08004)
- [SKILL.md supply-chain research](https://arxiv.org/abs/2605.11418)
