# Config-Driven Keybindings — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Keybindings foundation module | completed | medium | — |
| 02 | Composer line-editing commands and handlers | completed | medium | — |
| 03 | Reserved-key single chokepoint | completed | medium | — |
| 04 | Default keymap wiring into key routing | pending | high | task_01, task_02, task_03 |
| 05 | Data-driven Keys help tab | pending | low | task_04 |
| 06 | Config keybindings section and ConfigLayer trust boundary | pending | medium | task_01 |
| 07 | Keybinding validation and EffectiveConfig wiring | pending | medium | task_06 |
| 08 | Resolve keybinding customizations end-to-end | pending | medium | task_04, task_05, task_07 |
| 09 | Keybinding doctor check and config surfaces | pending | medium | task_07 |

## Waves

- **Wave 1 (parity, no config):** task_01, task_02, task_03 → task_04 → task_05
- **Wave 2 (remap layer):** task_06 → task_07 → { task_08 (also needs task_04, task_05), task_09 }
