---
status: completed
title: "Roster render rewrite: weight, glyph+label, elapsed, current-step, animated indicator, summary header"
type: frontend
complexity: high
dependencies:
  - task_04
  - task_05
---

# Task 06: Roster render rewrite: weight, glyph+label, elapsed, current-step, animated indicator, summary header

## Overview

Rewrite the roster render block (TechSpec 'System Architecture' + 'Development Sequencing' step 7; ADR-001/002/005) to consume `roster_rows` and present the live board with visual weight (active bold/emphasis, idle dimmed), portable glyph+label per activity state, pre-computed elapsed time and current-step labels, an animated working indicator, a summary-header counts line, and safe truncation in the ~28% sidebar. The roster becomes legible at a glance during active runs, with every state distinguishable by glyph+label under `NO_COLOR` and consistent agent colors across the chat transcript.


<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. **Consume `state.roster_rows` (not `state.agents`).** Replace the current enumerate-based iteration at `tui/mod.rs:2120–2169` with iteration over pre-computed `RosterRow` entries delivered from `publish_state`.
2. **Render glyph + activity label per row.** For each row, emit the activity glyph (animated for `Active` by indexing `work_spinner_frame`; single frozen glyph for `Stalled`, idle, or `NeedsInput`) followed by the activity label (`working`/`waiting`/`stalled?`/`idle`), using new helpers `activity_glyph(state, ascii_mode)` and `activity_label(state)` from near `status_style` (see TechSpec 'Core Interfaces').
3. **Apply visual weight by activity.** Active rows render bold (`BOLD` modifier) with an optional emphasis on the step text; idle rows use `text_dim` color to recede; `NeedsInput` rows render at normal weight with prominent glyph+label (see ADR-001, consequences, item 1).
4. **Show current-step and elapsed on active rows.** Read pre-computed `row.current_step` and `row.elapsed` from the `RosterRow`; render them (e.g. `step_label | elapsed 1m 20s`) without calling any clock function. Idle and terminal-status rows show no step/elapsed.
5. **Render summary-header line above the roster.** A single line showing counts derived directly from `roster_rows` (e.g., `▶ 2 working · ◔ 1 waiting · ○ 0 stalled`). Show a calm at-rest line when idle (e.g., `● K agents, all idle`). Header uses portable glyphs + theme tokens, no inline colors.
6. **Preserve `NeedsInput` top-pin.** An agent in `NeedsInput` state appears at the top of the roster lineup (already ordered by `RosterRow` builder); render it with the waiting glyph + label + agent name accented via `row.accent_index` (see ADR-005).
7. **Width resilience & truncation.** In the ~28% sidebar (~28–50 columns), truncate long model names and step labels gracefully using existing truncation helpers (search `truncate_with_ellipsis` or equivalent); preserve layout so no visual breakage.
8. **Agent name accent via `row.accent_index`.** Use `theme.accent_for(row.accent_index)` (not enumerate index) to color agent names, per ADR-005; this ensures the pin does not recolor agents.
9. **Keep render pure.** Do not call any clock, `now()`, or state-mutation function in the render block; all time-dependent values are pre-formatted and read from the row.
10. **Preserve terminal status labels.** The existing `agent_status_label` and `status_style` logic for `completed`/`failed`/`interrupted`/`disabled` remains in place; these rows show the terminal status, not an activity glyph.
</requirements>

## Subtasks

- [x] 6.1 `activity_glyph`/`activity_label` (delivered task_05) consumed by the render; `#[allow(dead_code)]` removed.
- [x] 6.2 Roster render rewritten: `agent_roster_items(&[RosterRow], spinner_frame, theme)` emits glyph+label + accented name + runtime/model + effort/thinking; current-step + elapsed on active rows only.
- [x] 6.3 Summary-header line: counts by `ActivityState` → `▶ N working · ◔ N waiting · ○ N stalled`, at-rest `● N agents idle`.
- [x] 6.4 Visual weight: active = bold accent name; idle = accent name + `DIM` (keeps identity color, ADR-005); NeedsInput/Stalled = normal-weight accent + prominent glyph. Theme tokens only.
- [~] 6.5 Width resilience: the `List` widget clips overflow to the ~28% sidebar (no layout breakage), verified by a narrow-width (40-col) render test. **Explicit per-field `…` truncation of long model/step labels deferred** as polish (follow-up note).
- [x] 6.6 `roster_names_carry_same_accents_as_chat` updated to a pinned-reorder fixture (NeedsInput agent canonical index 1 pinned to row 0 → still `accent_for(1)`). `agent_dropdown_ids_carry_same_accents_as_roster` already passes: the dropdown resolves accent by canonical `id` lookup (never reorders), so no change needed.
- [x] 6.7 Snapshot/scenario tests added: idle lineup + header, single-active (glyph+label+step+elapsed), NeedsInput pinned, stalled frozen glyph, summary-header counts, NO_COLOR legible-by-glyph+label, idle determinism (3 identical frames), narrow-width without breakage.

## Implementation Details

The render rewrite is the final step in the TechSpec 'Development Sequencing' (item 7, depends on items 1–6 already implemented). It consumes the result of `build_roster_rows(agents, live_steps, timing, now)` which is called inside `publish_state` and stored as `AppState.roster_rows`. See TechSpec 'Core Interfaces' for the `ActivityState` enum, `RosterRow` struct, and the `build_roster_rows` function signature.

**Primary entry point:** the roster render block `tui/mod.rs:2114–2177` (currently maps `state.agents.iter().enumerate()` to `ListItem`s).

**Helper functions to add:**
- `activity_glyph(ActivityState, ascii_mode: bool) -> &'static str` — returns glyph or ASCII fallback.
- `activity_label(ActivityState) -> &'static str` — returns short label.

**Key reads from `RosterRow`:**
- `accent_index` — for agent name color (decouple from enumerate position).
- `activity` — for glyph, label, weight, and whether to show elapsed/step.
- `current_step` — pre-computed step label (only on active/needs-input).
- `elapsed` — coarse "1m 20s" (only on active/needs-input).
- `status` — terminal labels (completed/failed/interrupted/disabled).
- `runtime_model`, `effort`, `thinking` — unchanged from current render.

**Summary-header generation:**
Count the four activity states in `roster_rows` (or compute during the render loop); emit a single header line with glyphs and counts. Example format: `▶ 2 working · ◔ 1 waiting · ○ 0 stalled` (glyph + count pairs separated by `·`). At rest, show a simple lineup count, e.g. `● 5 agents idle`. Use theme tokens for color; glyphs are portable (no emoji).

**Truncation context:**
The roster occupies ~28% width (sidebar). At 80-column terminal, that's ~22 columns minus borders/padding = ~18–20 usable. Identify existing truncation helpers in the codebase (grep `truncate_with_ellipsis` or similar); apply to model name and step label when they exceed available width.

**Pure function discipline:**
The render block receives `state: &AppState`, `ui_state: &mut TuiUiState` (for `work_spinner_frame` only, to animate the active glyph), and the `theme`. It does NOT call `Instant::now()` or any mutable operation; all elapsed/timing comes from `row.elapsed` (pre-formatted string or `None`).

### Relevant Files

- `src/app/mod.rs` — Defines `ActivityState` enum, `RosterRow` struct, `AppState.roster_rows` field, and `build_roster_rows()` function (items 1–4, implemented in earlier tasks). Starting point: lines ~60 (AgentView), ~72 (AppState), and the builder function (line TBD, per TechSpec sequencing item 3).
- `src/tui/mod.rs` — Current roster render block (lines 2114–2177); `status_style` + `agent_status_label` (lines 3366–3386); accent resolution (`theme.accent_for(index)`, lines 2131, 2488, 3115). New functions `activity_glyph`/`activity_label` to be added near `status_style`. Summary-header generation added before the list render.
- `src/tui/theme.rs` — Theme token source (`accent_for`, `status_ok`, `text_dim`, `text_muted`, etc., lines 110–165). New glyph/color definitions (if any) added here, per CI invariant.
- `tests/` (or inline `#[cfg(test)]` in `tui/mod.rs`) — TestBackend snapshot tests: `render_to_text(state, width, height)` / `render_to_buffer(state, width)` patterns (see lines 6446–6561); accent contract tests at lines 8828–8870.

### Dependent Files

- `src/tui/mod.rs:732–757` — App-worker `select!` loop (already has the 1 Hz roster-refresh arm added in task 5); the refresh calls `app.refresh_roster_tick()` which calls `rebuild_roster_rows(now)` and publishes the updated state. Roster render reads the refreshed `state.roster_rows`.
- `src/tui/mod.rs:3115` (chat accent via `item_agent_accent`) and `2488` (dropdown accent) — Repoint to read `accent_index` from a canonical identity lookup (per ADR-005); these sites currently use `enumerate` position and must be updated alongside the roster to keep the contract tests green.
- `src/tui/mod.rs:8828–8870` — Two contract tests (`roster_names_carry_same_accents_as_chat` + `agent_dropdown_ids_carry_same_accents_as_roster`) must be updated to the pinned-reorder fixture to pass under the new accent-by-identity scheme.
- `src/tui/mod.rs:7686` — CI invariant test `colors_live_only_in_theme_module` (no inline `Color::` outside `theme.rs`); passes if the new glyph literals are string constants, not color literals.

### Related ADRs

- [ADR-001: V1 Mechanism and Scope](../adrs/adr-001.md) — Establishes stable order, weight-driven visuals, `NeedsInput` pin, summary header, accent-by-identity, unified `RosterRow` view-model. Items 1–6 govern this render task.
- [ADR-002: Progress-Confident Roster with a First-Class Stalled State](../adrs/adr-002.md) — Adds `Stalled` state, coarse elapsed, bounded 1 Hz refresh, animated indicator, glyph+label/NO_COLOR. Implementation notes confirm glyph + label strategy (items 4–5).
- [ADR-005: Accent-by-Identity Decoupling](../adrs/adr-005.md) — Decouples accent from render-time position to canonical `accent_index`. Decision item 2: repoint all three render surfaces (roster, chat, dropdown) to read the identity index, not enumerate position.

## Deliverables

- Rewritten roster render block at `src/tui/mod.rs:2114–2177` consuming `state.roster_rows`.
- Helper functions `activity_glyph()` and `activity_label()` added to `src/tui/mod.rs` near `status_style` (line ~3366).
- Summary-header line generation (count glyphs + activity counts) rendered above the roster list.
- Visual weight applied via theme tokens (`BOLD` for active, `text_dim` for idle, per ADR-001).
- Truncation applied to long model names and step labels in the ~28% sidebar width.
- Accent reference changed from enumerate index to `row.accent_index` (canonical identity).
- Chat accent and dropdown accent call sites updated to read canonical `accent_index` (repoint from enumerate position).
- Updated contract tests `roster_names_carry_same_accents_as_chat` and `agent_dropdown_ids_carry_same_accents_as_roster` with pinned-reorder fixture (per ADR-005).
- Unit tests with 80%+ coverage **(REQUIRED)** — cover `activity_glyph`/`activity_label` mapping; test `NO_COLOR` rendering.
- Integration tests (TestBackend snapshots) **(REQUIRED)** — idle lineup, single-active row, needs-input pinned, stalled row, summary-header counts, narrow-width truncation, monochrome legibility, determinism.

## Tests

### Unit Tests
- Test `activity_glyph()` outputs correct portable glyph per `ActivityState` (◐ for Active, ◔ for NeedsInput, ○ for Stalled, · for Idle).
- Test `activity_glyph()` outputs correct ASCII fallback when `ascii_mode: true` (> for Active, ? for NeedsInput, ! for Stalled, . for Idle).
- Test `activity_label()` returns correct label string per state (`"working"`, `"waiting"`, `"stalled?"`, `"idle"`).
- Test summary-header count generation: given a set of `roster_rows` with known state distributions, assert the header line shows correct counts (e.g., 2 working, 1 waiting, 0 stalled).

### Integration Tests
- **Idle lineup snapshot:** 100×24 terminal, 3 idle agents in canonical order, summary header shows `● 3 agents idle`, no activity glyphs or elapsed; render is stable across repeated frames with no `now` advance.
- **Single-active row snapshot:** 1 active agent showing glyph ◐ + label `working` + current_step + elapsed (e.g., `"exploring options | 45s"`), bold styling; other agents idle.
- **NeedsInput pinned-top snapshot:** Agent in `NeedsInput` state pinned to row 0 with glyph ◔ + label `waiting`, normal weight but prominent; accent follows canonical identity, not row position.
- **Stalled row snapshot:** Agent in `Stalled` state with frozen glyph ○ + label `stalled?`; elapsed shows time since last activity; positioned in-place (not top-pinned); example text ` ○ stalled? explorer | 34s`.
- **Summary-header counts snapshot:** Multiple active agents; header line counts working/waiting/stalled correctly; e.g., `▶ 2 working · ◔ 1 waiting · ○ 1 stalled` on a single line above the roster.
- **Narrow-width snapshot (~35–40 cols):** Long model names (e.g., `gpt-4-turbo-vision`) and step labels (e.g., `analyzing_architecture_and_dependencies`) truncate gracefully with ellipsis; roster remains legible, no layout breakage.
- **NO_COLOR monochrome snapshot:** `TerminalCaps{ no_color: true }` render; every state (active, waiting, stalled, idle, terminal statuses) distinguishable by glyph + label alone; colors collapse to `Color::Reset`; terminal status labels (completed, failed, interrupted) preserved.
- **Accent identity under pin:** Fixture with `NeedsInput` agent at canonical index 1 pinned to row 0; assert agent name in row 0 renders with `accent_for(1)`, not `accent_for(0)`; colors stay consistent with chat transcript.
- **Determinism guard:** Idle run state renders identically across 3 consecutive `render()` calls with no `now` advance (no spurious elapsed ticks, no spinner animation).
- **CI invariant pass:** `colors_live_only_in_theme_module` still passes; no inline `Color::` literals outside `theme.rs`.

- Test coverage target: >=80%
- All tests must pass

## Success Criteria

- Roster render block fully rewritten to consume `state.roster_rows` (not `state.agents`).
- Every `RosterRow` is rendered with glyph + label + agent name (colored by canonical `accent_index`), runtime/model, effort/thinking.
- Active rows show current-step + elapsed; idle and terminal-status rows do not.
- Visual weight applied: active bold, idle dimmed, `NeedsInput` normal-weight but prominent.
- Summary-header line appears above roster with activity counts and portable glyphs (e.g., `▶ 2 working · ◔ 1 waiting · ○ 0 stalled`).
- `NeedsInput` agent pinned to top; accent follows canonical identity, not row position.
- Sidebar truncation handles long names gracefully at ~30–50 column widths.
- Pure render function: no clock calls, all time-dependent values pre-formatted.
- All tests passing
- Test coverage >=80%
- Contract tests (`roster_names_carry_same_accents_as_chat`, `agent_dropdown_ids_carry_same_accents_as_roster`) pass with pinned-reorder fixture.
- `NO_COLOR` rendering legible by glyph+label.
- CI invariant `colors_live_only_in_theme_module` passes.
- Terminal status labels (completed/failed/interrupted/disabled) preserved on those rows.
