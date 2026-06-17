---
status: completed
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
- [x] 5.1 Add `App.hook_sender` + dropped-counter handle and populate them at construction.
- [x] 5.2 Insert the gated tap (map → match → resolve actor → normalize → try_send) after `append_event`.
- [x] 5.3 Create the dispatch channel + counter in `run_tui` and spawn the dispatcher.
- [x] 5.4 Add the back-channel command variant + `run_app_worker` arm that records `hook_started`/`hook_completed`.
- [x] 5.5 Add the end-to-end fake-runtime test and the cross-runtime conformance test.

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
  - [x] With no hooks configured, `record_event_with_group` performs zero dispatch work (sender never touched). — `tap_does_no_dispatch_when_no_hooks_configured`
  - [x] The tap resolves `actor.runtime` from the active step's agent profile for an `agent_step_started` event. — `tap_resolves_actor_runtime_from_active_step_agent`
  - [x] A full dispatch channel causes the tap to increment the dropped counter and not block. — `tap_increments_dropped_counter_on_full_channel_without_blocking`
  - [x] An internal kind outside the public vocabulary produces no dispatch. — `tap_ignores_kinds_outside_public_vocabulary`
- Integration tests:
  - [x] End-to-end (`fake` runtime): a `[[hooks.handler]] on="run_completed", command=...` writes a sentinel file containing the normalized payload after a run. — `end_to_end_command_hook_writes_normalized_payload`
  - [x] Cross-runtime conformance: the payload shape for `run_completed` is uniform across two runtime configurations (`fake` and a second `alt` runtime id). — `cross_runtime_run_completed_payload_shape_is_uniform`
  - [x] `hook_started`/`hook_completed` are recorded into history after a dispatched command hook. — `hook_lifecycle_events_are_recorded_into_history`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The write path never blocks; zero overhead when no hooks are configured
- A configured hook fires end-to-end through the `fake` runtime with the normalized payload
- The cross-runtime conformance test passes (PRD MVP proceed-criterion)

## As-built notes
- **Tap**: `App::dispatch_hooks_for_event(&self, event)` is called in `record_event_with_group` immediately after `append_event`. Gated on a wired `hook_sender` first (zero cost when `None`), then maps via `public_name_for_kind` (so `hook_*`/stream-delta short-circuit → no re-trigger), matches handlers, resolves the actor, `normalize(.., Full)`, and `try_dispatch`. Pure-read `&self`, no `.await`.
- **Actor resolution** prefers the step matching `event.step_id` (covers parallel children via `active_steps`), falling back to `active_step`, then `self.agent(id).runtime`; `None/None` for pre-agent orchestrator events.
- **Sender attached post-construction** via `App::attach_hook_sender` (mirrors `attach_state_sender`), so `App::new` is unchanged and every existing test behaves as today (`hook_sender: None`).
- **`run_tui`** captures `config.hooks` before the move and, only when handlers exist, creates the channel + `DroppedHookCounter`, attaches the tap, spawns `run_hook_dispatcher` (notifier = `CommandNotifier` if `notify_fallback_command` set, else `OscNotifier`), and spawns a small forwarder that turns dispatcher `HookLifecycleRecord`s into `AppWorkerCommand::RecordHookLifecycle`.
- **`run_app_worker`** gains a `RecordHookLifecycle` arm calling `App::record_hook_lifecycle` (the only `&mut App` site) — `run_app_worker`'s signature is unchanged, so its test caller is unaffected.
- **Cross-runtime conformance** is asserted as payload-*shape* uniformity (identical top-level key set + equal `event`/`outcome`/`schema_version`) across two `fake`-typed runtimes (`fake` vs `alt`); the `actor.runtime` value legitimately differs (that is the uniform-field point), while the structure is identical — the PRD proceed-criterion.
- **Test-flakiness note (not a regression):** under full-suite parallel load the env-sensitive `runtime::codex` tests can flake; they pass 119/119 in isolation. Unrelated to this task.
