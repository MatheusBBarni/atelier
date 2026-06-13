---
status: pending
title: "TuiUiState recall fields and async history loader"
type: frontend
complexity: medium
dependencies:
  - task_01
  - task_02
---

# TuiUiState recall fields and async history loader

## Overview

Hold the recall ring in the UI and populate it without blocking the render. Add
three `TuiUiState` fields and a detached background loader (mirroring the file-index
walk) that runs `project_prompt_history` off-thread and delivers the result over a
`watch` channel into UI state (ADR-004), gated by the config toggle.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `prompt_history: Vec<String>`, `prompt_history_cursor: usize`, and `prompt_history_draft: String` to `TuiUiState`, initialized empty/zero in every constructor and test builder.
- MUST add a `watch::Sender<Vec<String>>`/receiver pair in `run_tui` and sync the latest value into `TuiUiState.prompt_history` on each render-loop tick.
- MUST spawn a detached `spawn_blocking` load after the first frame that calls `project_prompt_history(root, prompt_history_max)` and sends the result, mirroring `spawn_file_index_refresh`/`refresh_file_index`.
- MUST skip spawning the loader and leave the ring empty when `prompt_history_enabled == false`.
- MUST NOT block the first paint and MUST NOT re-scan disk on every keystroke (load once).
</requirements>

## Subtasks
- [ ] 4.1 Add the three fields and update constructors/`Default`/test builders.
- [ ] 4.2 Add the `watch` channel in `run_tui` and thread the receiver into `run_loop`.
- [ ] 4.3 Spawn the detached loader after the first render, gated on the toggle.
- [ ] 4.4 Sync received history into `TuiUiState` each loop tick.
- [ ] 4.5 Test that the ring populates from a delivered value and stays empty when disabled.

## Implementation Details

Edit `src/tui/mod.rs`: `TuiUiState` (insert fields after `input_width`),
`run_tui`/`run_loop`, and a new `spawn_prompt_history_load` modeled on
`spawn_file_index_refresh`/`refresh_file_index`; sync via the same approach as
`sync_file_index`. Calls `history::project_prompt_history` (task_01) and reads the
config knobs (task_02). See TechSpec "System Architecture" (data flow: load) and
"Implementation Design → Core Interfaces" (async delivery).

### Relevant Files
- `src/tui/mod.rs` — `TuiUiState`, `run_tui`, `run_loop`, `spawn_file_index_refresh`/`refresh_file_index` (the pattern to mirror), `sync_file_index` (the sync pattern).
- `src/history/mod.rs` — provides `project_prompt_history` (task_01).
- `src/config/mod.rs` — provides `prompt_history_enabled` / `prompt_history_max` (task_02).

### Dependent Files
- `src/tui/mod.rs` (task_05) — recall navigation reads `prompt_history`, cursor, and draft.
- `src/tui/mod.rs` (task_07) — the hint reads `prompt_history` presence.

### Related ADRs
- [ADR-004: Asynchronous Background History Projection](../adrs/adr-004.md) — detached load + `watch` delivery, gated by the toggle.

## Deliverables
- Three UI-state fields, a non-blocking background loader, and per-tick sync.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: with history on disk, the ring populates after the load tick; disabled → empty **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] Sending `["b","a"]` on the watch channel → after a sync tick `ui_state.prompt_history == ["b","a"]`.
  - [ ] `prompt_history_enabled == false` → loader is not spawned and the ring stays empty.
  - [ ] New `TuiUiState` builders initialize `prompt_history_cursor == 0` and an empty draft/ring.
- Integration tests:
  - [ ] A `.multiagent/` with two `prompt_submitted` events → after startup load, `ui_state.prompt_history` holds both, newest-first.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- First paint is not blocked; the ring is populated off-thread
- Disabling the toggle leaves the ring empty and skips the load
