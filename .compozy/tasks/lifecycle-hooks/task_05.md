---
status: pending
title: Event tap + App wiring + dispatcher spawn
type: backend
complexity: high
dependencies:
  - task_01
  - task_02
  - task_04
---

# Task 5: Event tap + App wiring + dispatcher spawn

## Overview
Wire the feature into the running app: a non-blocking tap inside `record_event_with_group` that resolves the actor, normalizes the event, and `try_send`s matched handlers to the dispatcher; the `App` plumbing to hold the sender; and the `run_tui` wiring that spawns the dispatcher and records the back-channel lifecycle events. This is the integration linchpin and carries the cross-runtime conformance test that gates the MVP.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST insert the tap in `record_event_with_group` immediately AFTER the durable `append_event`, gated on "any hooks configured" so there is zero cost when none are.
- MUST resolve the uniform actor at the tap from `active_step.agent` → `self.agent(id).runtime`, call `normalize()`, match the public event against configured handlers, and `try_send` (non-blocking) — incrementing the dropped counter on a full channel.
- MUST add a `hook_sender` (and dropped-counter handle) to `App` and populate it at construction; when unset, recording behaves exactly as today.
- MUST create the bounded channel + dropped counter in `run_tui`, spawn the task_04 dispatcher, and give the dispatcher a sender back to the worker.
- MUST handle the dispatcher's back-channel in `run_app_worker` by recording `hook_started`/`hook_completed` via `record_event` (the only `&mut App` site).
- MUST NOT make the write path block or `.await`.
</requirements>

## Subtasks
- [ ] 5.1 Add `App.hook_sender` + dropped-counter handle and populate them at construction.
- [ ] 5.2 Insert the gated tap (map → match → resolve actor → normalize → try_send) after `append_event`.
- [ ] 5.3 Create the dispatch channel + counter in `run_tui` and spawn the dispatcher.
- [ ] 5.4 Add the back-channel command variant + `run_app_worker` arm that records `hook_started`/`hook_completed`.
- [ ] 5.5 Add the end-to-end fake-runtime test and the cross-runtime conformance test.

## Implementation Details
Edit `src/app/mod.rs`: `record_event_with_group` (`:4215`, tap after `append_event` ~`:4232`), the `App` struct (`:316`) and its constructor, and `App::agent` (`:4066`) for actor resolution from `ActiveStep.agent` (`:372`). Edit `src/tui/mod.rs`: `run_tui` (where `run_app_worker` and its channels are created, near `:957`) to spawn the dispatcher and wire senders, and add a `AppWorkerCommand` arm to record lifecycle events. Use the `fake` runtime (`src/runtime/fake.rs`) for the end-to-end test, per CLAUDE.md's app-test convention. See TechSpec "System Architecture", "Impact Analysis", and "Testing Approach".

### Relevant Files
- `src/app/mod.rs:4215` — `record_event_with_group`; insert the tap after the append.
- `src/app/mod.rs:316` — `App` struct; add `hook_sender` + counter handle.
- `src/app/mod.rs:4066,372` — `App::agent` + `ActiveStep.agent` for actor resolution.
- `src/tui/mod.rs:957` — `run_tui`/`run_app_worker`; spawn dispatcher, wire channels, add back-channel arm.
- `src/runtime/fake.rs` — drives the end-to-end and conformance tests.

### Dependent Files
- `src/hooks/dispatch.rs` — consumes the dispatch channel this task creates (task_04).
- `src/app/chat/projection.rs` — projects the lifecycle events this task records (task_06).
- `src/doctor/mod.rs` — reads the dropped counter created here (task_08).

### Related ADRs
- [ADR-003: Off-funnel hook dispatch with enrich-at-tap actor resolution](../adrs/adr-003.md) — tap placement, actor resolution, back-channel recording.
- [ADR-001: V1 ships cross-runtime observer hooks](../adrs/adr-001.md) — non-blocking write path requirement.

## Deliverables
- A gated, non-blocking tap that dispatches matched handlers.
- `App` + `run_tui` + `run_app_worker` wiring for the dispatcher and back-channel.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test: end-to-end fake-runtime run fires a configured hook; cross-runtime conformance check **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] With no hooks configured, `record_event_with_group` performs zero dispatch work (sender never touched).
  - [ ] The tap resolves `actor.runtime` from the active step's agent profile for an `agent_step_started` event.
  - [ ] A full dispatch channel causes the tap to increment the dropped counter and not block.
  - [ ] An internal kind outside the public vocabulary produces no dispatch.
- Integration tests:
  - [ ] End-to-end (`fake` runtime): a `[[hooks.handler]] on="run_completed", command=...` writes a sentinel file containing the normalized payload after a run.
  - [ ] Cross-runtime conformance: the payload shape for `run_completed` is byte-identical across two runtime configurations (e.g. `fake` and a second configured runtime id).
  - [ ] `hook_started`/`hook_completed` are recorded into history after a dispatched command hook.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The write path never blocks; zero overhead when no hooks are configured
- A configured hook fires end-to-end through the `fake` runtime with the normalized payload
- The cross-runtime conformance test passes (PRD MVP proceed-criterion)
