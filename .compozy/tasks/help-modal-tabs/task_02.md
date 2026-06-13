---
status: pending
title: Extract shared agent_roster_items builder
type: refactor
complexity: low
dependencies:
  - task_01
---

# Task 02: Extract shared agent_roster_items builder

## Overview
Extract the inline per-agent rendering loop used by the Ctrl-L Agent Roster into a shared
`agent_roster_items(agents, style, theme)` builder, parameterized by `RosterRowStyle`. This
removes the only obstacle to the Getting Started tab showing live agents without duplicating
the roster's rendering logic.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `fn agent_roster_items(agents: &[AgentView], style: RosterRowStyle, theme: &Theme) -> Vec<ListItem>`.
- `Full` MUST reproduce the current roster row exactly: line 1 name + status, line 2 `runtime/model` + availability, line 3 `effort` + thinking state.
- `Compact` MUST render one line per agent: name · `runtime/model` · availability label.
- MUST refactor the inline roster loop (`src/tui/mod.rs:2120`) to call the builder with `RosterRowStyle::Full`; the Ctrl-L roster output MUST be byte-for-byte unchanged.
- MUST reuse existing helpers (`agent_status_label`, `status_style`, `availability_label`, `availability_style`, `theme.accent_for`); MUST NOT add inline `Color::` literals.
</requirements>

## Subtasks
- [ ] 02.1 Add the `agent_roster_items` builder with `Full` and `Compact` arms.
- [ ] 02.2 Replace the inline roster loop at `:2120` with a `Full`-style call.
- [ ] 02.3 Add a regression test proving the Ctrl-L roster render is unchanged.
- [ ] 02.4 Add unit tests for `Full` (3 lines/agent) and `Compact` (1 line/agent), including availability styling.

## Implementation Details
The current roster builds a `Vec<ListItem>` inline in `render` (`src/tui/mod.rs:2120`). Move
that mapping into the new free function and call it from both the roster and (later, task_05)
the Getting Started tab. Helper signatures are confirmed in TechSpec "Integration Points";
`theme.accent_for(index)` cycles agent colors. See TechSpec "Core Interfaces" for the builder
signature.

### Relevant Files
- `src/tui/mod.rs` — roster loop at `:2120`; helpers `status_style` `:3366`, `agent_status_label` `:3380`, `availability_style` `:3388`, `availability_label` `:3408`.
- `src/tui/theme.rs` — `accent_for` `:162`.

### Dependent Files
- `src/tui/mod.rs` (Ctrl-L roster render) — switches to the shared builder.
- task_05 (Getting Started builder) — consumes `Compact`.

### Related ADRs
- [ADR-003: Tabbed Help Modal — Technical Architecture](../adrs/adr-003.md) — shared `agent_roster_items` with `Full`/`Compact`, one data path, no duplication.

## Deliverables
- `agent_roster_items` builder used by the Ctrl-L roster (`Full`).
- Unit tests with 80%+ coverage **(REQUIRED)**
- Regression test that the roster render is unchanged **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] `agent_roster_items(&[a,b], Full, &theme)` returns items whose flattened text has 3 lines per agent.
  - [ ] `agent_roster_items(&[a], Compact, &theme)` returns one line containing the name, `runtime/model`, and availability label.
  - [ ] An agent with `availability: Some(RuntimeAvailabilityStatus::Unavailable)` renders the `"down"` label (build via the `agent_view(...)` helper or an `AgentView` literal as at `:4131`).
- Integration tests:
  - [ ] With `roster_visible == true` and two agents, `render_to_text_with_ui` still shows each agent's name, `runtime/model`, and availability after extraction (Ctrl-L roster regression).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Ctrl-L roster output identical to pre-refactor; `colors_live_only_in_theme_module` passes.
- `cargo clippy --all-targets` clean.
