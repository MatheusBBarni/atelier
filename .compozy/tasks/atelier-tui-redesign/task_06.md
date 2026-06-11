---
status: pending
title: Persistent status footer
type: frontend
complexity: medium
dependencies:
  - task_03
  - task_05
---

# Task 6: Persistent status footer

## Overview

Grow the existing one-line status area into the ambient-state footer (PRD F3): `repo · branch` (omitted outside git) · run state · active agent count, with the existing working spinner and `/help` hint preserved. The footer answers "where am I, what's happening, who's working" continuously — the wrong-branch footgun guard.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. The footer MUST display, left to right: repo+branch segment (from `state.git_context`, omitted entirely when `None`), run-state label (Idle/Planning/Running/Waiting for user — from `RunState`, `src/orchestrator/mod.rs:14-23`), and active agent count (e.g. "3 agents · 2 running") computed from `AgentView.status` running states (`running`, `streaming`, `running_parallel`).
2. The current status line is 1 line tall and already full (spinner + "Working" + status_message + `/help`, `render_input_status` :2037-2100). The footer MUST gain a second line: increase `INPUT_COMPOSER_HEIGHT` (:40) and the `input_areas` constraints (:2022-2029) so existing content keeps line 1 and ambient state takes line 2 — mirroring the reference design's two-line footer.
3. Existing behaviors MUST be preserved: spinner during work, `status_message` display when idle, `/help` hint, stable composer height between idle and running (test :2435 updated for the new constant, still asserting stability).
4. All styling MUST use theme tokens (the task_03 invariant test enforces this).
5. On narrow terminals the footer MUST degrade gracefully: truncate the branch segment first, never panic or wrap.
6. Footer content updates MUST come only from state changes (the change-gated poll, task_05) — no per-frame computation beyond formatting.
</requirements>

## Subtasks
- [ ] 6.1 Extend the input-area layout: new footer line constant, adjusted constraints, updated `composer_height`.
- [ ] 6.2 Implement the footer line builder: git segment, run-state label, agent-count summary, theme-token styling.
- [ ] 6.3 Compute running counts from `AgentView.status` using the established status-string sets (:1954-1973).
- [ ] 6.4 Preserve spinner/status_message/hint behaviors on the original line.
- [ ] 6.5 Truncation behavior for narrow widths.
- [ ] 6.6 Update height-stability and work-indicator tests for the new layout; add footer content tests.

## Implementation Details

Exploration confirmed the constraint: `INPUT_COMPOSER_HEIGHT = 5` (4 input + 1 status) has no spare room — the two-line approach is the deliberate resolution, not an accident. `work_indicator_active` (:2004) and `render_input_status` (:2037-2100) are the integration points. Run-state read pattern at :1372. See TechSpec "Component Overview".

### Relevant Files
- `src/tui/mod.rs` — `INPUT_COMPOSER_HEIGHT` (:40), `WORK_INDICATOR_HEIGHT` (:49), `input_areas` (:2014-2035), `render_input_status` (:2037-2100), `work_indicator_active` (:2004), status-string sets (:1954-1973).
- `src/orchestrator/mod.rs` — `RunState` enum (:14-23).
- `src/app/mod.rs` — `AppState.run_state`, `agents`, `git_context` (task_05).

### Dependent Files
- `src/tui/mod.rs` tests — `renders_work_indicator_below_input_while_run_is_active` (:2397), `hides_work_indicator_when_run_is_idle` (:2425), `input_area_height_is_stable_between_idle_and_running` (:2435) need the new height constant.

### Related ADRs
- [ADR-006: Polled Git Context Refresh](../adrs/adr-006.md) — data source and freshness semantics.
- [ADR-001: V1 Scope and Sequencing](../adrs/adr-001.md) — footer as the primary git-context consumer.

## Deliverables
- Two-line footer with git segment, run state, agent counts; existing indicators intact.
- Unit tests with 80%+ coverage of segment formatting **(REQUIRED)**
- Integration tests for footer rendering across states **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Footer line with `Some(GitContext{repo:"atelier", branch:"main"})` contains "atelier" and "main"; with `None` contains neither and no separator artifact.
  - [ ] Agent summary for 3 agents with statuses [running, idle, streaming] reads "3 agents · 2 running"; all idle reads "3 agents".
  - [ ] Run-state labels render for each `RunState` variant used in the footer.
  - [ ] Narrow width (40 cols) truncates the branch segment without panicking.
- Integration tests:
  - [ ] `render_to_lines_with_ui_mut` at 80x24: footer line visible below the status line in idle and running states; composer height stable between the two.
  - [ ] State update with a changed branch re-renders the footer with the new value (watch-channel path).
  - [ ] Existing work-indicator tests pass with updated height constant.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Footer reflects branch switches mid-session (PRD phase-2 criterion, via task_05 polling).
- Non-git directories show no footer errors or empty separators.
