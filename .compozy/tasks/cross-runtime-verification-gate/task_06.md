---
status: pending
title: /review command wiring
type: backend
complexity: medium
dependencies:
  - task_05
---

# Task 06: /review command wiring

## Overview
Expose the review engine as the user-facing `/review` slash command — catalog entry, dispatch guard, and handler — so a developer can request an independent review of the working diff on demand. `/review` is the discoverable entry point that mirrors Claude Code's `/code-review` (ADR-002/004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `/review` entry of kind `AppCommand` to the `slash_commands.rs` `CATALOG` (the single source feeding the dropdown, help overlay, and unknown-command guidance).
- MUST update the three catalog enumeration tests (`FIXED_V1_LABELS`, `catalog_labels_are_exactly_the_fixed_v1_set`, `tui_local_and_app_command_entries_are_categorized_distinctly`).
- MUST add a dispatch guard for `handle_review_command` in `submit_prompt_with_source` BEFORE `reject_unknown_slash_command`.
- MUST implement `handle_review_command` (modeled on `handle_provider_status_command`) that acquires the diff (task_02), computes the producer set (task_04), invokes the engine (task_05), and returns `Ok(true)`.
- MUST surface a clear message when there are no working changes / not a git repo.
</requirements>

## Subtasks
- [ ] 06.1 Add the `/review` catalog entry and update the three enumeration tests.
- [ ] 06.2 Add the dispatch guard before unknown-command rejection.
- [ ] 06.3 Implement `handle_review_command` invoking the engine.
- [ ] 06.4 Handle the no-changes / no-repo case with a clear message.
- [ ] 06.5 Integration-test `/review` end to end and its presence in help/catalog.

## Implementation Details
Modify `src/slash_commands.rs` (`CATALOG` `:49`; enumeration tests `:172`, `:189`, `:251`) and `src/app/mod.rs` (dispatch chain `:1934`; handler modeled on `handle_provider_status_command` `:2235`; `reject_unknown_slash_command` `:9966`). Help is auto-derived from the catalog — no separate help edit. See TechSpec "Command & Event Surface".

### Relevant Files
- `src/slash_commands.rs` — `CATALOG` and the three enumeration tests that hard-code the V1 label set.
- `src/app/mod.rs` — dispatch chain (`:1934`), handler template (`:2235`), `reject_unknown_slash_command` (`:9966`).

### Dependent Files
- `src/app/chat/projection.rs` (task_07) renders the events this handler causes.

### Related ADRs
- [ADR-002: On-request `/review` command](../adrs/adr-002.md).
- [ADR-004: Handler acquires the diff and runs the opinion-only reviewer](../adrs/adr-004.md).

## Deliverables
- `/review` catalog entry, dispatch guard, and `handle_review_command`.
- Updated catalog enumeration tests.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for the `/review` flow **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] The catalog contains `/review` categorized as `AppCommand`; `FIXED_V1_LABELS` and the two catalog tests pass with the new label.
  - [ ] `reject_unknown_slash_command` still rejects an unknown command such as `/revieww`.
- Integration tests:
  - [ ] `app.submit_prompt("/review")` on a dirty repo invokes the engine and records review events.
  - [ ] `/review` on a clean repo (or non-repo) surfaces a clear "no working changes to review" message and dispatches no reviewer.
  - [ ] `/review` appears in the help/dropdown command listing derived from the catalog.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/review` is discoverable in the catalog/help and routes to the review engine
- The no-changes/no-repo path is handled with a clear message and no reviewer dispatch
