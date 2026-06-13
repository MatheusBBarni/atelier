---
status: pending
title: HelpTab enum and roster row style types
type: frontend
complexity: low
dependencies: []
---

# Task 01: HelpTab enum and roster row style types

## Overview
Introduce the foundational value types for the tabbed help modal: a `HelpTab` enum that
identifies the six tabs and provides ordered iteration/navigation, and a `RosterRowStyle`
enum (`Full`/`Compact`) used later by the shared agent-row builder. These pure types unblock
every other task and carry no rendering or state.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define `enum HelpTab { GettingStarted, Commands, Keys, Skills, Approvals, Cli }` in `src/tui/mod.rs`.
- MUST provide `HelpTab::ALL` in declared left-to-right order (Getting Started first, CLI last), plus `title() -> &'static str`, `next()`, and `prev()` that wrap around `ALL`.
- MUST define `enum RosterRowStyle { Full, Compact }` for later reuse by the roster builder.
- MUST derive `Clone, Copy, Debug, PartialEq, Eq` on `HelpTab` so it can live in `TuiUiState` and be asserted in tests.
- MUST NOT introduce any rendering, state mutation, or color literals (honor `colors_live_only_in_theme_module`).
</requirements>

## Subtasks
- [ ] 01.1 Add the `HelpTab` enum with the six variants in tab order.
- [ ] 01.2 Implement `ALL`, `title()`, `next()`, and `prev()` with wrap-around semantics.
- [ ] 01.3 Add the `RosterRowStyle` enum.
- [ ] 01.4 Add unit tests covering ordering, titles, and next/prev wrap-around.

## Implementation Details
Add both enums near the other UI types at the top of `src/tui/mod.rs` (the `TuiCommand` enum
sits at `:91`). Keep them as plain enums with an `impl HelpTab` block. See TechSpec
"Core Interfaces" for the exact shape of `HelpTab` and `RosterRowStyle`.

### Relevant Files
- `src/tui/mod.rs` — host module for TUI types; add the enums here (`TuiCommand` at `:91`, `TuiUiState` at `:191`).

### Dependent Files
- None directly; every downstream task (02–09) imports these types.

### Related ADRs
- [ADR-003: Tabbed Help Modal — Technical Architecture](../adrs/adr-003.md) — mandates `enum HelpTab` + per-tab builders and rejects a stateful tab widget.

## Deliverables
- `HelpTab` enum with `ALL`/`title`/`next`/`prev`.
- `RosterRowStyle` enum.
- Unit tests with 80%+ coverage **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `HelpTab::ALL` has 6 entries and starts with `GettingStarted`, ends with `Cli`.
  - [ ] `HelpTab::GettingStarted.next() == Commands`; `HelpTab::Cli.next() == GettingStarted` (wrap).
  - [ ] `HelpTab::GettingStarted.prev() == Cli` (wrap); `HelpTab::Commands.prev() == GettingStarted`.
  - [ ] `HelpTab::Skills.title() == "Skills"` (spot-check one multi-word title, e.g. `GettingStarted.title() == "Getting Started"`).
- Integration tests:
  - [ ] N/A — pure types with no render path; covered by unit tests (note in PR).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `cargo clippy --all-targets` clean; `colors_live_only_in_theme_module` still passes.
- Types compile and are usable from `TuiUiState` and builder functions.
