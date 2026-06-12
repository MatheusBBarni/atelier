# Workflow Command

## Overview

`/workflow <prompt>` is an execution-oriented workflow mode for `atelier`. It helps solo developers hand the Harness a broad engineering Prompt, have the Orchestrator decompose it into Specialized Agent work, execute safe steps, validate outcomes, and return one consolidated result.

V1 should be a complete common workflow, not planning-only: decompose, execute, validate, and synthesize. It should remain inside one normal Run and use Parallel Step Groups only when scopes are safe and enabled. If Parallel Step Groups are disabled, `/workflow` should fail with a clear workflow-specific message rather than silently degrade to a planning-only or sequential alias.

## Problem

Developers increasingly use AI coding tools for real work, but broad tasks still fail in predictable ways: weak decomposition, incomplete follow-through, unclear validation, and polished summaries that hide uncertainty. The problem is not just speed. The core user pain is trust: a solo developer wants to know what the Harness planned, which files each Specialized Agent handled, what actually changed, what verification ran, and which risks remain.

Claude Code Dynamic Workflows, Cursor `/multitask`, and OpenAI Codex show the market moving toward coordinated multi-agent coding. These products establish expectations around decomposition, parallel specialized work, and consolidated review. They also expose the adoption constraints: higher runtime cost, unclear completion semantics, and user concern over whether complex AI work is accurate enough to trust.

`atelier` can compete by making orchestration provider-neutral and Harness-owned. The Orchestrator may plan and delegate, but the Run Plan, file scopes, Harness Actions, Action Approval, validation evidence, Session History, and final answer remain visible inside the Harness boundary.

### Market Data

- Claude Dynamic Workflows launched on May 28, 2026 as a research preview for parallel subagent orchestration, with explicit warnings that workflows can consume substantially more tokens than a normal Claude Code session.
- Anthropic's agent guidance identifies orchestrator-worker workflows as useful for complex tasks where subtasks cannot be predicted upfront, including multi-file coding tasks.
- Cursor added `/multitask` on April 24, 2026 to split larger requests across async subagents.
- OpenAI Codex positions parallel coding work around isolated environments, logs, and test evidence.
- Stack Overflow's 2025 Developer Survey reports that 84% of respondents use or plan to use AI tools in their development process, but complex-task handling and output accuracy remain trust concerns.
- JetBrains AI Pulse reported that 90% of professional developers use at least one AI tool at work and 74% use specialized developer AI tools.

## Summary / Differentiator

Unlike runtime-owned workflow scripts, `/workflow` should preserve Harness-owned execution. The differentiator is not maximum fan-out in V1; it is reliable, inspectable coordination. The user gets broad-task execution with explicit plan visibility, scoped agent work, validation evidence, skipped-check reporting, and unresolved-risk disclosure.

## Core Features

| #   | Feature | Priority | Description |
| --- | ------- | -------- | ----------- |
| F1 | Workflow Slash Command | Critical | `/workflow <prompt>` starts one workflow-mode Run and rejects empty or malformed usage. |
| F2 | Execution-Oriented Decomposition | Critical | The Orchestrator creates a Run Plan and executes Specialized Agent steps, not just a plan report. |
| F3 | Parallel-Required Workflow Mode | Critical | If Parallel Step Groups are disabled, `/workflow` fails with a clear workflow-specific message instead of silently degrading. |
| F4 | Safe Parallel Step Use | Critical | Uses Parallel Step Groups only when file scopes are exact, disjoint, and safe. |
| F5 | Completion Gate | Critical | Workflow completion requires finishing the planned file-edit targets or explicitly reporting skipped, blocked, or failed targets. |
| F6 | Validation Pass | High | Schedules verification after grouped or mutation-capable work and records evidence or skipped-check reasons. |
| F7 | Evidence-First Final Answer | High | Final summary includes plan, child outcomes, changed files, commands, verification, skipped checks, and unresolved risks. |
| F8 | Workflow Visibility | High | Chat and Agent Roster show active workflow steps, scopes, and statuses during execution. |

## Integration with Existing Features

| Integration Point | How |
| ----------------- | --- |
| Slash command parsing | Add `/workflow <prompt>` before unknown slash-command rejection. |
| Run / Run Plan | Start one normal Run with a workflow-mode prompt envelope. |
| Parallel Step Groups | Reuse existing group planning, child execution, scope validation, and joined result behavior. |
| Harness Actions | Keep reads, edits, commands, approvals, and VCS policy under existing enforcement. |
| Chat / Agent Roster | Surface plan, active children, validation, and final synthesis as workflow evidence. |
| Session History | Record a distinct `workflow_started` event and preserve workflow evidence for later review. |

## KPIs

| KPI | Target | How to Measure |
| --- | ------ | -------------- |
| Run Plan visibility | >= 80% | Workflow runs emit an explicit plan before mutation-capable action. |
| Coordinated execution | >= 70% | Applicable workflow runs schedule 2+ Specialized Agent steps. |
| Verification coverage | >= 90% | Completed workflow runs include verification evidence or skipped-check reason. |
| Scope ambiguity | <= 5% | Child steps are blocked by preventable scope ambiguity. |
| Final evidence quality | >= 80% | Final summaries include child outcomes, changed files, verification, and residual risks. |

## Feature Assessment

| Criteria | Question | Score |
| -------- | -------- | ----- |
| **Impact** | How much more valuable does this make the product? | Must do |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Strong |
| **Defensibility** | Is this easy to copy or does it compound over time? | Strong |
| **Feasibility** | Can we actually build this? | Strong |

Leverage type: Strategic Bet, with Compounding Feature potential.

## Council Insights

- **Recommended approach:** Build `/workflow <prompt>` as one evidence-first Run mode, not a separate workflow engine.
- **Key trade-offs:** A thin implementation improves delivery speed, but the user-facing contract must expose evidence so workflow mode does not overpromise.
- **Risks identified:** prompt-only orchestration, cost and latency, ambiguous file scopes, and pressure to bypass Harness Actions.
- **Stretch goal (V2+):** saved workflow templates, worktrees, background execution, reusable workflow libraries, and stronger model-routing controls.

## Out of Scope (V1)

- **Saved workflow scripts** - Premature persistence and trust model.
- **Direct Specialized Agent delegation** - Would bypass Orchestrator-owned Run Plan semantics.
- **Worktree-isolated children** - Adds merge, cleanup, and branch complexity.
- **Autonomous background execution** - Adds recovery, interruption, and approval lifecycle complexity.
- **Unbounded fan-out** - Conflicts with conservative local execution and cost control.
- **Planning-only workflow mode** - The feature is explicitly execution-oriented.

## Cost Estimate

| Type | Volume | Estimated Cost |
| ---- | ------ | -------------- |
| Runtime usage | 2-4 Specialized Agent steps per workflow | Higher than normal prompts; bounded by max child count and run limits. |
| Local execution | Verification commands after grouped work | Depends on repo test cost; should be visible in final evidence. |
| User attention | Plan review, approvals, and final evidence review | Higher than a normal prompt, but lower than manually coordinating agents. |

## Architecture Decision Records

- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - V1 keeps `/workflow` inside one normal Run, fails when Parallel Step Groups are disabled, and requires visible workflow evidence.

## Open Questions

- The TechSpec must refine the exact completion contract: how planned file-edit targets are represented, updated, checked, and mapped to successful, skipped, blocked, or failed outcomes.
- The TechSpec must define the payload shape for `workflow_started` and whether it carries mode metadata, original command text, or both.
- The TechSpec must define how the final summary proves that all planned file-edit targets were finished or explicitly accounted for.

## Sources

- Claude: Introducing dynamic workflows in Claude Code - https://claude.com/blog/introducing-dynamic-workflows-in-claude-code
- Claude: A harness for every task - https://claude.com/blog/a-harness-for-every-task-dynamic-workflows-in-claude-code
- Anthropic: Building effective agents - https://www.anthropic.com/engineering/building-effective-agents
- Cursor: Multitask, Worktrees, and Multi-root Workspaces - https://cursor.com/changelog/04-24-26
- OpenAI Codex - https://openai.com/codex/
- Stack Overflow 2025 AI survey - https://survey.stackoverflow.co/2025/ai
- JetBrains AI Pulse - https://blog.jetbrains.com/research/2026/04/which-ai-coding-tools-do-developers-actually-use-at-work/
