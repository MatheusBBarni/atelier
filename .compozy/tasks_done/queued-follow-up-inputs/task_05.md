---
status: completed
title: "Align Queue Command Discoverability And Documentation"
type: docs
complexity: medium
dependencies:
  - task_01
  - task_04
---

# Task 05: Align Queue Command Discoverability And Documentation

## Overview
Finish the feature by making `/queue` and `/q` discoverable across the visible command surfaces and documentation. This task aligns help text, unknown-command guidance, README command docs, and any available shared slash-command catalog without expanding the MVP into a general command-platform feature.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST include `/queue <message>` and `/q <message>` in TUI help.
- MUST include queue command guidance in active-run rejection text.
- MUST include `/queue` and `/q` in unknown-command guidance where command lists are shown.
- MUST update README or equivalent user-facing command documentation.
- SHOULD integrate with `src/slash_commands.rs` if the shared slash-command catalog exists.
- MUST NOT require the full slash-command-dropdown feature to be completed before queued follow-ups can work.
</requirements>

## Subtasks
- [ ] 5.1 Update TUI help rows for queue commands.
- [ ] 5.2 Update active-run rejection guidance to mention `/queue`.
- [ ] 5.3 Update unknown-command guidance or command metadata to include queue commands.
- [ ] 5.4 Update README command documentation.
- [ ] 5.5 Add tests that prevent command guidance drift.
- [ ] 5.6 Run focused regression tests for command help and app guidance.

## Implementation Details
Update documentation and command visibility after the queue feature works. If the shared slash-command catalog from `.compozy/tasks/slash-command-dropdown` is implemented, add queue commands to that catalog and consume it. If it is not implemented yet, update current hardcoded help and guidance in the existing files while keeping the future catalog integration straightforward.

### Relevant Files
- `src/tui/mod.rs` — Contains help modal rows and TUI render tests.
- `src/app/mod.rs` — Contains unknown-command and active-run rejection guidance.
- `README.md` — Contains public TUI command documentation.
- `src/slash_commands.rs` — Optional shared command catalog if present from slash-command-dropdown work.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines shared command metadata direction that should be respected when present.
- `.compozy/tasks/queued-follow-up-inputs/_prd.md` — Requires help text and slash-command suggestions for discoverability.

### Dependent Files
- `.compozy/tasks/slash-command-dropdown/task_01.md` — May create shared command metadata consumed by this task if that feature lands first.
- `.compozy/tasks/slash-command-dropdown/task_03.md` — May route TUI help through the shared catalog.
- `.compozy/tasks/slash-command-dropdown/task_04.md` — May provide slash-command suggestions that should include queue commands when present.

### Related ADRs
- [ADR-002: Select Explicit Queue-Next MVP For PRD](adrs/adr-002.md) — Requires discoverability through help and slash-command suggestions.
- [ADR-003: App-Owned Queue State And Replay](adrs/adr-003.md) — Notes command metadata drift as a risk.

## Deliverables
- Help text and command guidance that mention `/queue` and `/q`.
- README command documentation for queued follow-up inputs.
- Shared command catalog integration when available, or equivalent current-surface updates when not.
- Drift-prevention tests for help/guidance visibility.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for user-facing command guidance **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Help modal rendering includes `/queue <message>`.
  - [ ] Help modal rendering includes `/q <message>`.
  - [ ] Unknown-command guidance includes queue commands when listing available commands.
  - [ ] Active-run normal-prompt rejection mentions `/queue`.
  - [ ] Shared command catalog includes queue commands if `src/slash_commands.rs` exists.
- Integration tests:
  - [ ] README command examples align with the visible TUI command labels.
  - [ ] Existing slash-command dropdown tests still pass when the shared catalog is present.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/queue` and `/q` are discoverable in TUI help and user-facing docs.
- Active-run rejection guides users toward explicit queueing.
- The implementation works whether the shared slash-command catalog has already landed or not.
