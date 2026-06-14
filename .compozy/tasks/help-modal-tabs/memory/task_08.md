# Task Memory: task_08.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Empty-state onboarding hint in `welcome::facts_lines`. Render-time only; no state/events.
DONE.

## Important Decisions
- Final copy (PRD Open Question): "describe a task — it routes through an orchestrator to
  named agents" as a new muted line, kept ABOVE the retained "type /help for commands" cue.
  Two lines, both `theme.text_muted`. Did not fold `/help` into the routing line because the
  task MUST keep the existing cue verbatim.

## Learnings
- `facts_lines` is pure over inputs (unit-testable, no render). Integration coverage via
  `render_to_text(&state, w, h)` with `state.chat_items = vec![ChatItemView::welcome()]`
  (welcome tests at `src/tui/mod.rs` ~`:8534`).

## Files / Surfaces
- `src/tui/welcome.rs`: hint appended in `facts_lines` (~`:345`); unit test
  `facts_box_includes_routing_hint_and_help_cue`.
- `src/tui/mod.rs`: integration test `welcome_shows_routing_onboarding_hint_on_empty_chat`
  in the task_04 welcome section.

## Errors / Corrections

## Ready for Next Run
- Independent task; nothing to hand off.
