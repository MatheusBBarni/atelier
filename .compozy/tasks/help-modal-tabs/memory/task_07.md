# Task Memory: task_07.md

Keep only task-local execution context here. Do not duplicate facts that are obvious from the repository, task file, PRD documents, or git history.

## Objective Snapshot
Made the tabbed help modal interactive: added `TuiCommand::HelpNextTab`/`HelpPrevTab`,
executor arms advancing `ui_state.help_active_tab` via `HelpTab::next/prev`, and key bindings
inside the help-visible branch of `key_event_to_tui_command_with_ui`. Esc still closes from any
tab; no key leaks to the base handler. DONE — all gates green.

## Important Decisions
- Bound BOTH `KeyCode::Tab`/SHIFT and `KeyCode::BackTab` to `HelpPrevTab`. The spec only
  named Tab+SHIFT, but crossterm emits `BackTab` for Shift-Tab in most terminals — adding it is
  strictly additive and correct, tested explicitly.
- Removed `#[allow(dead_code)]` on `impl HelpTab` (next/prev are now consumed by the executor).

## Learnings
- Stable Getting Started assertion string: "How Atelier works" (header in `getting_started_lines`).
- `fmt` reformats `KeyCode::Left, ..` single-line patterns onto two lines — fixed by hand to
  avoid a blanket `cargo fmt` (user runs parallel WIP).

## Files / Surfaces
- `src/tui/mod.rs`: enum `:166` (+2 variants after `ToggleHelp`); executor arms after the
  `ToggleHelp` arm; help-visible key branch `:879`; tests added after
  `esc_closes_help_modal_only_when_visible` (routing + executor) and at end of test module
  (e2e cycle).

## Errors / Corrections
- None.

## Ready for Next Run
- task_09 (Phase 2 filter) extends the same help-visible branch for character input — it must
  insert filter-character mapping BEFORE the `_ => None` arm and after the nav arms.
