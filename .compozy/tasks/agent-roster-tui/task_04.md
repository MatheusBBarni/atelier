---
status: pending
title: "Wire rebuild into publish_state + 1Hz gated refresh tick"
type: backend
complexity: medium
dependencies:
  - task_03
---

# Task 04: Wire rebuild into publish_state + 1Hz gated refresh tick

## Overview

This task integrates roster rebuild calls into the publish_state pathway and adds a bounded 1 Hz refresh timer to keep roster_rows fresh during active runs. Every state mutation already publishes via `publish_state`; we hook roster rebuild there so rows never drift from agents/live_steps. A separate 1 Hz `select!` arm (gated to active runs and change-gated before publish) keeps elapsed-time and stall-detection up-to-date even when no stream events arrive.


<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. **MUST** implement `rebuild_roster_rows(&mut self)` on `App` that calls `build_roster_rows` (task 03 output) with current `now`, stores the result into `state.roster_rows`, and invokes it inside `publish_state` (`app/mod.rs:4050`).
2. **MUST** implement `refresh_roster_tick(&mut self)` that early-returns unless `work_indicator_active` is true, then rebuilds with fresh `Instant::now()`, and publishes only if the rebuilt rows differ from current ones (change-gate mirroring `set_git_context`).
3. **MUST** add a 4th arm to the app-worker `tokio::select!` loop (`tui/mod.rs:731-756`) with `tokio::time::interval(Duration::from_secs(1))` + `set_missed_tick_behavior(Skip)` that calls `app.refresh_roster_tick()`.
4. **MUST** ensure the change-gate compares activity state **and** elapsed-time bucket (not raw seconds) so coarse-grained updates don't trigger spurious publishes; adopt the same idiom as `set_git_context`.
5. **SHOULD** add a `STALL_THRESHOLD: Duration = Duration::from_secs(30)` constant in `src/app/mod.rs` for reuse across `build_roster_rows` and tests.
6. **SHOULD** initialize `App.step_timing` (the `BTreeMap<String, StepTiming>` from task 02) as empty on app creation; clear entries when steps end or reach terminal status.
</requirements>

## Subtasks

- [ ] 4.1 — Write `rebuild_roster_rows(&mut self)` in `App`, calling `build_roster_rows(...)` with `Instant::now()` and storing into `state.roster_rows`.
- [ ] 4.2 — Add the call to `rebuild_roster_rows()` inside `publish_state` (after state is mutated but before the watch sender sends).
- [ ] 4.3 — Implement `refresh_roster_tick(&mut self)` with early-return on idle, rebuild with fresh `now`, and a change-gate before publishing (compare activity + elapsed-bucket).
- [ ] 4.4 — Add the 4th `tokio::select!` arm (1 Hz interval with `Skip` missed-tick behavior) in the app-worker loop (`tui/mod.rs:731-756`).
- [ ] 4.5 — Verify that the `StepTiming` map is initialized, stamped in `set_active_step_with_metadata` (task 02 step 1), and cleared in `clear_active_step` and on terminal status.
- [ ] 4.6 — Write unit tests: `publish_state` populates `roster_rows`; idle run with `refresh_roster_tick` performs no publish; active run with advanced `now` publishes on elapsed-bucket change; change-gate suppresses identical rebuild; step quiet ≥30s flips to `Stalled` on next tick.
- [ ] 4.7 — Add integration snapshot test: idle roster, single-active with elapsed, needs-input pinned, stalled in-place, summary header, and `NO_COLOR` determinism.

## Implementation Details

See TechSpec **"System Architecture"** (component overview + data flow), **"Core Interfaces"** (`rebuild_roster_rows`, `refresh_roster_tick` signatures), **"Integration Points"** (publish_state hook, select! arm pattern), and **"Development Sequencing"** (steps 4–5 dependency chain).

### Relevant Files

- `/Users/matheusbbarni/projects/multiagent-harness/src/app/mod.rs` — `App` struct (`pub struct App`, line 225); `publish_state` method (`fn publish_state`, line 4050); `set_active_step_with_metadata` (line 3246), `clear_active_step` (line 3286), `set_live_step_status` (line 4362), `push_live_stream_content` (line 4320); test module (`#[cfg(test)] mod tests`, line 6046).
- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/mod.rs` — app-worker `tokio::select!` loop (line 732–756); `work_indicator_active` function (line 3419); test module (`#[cfg(test)] mod tests`, line 4076) with `render_to_text` helper (line 6446) and `title_cell_fg` helper (line 8741).
- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/theme.rs` — `Theme` and `TerminalCaps` for color/capability testing (referenced in TechSpec).

### Dependent Files

- `src/app/mod.rs:AppState` (line 72) — gains `roster_rows: Vec<RosterRow>` field (task 01 output).
- `src/app/mod.rs:App.step_timing` — new `BTreeMap<String, StepTiming>` internal field holding timing data for elapsed/stall classification.
- `src/tui/mod.rs` — the select! loop (`tui/mod.rs:731`) gains a 4th gated poller arm; existing test infrastructure (`render_to_text`, `TestBackend`) reused.

### Related ADRs

- [ADR-003: Roster View-Model Architecture and Render-Time Determinism](../adrs/adr-003.md) — app-layer builder with injected clock; rebuild in publish_state ensures rows never drift from state.
- [ADR-004: Refresh Cadence and Stall-Detection Mechanism](../adrs/adr-004.md) — 1 Hz select! arm gated on active runs; change-gate mirrors `set_git_context` idiom; stall from elapsed-since-last-activity (30 s threshold).

## Deliverables

- `rebuild_roster_rows(&mut self)` method on `App`, callable from `publish_state` or tests, producing deterministic roster_rows with fresh `now`. **Integrated into `publish_state` pipeline.**
- `refresh_roster_tick(&mut self)` method on `App`, implementing the gated 1 Hz refresh with early-return on idle and change-gate before publish. **Wired into the app-worker select! loop.**
- A 4th `tokio::select!` arm in `tui/mod.rs:731-756` driving the 1 Hz tick, matching the `file_index_poll` pattern (skip missed ticks, no burst). **Integrated into the event loop.**
- Updated `App.step_timing` initialization, stamping, and clearing across the four lifecycle sites (`set_active_step_with_metadata`, `push_live_stream_content`, `set_live_step_status`, `clear_active_step`). **Verified in unit tests.**
- Unit tests with 80%+ coverage **(REQUIRED):** `publish_state` populates roster_rows; idle-run refresh is a no-op; active-run refresh with advanced `now` publishes on change; change-gate suppresses identical rebuild; stall-transition on 30s quiet.
- Integration snapshot tests with `TestBackend` **(REQUIRED):** idle roster, active single agent (glyph+label+elapsed+step), needs-input pinned, stalled in-place, summary header, narrow-width truncation, `NO_COLOR` determinism.

## Tests

### Unit Tests

- **`test_publish_state_populates_roster_rows`**: Create an `App` with agents and a live step at `Running` status; call `publish_state`; assert `state.roster_rows` is non-empty and contains the agent with `ActivityState::Active`.
- **`test_refresh_roster_tick_idle_no_publish`**: `App` with `RunState::Idle`; call `refresh_roster_tick()`; verify no `publish_state` call occurs (via a spy or by checking that `state.roster_rows` does not change).
- **`test_refresh_roster_tick_active_elapsed_bucket_advance`**: `App` with a `Running` step, `StepTiming` entry showing a step started 15s ago; call `refresh_roster_tick()` with simulated `now` advanced by 20s (crossing an elapsed-bucket boundary); assert the rebuild publishes and `state.roster_rows[.].elapsed` changes from `"15s"` to `"35s"` or similar coarse bucket.
- **`test_change_gate_suppresses_identical_rebuild`**: `App` with a `Running` step; call `refresh_roster_tick()` twice in quick succession (same elapsed bucket); on the second call, verify the change-gate suppresses the publish.
- **`test_step_stall_after_30s_quiet`**: `App` with a `Streaming` step, `StepTiming::last_activity` set to `now - 35s`; call `build_roster_rows(..., now)` with `now`; assert the row's `ActivityState` is `Stalled`.
- **`test_step_timing_cleared_on_terminal_status`**: `App` with a `Running` step in `step_timing`; call `set_live_step_status(..., Completed)`; assert the entry is cleared from `step_timing`.
- Test coverage target: >=80%
- All tests must pass

### Integration Tests

- **`test_render_idle_roster_no_churn`**: Render the roster 5 times with `RunState::Idle`, no state changes, no `now` advance; assert all 5 renders are identical (no tick-driven churn).
- **`test_render_active_single_agent_shows_glyph_label_elapsed_step`**: Render with a single `Running` agent, elapsed ~1m 20s, step label "thinking"; assert the row contains the active glyph, "working" label, elapsed string, and step text.
- **`test_render_needs_input_pinned_to_top`**: Two agents (explorer first in canonical order, fixer second); fixer has `WaitingForApproval` status; render and assert fixer row appears above explorer.
- **`test_render_stalled_agent_in_place_frozen_glyph`**: `Running` agent stalled 35s; render and assert the row shows the stalled glyph + "stalled?" label in its canonical position (not pinned to top).
- **`test_render_summary_header_counts_working_waiting_stalled`**: Three agents (one `Active`, one `NeedsInput`, one `Stalled`); render and assert the summary header shows "1 working · 1 waiting · 1 stalled".
- **`test_render_no_color_all_states_distinguishable_by_glyph_label`**: Render with `TerminalCaps{ no_color: true }`; assert all agent states are readable from glyph + label without color (verify via `render_to_text` and manual inspection or regex match).
- **`test_accent_identity_preserved_after_pin_reorder`**: Render with agents in canonical order; assert each agent's `accent_index` matches its position in the canonical list, even if pinning reorders the row visually.
- **Test coverage target: >=80%**
- **All tests must pass**

## Success Criteria

- `rebuild_roster_rows` called inside `publish_state` after every state mutation (verified by unit test on status change).
- `refresh_roster_tick` is a no-op during idle runs; active runs with elapsed-bucket changes publish (verified by unit tests above).
- Change-gate suppresses identical rebuilds (no publish churn; verified by test `test_change_gate_suppresses_identical_rebuild`).
- A step quiet ≥30s flips to `Stalled` on the next tick (verified by test `test_step_stall_after_30s_quiet`).
- All tests passing with >=80% code coverage.
- The app-worker select! loop includes the 4th 1 Hz arm and runs without errors under active runs.
- Snapshot tests for idle/active/stalled/needs-input states all match the expected glyph+label+elapsed output.
- `NO_COLOR` rendering is deterministic and every state is distinguishable by glyph+label alone.
