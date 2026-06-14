# PRD: Default Agent Prompts

Date: 2026-06-13
Status: Approved

## Overview

The multiagent harness depends on default role prompts to keep structured runtime agents reliable. Today, runtime failures can occur when agents return prose instead of the requested JSON contract, claim direct tool access, embed action descriptors inside result fields, blur role boundaries, or finish without enough evidence for the orchestrator to make the next decision.

This feature improves the default prompts for the six core runtime agents: orchestrator, explorer, fixer, reviewer, oracle, and consul. The prompts should act as concise role contracts that make each agent understand what it owns, what it must not do, when it must request harness actions, and what completion evidence it must provide.

The primary user is the workflow operator who runs multiagent tasks and needs reliable, contract-first behavior across planning, exploration, fixing, review, consultation, and adversarial critique. The operator should not need to infer whether an agent broke protocol, exceeded its role, or finished without evidence. Each handoff should make the next orchestration decision obvious.

## Goals

1. Reduce runtime contract violations by making structured output requirements explicit in every core default prompt.
2. Reduce role-boundary violations by giving each agent a clear behavioral contract and explicit non-goals.
3. Improve action-request discipline so agents ask the harness for file, command, edit, or verification operations instead of claiming direct tool access.
4. Improve completion evidence so successful steps include factual findings, changed files, commands, and verification when applicable.
5. Improve blocker handling so agents report specific, actionable blockers instead of vague inability claims.
6. Keep built-in default prompts and generated starter instruction files aligned so prompt behavior does not drift across surfaces.

## User Stories

- As a workflow operator, I want every runtime agent to return the exact JSON contract requested by the harness so the run can continue without parse repair or manual intervention.
- As a workflow operator, I want agents to request repository reads, commands, edits, and verification through harness actions so runtime permissions remain enforceable.
- As a workflow operator, I want explorers to gather context without editing files so discovery remains low risk.
- As a workflow operator, I want fixers to make scoped changes and verify them so implementation steps produce usable evidence.
- As a workflow operator, I want reviewers to identify risks without modifying files so review output stays independent from fixes.
- As a workflow operator, I want oracles to answer from available evidence and label uncertainty so their guidance is useful without pretending to have unseen data.
- As a workflow operator, I want consuls to challenge plans and assumptions without taking over execution so decision quality improves without disrupting ownership.
- As a workflow operator, I want blockers to name the missing permission, data, file, command result, or user decision so I can resolve the run quickly.

## Core Features

| Priority | Feature | Description |
| --- | --- | --- |
| 1 | Structured output discipline | Every core prompt explicitly requires the runtime-requested JSON contract and forbids prose outside that contract when the runtime demands structured output. |
| 1 | Action-request discipline | Agents must ask the harness for file, command, edit, or verification actions when needed instead of claiming direct tool access or embedding action descriptors in completion fields. |
| 1 | Capability boundaries | Each role must describe what it can do, what it must not do, and how to respond when the requested work exceeds its assigned capabilities. |
| 1 | Completion evidence | Agent results must report factual findings, changed files, commands, verification, and blockers in the appropriate fields instead of vague summaries. |
| 2 | Role-specific contracts | Orchestrator, explorer, fixer, reviewer, oracle, and consul each receive concise guidance tailored to their expected workflow contribution. |
| 2 | Blocker handling | Prompts must instruct agents to return a specific blocked or failed status when required input, permission, data, or runtime action is unavailable. |
| 3 | Prompt-surface alignment | Built-in default prompts and generated starter instruction files must remain aligned for the six core roles. |
| 3 | Measurable reliability language | Prompt wording should tie reliability expectations to observable failure modes such as parse errors, role violations, missing evidence, and ambiguous handoffs. |

## User Experience

A workflow operator starts or resumes a multiagent run and sees agents behave as predictable runtime participants instead of standalone coding assistants. The orchestrator plans and routes work, specialists stay within their roles, and every agent response is shaped for machine handling first.

Expected experience after this change:

1. Orchestrators choose the next step, ask targeted user questions when needed, and do not perform implementation work directly.
2. Explorers gather repository and context evidence, then return findings without touching files.
3. Fixers request the reads, edits, commands, and verification needed to complete scoped implementation work.
4. Reviewers report bugs, regressions, missing tests, and residual risk without editing files.
5. Oracles answer narrow questions from available evidence and clearly label uncertainty.
6. Consuls challenge plans, risks, and assumptions without taking over execution.
7. Every role either returns the required contract or reports a specific blocker.

The successful user experience is quiet reliability: fewer parse errors, fewer role surprises, fewer unsupported claims, and clearer handoffs for the next orchestration decision.

## High-Level Technical Constraints

- V1 covers only the six core runtime agents: orchestrator, explorer, fixer, reviewer, oracle, and consul.
- The feature must improve prompt behavior without changing JSON schemas, action contract shapes, harness permissions, or runtime protocol mechanics.
- Agents must continue to respect capability constraints supplied by the runtime for each step.
- Built-in default prompts and generated starter instruction files must use consistent role expectations.
- Prompt content should stay concise enough to reuse across runtime contexts without excessive token overhead.

## Non-Goals

- Changing runtime JSON schemas, action contract shapes, or harness protocol mechanics.
- Building telemetry dashboards for prompt failures.
- Adding new agent roles beyond the six core runtime agents.
- Rewriting the multiagent planner, scheduler, or action execution system.
- Creating configurable prompt profiles or per-project prompt customization.
- Defining implementation-level code structure, storage, parsing, or testing strategy.
- Solving all possible model behavior issues through prompting alone.

## Phased Rollout Plan

### Phase 1: Prompt Contract Definition

Define the desired role contract for each of the six core agents. Each contract must cover role ownership, allowed behavior, forbidden behavior, action-request expectations, completion evidence, and blocker handling.

Success criteria to proceed:

- Each of the six core roles has a complete role contract.
- Prompt text is concise and unambiguous.
- Contract-first behavior is explicit in every prompt.

### Phase 2: Prompt Surface Alignment

Apply the approved prompt language to the relevant default prompt surfaces. Ensure built-in defaults and generated starter instruction files describe the same behavior for the same role.

Success criteria to proceed:

- Built-in defaults and generated starter prompts are aligned.
- No role has conflicting instructions across surfaces.
- Prompt text does not imply direct tool access where harness actions are required.

### Phase 3: Scenario Validation

Validate the prompts against representative workflow scenarios that exercise planning, discovery, fixing, review, question answering, adversarial critique, blocked actions, and structured output handling.

Success criteria to proceed:

- Agents return the requested structured contracts in representative scenarios.
- Agents request harness actions when repository data, commands, edits, or verification are needed.
- Agents report blockers with actionable detail when they cannot proceed.

### Phase 4: Release And Monitor

Release the improved defaults and monitor operator-visible reliability signals. Use observed parse errors, role-boundary issues, missing evidence, and manual intervention cases to guide follow-up improvements.

Success criteria to proceed:

- Contract violations decline after release.
- Operator intervention caused by malformed agent output declines.
- Prompt drift between built-in and starter surfaces remains absent.

## Success Metrics

| Metric | Target |
| --- | --- |
| Structured output parse failures | Reduce by at least 50 percent within 30 days of release. |
| Capability-boundary violations | Reduce by at least 60 percent within 30 days of release. |
| Action-request misuse | Reduce cases where agents claim direct tool access or embed action descriptors in result fields. |
| Completion evidence quality | At least 90 percent of applicable fixer and reviewer completions include verification evidence or a specific blocker. |
| Ambiguous role-routing incidents | Reduce by at least 40 percent. |
| Prompt-surface alignment | Built-in defaults and generated starter instruction files remain aligned for all six core roles. |

## Risks and Mitigations

| Risk | Mitigation |
| --- | --- |
| Prompt wording becomes too verbose and increases runtime overhead. | Keep each prompt as a concise role contract focused on observable behavior. |
| Agents become over-constrained and report blockers when they could continue. | Make blocker guidance specific: block only when required input, permission, data, or action results are unavailable. |
| Prompt changes improve one role while creating new ambiguity in another. | Review the six role contracts together and validate cross-role handoffs. |
| Built-in prompts and generated starter files drift over time. | Treat prompt-surface alignment as an acceptance requirement for every prompt change. |
| Success metrics lack clean historical baselines. | Use available runtime logs and operator-reported incidents as the initial baseline, then refine measurement after release. |
| Users expect this feature to fix runtime protocol problems outside prompt behavior. | State clearly that V1 improves default prompt behavior while leaving protocol mechanics out of scope. |

## Architecture Decision Records

- [ADR-001: Improve Default Core Runtime Agent System Prompts](adrs/adr-001.md) - Accepted the overall direction to improve the six core runtime agent prompts as concise role contracts for reliable structured operation.
- [ADR-002: Prioritize Contract-First Reliability For Default Agent Prompts](adrs/adr-002.md) - Accepted contract-first reliability as the V1 product approach, focused on reducing contract violations and making handoffs predictable for workflow operators.

## Open Questions

- Which observed runtime failures should count in the initial 30-day baseline for structured output parse failures?
- Which representative workflow scenarios should be used as acceptance examples before release?
- Should future versions support configurable prompt profiles for different operator preferences after the default prompts stabilize?
- How should prompt-surface alignment be checked when new starter instruction templates are added later?
