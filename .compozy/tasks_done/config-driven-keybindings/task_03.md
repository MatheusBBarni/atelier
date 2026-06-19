---
status: completed
title: Reserved-key single chokepoint
type: refactor
complexity: medium
dependencies: []
---

# Reserved-key single chokepoint

## Overview
Consolidate the three duplicated `Ctrl-C` handling branches into one reserved-key guard at the top
of key routing that returns the interrupt command before any other matching. This is
behavior-preserving today and makes the kill-switch structurally unshadowable — a prerequisite for
safely consulting a user keymap (task_04/task_08).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a single reserved-key guard at the top of `key_event_to_tui_command_with_ui`
  (`src/tui/mod.rs:1056`) that returns `DispatchAndQuit(RunInterruptRequested)` for the reserved
  `Ctrl-C` event before the modal cascade and normal routing.
- MUST remove (or route through the guard) the redundant `Ctrl-C` arms at `src/tui/mod.rs:1070`,
  `1123`, and `1394` so the kill-switch has exactly one source.
- MUST preserve `Ctrl-C` behavior in EVERY context (help, clarification, approval, dropdowns,
  normal input) — observable output identical to today.
- MUST keep the reserved key non-bindable by construction (it is excluded from the bindable
  allowlist in task_01); the guard is the runtime enforcement half.
</requirements>

## Subtasks
- [x] 3.1 Add the reserved-key guard at the top of `key_event_to_tui_command_with_ui`.
- [x] 3.2 Remove the now-redundant `Ctrl-C` branches at the three sites (plus the newer governance branch — see notes).
- [x] 3.3 Confirm behavior is unchanged across all modal contexts.
- [x] 3.4 Add a regression test asserting `Ctrl-C` → interrupt in every context.

## Implementation Details
All within `src/tui/mod.rs`: the wrapper `key_event_to_tui_command_with_ui` (`:1056`), the
existing `Ctrl-C` arms at `:1070` (help branch), `:1123` (clarification branch), and `:1394` (base
fn), and the `DispatchAndQuit` execution at `:770`. See TechSpec "Impact Analysis" (routing row)
and ADR-004 (structural reserved guard). This is a behavior-preserving refactor; do not change the
interrupt semantics, only their single point of definition.

### Relevant Files
- `src/tui/mod.rs` — routing wrapper, the three `Ctrl-C` arms, `DispatchAndQuit` execution.

### Dependent Files
- `src/tui/mod.rs` routing (task_04) — the keymap lookup is added immediately after this guard.

### Related ADRs
- [ADR-004: Config Trust Boundary and Validation Severity](adrs/adr-004.md) — single pre-lookup guard.
- [ADR-001: V1 Scope](adrs/adr-001.md) — reserved safety set.

## Deliverables
- A single reserved-key guard; the three duplicated `Ctrl-C` arms removed.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test confirming interrupt-and-quit still fires via the run loop path **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `key_event_to_tui_command_with_ui` returns `DispatchAndQuit(RunInterruptRequested)` for `Ctrl-C` when help is visible.
  - [x] Same for clarification pending, approval pending, governance pending, and plain normal input (`ctrl_c_interrupts_in_every_context`).
  - [x] A non-`Ctrl-C` key in each of those contexts is unaffected (`non_ctrl_c_keys_route_normally_in_each_context`).
- Integration tests:
  - [x] Existing app/run-loop tests that depend on `Ctrl-C` interrupting an active run still pass unchanged (no routing test regressed; passed count rose by exactly the 2 new tests).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `Ctrl-C` interrupts identically in all contexts; only one definition of the reserved binding remains.
- No change to any other key's routing.

## Implementation Notes
- The single guard (`is_reserved_interrupt` + early-return at the top of
  `key_event_to_tui_command_with_ui`) replaced **four** Ctrl-C arms, not three: the
  help, clarification, and base-fn arms named in the spec, **plus** a newer governance
  branch (`pending_governance_decision`) added on this same branch after the task was
  authored. Consolidating it too was required by "exactly one source" / "preserve in
  EVERY context"; leaving it would have been a second definition.
- The base `key_event_to_tui_command` no longer matches Ctrl-C (the guard owns it before
  any base call in prod). The existing `ctrl_c_is_the_only_exit_key` unit test, which
  called the base fn directly, was updated to assert via the wrapper (the real entry point).
- Verified `2026-06-16`: `ctrl_c_*` (4) + `non_ctrl_c_*` (1) pass; `cargo clippy
  --all-targets` clean; `cargo fmt --check` clean. Full `cargo test --lib`: passed rose
  1008 → 1010 (exactly the 2 new tests); the 12 remaining failures are the unchanged
  pre-existing environmental skill-discovery + codex-CLI failures.
