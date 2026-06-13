---
status: completed
title: "StepTiming map + lifecycle stamping"
type: backend
complexity: medium
dependencies: []
---

# Task 02: StepTiming map + lifecycle stamping

## Overview

This task introduces an internal timing map to the `App` struct that tracks per-step lifecycle timestamps (`started_at` and `last_activity`). These timestamps drive the elapsed-time display and stall detection (Task 03). The task focuses solely on maintaining accurate timing state—stamping on step registration, bumping on every stream arrival and status transition, and removing on step completion. It does not compute elapsed, detect stalls, or rebuild the roster (those are Tasks 03 and 01 respectively).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. **Add internal `StepTiming` map to `App`**: a `BTreeMap<String, StepTiming>` field (NOT serialized, MUST be private) storing `step_id` → `StepTiming { started_at: Instant, last_activity: Instant }`.
2. **Stamp on step registration**: In `set_active_step_with_metadata` (line 3246), initialize both `started_at` and `last_activity` to `Instant::now()` when a step becomes active; support multiple concurrent steps keyed by their unique `step_id`.
3. **Bump `last_activity` on stream arrival**: In `push_live_stream_content` (line 4320, the single stream chokepoint verified in ADR-004), update the `last_activity` timestamp for the corresponding `step_id` to `Instant::now()` **before** any other processing.
4. **Bump `last_activity` on status transitions**: In `set_live_step_status` (line 4362), update the `last_activity` timestamp to `Instant::now()` for `Running` and `Streaming` statuses (the active states where stall detection matters).
5. **Clear on step end**: In `clear_active_step` (line 3286), remove the step's entry from the `StepTiming` map after clearing it from `active_steps` and `live_steps` to prevent unbounded growth.
6. **Support parallel groups independently**: Entries are keyed by `step_id` (not agent or group), so multiple concurrent steps in a parallel group each maintain their own timing independent of peers.
7. **MUST NOT serialize or expose timing to history**: The map is internal (private field, `#[serde(skip)]` or similar); the `StepTiming` struct is NOT added to the serialized `AppState` or `LiveStepView` per ADR-004.
</requirements>

## Subtasks

- [x] 2.1 — Define the `StepTiming` struct and add it as a private, non-serialized field to `App`.
- [x] 2.2 — Stamp `started_at` and `last_activity` when `set_active_step_with_metadata` registers a new active step.
- [x] 2.3 — Bump `last_activity` in `push_live_stream_content` immediately upon stream arrival.
- [x] 2.4 — Bump `last_activity` in `set_live_step_status` for `Running`/`Streaming` status transitions.
- [x] 2.5 — Clear the timing entry in `clear_active_step` when a step terminates.
- [x] 2.6 — Write unit tests validating timing lifecycle (stamp, bump, clear) using the `Instant::now() - Duration` offset pattern.
- [x] 2.7 — Verify no timing data leaks into serialized history or public views; pass all existing tests without regression.

## Implementation Details

### Relevant Files

- **`src/app/mod.rs:225–243`** — `App` struct definition; add `StepTiming` map field here.
- **`src/app/mod.rs:3246–3284`** — `set_active_step_with_metadata` function; stamp both timestamps when a step becomes active.
- **`src/app/mod.rs:3286–3315`** — `clear_active_step` function; remove the timing entry after clearing the step.
- **`src/app/mod.rs:4320–4360`** — `push_live_stream_content` function; bump `last_activity` on every stream arrival (the single chokepoint).
- **`src/app/mod.rs:4362–4384`** — `set_live_step_status` function; bump `last_activity` for active status transitions.

### Dependent Files

- **`src/tui/mod.rs:4076+`** — Unit and integration test module; tests will verify timing lifecycle and parallel-step independence.
- **`src/app/mod.rs:6046+`** — Existing app test module; integrate new timing tests alongside current test helpers.

### Related ADRs

- [ADR-004: Refresh Cadence and Stall-Detection Mechanism](../adrs/adr-004.md) — Specifies the `StepTiming` map, the single chokepoint (`push_live_stream_content`), when timestamps are bumped/cleared, and the 30 s stall threshold (used in Task 03, not here).

## Deliverables

- Add `StepTiming` struct and internal map field to `App` (`src/app/mod.rs`).
- Stamp both `started_at` and `last_activity` in `set_active_step_with_metadata`.
- Bump `last_activity` in `push_live_stream_content` (stream arrival) and `set_live_step_status` (status transitions).
- Clear the map entry in `clear_active_step` when a step ends.
- Unit tests with 80%+ coverage **(REQUIRED)**: test timestamp lifecycle (stamp on registration, bump on stream and status change, unchanged `started_at`), parallel steps tracked independently, entry removal on clear, offset-based deterministic `Instant` assertions.
- Integration test validating multi-step parallel behavior through the app layer **(REQUIRED)**: register two concurrent steps, push streams to both, verify each maintains independent `last_activity`, clear one and verify the map entry is gone while the other remains.

## Tests

### Unit Tests

- **Test: "After registering an active step, the StepTiming map holds an entry with `started_at == last_activity`"** — Register a step via `set_active_step_with_metadata`, read the internal map (via a test accessor if needed), assert both timestamps are equal and match the expected offset from a control `Instant`.
- **Test: "After pushing a stream, `last_activity` advances while `started_at` remains unchanged"** — Register a step at a fixed `Instant::now() - Duration::from_secs(10)`, simulate a stream push (via `push_live_stream_content`), verify `last_activity` is strictly later than `started_at` and closer to the current time.
- **Test: "After calling `clear_active_step`, the timing entry is removed from the map"** — Register a step, clear it, assert the map contains no entry for that `step_id`.
- **Test: "Two parallel steps in a group are tracked independently"** — Register two steps with different `step_id`s, push streams to each at different times, verify each maintains its own `last_activity` independently, with no cross-contamination.
- **Test coverage target: >=80%** on the new `StepTiming` logic (stamp, bump, clear paths).
- **All tests must pass**: no regression to existing test suite.

### Integration Tests

- **Test: "Multi-step parallel group maintains independent timings through stream and status changes"** — Register two steps in a parallel group, push streams to the first, change status on the second, verify the map reflects both independent timing progressions. Clear the first step and assert only the second remains.
- **All integration tests must pass**: verify app-layer consistency without mocking the timing map.

## Success Criteria

- The `StepTiming` map is added to `App` as a private, non-serialized field.
- Both `started_at` and `last_activity` are stamped when a step registers in `set_active_step_with_metadata`.
- `last_activity` is bumped in `push_live_stream_content` (stream arrival) and `set_live_step_status` (for `Running`/`Streaming` transitions).
- The entry is cleared in `clear_active_step` when a step ends.
- All tests passing.
- Test coverage >=80% on new timing logic.
- No serialized data leakage; history tests remain green.
- No regression to existing roster, chat, or approval tests.
