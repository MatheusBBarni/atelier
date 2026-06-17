# Sub-task DAG Execution — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Config execution_graph feature flag | completed | low | — |
| 02 | DAG decision schema, types, validation, and prompt guidance | completed | high | task_01 |
| 03 | DAG events, graph_id, and ExecutionGraphResult | completed | medium | task_02 |
| 04 | Ready-set scheduler with fail-closed admission | completed | critical | task_02, task_03 |
| 05 | Whole-plan approval gate (normal mode) | completed | high | task_03, task_04 |
| 06 | Single evolving Plan chat projection | completed | high | task_03 |
| 07 | Surface DAG state in /config | completed | low | task_01 |
| 08 | Fake-runtime DAG harness and integration suite | pending | high | task_04, task_05, task_06 |
