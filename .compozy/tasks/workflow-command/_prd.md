# Workflow Command PRD

## Overview

`/workflow <prompt>` is an execution-oriented workflow mode for `atelier`. It lets a solo developer submit a broad implementation or refactor Prompt and receive decomposed Specialized Agent work, validation evidence, and one consolidated final result.

V1 optimizes for trustworthy completion: the user can see what was planned, what ran, which file-edit targets were completed, which targets were skipped, blocked, or failed, what verification ran, and what risks remain.

## Goals

- Let users run broad implementation or refactor Prompts without manually coordinating multiple agent prompts.
- Make workflow completion inspectable through plan, target, child outcome, verification, and risk evidence.
- Fail clearly when workflow prerequisites are unavailable.
- Preserve existing user controls: Action Approval, Clarifying Questions, and run interrupt.
- Support completed-with-issues when all unfinished planned targets are explicitly accounted for.

## User Stories

- As a solo developer, I want `/workflow <prompt>` to decompose and execute broad code changes so I can delegate larger work safely.
- As a solo developer, I want a final summary that names completed and unfinished planned targets so I can trust the result.
- As a maintainer, I want verification evidence and skipped-check reasons so I can decide whether follow-up work is needed.
- As a cautious user, I want normal approvals and interrupt behavior to remain active during workflow runs.

## Core Features

| Feature | Priority | Requirement |
| ------- | -------- | ----------- |
| Workflow command | Critical | `/workflow <prompt>` starts one workflow-mode Run and rejects missing prompt text. |
| Parallel prerequisite check | Critical | If Parallel Step Groups are unavailable, the user sees a clear workflow-specific failure. |
| Executing decomposition | Critical | The workflow decomposes broad implementation or refactor work and executes Specialized Agent steps. |
| Planned target accounting | Critical | Planned file-edit targets must end as completed, skipped, blocked, or failed. |
| Completed-with-issues result | Critical | Unfinished targets do not hide inside success; they produce a consolidated completed-with-issues answer. |
| Evidence-first final answer | High | Final result includes plan, target status, child outcomes, changed files, verification, skipped checks, and risks. |
| Existing controls | High | Existing approvals, Clarifying Questions, and run interrupt remain the V1 control model. |

## User Experience

1. User enters `/workflow <prompt>` in the Input Composer.
2. If workflow prerequisites are unavailable, `atelier` shows a clear failure before starting execution.
3. The workflow starts and records that the Run is in workflow mode.
4. Chat shows the Run Plan and active Specialized Agent work.
5. Agent Roster shows workflow child activity and status.
6. The workflow executes scoped work, requests normal approvals when needed, and validates after mutation-capable work.
7. The final answer separates completed targets from skipped, blocked, or failed targets and includes verification evidence.

## High-Level Technical Constraints

- Workflow mode must remain inside one normal Run.
- Workflow mode must preserve Harness-owned actions, approvals, limits, cancellation, and Session History.
- Workflow mode must use existing Parallel Step Group semantics and respect configured concurrency limits.
- Workflow mode must record a distinct `workflow_started` history event.
- The TechSpec must refine the exact planned-target completion contract.

## Non-Goals

- Saved workflow scripts or reusable workflow files.
- Direct Specialized Agent delegation.
- Worktree-isolated workflow children.
- Autonomous background workflow execution.
- Unbounded fan-out.
- Planning-only workflow mode.
- Mandatory plan approval before execution.

## Phased Rollout Plan

### MVP

- `/workflow <prompt>` command.
- Clear failure when workflow prerequisites are unavailable.
- Executing decomposition for broad implementation or refactor Prompts.
- Planned target accounting.
- Evidence-first final result.

### Phase 2

- Better workflow discoverability and examples.
- Optional workflow templates for common user journeys.
- More precise completed-with-issues UX.

### Phase 3

- Saved workflow definitions.
- Worktree-backed isolation.
- Background workflow execution.
- Richer workflow history and resumption.

## Success Metrics

- >= 80% of workflow runs show an explicit Run Plan before mutation-capable action.
- >= 70% of applicable workflow runs schedule 2+ Specialized Agent steps.
- >= 90% of completed workflow runs include verification evidence or skipped-check reasons.
- <= 5% of child steps block due to preventable scope ambiguity.
- >= 80% of final summaries include child outcomes, target status, verification, and residual risks.

## Risks and Mitigations

- **Overpromising reliability:** Make unfinished targets and skipped checks explicit.
- **User confusion when prerequisites are disabled:** Fail early with a workflow-specific message.
- **Evidence overload:** Prioritize target status, verification, and residual risk in the final answer.
- **Higher runtime cost:** Keep conservative concurrency limits and make workflow mode intentional.
- **False confidence in completed-with-issues:** Label incomplete target states clearly.

## Architecture Decision Records

- [ADR-001: Workflow Command Uses One Evidence-First Run](adrs/adr-001.md) - Keeps `/workflow` inside one normal Run and requires workflow evidence.
- [ADR-002: Evidence-First Executing Workflow Approach](adrs/adr-002.md) - Selects executing workflow mode over plan-gated or investigation-only variants.

## Open Questions

- How exactly should planned file-edit targets be represented, updated, and matched to outcomes?
- What fields should the `workflow_started` event include?
- What final-summary evidence is mandatory before a workflow can be marked completed or completed-with-issues?

## Sources

- Claude: Introducing dynamic workflows in Claude Code - https://claude.com/blog/introducing-dynamic-workflows-in-claude-code
- Anthropic: Building effective agents - https://www.anthropic.com/engineering/building-effective-agents
- Cursor: Multitask, Worktrees, and Multi-root Workspaces - https://cursor.com/changelog/04-24-26
- OpenAI Codex - https://openai.com/codex/
- Stack Overflow 2025 AI survey - https://survey.stackoverflow.co/2025/ai
- JetBrains AI Pulse - https://blog.jetbrains.com/research/2026/04/which-ai-coding-tools-do-developers-actually-use-at-work/
