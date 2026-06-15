---
status: completed
title: "View-model types: ActivityState + RosterRow + AppState field"
type: backend
complexity: low
dependencies: []
---

# Task 01: View-model types: ActivityState + RosterRow + AppState field

## Overview

Define the new view-model types (`ActivityState` enum and `RosterRow` struct) that represent activity state and roster data for the live-activity-first agent roster, expose them on `AppState`, and ensure all AppState constructors/defaults/test fixtures initialize the new field. This is a types-only task establishing the foundation for later builder and render tasks — no behavior or business logic, just structure and wiring.


<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. Define `pub enum ActivityState { Active, NeedsInput, Stalled, Idle }` in `src/app/mod.rs` near the existing `AgentView` and `LiveStepView` structs (around line 58–92).
2. Derive `Clone, Debug, PartialEq, Eq, Serialize, Deserialize` on `ActivityState`; use `#[serde(rename_all = "snake_case")]` to match `LiveStepStatus` convention.
3. Define `pub struct RosterRow` with fields: `agent_id: String`, `name: String`, `accent_index: usize`, `activity: ActivityState`, `runtime_model: String`, `effort: String`, `thinking: bool`, `current_step: Option<String>`, `elapsed: Option<String>`, `status: String` — matching TechSpec 'Core Interfaces'.
4. Derive the same trait set on `RosterRow` as `LiveStepView` (Clone, Debug, PartialEq, Eq, Serialize, Deserialize).
5. Add `pub roster_rows: Vec<RosterRow>` field to `AppState` (around line 72–90).
6. Update the `AppState` constructor in `App::new_with_debug` (line ~751) to initialize `roster_rows: Vec::new()`.
7. Update all test fixtures in `src/tui/mod.rs` that construct `AppState` directly (identified via grep: `AppState {` in `render_*.rs` tests) to initialize `roster_rows: Vec::new()`.
8. Ensure serde round-trip tests for `ActivityState` and `RosterRow` verify all variants and fields survive JSON serialization.
9. Add an integration test that publishes an `AppState` with a non-empty `roster_rows` vec via `watch_sender`, reads it via `watch_receiver`, and verifies `roster_rows` are present in the received state.
</requirements>

## Subtasks

- [x] 1.1 Define `ActivityState` enum with four variants and serde conventions
- [x] 1.2 Define `RosterRow` struct with all required fields and trait derives
- [x] 1.3 Add `roster_rows` field to `AppState` struct
- [x] 1.4 Update the primary `AppState` constructor (`App::new_with_debug`) to initialize `roster_rows`
- [x] 1.5 Identify and update all test `AppState` fixtures in `src/tui/mod.rs`
- [x] 1.6 Write serde round-trip unit tests for `ActivityState` and `RosterRow`
- [x] 1.7 Write integration test for `publish_state`→`watch` round-trip with `roster_rows`

## Implementation Details

### Relevant Files

- `src/app/mod.rs` — Home for `ActivityState`, `RosterRow`, and the `AppState` field (see TechSpec 'Data Models' for struct definitions; ADR-003 anchors the view-model in the app layer).
- `src/tui/mod.rs` — Multiple test fixtures construct `AppState` manually (grep confirms ~15–20 instances in tests starting at line 4088). Each must initialize `roster_rows: Vec::new()`.

### Dependent Files

- `src/tui/mod.rs` — The renderer and test framework depend on `AppState`; adding the field requires updating test fixtures to pass `roster_rows: Vec::new()` to avoid compilation errors.
- Any file that serializes/deserializes `AppState` (history, checkpoint) — the new field is public and serializable, so it will appear in persisted state; this is intentional (ADR-003) but means durable records will carry empty `roster_rows` until the builder (task 03) populates it.

### Related ADRs

- [ADR-001: V1 Mechanism and Scope](adrs/adr-001.md) — Establishes the unified `RosterRow` view-model as a foundational design decision.
- [ADR-002: Progress-Confident Roster with a First-Class Stalled State](adrs/adr-002.md) — Confirms the `Stalled` variant in `ActivityState`.
- [ADR-003: Roster View-Model Architecture and Render-Time Determinism](adrs/adr-003.md) — Specifies exactly where and how `RosterRow` and `ActivityState` are defined; the "injected clock" pattern for builder testability.
- [ADR-004: Refresh Cadence and Stall-Detection Mechanism](adrs/adr-004.md) — Describes the `StepTiming` map and internal state (separate from this task).
- [ADR-005: Accent-by-Identity Decoupling](adrs/adr-005.md) — Explains the `accent_index` field on `RosterRow` and its role in color stability.

## Deliverables

- New `ActivityState` enum and `RosterRow` struct in `src/app/mod.rs` with all required derives.
- `AppState` field `roster_rows: Vec<RosterRow>` added and initialized in all constructors and test fixtures.
- Unit tests with 80%+ coverage **(REQUIRED)** covering serde round-trip for each `ActivityState` variant and `RosterRow` with representative field values.
- Integration test verifying `publish_state` → `watch_receiver` preserves `roster_rows` **(REQUIRED)** by checking a published non-empty vec is received intact.
- All new code passes the CI invariant `colors_live_only_in_theme_module` (no inline `Color::` literals).

## Tests

### Unit Tests

- `activity_state_serializes_active` — Serialize and deserialize `ActivityState::Active`; verify variant name is "active" and round-trip succeeds.
- `activity_state_serializes_needs_input` — Serialize and deserialize `ActivityState::NeedsInput`; verify variant name is "needs_input".
- `activity_state_serializes_stalled` — Serialize and deserialize `ActivityState::Stalled`; verify variant name is "stalled".
- `activity_state_serializes_idle` — Serialize and deserialize `ActivityState::Idle`; verify variant name is "idle".
- `roster_row_serializes_with_all_fields` — Construct a `RosterRow` with representative values (e.g. agent_id="explorer", activity=Active, elapsed=Some("1m 20s"), etc.); serialize to JSON, deserialize, and assert equality.
- `roster_row_serializes_with_optional_nones` — Construct a `RosterRow` with `current_step: None` and `elapsed: None`; verify JSON carries nulls and round-trip succeeds.
- `app_state_default_has_empty_roster_rows` — Create an `AppState` via the test fixture pattern (`AppState { session_id, ..., roster_rows: Vec::new() }`); assert `roster_rows.is_empty()`.

### Integration Tests

- `publish_state_carries_roster_rows_through_watch` — Create an `AppState` with a manual `RosterRow` entry (e.g., `roster_rows: vec![RosterRow { agent_id: "explorer", ... }]`); call a hypothetical `publish_state()` that sends via `watch_sender`; read from `watch_receiver`; assert the received state's `roster_rows` is non-empty and contains the expected entry.

### Test Coverage

- Test coverage target: >=80% of the new types and initialization paths.
- All tests must pass: `cargo test --lib app::` and `cargo test --lib tui::` (TUI tests for fixtures).

## Success Criteria

- `ActivityState` and `RosterRow` compile with all required derives and no `Color::` literals.
- All `AppState` constructors and ~20 test fixtures in `src/tui/mod.rs` initialize `roster_rows: Vec::new()` without compilation errors.
- All unit tests pass: serde round-trip for all four `ActivityState` variants and `RosterRow` with field coverage.
- Integration test passes: `publish_state` → `watch` round-trip preserves `roster_rows`.
- All tests passing and test coverage >=80%.
- No new linter/CI failures introduced.
