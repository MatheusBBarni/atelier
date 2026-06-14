---
status: completed
title: Static reference tab builders (Keys, CLI, Approvals)
type: frontend
complexity: low
dependencies:
  - task_01
---

# Task 04: Static reference tab builders (Keys, CLI, Approvals)

## Overview
Implement the three static-content tab builders: `keys_tab_lines` (keybindings),
`cli_tab_lines` (`atelier` flags), and `approvals_tab_lines` (plain-language prose on yolo vs
normal approval, capabilities, and read/write roots). These are pure functions returning
`Vec<Line>` and carry the content that exists today plus the new Approvals explainer.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `keys_tab_lines(theme) -> Vec<Line>` carrying the existing keybinding rows (currently literals at `src/tui/mod.rs:3277`): Enter, Ctrl-L, Arrow keys, PageUp/PageDown, Mouse wheel, Home/End, Ctrl-C, Backspace, Text.
- MUST add `cli_tab_lines(theme) -> Vec<Line>` carrying the existing CLI flag rows (literals at `:3290`): the `atelier` flags through `--help`.
- MUST add `approvals_tab_lines(theme) -> Vec<Line>` with static prose explaining yolo vs normal approval modes, agent capabilities, and the read/write-roots concept (new content; see PRD "Approvals & Modes" and `ApprovalMode`).
- MUST use only theme tokens for any styling; MUST NOT add inline `Color::` literals.
- Each builder MUST be pure (no `AppState`/`TuiUiState` reads) and independently testable.
</requirements>

## Subtasks
- [x] 04.1 Implement `keys_tab_lines` from the existing keybinding literals.
- [x] 04.2 Implement `cli_tab_lines` from the existing CLI-flag literals.
- [x] 04.3 Write the Approvals & Modes static prose and implement `approvals_tab_lines`.
- [x] 04.4 Add unit tests asserting key rows, CLI rows, and approval-mode terms are present.

## Implementation Details
The keybinding and CLI content lives today as literal `Line::from(...)` rows inside
`render_help_modal` (`:3277`, `:3290`); relocate them into the new builders verbatim. The
Approvals prose is net-new; keep it short and scannable (PRD "User Experience" forbids walls
of text). See TechSpec "Core Interfaces" for builder shapes.

### Relevant Files
- `src/tui/mod.rs` — current Keys literals `:3277`, CLI literals `:3290`; add builders nearby.
- `src/config/mod.rs` — `ApprovalMode` (yolo/normal) and workspace roots, for accurate prose wording.

### Dependent Files
- task_06 (tabbed render) calls these builders for the Keys/CLI/Approvals tabs.

### Related ADRs
- [ADR-001: V1 Scope for the Tabbed Help Modal](../adrs/adr-001.md) — Approvals tab is static prose in V1.

## Deliverables
- `keys_tab_lines`, `cli_tab_lines`, `approvals_tab_lines` pure builders.
- Unit tests with 80%+ coverage **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `keys_tab_lines` output contains `"Ctrl-L"`, `"PageUp/PageDown"`, and `"Home/End"`.
  - [x] `cli_tab_lines` output contains `"atelier --doctor"` and `"atelier --init-config"`.
  - [x] `approvals_tab_lines` output contains both `"yolo"` and `"normal"` and mentions read/write roots.
  - [x] No builder references `AppState`/`TuiUiState` (compiles as a free `fn(&Theme) -> Vec<Line>`).
- Integration tests:
  - [ ] Exercised via task_06 when the Keys/CLI/Approvals tabs render.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Keys/CLI content matches today's wording; Approvals prose is accurate to `ApprovalMode`.
- `colors_live_only_in_theme_module` passes; `cargo clippy --all-targets` clean.
