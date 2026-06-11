---
status: completed
title: Thread theme through TUI and migrate all inline colors
type: refactor
complexity: high
dependencies:
  - task_02
---

# Task 3: Thread theme through TUI and migrate all inline colors

## Overview

Add the resolved `Theme` to `TuiUiState` and replace all 96 inline `Color::` literals in `src/tui/mod.rs` with semantic tokens, then lock the invariant with a source-scanning test. This is the load-bearing refactor (ADR-001): after it, one file owns every color, yellow-overload is resolved by semantic assignment, and NO_COLOR/256-color correctness applies to the whole TUI.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. `TuiUiState` MUST gain a `theme: Theme` field; `run_tui` MUST construct it via `Theme::resolve(TerminalCaps::detect())` once at startup (ADR-004).
2. All 96 `Color::` literals in `src/tui/mod.rs` MUST be replaced with theme tokens; the migration MUST be behavior-preserving in structure (no layout/text changes) while colors move to the brand palette.
3. Semantic helpers (`status_style` :1954, `severity_badge_style` :1817, `severity_title_style` :1826, `availability_style` :1976) MUST keep their signatures and read tokens from the threaded theme (ADR-004).
4. A source-invariant test MUST assert `Color::` appears nowhere in `src/tui/**.rs` outside `theme.rs` (TechSpec "Testing Approach").
5. All ~36 `TuiUiState` construction sites (Default impl :138-160, 34 test literals, 2 test helpers :4009/:4016) MUST be updated; tests SHOULD default to a fixed truecolor test theme so assertions are deterministic.
6. The existing 59+ render tests MUST pass; they assert text content only (verified — no color assertions), so failures indicate real regressions.
7. The yellow-overload MUST be resolved by semantic assignment: input border, dropdown chrome, help modal, and status styling each map to their designated tokens, not one shared color.
</requirements>

## Subtasks
- [x] 3.1 Add `theme` to `TuiUiState`, its `Default` impl (test theme), and the startup construction in `run_tui`.
- [x] 3.2 Update the ~36 test construction sites and the two `ui_state_with_*` helpers. (Most inherit `theme` via `..Default::default()`; only the two direct-call sites — `legacy_chat_line`, `render_skill_loading` — needed edits.)
- [x] 3.3 Migrate the helper functions (`status_style`, `severity_badge_style`, `severity_title_style`, `availability_style`) to tokens. (Each gains a `theme: &Theme` param — they take no `ui_state`, and ADR-004 forbids a global theme.)
- [x] 3.4 Migrate the remaining literals function by function.
- [x] 3.5 Remove `USER_EVENT_BG` const in favor of `theme.user_prompt_bg`.
- [x] 3.6 Add the source-invariant test scanning `src/tui/` for `Color::` outside `theme.rs`.
- [x] 3.7 Run the full test suite; string assertions unchanged (content-only, as verified).

## Implementation Details

Single-file mechanical migration in `src/tui/mod.rs` (4,673 lines; render code to ~2,290, tests from :2293). `TuiUiState` defined at :116-160. Literal distribution by function is enumerated in the exploration notes; migrate in that order to keep diffs reviewable. No structural refactors beyond the field addition (ADR-001 scope discipline). See TechSpec "Implementation Design" for token mapping.

### Relevant Files
- `src/tui/mod.rs` — `TuiUiState` (:116-160), all 96 literals (:39, :1257-2266), helpers (:1817-1991), test module (:2293-4673), `render_to_text*` helpers (:4005-4158).
- `src/tui/theme.rs` — token source (task_02 output).

### Dependent Files
- `src/app/mod.rs` — `AppState`/`AgentView` consumed by render functions; unchanged but exercised by every test.
- `src/app/chat/mod.rs` — `ChatSeverity`/`ChatLineStyle` drive the severity helpers being migrated.
- `src/config/mod.rs` — `EffectiveConfig` flows into `render()`; unchanged.

### Related ADRs
- [ADR-001: V1 Scope and Sequencing](../adrs/adr-001.md) — migration in the same window; no structural refactor.
- [ADR-004: Theme Module Architecture](../adrs/adr-004.md) — threading via `TuiUiState`.

## Deliverables
- `theme` field threaded; zero inline `Color::` literals outside `theme.rs` (from 96).
- Source-invariant test guarding the migration permanently.
- Unit tests with 80%+ coverage of changed helper logic **(REQUIRED)**
- Integration: full existing render suite green **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Source-invariant: scanning `src/tui/` finds `Color::` only in `theme.rs`.
  - [x] `status_style("running")` returns the theme's `status_ok` with bold; `status_style("disabled")` returns `text_dim`.
  - [x] `severity_badge_style(Error)` uses `status_error` background.
- Integration tests:
  - [x] All pre-existing render tests pass (content assertions unchanged).
  - [x] `render_to_text` smoke at 80x24 with a NO_COLOR-resolved theme contains identical text content to the truecolor render.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `grep -c "Color::" src/tui/mod.rs` returns 0.
- `cargo clippy` clean; no behavior change in layout/text (diff of `render_to_text` output is color-only).
