---
status: pending
title: TUI security report card rendering and /help entry
type: frontend
complexity: low
dependencies:
  - task_04
  - task_06
---

# Task 07: TUI security report card rendering and /help entry

## Overview
Render the `SecurityReview` chat item kind in the TUI using the existing semantic theme tokens (severity badge/colors, body line styles), and add a one-line `/security-review` entry to the `/help` overlay describing its scope and advisory nature. This is the final user-visible polish that makes the report card read like a credible security report (ADR-003).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render `ChatItemKind::SecurityReview` in the TUI chat item path (`src/tui/mod.rs`), mapping status/severity to the existing badge and title styling and rendering the body lines.
- MUST use only semantic theme tokens from `src/tui/theme.rs` — NO inline `Color::` literals (the `colors_live_only_in_theme_module` guard test must continue to pass).
- MUST add a `/help` overlay line for `/security-review` stating it reviews the branch diff and is advisory.
- The card MUST visually distinguish severities (Critical/High vs Medium/Low/Info) using existing risk/status tokens, and MUST NOT present a green "secure" affordance for zero findings.
- MUST keep the rendering a pure function of the `ChatItemView` (no app-state reads), consistent with the existing render path.

## Subtasks
- [ ] 7.1 Add the `SecurityReview` arm to the chat-item rendering match, mapping to badge/title/body styling.
- [ ] 7.2 Ensure severity colors come from theme tokens (risk/status) only.
- [ ] 7.3 Add the `/security-review` line to the `/help` overlay.
- [ ] 7.4 Add a rendering test and confirm the no-inline-color guard still passes.

## Implementation Details
Extend the chat-item rendering in `src/tui/mod.rs` (`chat_item_lines` ~4412; severity/badge styling ~4608-4712) and the `/help` overlay content. Reuse theme tokens from `src/tui/theme.rs` (`status_ok/warn/error`, `risk_low/medium/high`, `accent`). Most rendering is generic over `ChatItemView`; this task adds the kind-specific label/treatment and the help line. See TechSpec "Impact Analysis" (tui row) and ADR-003 (report shape). The `ChatItemView` data is produced by task_04; invocation by task_06.

### Relevant Files
- `src/tui/mod.rs` — `chat_item_lines` (~4412), badge/severity styling (~4608-4712), the `/help` overlay; `colors_live_only_in_theme_module` guard test.
- `src/tui/theme.rs` — semantic tokens (`status_*`, `risk_*`, `accent`).
- `src/app/chat/mod.rs` — `ChatItemKind::SecurityReview` (task_04) being rendered.

### Dependent Files
- (None — this is the leaf presentation task.)

### Related ADRs
- [ADR-003: Scope-honest security report card](../adrs/adr-003.md) — visual report shape, no "secure" affordance.

## Deliverables
- TUI rendering for `ChatItemKind::SecurityReview` using theme tokens only.
- `/help` overlay entry for `/security-review`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration/render test asserting the card renders without inline colors **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Rendering a `SecurityReview` `ChatItemView` with a Critical finding produces an error-tier badge using a theme token (no inline `Color::`).
  - [ ] Rendering a zero-findings card produces an info-tier (not success/"secure") badge and shows the disclaimer line.
  - [ ] The `/help` overlay content includes a `/security-review` line mentioning branch-diff scope and advisory nature.
- Integration tests:
  - [ ] The existing `colors_live_only_in_theme_module` guard test still passes after adding the rendering arm.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The security report card renders with severity-appropriate theme tokens and no inline color literals.
- `/help` documents the command's scope and advisory nature.
