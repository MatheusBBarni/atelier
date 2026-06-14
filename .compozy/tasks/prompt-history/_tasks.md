# Prompt History — Per-Project ↑/↓ Recall — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | History reader and prompt projection | completed | medium | — |
| 02 | UI config: prompt-history enable flag and size cap | completed | low | — |
| 03 | PromptSource enum and AppEvent::PromptSubmitted extension | completed | medium | — |
| 04 | TuiUiState recall fields and async history loader | completed | medium | task_01, task_02 |
| 05 | Up/Down recall interaction with collision gate and draft preservation | completed | high | task_04 |
| 06 | Submission provenance tagging at submit | completed | medium | task_03, task_05 |
| 07 | Recall discoverability hint line and help entry | completed | low | task_04, task_05 |
