---
status: completed
title: Add chat governance-decision variants and projection arm
type: backend
complexity: medium
dependencies:
  - task_01
---

# Add chat governance-decision variants and projection arm

## Overview
Make a governance decision renderable in the chat transcript. This adds the `ChatItemKind::GovernanceDecision` and `ChatLifecycleKey::GovernanceDecision` variants, updates the three exhaustive matches they break (so the build stays green), and adds the projection arm that turns a `governance_decision_requested` event into a decision-card `ChatItemView`.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ChatItemKind::GovernanceDecision` and `ChatLifecycleKey::GovernanceDecision { run_id, decision_id }`.
- MUST update the three exhaustive matches the new variants break so the crate compiles: `ChatItemKind::slug()`, `chat_kind_label()`, and `ChatLifecycleKey::item_id()`.
- MUST add a projection arm that, on a `governance_decision_requested` event, upserts a `ChatItemKind::GovernanceDecision` item with `WaitingForUser` status showing intent, approach, agent, write-scope, and the plain-language risk label.
- MUST convey risk by an explicit text label (legible under `NO_COLOR`), not color alone.
- MUST reuse the existing content-preview cap for any long fields.
</requirements>

## Subtasks
- [x] 2.1 Add the `ChatItemKind` + `ChatLifecycleKey` variants.
- [x] 2.2 Add arms to `ChatItemKind::slug()`, `chat_kind_label()`, `ChatLifecycleKey::item_id()`.
- [x] 2.3 Add the `apply_governance_decision_requested` projection arm.
- [x] 2.4 Add a `governance_decision`-kind dispatch in `apply_history_event`.

## Implementation Details
Modify `src/app/chat/mod.rs` (enum variants + `slug()` + `item_id()`), `src/tui/mod.rs` (`chat_kind_label()`), and `src/app/chat/projection.rs` (new arm + dispatch). Mirror `apply_clarification_requested` for the arm structure. Reference TechSpec "Data Models" for the event payload shape; do not reproduce it.

### Relevant Files
- `src/app/chat/mod.rs` — `ChatItemKind` (~27) + `slug()` (~233); `ChatLifecycleKey` (~117) + `item_id()` (~207).
- `src/tui/mod.rs` — `chat_kind_label()` exhaustive match (~3633).
- `src/app/chat/projection.rs` — `apply_clarification_requested` (~1214) as the arm template; `apply_history_event` dispatch.
- `src/governance.rs` — the view type rendered (task_01).

### Dependent Files
- `src/tui/mod.rs` (task_04) — renders the projected governance item.

### Related ADRs
- [ADR-003: Unified GovernanceDecision data model + single pending_governance_decision state](../adrs/adr-003.md) — one shared projection arm + lifecycle key.

## Deliverables
- New `ChatItemKind` + `ChatLifecycleKey` variants with all three exhaustive matches updated.
- A projection arm rendering the governance decision.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- An integration test of the event→transcript path **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] `ChatItemKind::GovernanceDecision.slug()` returns `"governance_decision"`.
  - [x] `chat_kind_label(GovernanceDecision)` returns `"governance"`.
  - [x] `ChatLifecycleKey::GovernanceDecision { run_id, decision_id }.item_id()` returns the expected `chat:governance_decision:{run}:{decision}` string.
  - [x] The projection arm, given a `governance_decision_requested` payload, produces an item with `WaitingForUser` status and body lines for intent, approach, and risk label.
- Integration tests:
  - [x] A recorded `governance_decision_requested` event projects into the transcript as a single `GovernanceDecision` item carrying the intent text.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The crate compiles with all exhaustive matches handled; a governance decision renders in the transcript.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.

## Completion Notes
- The body cap (`bounded_body`) runs `concise()` on every line, which collapses leading whitespace and truncates after `MAX_BODY_LINES` (12). So: (a) the approach bullets render as `- <text>` (leading indent does not survive), and (b) the mandatory `Risk:` line is placed at body position 2 (right after the intent) so the line cap can never drop it. This also satisfies "reuse the existing content-preview cap for any long fields" — every line/title/summary is capped by the shared `concise`/`bounded_body` path.
- Only the three named exhaustive matches (`slug()`, `chat_kind_label()`, `item_id()`) broke; `cargo build` confirmed no others.
- The dispatch arm matches the `"governance_decision_requested"` string literal (consistent with every other arm in `apply_history_event`); the `GOVERNANCE_DECISION_*` constants from `src/governance.rs` are used at the event *write* sites (task_03/05) and in the integration test.
- Verified: `cargo test --lib governance` (15 passed), `cargo test --test governance_chat` (1), `cargo test --test governance_types` (3), `cargo fmt --check` clean, `cargo clippy --all-targets` clean.
- Pre-existing/environmental: the full `cargo test --lib` shows 12 failing skill-resolution tests (`app::tests::*skill*`, `tui::tests::reload_skills_*`); proven to fail identically on the clean task_01 commit (machine's installed skills, unrelated to this work).
