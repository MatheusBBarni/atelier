# Atelier TUI Visual Identity — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Add `[ui]` config section with `hide_banner` | completed | low | — |
| 02 | Create theme module: caps detection, tokens, resolution, agent accents | completed | medium | — |
| 03 | Thread theme through TUI and migrate all inline colors | completed | high | task_02 |
| 04 | Branded welcome screen as synthetic chat item | completed | high | task_01, task_03 |
| 05 | Git context module with change-gated polling | completed | medium | — |
| 06 | Persistent status footer | completed | medium | task_03, task_05 |
| 07 | Per-agent accent colors and run-summary restyle | completed | medium | task_03 |
| 08 | Surface polish: dropdowns, dialogs, help modal, input composer | completed | low | task_03 |
| 09 | README assets and 3-terminal release verification | completed | low | task_04, task_06, task_07, task_08 |

> Task 09 status: docs done (README hero structure, CONTEXT.md surfaces, website decision, link check); **blocked** on human asset capture (welcome screenshot + parallel GIF) and the 3-terminal/NO_COLOR visual verification. Handoff: `release-verification.md`.
