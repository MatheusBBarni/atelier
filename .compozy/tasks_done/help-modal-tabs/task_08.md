---
status: completed
title: Empty-state onboarding hint in welcome facts
type: frontend
complexity: low
dependencies: []
---

# Task 08: Empty-state onboarding hint in welcome facts

## Overview
Add a one-line routing hint to the welcome facts box, beside the existing
"type /help for commands" cue, so a newcomer staring at an empty chat learns that prompts route
through an orchestrator to named agents and where to find help. The hint is render-time text,
self-gating (the welcome only renders on an empty chat) and needs no state or events.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add one muted hint line to `facts_lines` in `src/tui/welcome.rs` (near the existing "type /help for commands" line at `:345`), explaining that a described task routes through an orchestrator to named agents and pointing to `/help`.
- MUST keep the existing "type /help for commands" cue.
- MUST style the hint with a theme token (e.g., `theme.text_muted`); MUST NOT add inline `Color::` literals.
- MUST NOT introduce new state, events, or persistence; the hint is render-time only.
- SHOULD keep the copy to a single short line (PRD forbids walls of text). Final wording is a PRD Open Question — choose concise copy and note it in the PR.
</requirements>

## Subtasks
- [x] 08.1 Draft the one-line routing hint copy.
- [x] 08.2 Add the hint line to `facts_lines`, preserving the `/help` cue.
- [x] 08.3 Add a unit test in `welcome.rs` tests asserting both lines are present.

## Implementation Details
`facts_lines` (`src/tui/welcome.rs:312`) builds the welcome facts `Vec<Line>` and already ends
with the muted `/help` cue. Append the routing hint there. `welcome.rs` has its own
`#[cfg(test)] mod tests` (`:376`) with an `agent(name)` helper for `WelcomeFacts`. See TechSpec
"System Architecture" (welcome integration) and PRD "User Experience".

### Relevant Files
- `src/tui/welcome.rs` — `facts_lines` `:312`, `/help` cue `:345`, `WelcomeFacts` struct `:92`, tests module `:376`.

### Dependent Files
- None — independent of the tab work; can land in parallel.

### Related ADRs
- [ADR-001: V1 Scope for the Tabbed Help Modal](../adrs/adr-001.md) — in-flow empty-state hint as a V1 companion.
- [ADR-002: Phased Delivery Approach](../adrs/adr-002.md) — empty-state hint in MVP; first-approval explainer deferred to Phase 2 (task_10).

## Deliverables
- Routing hint line in the welcome facts box.
- Unit tests with 80%+ coverage **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `facts_lines` output contains the new routing-hint substring (e.g., "orchestrator").
  - [x] `facts_lines` still contains "type /help for commands".
  - [x] The hint line carries no inline color (the `colors_live_only_in_theme_module` scan over `src/tui/*.rs` still passes).
- Integration tests:
  - [x] With an empty chat (no events, no chat items), `render_to_text` shows the routing hint in the welcome area.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- New users see the routing hint on first launch; existing `/help` cue retained.
- `colors_live_only_in_theme_module` passes; `cargo clippy --all-targets` clean.
