# Improve Default Core Runtime Agent System Prompts

## Overview

Improve the default system prompts for the six core runtime agents: orchestrator, explorer, fixer, reviewer, oracle, and consul. V1 should make each default prompt a concise runtime role contract that explains purpose, allowed behavior, prohibited behavior, capability boundaries, action-request discipline, handoff evidence, blocker reporting, and stop condition. The goal is runtime reliability, not cosmetic prompt polish.

## Problem

Current built-in and starter agent prompts are minimal. They do not consistently teach structured-runtime protocol compliance, role boundaries, or evidence expectations. This creates avoidable risk: malformed contracts, attempts to use unavailable tools, unclear handoffs, prompt drift between built-in defaults and generated starter files, and weak separation between read-only, edit-capable, review, advisory, and routing roles.

Codebase exploration found default agent text in `src/config/mod.rs` via `insert_builtin_agent`, plus generated starter config and `agents/*.md` text in `starter_config_text` and `starter_instruction_files`. Market research was limited because live web search was unavailable through this harness step, but prior analysis identified common multi-agent patterns: role specialization by work phase, protocol-first prompting, capability scoping, explicit handoffs, and verification-first completion behavior.

## Core Features

| Priority | Feature | Description |
| --- | --- | --- |
| 1 | Core role contracts | Rewrite prompts for orchestrator, explorer, fixer, reviewer, oracle, and consul as compact role contracts. |
| 2 | Capability boundaries | Add explicit capability-boundary language per role, especially read-only explorer, edit-capable fixer, review-only reviewer, advisory oracle, challenge-focused consul, and routing-only orchestrator. |
| 3 | Structured-runtime discipline | Require agents to return the requested JSON contract, request actions instead of using unavailable tools, and report blockers explicitly. |
| 4 | Completion evidence | Define role-specific completion evidence, including changed files, commands, verification, risks, or decision rationale as applicable. |
| 5 | Drift prevention | Prevent drift between built-in defaults and generated starter instruction files with a shared source of truth or alignment checks. |

## KPIs

| KPI | Target |
| --- | --- |
| Malformed structured-runtime responses | Reduce by at least 50 percent within 30 days of release. |
| Capability-boundary violations | Reduce by at least 60 percent within 30 days. |
| Completion evidence quality | Ensure at least 90 percent of fixer and reviewer completions include applicable changed files, commands, verification evidence, findings, or blocker details. |
| Ambiguous role-routing incidents | Reduce by at least 40 percent. |
| Prompt maintainability | Limit each default prompt to concise role-contract text and add drift-prevention coverage before release. |

## Feature Assessment

| Criteria | Score | Rationale |
| --- | --- | --- |
| Impact | Strong | Every structured runtime workflow depends on reliable default agents. |
| Reach | Strong | New projects and implicit built-ins use these defaults. |
| Frequency | Must do | Each delegated step depends on prompt-level role clarity. |
| Differentiation | Strong | Atelier can encode runtime semantics directly into defaults. |
| Defensibility | Maybe | Prompt wording is copyable, but runtime-specific alignment checks and telemetry can compound. |
| Feasibility | Strong | Implementation touchpoints are known in `src/config/mod.rs`. |

## Council Insights

Proceed with focused V1. Keep scope to the six core runtime agents. Avoid a large shared policy prompt that makes all agents sound the same. Do not rely only on runtime validation, because validation catches malformed output after the fact but does not guide correct behavior. Treat built-in and starter prompt alignment as a non-negotiable acceptance criterion.

## Out of Scope (V1)

| Item | Justification |
| --- | --- |
| Runtime JSON schema or action contract changes | V1 is prompt-quality infrastructure and should not expand protocol mechanics. |
| Telemetry dashboards | Useful later, but not required to improve defaults. |
| Full prompt-evaluation framework | Defer until the V1 prompts are stable and measurable failure modes are clearer. |
| Full agent roster redesign or new agents | The user clarified core runtime agents as the target. |
| Disabled or non-core bundled agents | Handle later unless separately prioritized. |

## Architecture Decision Records

- `adr-001.md`: Accepted decision to improve default system prompts for the six core runtime agents as concise role contracts and keep built-in defaults aligned with starter instruction files.

## Open Questions

1. Should prompt text be extracted into a shared source of truth now, or should V1 use snapshot/alignment tests around the two existing text surfaces?
2. What exact event fields should be used to measure malformed contracts and capability-boundary violations?
3. Should disabled bundled agents be handled in a later cleanup task after core runtime prompts stabilize?
