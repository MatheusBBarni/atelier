---
status: completed
title: Grade-round events and collapsing chat projection
type: backend
complexity: medium
dependencies:
  - task_03
---

# Task 04: Grade-round events and collapsing chat projection

## Overview
Add the `grade_round` and `grader_verdict` history events and project them into ONE evolving chat item ("verifying… → FAIL retry 1/2 → PASS") via a new `Grade` lifecycle key. This delivers the user-visible loop with a retry counter — the answer to the runaway-loop complaint — without copying the council anti-pattern, which scatters into non-collapsing items.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define a `grade_round` event payload carrying `round`, `max_rounds`, `outcome` (`working|pass|fail|skip`), and optional `command`/`exit_code`/`critique`, and a `grader_verdict` event carrying the serialized verdict.
- MUST add a `ChatLifecycleKey::Grade { run_id }` variant (with its `item_id` arm) and a `ChatItemKind::GradeLoop` variant (with its `slug` arm).
- MUST add an `apply_grade_round` projection arm that reads the prior item and accumulates round lines into ONE item keyed by `Grade`, mirroring `apply_clarification_answered` — NOT routed to `apply_diagnostic`.
- MUST drive item status/severity from `outcome`: working → Running; fail (round < max) → Running/Warning; fail (round == max) → Failed; pass → Completed; skip → Completed with a muted "unverified" note.
- MUST keep the accumulated body within `MAX_BODY_LINES` (12), preferring a header + most-recent rounds when rounds exceed the cap.
- MUST be a pure function of the event stream (round/max in the payload) so `ChatProjection::rebuild` is deterministic.
</requirements>

## Subtasks
- [x] 04.1 Define the `grade_round` and `grader_verdict` event payloads.
- [x] 04.2 Add the `Grade` lifecycle key and `GradeLoop` item kind with their match arms.
- [x] 04.3 Register a `grade_round` dispatch arm before the catch-all in `apply_history_event`.
- [x] 04.4 Implement `apply_grade_round` to read-prior-then-accumulate under the `Grade` key.
- [x] 04.5 Map outcomes to status/severity and bound the body to 12 lines.
- [x] 04.6 Cover collapse, counter, status transitions, and skip rendering with tests.

## Implementation Note
`GRADE_ROUND_KIND`/`GRADER_VERDICT_KIND` constants in `history/mod.rs`; `ChatItemKind::GradeLoop`
(+ slug + `chat_kind_label` "verify") and `ChatLifecycleKey::Grade { run_id }` (+ item_id
`chat:grade:{run_id}`) in `chat/mod.rs`. `apply_grade_round` accumulates one line per round (replacing
the current round's line so working→resolved updates in place), maps outcome→status/severity
(working→Running, fail<max→Running/Warning, fail==max→Failed, pass→Completed, skip→Completed+muted),
and bounds to `MAX_BODY_LINES` with an omission header computed from the latest round number (stable
under refill). Six tests incl. rebuild-determinism. `grader_verdict` is recorded for durability by
task 05; it is not separately projected (the GradeLoop item is the user-facing view).

## Follow-up (out of scope)
Full `cargo test --lib` shows 13 pre-existing, environment-sensitive failures unrelated to this task
(skill discovery erroring over the developer's real `~/.agents/skills`/`~/.claude/skills` roots, plus
the codex-CLI availability test). Proven non-regressive: they fail identically with this task's changes
stashed. Same root cause noted in task_07 of config-setup-skill; CI's clean HOME is unaffected.

## Implementation Details
Model the arm on `apply_clarification_answered` (read prior item via the key index, rebuild an accumulated body, upsert under the same key). See TechSpec "Data Models" (event payloads, `Grade` key) and "Build Order" step 4. The chat-projection findings in `_research-techspec.json` give the exact `upsert`, `MAX_BODY_LINES`, and council-anti-pattern locations.

### Relevant Files
- `src/app/chat/projection.rs` — `apply_history_event` dispatch (~:58-110, catch-all at :110), `upsert` (~:1339), `MAX_BODY_LINES` (:19), `apply_clarification_answered` (~:1266) as the model.
- `src/app/chat/mod.rs` — `ChatLifecycleKey` (~:115) + `item_id` (~:205), `ChatItemKind` (~:25) + `slug` (~:232).
- `src/app/mod.rs` — `record_event` (~:4164) is the only sanctioned emit path (used by task 05).

### Dependent Files
- `src/app/mod.rs` — task 05's executor emits `grade_round`/`grader_verdict` through `record_event`.
- `src/orchestrator/mod.rs` — task 03's `GraderVerdict` is serialized into the events.

### Related ADRs
- [ADR-003: Harness-driven bounded grade→fix loop](../adrs/adr-003.md) — the rounds and counter the projection renders.

## Deliverables
- `grade_round`/`grader_verdict` events and an `apply_grade_round` arm that collapses rounds into one evolving item.
- New `Grade` lifecycle key and `GradeLoop` item kind.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for the collapsing projection **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Three successive `grade_round` events (working, fail round 1/2, pass) collapse into ONE `Grade` item.
  - [ ] A `fail` round renders "Round 1/2: FAIL — <command> exit 1" and sets Running/Warning.
  - [ ] A terminal `fail` at `round == max_rounds` sets the item status to Failed.
  - [ ] A `skip` outcome renders a muted "unverified" note and Completed status.
  - [ ] More rounds than `MAX_BODY_LINES` keeps a header + most-recent rounds (no overflow).
- Integration tests:
  - [ ] Replaying the event log (`ChatProjection::rebuild`) reproduces the identical collapsed item.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The grade loop renders as exactly one chat item with a visible `round N/max` counter
- Grade events are never routed through the non-collapsing diagnostic path
