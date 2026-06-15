# Task Memory: task_03.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Pure `build_roster_rows` builder + `format_coarse_elapsed` helper + `rebuild_roster_rows` wired into `publish_state`. DONE.

## Important Decisions

- **`publish_state` made `&mut self`** to call `rebuild_roster_rows()` centrally (ADR-003). Build was clean — no `&self` callers existed, so zero blast radius.
- **`Starting` and `Cancelling` statuses classify as Active/Stalled** (same branch as `Running`/`Streaming`). The spec/techspec only name Running/Streaming explicitly, but a Starting step has a stamped timing entry and is clearly active work, not Idle; Cancelling is winding down but still active. Defensible interpretation; terminal statuses (Completed/Failed/Interrupted) remain Idle.
- **Sub-minute elapsed format = `"8s"`** (not `"0:08"`). Requirement 4 allowed either; chose the form consistent with `"1m 20s"`/`"1h 5m"` coarse style (ADR-004 line 28).
- **`RosterRow.status` always carries `agent.status`** for every row; the activity-driven label is conveyed via `activity`. Renderer (task_06) chooses label by activity for active states and by `status` for terminal — keeps terminal labels preserved.
- Used `now.saturating_duration_since(...)` (not `now - t`) to be panic-safe if a future `now` ordering edge ever appears.

## Learnings

- Join key: `LiveStepView.agent` holds the agent **id** (matches `AgentView.id`).
- fake_config yields 8 agents (orchestrator, explorer, fixer, reviewer, oracle, consul + 2 council-derived), not 6 — assert against `state.agents.len()` rather than a literal.
- Two task_01 placeholder tests had to be updated for the new rebuild-on-publish behavior: `app_state_default_has_empty_roster_rows` → renamed `app_state_after_construction_has_roster_row_per_agent`; `publish_state_carries_roster_rows_through_watch` now asserts rebuilt canonical rows.

## Files / Surfaces

- `src/app/mod.rs`: `STALL_THRESHOLD` const (near `StepTiming`); `format_coarse_elapsed`, `build_roster_rows`, `classify_step`, `step_display_label` (after `build_agent_views`); `rebuild_roster_rows` + `publish_state` (`&mut self` now).
- 13 new unit tests + 2 updated placeholder tests in the `app::tests` module.

## Errors / Corrections

- N/A beyond the 2 placeholder-test updates above.

## Ready for Next Run

- **Integration render-snapshot tests DEFERRED to task_06/07.** The renderer still reads `state.agents` (tui/mod.rs:2473), not `roster_rows`. Snapshot tests for idle/active/needs-input/stalled/narrow/NO_COLOR can only be authored once task_06 rewrites the render block to consume `roster_rows`. Writing them now would snapshot the old render and ignore the builder output.
- Pre-existing env failures persist (unrelated): `actions::tests::unrestricted_reads_flag_*` (2). Confirmed not regressions.
- task_04 will add the change-gate to the 1Hz path; current `publish_state` rebuilds + publishes unconditionally (correct for task_03).
