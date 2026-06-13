# Task Memory: task_04.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot

Rendered the queued follow-up state in the TUI and added keyboard controls to cancel/resume items. Compact queue panel + a "queue focus" key mode. Completed 2026-06-11. Builds on [[task_01]], [[task_02]], [[task_03]].

## Important Decisions

- **"Queue focus" key mode** = `queue_control_active(state, ui_state)`: input empty AND `queued_follow_ups` non-empty AND no help/clarification/approval. Inserted into the `key_event_to_tui_command_with_ui` precedence chain AFTER the skill dropdown, BEFORE normal input: `queue_control_key_command(...).or_else(|| key_event_to_tui_command(state, key))`. The `/agent:`/`/skill:` dropdowns require non-empty input, so they're mutually exclusive with queue focus (which requires empty input) — this is why "dropdown routing unchanged" holds for free.
- **Keys (queue focus only)**: `Up`/`Down` → `TuiCommand::QueueSelection(Previous/Next)` (new variant) updating `ui_state.queue_selection_index` (new field) with wrap; `Delete` → `Dispatch(FollowUpCancelled(id))` for Pending/Paused selected item; `Ctrl+R` → `Dispatch(FollowUpResumeRequested(id))` for Paused selected item only. Cancel/resume reuse the existing `TuiCommand::Dispatch` arm (no new dispatch plumbing). All other keys fall through → Ctrl+C interrupt, Ctrl+L roster, scroll, and char-to-compose are preserved. Chose non-char keys (Delete, Ctrl+R) so the first char typed exits queue focus by composing rather than triggering an action.
- **Compact panel** rendered as a NEW middle layout row in `render()` (between the chat/roster area `outer[0]` and the composer), present only when `queue_panel_height(state) > 0` (queue non-empty). When empty → 2-row layout exactly as before, so all prior render tests are untouched. Panel is full-width (not split by the roster). Title ` Queue (N) `; one line per item `{marker}[status] {prompt}[ — pause_reason]`; selected item marked `> ` only while queue focus is active; caps at `QUEUE_VISIBLE_MAX=6` + a "…and N more" row + a dark-gray hint row (`QUEUE_HINT`).
- Status styling distinguishes all four states: pending=cyan, paused=yellow(+reason), replaying=green bold, cancelled=dark gray. `Replaying` never appears in app-produced state (task_02 pops on replay) but the panel handles it for completeness/robustness and the spec test builds it manually.
- Refactored `render()` to compute `main_area`/`queue_area`/`composer_area` from the conditional outer split and replaced the old `outer[0]`/`outer[1]` uses with those names (`composer_area = outer[outer.len()-1]`).

## Learnings

- Stale `queue_selection_index` (after a replay pops an item, shrinking the Vec) is handled by clamping with `.min(len-1)` in `selected_queue_item`, `apply_queue_selection_command`, and `render_queue_panel` — never indexes out of bounds.
- Test harness: `state_with_input("", false)` + set `.queued_follow_ups`; `render_to_text(state, w, h)` renders with default `TuiUiState` (input empty → queue focus active → selected marker shows). Key tests use `key(code)` / `key_with_modifiers(code, mods)` + `key_event_to_tui_command_with_ui(&state, &ui_state, key)` and assert the returned `TuiCommand`; dispatch side-effects via `execute_tui_command(&mut state, &mut ui_state, &sender, cmd)` then `receiver.try_recv()` → `AppWorkerCommand::Event(...)`.
- Added imports `QueuedFollowUpStatus, QueuedFollowUpView` to the `crate::app` use block; added `queue_prompt_summary` (whitespace-collapse) next to `truncate_to_char_width` (Paragraph without `.wrap()` clips long lines to width).

## Files / Surfaces

- `src/tui/mod.rs` — imports; `TuiCommand::QueueSelection` + `QueueSelectionCommand` enum; `TuiUiState.queue_selection_index` (+Default); precedence branch in `key_event_to_tui_command_with_ui`; `queue_control_active`/`selected_queue_item`/`queue_control_key_command`/`apply_queue_selection_command`; `QueueSelection` arm in `execute_tui_command_with_interrupt`; queue constants; `queue_panel_height`/`queue_status_label`/`queue_status_style`/`render_queue_panel`; `render()` layout refactor; `queue_prompt_summary`; 10 tests + `queue_view`/`state_with_queue` helpers.

## Errors / Corrections

- First compile failed: used a nonexistent `single_line` helper → added `queue_prompt_summary`. clippy clean after; full serial suite 490 passed / 0 failed.

## Ready for Next Run

- **task_05 (discoverability + docs)**: add `/queue` and `/q` to the help modal (`render_help_modal`) and any slash-command suggestion list; consider also documenting the queue-focus controls (↑/↓ select · Del cancel · Ctrl-R resume) shown in `QUEUE_HINT`; improve the active-run rejection message in `App::submit_prompt` to point at `/queue` (PRD "Active-run guidance"); update README commands section. The TUI rendering + controls are done; task_05 is mostly help text / suggestions / docs, not new mechanics.
- Not committed by this run yet (committing per-task on this branch per user direction).
