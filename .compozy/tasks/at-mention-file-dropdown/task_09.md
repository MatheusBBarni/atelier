---
status: pending
title: Wire routing and render chains with integration tests
type: frontend
complexity: medium
dependencies:
  - task_07
  - task_08
---

# Task 09: Wire routing and render chains with integration tests

## Overview
Make the feature live by slotting `@` activation into both the key-routing chain and the render chain (after the skill branch, kept in sync), dispatching the file-mention commands to their handlers, removing staging shims, and adding end-to-end integration tests including a routing-vs-render parity check.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add the `file_mention_dropdown(..)` branch to `key_event_to_tui_command_with_ui`, positioned after the skill branch.
- MUST add the matching branch to the render `if-let` chain in the same relative position, keeping the two chains in sync (the in-code invariant).
- MUST dispatch `TuiCommand::FileMentionDropdown(..)` to the handlers from task_07 in the command executor.
- MUST remove any `#[allow(dead_code)]` staging shims introduced by tasks 04–08.
- MUST add an integration test asserting routing↔render parity (whenever activation is `Some`, the render path draws the dropdown).
</requirements>

## Subtasks
- [ ] 9.1 Add the activation branch to the key-routing chain after the skill branch.
- [ ] 9.2 Add the render branch to the render chain in the same position.
- [ ] 9.3 Dispatch the file-mention command variant in the executor.
- [ ] 9.4 Remove staging `#[allow(dead_code)]` shims.
- [ ] 9.5 Add end-to-end and parity integration tests.

## Implementation Details
Modify `src/tui/mod.rs`: the `key_event_to_tui_command_with_ui` precedence chain (after skill, before/around the command branch), the render `if-let` chain (same relative position), and the `TuiCommand` executor. See TechSpec "Component Overview" and "Impact Analysis" (the routing/render sync invariant).

### Relevant Files
- `src/tui/mod.rs` — `key_event_to_tui_command_with_ui` precedence chain, the render `if-let` chain, and the `TuiCommand` executor/dispatch.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Component Overview", "Impact Analysis".

### Dependent Files
- `README.md` — task_10 documents the now-live feature.

### Related ADRs
- [ADR-005: Component Placement and Dropdown Integration](../adrs/adr-005.md) — chain placement and the sync invariant.

## Deliverables
- `@` activation wired into both chains, command dispatch, and removal of staging shims.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- End-to-end integration tests **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] With an active `@` token, key routing returns a `FileMentionDropdown` command rather than normal input.
  - [ ] During `pending_approval` the `@` branch is skipped and the key is treated as normal input.
  - [ ] The render chain draws the file dropdown when active and draws nothing while the help overlay is visible.
- Integration tests:
  - [ ] End-to-end: typing `look at @run`, pressing Down then Enter rewrites the buffer to `look at <ranked-path> ` and dispatches no run.
  - [ ] Parity: for inputs where `file_mention_dropdown(..)` is `Some`, the render pass produces the dropdown overlay (the two chains agree).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The `@` dropdown is live in both chains and in sync; staging shims are removed; end-to-end and parity tests pass
