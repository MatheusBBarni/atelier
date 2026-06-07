# Product Requirements: Clarification Select UI

Status: Draft
Date: 2026-06-06
Source Idea: `.compozy/tasks/agent-question-tool/_idea.md`

## Overview

Clarification Select UI upgrades the existing orchestrator Clarifying Question flow in `atelier`. When a Run pauses because the Orchestrator needs user input, the Input Composer changes from free-text prompt entry into a focused select experience with 2-4 recommended answers and a custom answer option.

The feature is for human operators using the TUI. Its primary value is reducing abandoned `WaitingForUser` runs by making the required decision obvious, fast, and distinct from Action Approval.

## Goals

- Reduce abandoned waiting runs by 50% after release.
- Resume at least 85% of Runs that pause for a Clarifying Question.
- Keep median time from question display to answer under 30 seconds.
- Ensure 100% of clarification requests and answers are visible in Chat and Session History.
- Keep V1 focused on orchestrator Clarifying Questions only.

## User Stories

1. As a TUI operator, I want a paused Run to show the question in the Input Composer, so I immediately know what input is needed.
2. As a TUI operator, I want recommended answers, so I can unblock the Run without writing a custom response.
3. As a TUI operator, I want a custom answer option, so I can respond when none of the recommendations fit.
4. As a TUI operator, I want clarification questions to look different from Action Approval, so I do not confuse a product decision with a safety approval.
5. As a maintainer, I want the question and answer to appear in Session History, so later review explains why the Run changed direction.

## Core Features

1. **Clarification Composer State**  
   When the Orchestrator asks a Clarifying Question, the Input Composer presents the question as the active interaction instead of accepting a new Prompt.

2. **Recommended Answers**  
   Every V1 Clarifying Question shows 2-4 recommended answers. One option may be visually recommended by default when the question has a clear likely answer.

3. **Custom Answer Option**  
   The final option always lets the operator type a custom answer when recommended answers are insufficient.

4. **Answer Or Interrupt Flow**  
   V1 supports answering the question or interrupting the Run. It does not include skip as a separate action.

5. **Visible Pause Context**  
   Chat shows that the Run is waiting for a Clarifying Question and records the selected or custom answer once submitted.

6. **Session History Auditability**  
   Session History records compact lifecycle events for the question being requested and answered.

## User Experience

Primary flow:

1. The user submits a Prompt.
2. The Run proceeds until the Orchestrator cannot safely continue without clarification.
3. Chat indicates the Run is waiting for a Clarifying Question.
4. The Input Composer changes into a select-style question view.
5. The user chooses one recommended answer or selects custom text.
6. The Run resumes with the answer included as user clarification.
7. Chat and Session History show what was asked and what answer was provided.

UX requirements:

- The composer must make the waiting state obvious without requiring the user to scan raw logs.
- The answer options should be concise enough to choose quickly.
- The custom answer path must be visible as the last option.
- The interaction must remain keyboard-friendly.
- Interrupt must remain available while a question is pending.
- Clarifying Questions must use wording and status distinct from Action Approval.

## High-Level Technical Constraints

- Preserve the existing domain boundary: the Orchestrator asks Clarifying Questions in V1.
- Preserve Session History as the durable record and Chat as the user-facing projection.
- Do not move Harness Actions, Action Approval, or runtime execution into the TUI.
- Interactive question UI applies to TTY/TUI use. Noninteractive behavior should fail clearly or remain unchanged until a future phase.

## Non-Goals (Out of Scope)

- Any-agent question requests.
- Skip as a separate V1 answer outcome.
- Multiple simultaneous questions.
- Multi-select questions.
- Rich question history browser.
- Headless or CI question handling.
- General form, wizard, or validation-schema support.
- Replacing Action Approval.

## Phased Rollout Plan

### MVP (Phase 1)

- Show Clarifying Questions as a composer select state.
- Require 2-4 recommended answers plus custom text.
- Support answer or interrupt.
- Show pending question and answer in Chat.
- Record compact Session History events.

Success criteria: at least 85% of question-paused Runs resume after an answer in local validation scenarios.

### Phase 2

- Improve option wording based on observed custom-answer frequency.
- Add clearer Chat summaries for repeated question patterns.
- Add documentation for operators explaining Clarifying Questions versus Action Approval.

Success criteria: recommended-option usage reaches at least 70%.

### Phase 3

- Revisit broader interaction needs after V1 data exists.
- Evaluate whether any-agent questions or headless question handling should become separate features.

Success criteria: V1 abandonment reduction target is met without increasing user confusion around approvals.

## Success Metrics

| Metric | Target | Measurement |
|---|---:|---|
| Abandoned waiting Runs | -50% | Compare unresolved `WaitingForUser` Runs before and after release. |
| Question continuation rate | >= 85% | Share of question-paused Runs that resume after answer. |
| Median answer time | < 30s | Time from question shown to answer submitted. |
| Recommended-answer usage | >= 70% | Share of answers submitted from recommended options. |
| History completeness | 100% | Every question request and answer appears in Session History. |

## Risks and Mitigations

- **Users confuse questions with approvals**: Use distinct labels, status text, and Chat item wording.
- **Recommended answers are poor**: Keep custom answer mandatory and track custom-answer usage.
- **Users want to skip**: Keep interrupt available and defer skip until there is evidence it is needed.
- **Question UI adds friction**: Keep the flow to one decision and one submit action.
- **Market parity without differentiation**: Focus differentiation on clean TUI flow and durable run explainability.

## Architecture Decision Records

- [ADR-001: Scope Clarification Select UI](adrs/adr-001.md) — V1 upgrades existing orchestrator clarification instead of creating a broad agent question protocol.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — The PRD uses the focused clarification-select path and keeps any-agent questions out of V1.

## Open Questions

- What exact user-facing label best distinguishes Clarifying Questions from Action Approval?
- Should the custom answer option be labeled `Custom answer`, `Other`, or something domain-specific?
- What minimum documentation should ship with V1 so operators understand answer-or-interrupt behavior?
