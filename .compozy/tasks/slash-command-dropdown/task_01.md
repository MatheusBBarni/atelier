---
status: completed
title: "Add Shared Slash Command Catalog"
type: backend
complexity: medium
dependencies: []
---

# Task 01: Add Shared Slash Command Catalog

## Overview
Create the shared metadata-only slash command catalog required by the TechSpec. This task establishes the single command metadata source that later TUI and app tasks will consume without changing command execution behavior.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST create a metadata-only catalog for exactly `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/agent:`, `/skill:`, and `/reload:skills`.
- MUST expose command label, insertion text, usage text, description, and kind/category for each entry.
- MUST NOT add dispatch callbacks, runtime state, app-event creation, or command execution behavior to the catalog.
- MUST export the catalog so both `src/tui/mod.rs` and `src/app/mod.rs` can consume it in later tasks.
- SHOULD provide formatting helpers needed by later tasks for visible command guidance.
</requirements>

## Subtasks
- [x] 1.1 Create the shared slash command metadata module.
- [x] 1.2 Add the fixed V1 command entries from the TechSpec.
- [x] 1.3 Export the module from the crate root.
- [x] 1.4 Add catalog formatting helpers required by downstream help and error guidance.
- [x] 1.5 Add unit tests proving the catalog has exactly the approved V1 command set.

## Implementation Details
Create `src/slash_commands.rs` and expose it from `src/lib.rs`. Follow the TechSpec "Core Interfaces" and ADR-003: keep the module metadata-only and avoid moving command execution out of existing TUI/app handlers.

### Relevant Files
- `src/lib.rs` — Exports top-level modules consumed across the crate.
- `src/slash_commands.rs` — New shared metadata module for the fixed V1 command catalog.
- `.compozy/tasks/slash-command-dropdown/_techspec.md` — Defines the catalog fields, fixed command set, and metadata-only boundary.

### Dependent Files
- `src/tui/mod.rs` — Later tasks will consume the catalog for help text and dropdown rows.
- `src/app/mod.rs` — Later tasks will consume the catalog for unknown-command guidance.

### Related ADRs
- [ADR-003: Use Shared Metadata-Only Slash Command Catalog](adrs/adr-003.md) — Defines the top-level metadata-only catalog boundary.
- [ADR-001: Scope Slash Command Dropdown V1](adrs/adr-001.md) — Fixes the V1 command set and prevents a command-platform expansion.

## Deliverables
- `src/slash_commands.rs` with shared command metadata and narrow formatting helpers.
- `src/lib.rs` export for the new module.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration-oriented catalog assertions for downstream visibility **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] Catalog labels are exactly `/help`, `/goal`, `/goal clear`, `/config`, `/subtask`, `/agent:`, `/skill:`, and `/reload:skills`.
  - [x] Every catalog entry has non-empty label, insert text, usage text, description, and kind.
  - [x] Prompt-prefix entries are categorized as prompt prefixes and do not imply dispatch behavior.
  - [x] TUI-local and app-command entries are categorized distinctly.
- Integration tests:
  - [x] Public catalog accessor is usable from another module without exposing mutable state.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The shared catalog contains exactly the approved fixed V1 command set.
- The catalog remains metadata-only and introduces no new command execution path.

## Follow-up Notes (recorded during execution, 2026-06-11)
- **Spec staleness — blocks task_02/task_03 scope, not task_01**: the app gained
  `/workflow <prompt>` (commit a86b94e, 2026-06-07) and `/queue`/`/q`
  (`handle_queue_command`) after the PRD/ADR command set was frozen on
  2026-06-06. `/workflow` appears today in `AVAILABLE_SLASH_COMMANDS`
  (`src/app/mod.rs:57`), the help modal (`src/tui/mod.rs:2304`), and is pinned
  by the test `unknown_slash_command_is_not_submitted_as_agent_prompt`
  (`src/app/mod.rs:8574`). Routing guidance/help through the catalog in
  task_02/task_03 as currently specified would drop `/workflow` from visible
  guidance and break that test. Decision needed before task_02: add `/workflow`
  (and decide `/queue`) to the catalog via a PRD/ADR-001 scope amendment, or
  amend task_02/task_03 to treat them as explicit catalog exceptions. task_01
  intentionally follows the frozen "exactly these 8" requirement.
