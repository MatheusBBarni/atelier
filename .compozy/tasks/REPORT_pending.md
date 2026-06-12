# Pending Tasks Report

Generated: 2026-06-11

These task groups have tasks that are not yet completed.

---

## atelier-tui-redesign — Atelier TUI Visual Identity

0 of 9 tasks completed. All tasks pending.

| # | Title | Status | Dependencies |
|---|-------|--------|--------------|
| 01 | Add `[ui]` config section with `hide_banner` | pending | — |
| 02 | Create theme module: caps detection, tokens, resolution, agent accents | pending | — |
| 03 | Thread theme through TUI and migrate all inline colors | pending | task_02 |
| 04 | Branded welcome screen as synthetic chat item | pending | task_01, task_03 |
| 05 | Git context module with change-gated polling | pending | — |
| 06 | Persistent status footer | pending | task_03, task_05 |
| 07 | Per-agent accent colors and run-summary restyle | pending | task_03 |
| 08 | Surface polish: dropdowns, dialogs, help modal, input composer | pending | task_03 |
| 09 | README assets and 3-terminal release verification | pending | task_04, task_06, task_07, task_08 |

---

## queued-follow-up-inputs — Queued Follow-Up Inputs

1 of 5 tasks completed. 4 tasks pending.

| # | Title | Status | Dependencies |
|---|-------|--------|--------------|
| 01 | Add App Queue State And Command Parsing | completed | — |
| 02 | Add Queue Replay, Pause, Cancel, And Resume Lifecycle | pending | task_01 |
| 03 | Project Queue Lifecycle Events Into Chat | pending | task_01, task_02 |
| 04 | Render Queue State And Controls In TUI | pending | task_01, task_02, task_03 |
| 05 | Align Queue Command Discoverability And Documentation | pending | task_01, task_04 |

---

## slash-command-dropdown — Slash Command Dropdown

0 of 7 tasks completed. All tasks pending.

| # | Title | Status | Dependencies |
|---|-------|--------|--------------|
| 01 | Add Shared Slash Command Catalog | pending | — |
| 02 | Route App Unknown-Command Guidance Through Catalog | pending | task_01 |
| 03 | Route TUI Help Command Rows Through Catalog | pending | task_01 |
| 04 | Add Command Dropdown Model And Activation Rules | pending | task_01 |
| 05 | Render Command Dropdown And Empty State | pending | task_04 |
| 06 | Add Command Dropdown Keyboard Handling And Text Insertion | pending | task_04, task_05 |
| 07 | Preserve Prefix Handoff And Final Regression Coverage | pending | task_06 |
