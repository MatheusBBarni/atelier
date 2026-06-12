---
status: pending
title: Document the @ file picker
type: docs
complexity: low
dependencies:
  - task_09
---

# Task 10: Document the @ file picker

## Overview
Document the `@` file picker in the README "TUI commands" section, alongside the existing `/`, `/agent:`, and `/skill:` dropdowns, so users discover it and understand its behavior and safety guarantees.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a README note describing: `@` activation anywhere in the composer, fuzzy search, recents on a bare `@`, Tab/Enter to accept (inserts a bare path, folders end with `/`), Esc to dismiss, and that results respect `.gitignore` and exclude secret files.
- MUST place the note with the existing dropdown documentation in the "TUI commands" section.
- SHOULD keep tone and formatting consistent with the surrounding documentation.
- MUST keep the documented behavior consistent with the integration tests in task_09 (no claims beyond what ships).
</requirements>

## Subtasks
- [ ] 10.1 Add the `@` picker description to the README "TUI commands" section.
- [ ] 10.2 Cross-reference the shared dropdown conventions (Up/Down, Tab/Enter, Esc).
- [ ] 10.3 Verify the rendered Markdown reads correctly and matches shipped behavior.

## Implementation Details
Modify `README.md` "TUI commands" section. Documentation-only — no source changes. Reference PRD "User Experience" for the described flow. Keep it factual and aligned with the behavior verified by task_09's tests.

### Relevant Files
- `README.md` — "TUI commands" section that already documents the `/`, `/agent:`, and `/skill:` dropdowns.
- `.compozy/tasks/at-mention-file-dropdown/_prd.md` — "User Experience" describes the user-facing flow.

### Dependent Files
- None.

### Related ADRs
- [ADR-002: Package as a Complete Single-Release V1](../adrs/adr-002.md) — the V1 surface being documented.

## Deliverables
- A README "TUI commands" entry for the `@` file picker.
- Documentation verification **(REQUIRED)** — the documented keys and behavior match the task_09 integration tests.

## Tests
- Unit tests:
  - [ ] Not applicable (documentation-only change; no code paths added).
- Integration tests:
  - [ ] Documentation accuracy: the documented `@` behavior (activation, fuzzy, recents, Tab/Enter bare-path insert, folder `/`, Esc dismiss, gitignore/secret exclusion) matches the behavior asserted by task_09's integration tests.
- Test coverage target: >=80% (documentation task — no new executable code; behavior is covered by task_09)
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80% (no new code paths; documentation verified against task_09 tests)
- The README documents the `@` picker consistently with the shipped, tested behavior
