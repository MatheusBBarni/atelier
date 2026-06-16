---
status: pending
title: TUI governance decision card and key routing
type: frontend
complexity: medium
dependencies:
  - task_02
  - task_03
---

# TUI governance decision card and key routing

## Overview
Make the governance decision interactive in the terminal: render the decision card (intent, approach, agent, write-scope, risk label) and route keys to Accept or Reject while a decision is pending, slotting it into the existing key-routing precedence.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render the pending governance decision as a card showing intent, approach, agent, write-scope, and the plain-language risk label, using theme tokens only (no inline color literals).
- MUST add a key-routing branch for `pending_governance_decision` in the precedence cascade, placed between clarification and approval.
- MUST map an explicit Accept key and a Reject key (with optional redirect via the input line) to the resolve dispatch; the safe default must not land on Accept.
- MUST exclude governance from `queue_control_active` so a pending decision suppresses queue keys.
- MUST keep the card legible under monochrome/`NO_COLOR` (risk conveyed by text).
</requirements>

## Subtasks
- [ ] 4.1 Add the governance branch to the key-routing cascade (between clarification and approval).
- [ ] 4.2 Implement `governance_decision_key_command` (accept/reject/redirect).
- [ ] 4.3 Render the decision card from the pending view.
- [ ] 4.4 Update `queue_control_active` to exclude a pending governance decision.

## Implementation Details
Modify `src/tui/mod.rs`. Add the branch in the cascade (~1117), a `governance_decision_key_command` modeled on `clarification_key_command` (~1317), a card render modeled on the approval/clarification cards, and the `queue_control_active` exclusion (~1150). All color via `src/tui/theme.rs` tokens (the `colors_live_only_in_theme_module` guard test forbids inline `Color::`). Reference TechSpec "User Experience" for card content.

### Relevant Files
- `src/tui/mod.rs` — key-routing cascade (~1078-1143), `clarification_key_command` (~1317), `queue_control_active` (~1150), card render helpers.
- `src/tui/theme.rs` — semantic color tokens.
- `src/app/mod.rs` — `AppState.pending_governance_decision` (task_03) read for routing/render.

### Dependent Files
- None; this is the interactive surface.

### Related ADRs
- [ADR-003: Unified GovernanceDecision data model + single pending_governance_decision state](../adrs/adr-003.md) — the shared card surface.
- [ADR-004: Single-agent turn-1 early-abort mechanism](../adrs/adr-004.md) — this card surfaces the early-abort.

## Deliverables
- A rendered governance decision card.
- A key-routing branch + `governance_decision_key_command`.
- `queue_control_active` exclusion.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An integration/render test of the card **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] With `pending_governance_decision` set, the cascade routes the accept key to a resolve-Accept dispatch and the reject key to a resolve-Reject dispatch (precedence over approval/input).
  - [ ] `queue_control_active()` returns false while a governance decision is pending.
  - [ ] The default/Enter key does not resolve to Accept (no accidental approve).
- Integration tests:
  - [ ] Rendering a `PendingGovernanceDecisionView` produces card lines containing the intent, agent, write-scope, and an explicit risk-tier word; the render holds under `NO_COLOR`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A pending governance decision is visible and answerable from the TUI with correct precedence; legible under monochrome.
- `cargo fmt --check`, `cargo clippy --all-targets`, and the `colors_live_only_in_theme_module` guard pass.
