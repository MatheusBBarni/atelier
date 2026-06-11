---
status: completed
title: "Update Runtime Prompt Contracts"
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 03: Update Runtime Prompt Contracts

## Overview
Update runtime-facing prompt contracts so every orchestrator-capable runtime knows how to return structured clarification options. This task prevents schema drift after the orchestrator decision contract expands.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST update Codex, Claude, and Cursor prompt examples to include `clarifying_options` and `recommended_option_id`.
- MUST update orchestrator guidance so `waiting_for_user` decisions include 2-4 concise recommended answers.
- MUST state that the app provides a custom text path, so runtimes should not emit a custom option as one of the 2-4 recommended options unless product requirements change.
- MUST include option count and recommended-id guidance in prompt text or prompt-adjacent tests.
- MUST NOT add a new direct runtime question tool or any-agent request contract.
- SHOULD verify Z.ai coverage through shared orchestrator guidance rather than a separate embedded schema if no hardcoded prompt example exists.
</requirements>

## Subtasks
- [x] 3.1 Update Codex runtime protocol text for structured clarification fields.
- [x] 3.2 Update Claude runtime protocol text for structured clarification fields.
- [x] 3.3 Update Cursor runtime protocol text for structured clarification fields.
- [x] 3.4 Update shared orchestrator guidance for option count, recommendation, and custom-answer boundary.
- [x] 3.5 Add prompt-contract tests that guard the new fields and guidance.

## Implementation Details
Follow TechSpec sections "Runtime Prompt Contracts" and "Known Risks". Keep this task scoped to prompt contract text and tests; validation belongs to task 02 and app/TUI behavior belongs to later tasks.

### Relevant Files
- `src/runtime/codex.rs` — Embeds Codex prompt text and prompt contract tests.
- `src/runtime/claude.rs` — Embeds Claude prompt text and prompt contract tests.
- `src/runtime/cursor.rs` — Embeds Cursor prompt text and prompt contract tests.
- `src/orchestrator/mod.rs` — Builds shared orchestrator instructions used by runtime requests.
- `src/runtime/zai.rs` — Uses shared parsing/guidance and should be checked for prompt-specific assumptions.

### Dependent Files
- `src/orchestrator/mod.rs` — Contract examples must match the schema from task 01 and validation from task 02.
- `src/runtime/fake.rs` — Fake runtime examples should stay aligned with the same structured option shape.
- `.compozy/tasks/agent-question-tool/_techspec.md` — Defines no any-agent question tool for V1.

### Related ADRs
- [ADR-003: Implement Clarification Select As Structured Orchestrator State](adrs/adr-003.md) — Requires prompt contract updates across runtime adapters.
- [ADR-002: Select Focused Clarification Select Approach](adrs/adr-002.md) — Keeps runtime changes scoped to orchestrator clarification.

## Deliverables
- Runtime prompt examples include structured clarification fields.
- Shared orchestrator guidance tells runtimes when and how to provide recommended options.
- Tests preventing prompt contract regression.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration-compatible prompt contract assertions for all orchestrator-capable runtimes **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Codex prompt text includes `clarifying_options`, `recommended_option_id`, and the 2-4 option rule.
  - [x] Claude prompt text includes `clarifying_options`, `recommended_option_id`, and the 2-4 option rule.
  - [x] Cursor prompt text includes `clarifying_options`, `recommended_option_id`, and the 2-4 option rule.
  - [x] Shared orchestrator prompt includes guidance to ask a Clarifying Question with recommended answers when the next safe step is ambiguous.
  - [x] Prompt text does not describe a general any-agent question tool.
- Integration tests:
  - [x] Existing runtime prompt contract tests still pass after the expanded schema text is added.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Runtime prompt contracts describe the same schema validated by the orchestrator.
- No runtime prompt introduces V1 behavior outside the approved orchestrator clarification flow.
