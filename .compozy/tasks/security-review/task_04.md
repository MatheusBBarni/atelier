---
status: pending
title: SecurityReview chat item and projection arm
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 04: SecurityReview chat item and projection arm

## Overview
Add a `SecurityReview` chat item kind and lifecycle key, plus an `apply_security_review` projection arm that collapses the `security_review_started`/`security_review_completed` events into one evolving "security report" card: scope line, verdict header, severity-grouped findings, and a persistent honest disclaimer. The card never renders a green "secure" affordance for zero findings (ADR-003).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `ChatItemKind::SecurityReview` and `ChatLifecycleKey::SecurityReview { review_id }` in `src/app/chat/mod.rs`.
- MUST add `apply_security_review(&mut self, event)` in `src/app/chat/projection.rs` and dispatch to it from `apply_history_event` for the `security_review_started` and `security_review_completed` event kinds.
- The handler MUST upsert a single chat item keyed by the lifecycle key (Scanning on start → Completed on finish), mirroring `apply_grade_round`'s in-place evolution.
- The card body MUST contain, in order: a scope/coverage line, a verdict header (severity counts), findings grouped by severity (Critical→Info), and a persistent disclaimer line.
- Severity → `ChatSeverity` mapping MUST use the maximum finding severity; zero findings MUST map to `Info` (never `Success`/"secure").
- MUST deserialize the `findings` payload into `Vec<Finding>` (task_01) and render `[SEV] title — location — why — fix` using existing `ChatLineStyle` variants.

## Subtasks
- [ ] 4.1 Add the `ChatItemKind` and `ChatLifecycleKey` variants and fix resulting exhaustive matches.
- [ ] 4.2 Implement `apply_security_review` modeled on `apply_grade_round` (lookup-or-create by key, in-place update).
- [ ] 4.3 Build the body: scope line, verdict header, severity-grouped finding lines, disclaimer.
- [ ] 4.4 Implement the max-severity → `ChatSeverity` mapping with the "never secure on zero findings" rule.
- [ ] 4.5 Wire the dispatch arm in `apply_history_event` for both event kinds.
- [ ] 4.6 Add projection unit tests for start→complete evolution, grouping, and the zero-findings case.

## Implementation Details
Mirror `apply_grade_round` (`src/app/chat/projection.rs:1842`) and the `apply_history_event` dispatch switch (`~77-159`). Reuse `ChatItemView`, `ChatItemStatus`, `ChatSeverity`, `ChatLineView`/`ChatLineStyle` from `src/app/chat/mod.rs`. Consume `Finding`/`Severity` from task_01. See TechSpec "Data Models" (payload shapes) and ADR-003 (report shape, no green "secure"). Do not render TUI styling here — that is task_07; this task produces the `ChatItemView` data.

### Relevant Files
- `src/app/chat/projection.rs` — `apply_grade_round` (~1842) template; `apply_history_event` (~77) dispatch.
- `src/app/chat/mod.rs` — `ChatItemKind`/`ChatLifecycleKey`/`ChatItemView`/`ChatSeverity`/`ChatLineStyle` definitions.
- `src/orchestrator/mod.rs` — `Finding`/`Severity` from task_01.

### Dependent Files
- `src/tui/mod.rs` — task_07 renders the new `ChatItemKind::SecurityReview`.
- `src/app/mod.rs` — task_05 emits the events this projection consumes.

### Related ADRs
- [ADR-003: Scope-honest security report card](../adrs/adr-003.md) — body shape, no "secure" affordance.
- [ADR-001: Own event family + ChatLifecycleKey](../adrs/adr-001.md) — distinct lifecycle key.

## Deliverables
- `ChatItemKind::SecurityReview` + `ChatLifecycleKey::SecurityReview` variants.
- `apply_security_review` projection arm wired into `apply_history_event`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests (end-to-end card from real events) exercised in task_05 **(REQUIRED)**

## Tests
- Unit tests (drive the projection with synthetic `HistoryEvent`s):
  - [ ] A `security_review_started` event creates one `SecurityReview` item with `status=Pending/Running` and a scope line.
  - [ ] A following `security_review_completed` with 1 High + 1 Medium finding updates the SAME item (one item total) to `status=Completed`, `severity=Warning`, body grouped High→Medium.
  - [ ] A completed event with a Critical finding maps to `severity=Error`.
  - [ ] A completed event with zero findings maps to `severity=Info` and the body says "no high-confidence findings surfaced" — never "secure".
  - [ ] The disclaimer line is present on every completed card.
  - [ ] A `truncated=true` scope renders a truncation note in the scope line.
- Integration tests:
  - [ ] (Covered in task_05: events emitted by the workflow project into the expected card.)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Start and completion events collapse into exactly one chat item.
- Zero findings never produces a success/"secure" severity.
