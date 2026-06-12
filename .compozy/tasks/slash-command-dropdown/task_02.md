---
status: completed
title: "Route App Unknown-Command Guidance Through Catalog"
type: backend
complexity: low
dependencies:
  - task_01
---

# Task 02: Route App Unknown-Command Guidance Through Catalog

## Overview
Update app-level unknown slash-command guidance to consume the shared slash command catalog. This removes the current hardcoded command list from the app and ensures `/reload:skills` appears in the same visible command set as the dropdown and help surfaces.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST replace hardcoded available-command guidance in app unknown-command errors with catalog-derived text.
- MUST preserve existing slash command execution behavior for `/goal`, `/config`, and `/subtask`.
- MUST preserve `/agent:` and `/skill:` prompt-prefix allowance.
- MUST preserve clarification answers that start with `/`, including `/tmp/project`.
- MUST include `/reload:skills` in unknown-command guidance.
</requirements>

## Subtasks
- [x] 2.1 Update unknown-command guidance to use the shared command catalog.
- [x] 2.2 Preserve existing named prompt-prefix handling.
- [x] 2.3 Preserve current app command handlers and execution order.
- [x] 2.4 Add assertions for catalog-derived unknown-command guidance.
- [x] 2.5 Re-run app slash-command tests affected by the guidance change.

## Implementation Details
Modify `reject_unknown_slash_command` in `src/app/mod.rs` to build available-command text from `src/slash_commands.rs`. Do not move app command handling or dispatch logic; this task is limited to visible guidance alignment.

### Relevant Files
- `src/app/mod.rs` — Contains `reject_unknown_slash_command`, app slash command handlers, and tests for unknown slash commands.
- `src/slash_commands.rs` — Provides the shared command metadata created by task_01.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines the app integration point and non-refactor boundary.

### Dependent Files
- `src/tui/mod.rs` — Uses the same catalog in later tasks, so app guidance must not require app-only formatting.
- `README.md` — Documentation should remain consistent with the visible command set after implementation.

### Related ADRs
- [ADR-003: Use Shared Metadata-Only Slash Command Catalog](adrs/adr-003.md) — Requires app guidance to consume the shared metadata.
- [ADR-002: Choose Error-Reduction Product Approach](adrs/adr-002.md) — Optimizes V1 around preventing and explaining unknown commands.

## Deliverables
- Updated app unknown-command guidance that consumes shared command metadata.
- Preserved existing app slash command behavior.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for app prompt submission behavior **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Submitting `/doctor` still returns an unknown-command error.
  - [x] Unknown-command error includes `/reload:skills`.
  - [x] Unknown-command error includes all fixed V1 command labels.
  - [x] `/agent:fixer inspect README` is still allowed as a prompt prefix.
  - [x] `/skill:reviewer inspect README` is still allowed as a prompt prefix.
- Integration tests:
  - [x] Existing clarification answer test still accepts `/tmp/project` while waiting for user input.
  - [x] Existing `/goal`, `/config`, and `/subtask` command tests still pass.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- App unknown-command guidance is catalog-derived and no longer omits `/reload:skills`.
- No command execution behavior changes are introduced.

## Follow-up Notes (recorded during execution, 2026-06-11)
- **Catalog scope amendment (resolves task_01 follow-up)**: per a recorded
  decision, `/workflow <prompt>` and `/queue <message>` (alias `/q`) were added
  to `src/slash_commands.rs` as `AppCommand` entries. Both shipped as real app
  commands after the ADR-001 freeze and were already user-visible (`/workflow`
  in app guidance + help; `/queue` handled before unknown-command rejection).
  Routing guidance purely through the frozen 8-command catalog would have
  dropped `/workflow` from guidance and broken
  `unknown_slash_command_is_not_submitted_as_agent_prompt`. The amendment keeps
  the catalog the single source of truth. `/agent:` and `/skill:` usage strings
  were also aligned to the README/help wording (`<agent_name>`/`<skill_name>`)
  so task_03's catalog-derived help rows match existing README-consistency
  tests without README churn.
- task_01's unit tests (`FIXED_V1_LABELS`, AppCommand categorization) and the
  `tests/slash_command_catalog.rs` length assertion were updated to the amended
  10-command set.
