---
status: completed
title: First-run onboarding & Approvals help
type: frontend
complexity: medium
dependencies:
  - task_07
---

# Task 09: First-run onboarding & Approvals help

## Overview
Introduce the changed behavior so it isn't a surprise: a tier-aware first-run explainer that also fires the first time the floor prompts a Yolo user, plus updated Approvals/Keys help tabs documenting tiers, trust, and the new keys. This satisfies the PRD's "announce the safety-default change" requirement using the existing once-only latch.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST update the first-run explainer copy to be tier-aware and to explain why a prompt can appear even in Yolo (the catastrophic floor).
- MUST reuse the existing persisted once-only latch (`first_approval_explainer_shown`) so the notice shows at most once per user; MUST NOT add new persisted trust state.
- MUST ensure the explainer fires on the first surfaced approval whether it originates in Normal or as a Yolo catastrophic prompt.
- MUST update the Approvals help tab to document risk tiers, the trust list, `/trust`, and `floor = warn|enforce`, and the Keys tab to document approve / approve-and-trust / deny.
- SHOULD keep copy concise (progressive disclosure); defer full rule lists to help, not the first-run card.

## Subtasks
- [x] 09.1 Update the first-run explainer copy (tier-aware; explains Yolo-catastrophic prompts).
- [x] 09.2 Confirm the latch fires once across Normal and Yolo-catastrophic first prompts.
- [x] 09.3 Expand the Approvals help tab (tiers, trust, `/trust`, floor posture).
- [x] 09.4 Update the Keys help tab for the new approval keys.
- [x] 09.5 Add tests for the once-only behavior and help content.

## Implementation Details
Work in `src/history/mod.rs` (latch: `first_approval_explainer_shown` ~199, `mark_first_approval_explainer_shown` ~205; copy currently in `src/app/chat/projection.rs` ~14) and `src/tui/mod.rs` (Approvals tab ~3774, Keys tab `keys_tab_lines` ~3737). Reuse `show_first_approval_explainer` on `AppState` (~123). See TechSpec "System Architecture (history)" and PRD "User Experience → First contact" / "Phased Rollout".

### Relevant Files
- `src/app/chat/projection.rs` — first-run explainer copy in the approval item.
- `src/history/mod.rs` — the once-only latch (reused, not extended).
- `src/tui/mod.rs` — Approvals and Keys help tabs.

### Dependent Files
- None downstream (final task).

### Related ADRs
- [ADR-002: Phased floor rollout with a non-bypassable catastrophic core](../adrs/adr-002.md) — announce the change; first-run notice + help.

## Deliverables
- Tier-aware first-run explainer reusing the existing latch, plus updated Approvals/Keys help tabs.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test for once-only behavior under a Yolo catastrophic prompt **(REQUIRED)**.

## Tests
- Unit tests:
  - [x] First surfaced approval sets `show_first_approval_explainer = true`; the explainer copy mentions tiers and the Yolo-catastrophic case.
  - [x] After the latch is marked, a subsequent approval does not show the explainer.
  - [x] The Approvals help tab text includes "trust", "/trust", risk tiers, and "floor".
  - [x] The Keys help tab lists the approve, approve-and-trust, and deny keys.
- Integration tests:
  - [x] FakeRuntime under Yolo: the first catastrophic prompt shows the explainer; a later catastrophic prompt in the same/next session (latch set) does not.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The explainer appears at most once (including the first Yolo catastrophic prompt) with tier-aware copy; help tabs document tiers, trust, `/trust`, and floor posture; no new persisted state.
