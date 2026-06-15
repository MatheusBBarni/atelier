---
status: pending
title: /trust list & revoke command
type: backend
complexity: medium
dependencies:
  - task_04
  - task_06
---

# Task 08: /trust list & revoke command

## Overview
Add the `/trust` command so users can inspect and revoke their session trust entries — the visible, revocable counterpart that answers the "approve once, exploit forever" risk. It follows the existing app-command pattern and records audit events that the transcript already projects.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST register `/trust` as an `AppCommand` in `src/slash_commands.rs` (keeping the dropdown, help overlay, and catalog tests aligned).
- MUST implement `handle_trust_command` following the `handle_config_command`/`handle_goal_command` pattern (returns `Ok(true)` when handled, usage error otherwise).
- MUST support `/trust` (list entries, numbered, with "this session only" scope), `/trust revoke <n>` (1-based index from the last listing), and `/trust clear`.
- MUST record `trust_revoked` and `trust_cleared` events (and a listing/`trust_listed` view event), reusing the task_06 projection.
- MUST report a clear usage message for malformed input (e.g., `/trust revoke abc`, out-of-range index) without mutating the store.

## Subtasks
- [ ] 08.1 Add the `/trust` catalog entry and keep `slash_command_catalog` tests aligned.
- [ ] 08.2 Implement `handle_trust_command` (list / revoke / clear) and dispatch it from the command handler.
- [ ] 08.3 Record the trust list/revoke/clear events.
- [ ] 08.4 Add unit tests for each subcommand and a FakeRuntime re-arm test.

## Implementation Details
Work in `src/slash_commands.rs` (`CATALOG` ~48, `SlashCommandKind` ~13) and `src/app/mod.rs` (`handle_config_command` ~1196 as the pattern; dispatch alongside it; `record_event` ~4164). Operate on the `TrustStore` from task_04; events are projected by task_06. See TechSpec "Command & Signal Surface".

### Relevant Files
- `src/slash_commands.rs` — command catalog (single source for dropdown/help/guidance).
- `src/app/mod.rs` — `handle_trust_command`, command dispatch, `TrustStore`.
- `tests/slash_command_catalog.rs` — catalog assertions to keep aligned.

### Dependent Files
- `src/app/chat/projection.rs` (task_06) — projects the trust events `/trust` emits.

### Related ADRs
- [ADR-004: In-memory exact-target session trust](../adrs/adr-004.md) — `/trust` is the list-and-revoke surface; restart is the fallback reset.

## Deliverables
- `/trust` command (list/revoke/clear) with catalog registration and audit events.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- FakeRuntime integration test for revoke re-arming the prompt **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `/trust` with an empty store reports "no trusted actions".
  - [ ] `/trust` with two entries lists both, numbered, with "this session only".
  - [ ] `/trust revoke 1` removes the first entry and emits `trust_revoked`.
  - [ ] `/trust revoke 9` (out of range) and `/trust revoke abc` report usage and leave the store unchanged.
  - [ ] `/trust clear` empties the store and emits `trust_cleared` with the count.
  - [ ] The catalog contains `/trust` as an `AppCommand` (catalog test).
- Integration tests:
  - [ ] FakeRuntime: approve-and-trust an action, `/trust revoke 1`, then the agent re-emits it → the modal is raised again (re-armed).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/trust` lists, revokes by index, and clears; malformed input is rejected without mutation; revoking re-arms the prompt.
