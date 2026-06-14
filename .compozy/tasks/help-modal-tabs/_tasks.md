# Tabbed Help Modal — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | HelpTab enum and roster row style types | completed | low | — |
| 02 | Extract shared agent_roster_items builder | completed | low | task_01 |
| 03 | Add help_active_tab state to TuiUiState | completed | low | task_01 |
| 04 | Static reference tab builders (Keys, CLI, Approvals) | completed | low | task_01 |
| 05 | Live and Getting Started tab builders | completed | medium | task_01, task_02 |
| 06 | Tabbed render_help_modal with active-tab dispatch | completed | medium | task_03, task_04, task_05 |
| 07 | Help tab navigation keys and commands | completed | medium | task_06 |
| 08 | Empty-state onboarding hint in welcome facts | completed | low | — |
| 09 | Commands tab substring filter (Phase 2) | completed | medium | task_06, task_07 |
| 10 | First-approval explainer with show-once latch (Phase 2) | completed | high | — |

## Phasing

- **MVP (Phase 1):** task_01 – task_08
- **Phase 2 (fast-follow):** task_09, task_10

Work MVP tasks in dependency order. `task_08` is independent and may land in parallel.
See `_techspec.md` "Development Sequencing" for the build order and `_prd.md` for scope.
