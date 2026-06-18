# Lifecycle Hooks — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Hooks core types & normalize() + public-event vocabulary | completed | medium | — |
| 02 | Hooks config through the ladder + drop local-layer hooks | completed | medium | task_01 |
| 03 | Notifier backends (OSC-native + fallback command) | completed | medium | task_01 |
| 04 | Off-thread hook dispatcher (channel + subprocess + hook events) | completed | high | task_01, task_03 |
| 05 | Event tap + App wiring + dispatcher spawn | completed | high | task_01, task_02, task_04 |
| 06 | Hook transcript projection | completed | low | task_01 |
| 07 | `atelier --events follow` CLI | completed | medium | task_01 |
| 08 | Doctor hooks check | completed | low | task_02, task_04 |
| 09 | Docs & recipes | completed | low | task_02, task_03, task_07, task_08 |
