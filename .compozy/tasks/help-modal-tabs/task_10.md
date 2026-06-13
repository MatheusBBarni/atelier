---
status: pending
title: First-approval explainer with show-once latch (Phase 2)
type: frontend
complexity: high
dependencies: []
---

# Task 10: First-approval explainer with show-once latch (Phase 2)

## Overview
Phase 2. Show a one-line explainer the first time a user ever hits an approval prompt, so a
newcomer understands what they are approving, displayed at most once. This requires a small
persisted "shown" latch because the app has no first-run/show-once tracking today.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST display a single-line explainer alongside the FIRST approval prompt a user encounters, clarifying that an agent is requesting to perform a gated action and how to approve/deny.
- MUST show the explainer at most once per user (a show-once latch), never repeating on later approvals.
- MUST gate the explainer on a persisted "explainer shown" flag; once set, the explainer never renders again.
- MUST keep the change additive to the approval rendering and MUST NOT alter approval/deny semantics.
- SHOULD reuse a theme token for styling; MUST NOT add inline `Color::` literals.

> **Open implementation gap (call out, do not invent):** the codebase has no first-run or
> show-once persistence today (each launch is a fresh session; `.multiagent/sessions/` is not
> enumerated). This task MUST first define a minimal persisted latch (e.g., a small flag stored
> outside per-session history). The exact storage location/mechanism is undecided — resolve it
> in design review before implementing, and record the choice as a new ADR.
</requirements>

## Subtasks
- [ ] 10.1 Decide and document the minimal show-once latch mechanism (new ADR).
- [ ] 10.2 Implement reading/writing the latch.
- [ ] 10.3 Render the explainer on the first approval only, gated by the latch.
- [ ] 10.4 Add tests for first-show, subsequent-suppression, and latch persistence.

## Implementation Details
Approvals render inline in the chat: a fallback at `src/tui/mod.rs:2370` and a projected
`ChatItemKind::Approval` in `src/app/chat/projection.rs:184`. The explainer attaches to the
first approval surface. Persistence is the open piece — keep it minimal and local. See PRD
"Phased Rollout Plan" (Phase 2) and the research note on hint fatigue (show ≤ once,
dismissible).

### Relevant Files
- `src/tui/mod.rs` — approval fallback render `:2370`.
- `src/app/chat/projection.rs` — `apply_pending_approval` `:184`.
- `src/history/mod.rs` — session/store layer; candidate (to be decided) home for a persisted latch.

### Dependent Files
- The approval render path — gains the conditional explainer.
- A new ADR file under `adrs/` — records the latch decision.

### Related ADRs
- [ADR-002: Phased Delivery Approach](../adrs/adr-002.md) — first-approval explainer deferred to Phase 2; defines the show-once guardrail.
- A new ADR (to be created in 10.1) for the persistence mechanism.

## Deliverables
- Show-once first-approval explainer + persisted latch.
- A new ADR documenting the latch mechanism.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for first-show vs suppression **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] With the latch unset, the first approval render includes the explainer line.
  - [ ] With the latch set, an approval render does NOT include the explainer.
  - [ ] Showing the explainer sets the latch exactly once (idempotent on repeat renders within the same approval).
- Integration tests:
  - [ ] First approval after a fresh install shows the explainer; a later approval (latch persisted) does not.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Explainer appears once ever and never repeats; approval semantics unchanged.
- Latch mechanism documented in a new ADR; `colors_live_only_in_theme_module` passes; `cargo clippy --all-targets` clean.
