---
status: pending
title: "activity_glyph / activity_label helpers (Set 1 glyphs + ASCII/NO_COLOR)"
type: frontend
complexity: low
dependencies:
  - task_01
---

# Task 05: activity_glyph / activity_label helpers (Set 1 glyphs + ASCII/NO_COLOR)

## Overview

Add two pure helper functions in `src/tui/mod.rs` to provide the glyph and label vocabulary for the four-state activity model (Active, NeedsInput, Stalled, Idle). These functions form the foundation for accessible, color-independent state rendering and must support both Unicode glyphs and ASCII fallbacks to ensure legibility under `NO_COLOR` and on constrained terminals.


<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>

1. **Function signatures:** Implement `activity_glyph(state: ActivityState, ascii: bool) -> &'static str` and `activity_label(state: ActivityState) -> &'static str` in `src/tui/mod.rs`, adjacent to the existing `status_style` (line ~3366) and `agent_status_label` (line ~3380) functions.

2. **Set 1 glyph mapping (Unicode):**
   - `Active` → `◐` (0x25D0, CIRCLE WITH LEFT HALF BLACK)
   - `NeedsInput` → `◔` (0x25D4, CIRCLE WITH RIGHT HALF BLACK)
   - `Stalled` → `○` (0x25CB, WHITE CIRCLE)
   - `Idle` → `·` (0x00B7, MIDDLE DOT)

3. **ASCII fallback mapping** (when `ascii=true`):
   - `Active` → `>`
   - `NeedsInput` → `?`
   - `Stalled` → `!`
   - `Idle` → `.`

4. **Label mapping:** Each state has a distinct, non-empty text label:
   - `Active` → `"working"`
   - `NeedsInput` → `"waiting"`
   - `Stalled` → `"stalled?"`
   - `Idle` → `"idle"`

5. **NO_COLOR compliance:** All glyphs and labels are plain text strings; no `Color::` literals must appear in these functions (the `colors_live_only_in_theme_module` CI invariant must remain satisfied). Portable BMP glyphs only; ban emoji-presentation symbols (⏸ and others) and double-width characters.

6. **Integration point:** These functions sit beside (not inside) `status_style` and `agent_status_label`, making them available throughout the TUI render path and future `activity_glyph(state, work_in_narrow_mode)` overloads in later tasks.
</requirements>

## Subtasks

- [ ] 05.1 Define `ActivityState` enum in `src/app/mod.rs` (derives Clone, Debug, PartialEq, Eq, Serialize, Deserialize) with four variants: Active, NeedsInput, Stalled, Idle.

- [ ] 05.2 Add `activity_glyph(state: ActivityState, ascii: bool) -> &'static str` function in `src/tui/mod.rs` with Set 1 glyph mapping and ASCII fallback; verify no Color literals appear.

- [ ] 05.3 Add `activity_label(state: ActivityState) -> &'static str` function in `src/tui/mod.rs` with label mapping; confirm labels are pairwise distinct and non-empty.

- [ ] 05.4 Write unit tests asserting each ActivityState maps to the expected Unicode glyph (ascii=false), expected ASCII glyph (ascii=true), and expected label.

- [ ] 05.5 Write unit test asserting all labels are pairwise distinct and non-empty.

- [ ] 05.6 Write NO_COLOR integration test via TestBackend snapshot confirming every state is disambiguated by glyph+label with colors collapsed to Color::Reset.

- [ ] 05.7 Run `cargo test --lib activity_glyph activity_label` and confirm `colors_live_only_in_theme_module` still passes (CI invariant satisfied).

## Implementation Details

See TechSpec 'Core Interfaces' for the `ActivityState` enum and `activity_glyph`/`activity_label` function signatures. These are pure free functions with no dependencies on `Theme`, `AppState`, or render-time context — they are deterministic string lookups suitable for snapshot-testing and CI guards.

### Relevant Files

- `/Users/matheusbbarni/projects/multiagent-harness/src/app/mod.rs` — location for `ActivityState` enum definition (add after line ~90, alongside `LiveStepView` and other view-model types).
- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/mod.rs` — location for `activity_glyph` and `activity_label` functions (add near line 3366, adjacent to `status_style` and `agent_status_label`).
- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/theme.rs` — reference only; verify that no `Color::` literals are added in `activity_glyph` or `activity_label` to ensure the `colors_live_only_in_theme_module` invariant (line 7686) remains satisfied.

### Dependent Files

- `/Users/matheusbbarni/projects/multiagent-harness/src/tui/mod.rs` — test module (line 4076) will host unit and integration tests for the new functions; uses existing `render_to_text`, `render_to_text_with_ui`, `TestBackend`, and `TuiUiState` helpers.
- Future tasks (Task 01–04 completed; Tasks 06–10 pending) will consume `activity_glyph` and `activity_label` in the roster render block rewrite.

### Related ADRs

- [ADR-002: Progress-Confident Roster with a First-Class Stalled State](../adrs/adr-002.md) — establishes glyph+label vocabulary, portable-BMP-only rule, ASCII fallback requirement, and `NO_COLOR` legibility success criterion.
- [ADR-005: Accent-by-Identity Decoupling](../adrs/adr-005.md) — confirms string literals (glyphs/labels) do not use `Color::` and do not conflict with the accent-identity system (independent concern).

## Deliverables

- **`ActivityState` enum** added to `src/app/mod.rs` with four variants (Active, NeedsInput, Stalled, Idle) and standard derives.
- **`activity_glyph(state: ActivityState, ascii: bool) -> &'static str` function** in `src/tui/mod.rs` with Set 1 Unicode and ASCII mappings.
- **`activity_label(state: ActivityState) -> &'static str` function** in `src/tui/mod.rs` with label mappings.
- **Unit tests with 80%+ coverage** **(REQUIRED)** — test matrix covering all four states, both `ascii=false` and `ascii=true` paths for glyphs, and label distinctness.
- **Integration test (NO_COLOR snapshot)** **(REQUIRED)** — TestBackend snapshot at standard dimensions (100×24) showing glyph+label legibility under a NO_COLOR theme (TerminalCaps{ no_color: true }).
- **CI invariant verification** — run `cargo test colors_live_only_in_theme_module` and confirm it passes; no inline `Color::` literals in the new functions.

## Tests

### Unit Tests

- **Test case 1:** `activity_glyph(Active, false)` returns `"◐"` (Unicode half-circle).
- **Test case 2:** `activity_glyph(Active, true)` returns `">"` (ASCII).
- **Test case 3:** `activity_glyph(NeedsInput, false)` returns `"◔"` (Unicode right-half-black).
- **Test case 4:** `activity_glyph(NeedsInput, true)` returns `"?"` (ASCII).
- **Test case 5:** `activity_glyph(Stalled, false)` returns `"○"` (Unicode white circle).
- **Test case 6:** `activity_glyph(Stalled, true)` returns `"!"` (ASCII).
- **Test case 7:** `activity_glyph(Idle, false)` returns `"·"` (Unicode middle dot).
- **Test case 8:** `activity_glyph(Idle, true)` returns `"."` (ASCII).
- **Test case 9:** `activity_label(Active)` returns `"working"`.
- **Test case 10:** `activity_label(NeedsInput)` returns `"waiting"`.
- **Test case 11:** `activity_label(Stalled)` returns `"stalled?"`.
- **Test case 12:** `activity_label(Idle)` returns `"idle"`.
- **Test case 13:** All four labels are pairwise distinct (no duplicate values).
- **Test case 14:** All four labels are non-empty strings.
- **Test coverage target:** >=80%
- **All tests must pass**

### Integration Tests

- **NO_COLOR snapshot test:** Render a TUI frame with a NO_COLOR TerminalCaps (no_color: true, truecolor: false) containing a simple agent view with state=NeedsInput or Stalled. Assert that glyph+label text is present in the rendered output and that no `Color::` values are in the buffer (all text colors resolve to `Color::Reset` or default). Confirm the snapshot text is legible without relying on semantic color names (e.g., "status_ok" must not appear in the text; the state must be clear from glyph and label alone).
- **Test coverage target:** >=80%
- **All tests must pass**

## Success Criteria

- The `ActivityState` enum is defined and derives Clone, Debug, PartialEq, Eq, Serialize, Deserialize.
- `activity_glyph` and `activity_label` are pure functions with no side effects or external dependencies.
- All unit tests pass; each ActivityState maps to the expected glyph (both unicode and ASCII) and label.
- All labels are distinct and non-empty.
- The NO_COLOR integration test confirms that glyph+label pairs are legible under a NO_COLOR terminal (no Color:: literals in the functions; text content alone disambiguates every state).
- The CI invariant `colors_live_only_in_theme_module` passes — no inline Color:: literals outside theme.rs.
- Test coverage >=80%
- All tests passing
