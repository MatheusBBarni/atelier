---
status: pending
title: "build_roster_rows builder (join, classify, elapsed, accent_index, NeedsInput pin)"
type: backend
complexity: medium
dependencies:
  - task_01
  - task_02
---

# Task 03: build_roster_rows builder (join, classify, elapsed, accent_index, NeedsInput pin)

## Overview

Implement the pure builder function that produces `Vec<RosterRow>` from canonical agents, live steps, and step timing. This function is the core orchestration layer that joins agent identity to activity state, classifies each row's lifecycle phase (Active, Stalled, NeedsInput, Idle), computes coarse elapsed times, assigns canonical accent indices immune to reordering, and applies a stable sort to surface NeedsInput rows first. With an injected `now: Instant` parameter, the function remains deterministic for testing and is rebuilt centrally in `publish_state` to keep the renderer pure and snapshot tests flaky-free.


<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. **Signature and injection.** Implement `fn build_roster_rows(agents: &[AgentView], live_steps: &[LiveStepView], timing: &BTreeMap<String, StepTiming>, now: Instant) -> Vec<RosterRow>` with an explicit injected clock for determinism. `StepTiming` is an internal struct on `App` carrying `started_at` and `last_activity` both as `Instant`; tests pass a fixed `now`, production passes `Instant::now()`.

2. **Join and state classification.** For each agent, find its corresponding `LiveStepView` by `agent_id`. Classify its `ActivityState` as:
   - `WaitingForApproval` or `WaitingForAction` from `LiveStepStatus` → `ActivityState::NeedsInput`
   - `Running` or `Streaming` with `now - last_activity < 30s` (const `STALL_THRESHOLD`) → `ActivityState::Active`
   - `Running` or `Streaming` with `now - last_activity >= 30s` → `ActivityState::Stalled`
   - No active step (or terminal status: `Completed`, `Failed`, `Interrupted`) → `ActivityState::Idle`
   - Terminal statuses preserve their existing string label in the `status` field; only the activity-driven states produce new labels.

3. **Canonical accent assignment.** Compute each agent's `accent_index` from its position in the canonical (sorted) agents vec *before* any pin-sort. The canonical order is orchestrator-first then alphabetical, the same order that `build_agent_views` yields. This index must never change due to the NeedsInput pin.

4. **Coarse elapsed formatting.** For active rows only (`ActivityState::Active` or `ActivityState::Stalled`), format elapsed time as `now - started_at` (whole seconds only):
   - 0–59 s: `"0:08"` or `"8s"` per TechSpec (confirm exact format from TechSpec 'Core Interfaces')
   - 60–3599 s: `"1m 20s"` with correct pluralization (`"1m"` vs `"2m"`)
   - 3600+ s: `"1h 5m"` with correct pluralization
   - Idle/terminal rows: `elapsed: None`
   - Non-active: `current_step: None`

5. **NeedsInput pin-sort.** Apply a stable sort using `pin_rank`: `NeedsInput` rows get `pin_rank=0`, all others `pin_rank=1`. Sort by `(pin_rank, canonical_agent_order)` so NeedsInput rows float to the top while everything else preserves the original agent order and `accent_index` values. This is the only permitted reorder.

6. **Step metadata.** On active rows, populate `current_step` from the step's `step_label` (or fallback if label is None). Preserve existing terminal status labels.
</requirements>

## Subtasks

- [ ] 3.1 Define `ActivityState` enum (`Active | NeedsInput | Stalled | Idle`) and `RosterRow` struct with `agent_id, name, accent_index, activity, runtime_model, effort, thinking, current_step, elapsed, status` in `src/app/mod.rs`; add `roster_rows: Vec<RosterRow>` to `AppState`.
- [ ] 3.2 Add the internal `StepTiming` struct (`started_at, last_activity` both `Instant`) to `App` as a `BTreeMap<String /*step_id*/, StepTiming>`; do not serialize. Stamp on `set_active_step_with_metadata` and bump on `push_live_stream_content` and `set_live_step_status`.
- [ ] 3.3 Implement `build_roster_rows(agents, live_steps, timing, now)`: join by agent, classify activity, assign `accent_index` from canonical order before pin-sort, format coarse elapsed on active rows, apply NeedsInput pin-sort, return `Vec<RosterRow>`.
- [ ] 3.4 Implement coarse elapsed formatter as a helper: takes `Duration` (from `now - started_at`) and returns `Option<String>` (None on idle, coarse format on active).
- [ ] 3.5 Unit tests for `build_roster_rows`: each classification branch (Active within threshold, Stalled at/after 30s, NeedsInput for both waiting statuses, Idle when no step), elapsed formatter edge cases (8s, 80s, whole-minute, singular/plural), NeedsInput pin preserves `accent_index` as canonical value, parallel-group multiple-Active rows each with correct elapsed.
- [ ] 3.6 Add `rebuild_roster_rows(&mut self)` on `App`; call it inside `publish_state` after agent/live-step changes so rows update atomically with state.

## Implementation Details

See TechSpec 'Core Interfaces', 'Data Models', 'Development Sequencing', and 'Impact Analysis' for definitive signatures, field lists, and classification thresholds.

### Relevant Files

- `src/app/mod.rs` — where `ActivityState`, `RosterRow`, and `build_roster_rows` live alongside `AgentView`/`LiveStepView` (lines ~59–150 for view-models); where `publish_state` is at line 4050.
- `src/app/mod.rs:4320` — `push_live_stream_content` (activity bump site).
- `src/app/mod.rs:4362` — `set_live_step_status` (status-transition bump site).
- `src/app/mod.rs:3246` — `set_active_step_with_metadata` (timing stamp site).
- `src/app/mod.rs:3286` — `clear_active_step` (timing clear site).

### Dependent Files

- `src/tui/mod.rs` — will consume `state.roster_rows` in the roster render block (rewritten in Task 07); currently reads agents directly (lines ~2114–2177).
- `src/tui/mod.rs:3115` — `item_agent_accent` will repoint to canonical `accent_index` on `RosterRow` (Task 08).
- `src/tui/mod.rs:2488` — `/agent:` dropdown accent lookup will repoint to canonical index (Task 08).
- `src/app/mod.rs:4940` — `build_agent_views` (canonical sort reference; unchanged).

### Related ADRs

- [ADR-003: Roster View-Model Architecture and Render-Time Determinism](../adrs/adr-003.md) — app-layer `build_roster_rows` with injected clock; rebuild in `publish_state`; pure renderer.
- [ADR-004: Refresh Cadence and Stall-Detection Mechanism](../adrs/adr-004.md) — stall from `now - last_activity >= 30s`; `StepTiming` map; `push_live_stream_content` as the single activity chokepoint.
- [ADR-005: Accent-by-Identity Decoupling](../adrs/adr-005.md) — canonical-order `accent_index` on `RosterRow`; immune to pin-sort; repoint roster/chat/dropdown.

## Deliverables

- `ActivityState` enum and `RosterRow` struct in `src/app/mod.rs`, fully serializable, with all required fields.
- `StepTiming` internal struct and `BTreeMap<String, StepTiming>` field on `App`, synchronized across step lifecycle (stamp/bump/clear).
- Pure function `build_roster_rows(agents, live_steps, timing, now) -> Vec<RosterRow>` with correct join, classification (all four states + terminal preservation), accent-index assignment, elapsed formatting, and NeedsInput pin-sort.
- Helper function for coarse elapsed formatting: `now - started_at` → whole-seconds string (`"1m 20s"` etc.) or `None`.
- `rebuild_roster_rows(&mut self)` on `App`, called in `publish_state`.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests verifying roster render snapshots at idle, single-active, needs-input-pinned, stalled, narrow-width, and NO_COLOR scenarios **(REQUIRED)**

## Tests

### Unit Tests

- **Active within threshold:** hand-built `RosterRow` with `now - started_at = 5s`, activity state is `Streaming`, no `last_activity` bump after start → classify as `Active`, elapsed is coarse format.
- **Stalled at 30s exactly:** `now - started_at = 30s`, `now - last_activity = 30s` → classify as `Stalled`, not `Active`.
- **Stalled after 30s:** `now - started_at = 35s`, `now - last_activity = 35s` → classify as `Stalled`.
- **NeedsInput from WaitingForApproval:** status is `WaitingForApproval` → classify as `NeedsInput`, not `Active`.
- **NeedsInput from WaitingForAction:** status is `WaitingForAction` → classify as `NeedsInput`.
- **Idle when no step:** agent has no entry in `live_steps` → classify as `Idle`, elapsed is `None`.
- **Terminal status preserved:** step status is `Completed` → `ActivityState::Idle`, `status` field contains original "completed" label.
- **Elapsed formatter: 8s:** duration `8s` → format is `"0:08"` or `"8s"` (confirm from TechSpec).
- **Elapsed formatter: 80s:** duration `80s` → format is `"1m 20s"`.
- **Elapsed formatter: 60s:** duration `60s` → format is `"1m"` (singular).
- **Elapsed formatter: 120s:** duration `120s` → format is `"2m"` (plural).
- **NeedsInput pin preserves accent_index:** fixture agents `[explorer, fixer]` in canonical order; push `fixer` to `NeedsInput`; assert row order is `[fixer, explorer]` but `fixer.accent_index` remains its canonical value (not 0).
- **Parallel group multiple Active rows:** fixture with group_id, multiple agents with `Running` status at different elapsed times; assert each row has correct elapsed and `accent_index` preserved in original order.
- **Test coverage target: >=80%**
- **All tests must pass**

### Integration Tests

- Render snapshots at 100×24 (TestBackend): idle lineup, single-active row with glyph+label+elapsed+current-step, needs-input pinned to top, stalled-in-place with frozen glyph.
- Narrow-width snapshot (~30–40 cols): step label and elapsed truncate gracefully.
- `NO_COLOR` snapshot (`TerminalCaps{ no_color: true }`): every state disambiguated by glyph+label (colors collapsed).
- Determinism guard: `RunState::Idle` produces no tick-driven churn across repeated renders with no `now` advance.
- **Test coverage target: >=80%**
- **All tests must pass**

## Success Criteria

- `ActivityState` and `RosterRow` are defined and integrated into `AppState`.
- `build_roster_rows` correctly classifies all four activity states, preserves terminal labels, assigns canonical accent indices immune to reordering, formats elapsed coarse times, and applies NeedsInput pin-sort.
- `StepTiming` map is synchronized across step lifecycle with no leaks or desync.
- `rebuild_roster_rows` is called in `publish_state` and rows update atomically with agent/step changes.
- All unit tests pass; test coverage >=80%.
- Snapshots verify idle/active/needs-input/stalled/narrow/NO_COLOR renderings (Task 07 will consume the rows; snapshot churn is expected here).
- No regression to existing accent tests (will be strengthened in Task 08).
- CI invariant `colors_live_only_in_theme_module` still passes (no inline `Color::` literals outside `theme.rs`).
