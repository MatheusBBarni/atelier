---
status: pending
title: Rich approval modal & resolution key routing
type: frontend
complexity: high
dependencies:
  - task_05
  - task_06
---

# Task 07: Rich approval modal & resolution key routing

## Overview
Render the decision-support modal from the enriched `PendingApprovalView` and route keys to the three resolutions, with habituation-resistant controls. This delivers the PRD's "faster, confident decisions": the user sees the resolved command, diff, boundary, and risk tier, and the dangerous tier resists reflexive approval.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render the modal from the enriched `PendingApprovalView`: tier label, one-line reason, resolved command (or diff), affected paths, boundary crossed, and reversibility — with progressive disclosure (lead line + expandable detail).
- MUST route an `ApproveOnce` key, a DISTINCT `ApproveAndTrust` key (shown only when `trust_target` is `Some`), and a `Deny` key; "approve" and "approve-and-trust" MUST NOT share a keystroke.
- MUST NOT bind Enter to approve on the High tier; the catastrophic core MUST require a type-to-confirm step before approval.
- MUST convey the tier with an explicit text label (not color alone) so it holds under monochrome/`NO_COLOR`.
- MUST define any new colors as semantic tokens in `src/tui/theme.rs` (the `colors_live_only_in_theme_module` test forbids inline `Color::` in `src/tui/mod.rs`).
- MUST preserve the existing key-routing precedence (help → clarification → approval → dropdowns → input).

## Subtasks
- [ ] 07.1 Build the rich modal layout from `PendingApprovalView` (reuse the clarification options-list pattern).
- [ ] 07.2 Add risk-tier semantic tokens in `theme.rs`.
- [ ] 07.3 Map keys to `ApproveOnce` / `ApproveAndTrust` / `Deny`; hide the trust option when no target.
- [ ] 07.4 Add the High-tier non-default keystroke and catastrophic type-to-confirm.
- [ ] 07.5 Update the Keys hint for the new approval keys.
- [ ] 07.6 Add render + key-routing tests, including a monochrome case.

## Implementation Details
Work in `src/tui/mod.rs` (approval render ~2765, key routing ~1061–1143, input parse ~1492, clarification layout to reuse ~4554) and `src/tui/theme.rs` (`Theme` ~122, `resolve` ~142, the `colors_live_only_in_theme_module` test ~173). Consume the enriched `PendingApprovalView` (task_05) and the projected tier (task_06). See TechSpec "Command & Signal Surface" and PRD "User Experience".

### Relevant Files
- `src/tui/mod.rs` — approval rendering, key routing, Keys/Approvals hints.
- `src/tui/theme.rs` — semantic risk-tier tokens; the inline-color guard test.
- `src/app/mod.rs` — `PendingApprovalView`, `ApprovalResolution`, `ApprovalHandle` (consumed).

### Dependent Files
- `src/tui/mod.rs` (task_09) — help tabs documenting the keys.

### Related ADRs
- [ADR-001: V1 scope — rich decision-support modal](../adrs/adr-001.md) — modal content and habituation controls.
- [ADR-002: Phased floor rollout](../adrs/adr-002.md) — tiering and type-to-confirm for catastrophic.

## Deliverables
- Rich approval modal, risk-tier theme tokens, and three-way key routing with type-to-confirm.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test for key routing under a simulated prompt **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A `Medium`-tier view renders the tier label, reason, and resolved command lines.
  - [ ] A view with `trust_target = Some` shows the "approve & trust" option; with `None` it is absent.
  - [ ] A `High`-tier view does not map Enter to approve; the dedicated approve key is required.
  - [ ] A catastrophic view requires the type-to-confirm input before the approve action is accepted.
  - [ ] The approve-once key dispatches `ApproveOnce`; the trust key dispatches `ApproveAndTrust`; `n` dispatches `Deny`.
  - [ ] Under `NO_COLOR`, the tier is still conveyed by its text label.
  - [ ] `colors_live_only_in_theme_module` still passes after the changes.
- Integration tests:
  - [ ] FakeRuntime + simulated keys: a High-tier prompt is not approved by Enter, then is approved by the dedicated key.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The modal shows tier + resolved command/diff + boundary; approve and approve-and-trust use distinct keys; catastrophic requires type-to-confirm; no inline colors.
