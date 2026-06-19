---
status: pending
title: Reviewer selection and review engine
type: backend
complexity: high
dependencies:
  - task_01
  - task_02
  - task_03
  - task_04
---

# Task 05: Reviewer selection and review engine

## Overview
Build the core engine: pick an enabled agent whose model family is outside the producer-family set (else SKIP loudly), run it once as an opinion-only reviewer over the working diff, parse `ReviewFinding`s, and record the review events. Also add a deterministic FakeRuntime branch so the whole flow is testable (ADR-004/005/006).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST select a reviewer by scanning enabled agents and choosing one whose `agent_family` is absent from the producer-family set, with a deterministic tie-break.
- MUST record a `review_skipped` event (producer families + reason) and run no reviewer when no independent family is reachable; MUST NOT downgrade to a same-family reviewer.
- MUST build an opinion-only reviewer `AgentProfile` (capabilities `Read`/`Review`, no Edit/Command, empty tools, reviewer brief) modeled on `council_member_agent`, on the selected runtime.
- MUST embed the working diff (task_02) in the reviewer prompt and dispatch exactly one `execute_runtime_step`.
- MUST parse the reviewer's structured output into `ReviewFinding`s, dropping any without a `file:line` (verify-before-surface), and record `review_started`/`review_finding`/`review_completed` events carrying provenance (reviewer family, producer families).
- MUST add a FakeRuntime control phrase that emits deterministic `ReviewFinding`s for tests.
- MUST keep the reviewer non-blocking — no approval prompt in normal mode.
</requirements>

## Subtasks
- [ ] 05.1 Implement reviewer selection over enabled agents vs the producer-family set, including the SKIP path.
- [ ] 05.2 Build the opinion-only reviewer profile and the prompt with the embedded diff.
- [ ] 05.3 Dispatch one `execute_runtime_step` and collect its output.
- [ ] 05.4 Parse and validate `ReviewFinding`s, dropping unlocated findings.
- [ ] 05.5 Record `review_started`/`review_finding`/`review_completed`/`review_skipped` with provenance fields.
- [ ] 05.6 Add the FakeRuntime review control branch.
- [ ] 05.7 Unit-test selection and parsing; integration-test the end-to-end review and the SKIP path.

## Implementation Details
Put pure logic (selection, finding parsing) in `src/review/mod.rs` and the orchestration (record events, dispatch) in an `App` method in `src/app/mod.rs`. Reuse `execute_runtime_step` (`src/runtime/mod.rs:466`), `runtime_request` (`src/app/mod.rs:6557`), and the opinion-only profile pattern from `council_member_agent` (`src/app/mod.rs:8602`). Add the FakeRuntime branch in `src/runtime/fake.rs` (`fake_agent_result` `:565`, control-phrase plumbing `:816`). Extend the reviewer output schema/brief as needed for structured findings. See TechSpec "Reviewer selector + engine" and "Command & Event Surface".

### Relevant Files
- `src/review/mod.rs` — selection + parsing (consumes task_03 types, task_01 resolver).
- `src/app/mod.rs` — `runtime_request` (`:6557`), `council_member_agent` (`:8602`), `record_event`, producer-set (task_04).
- `src/runtime/mod.rs` — `execute_runtime_step` (`:466`), `RuntimeRequest` (`:128`).
- `src/runtime/fake.rs` — `fake_agent_result` (`:565`), control-phrase plumbing (`:816`).
- `src/app/git.rs` — `working_diff` (task_02).
- `src/runtime/status.rs` — `agent_family` (task_01).

### Dependent Files
- `src/app/mod.rs` `/review` handler (task_06) invokes this engine.
- `src/app/chat/projection.rs` (task_07) consumes the recorded review events.

### Related ADRs
- [ADR-004: Opinion-only reviewer over an app-acquired git diff, single-step dispatch](../adrs/adr-004.md).
- [ADR-005: Auto-selected reviewer by family; loud SKIP, never downgrade](../adrs/adr-005.md).
- [ADR-006: Structured ReviewFinding in review events](../adrs/adr-006.md).

## Deliverables
- Reviewer selection (with SKIP), opinion-only dispatch, finding parsing, and review-event recording.
- FakeRuntime review control branch.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for the end-to-end review and SKIP paths **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Selection picks the agent whose family is not in the producer set when one exists.
  - [ ] Selection returns Skipped when every enabled agent's family is in the producer set.
  - [ ] A reviewer output entry without a `file:line` is dropped; a well-formed entry parses with its severity/confidence.
- Integration tests:
  - [ ] Producer step on `RuntimeKind` A, then the engine selects a family-B reviewer and records `review_started` whose `reviewer_family` is not in `producer_families`, plus at least one `review_finding`.
  - [ ] Single-family config records `review_skipped` naming the producer family and dispatches no reviewer step.
  - [ ] The reviewer step runs in `normal` approval mode with no pending approval.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The engine selects an independent-family reviewer or SKIPs loudly, never downgrading to a same-family reviewer
- Findings are located (`file:line`) and recorded as structured `review_finding` events with provenance
