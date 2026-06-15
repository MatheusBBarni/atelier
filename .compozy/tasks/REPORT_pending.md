# Pending Tasks Report

Generated: 2026-06-15

Task groups still in `.compozy/tasks/` that are not yet fully completed. See
`../TASKS_REPORT.md` for the combined remaining + archived report.

---

## approval-trust-list — Rich Approval Modal + Per-Session Trust List

0 of 9 tasks completed. All tasks pending.

| # | Title | Status | Dependencies |
|---|-------|--------|--------------|
| 01 | Risk assessment & command normalization | pending | — |
| 02 | Approval floor configuration | pending | — |
| 03 | Floor + trust enforcement in the single decision point | pending | task_01, task_02 |
| 04 | Session TrustStore & per-action context wiring | pending | task_02, task_03 |
| 05 | Approval resolution, trust grant & audit events | pending | task_03, task_04 |
| 06 | Chat projection for trust & floor events | pending | task_05 |
| 07 | Rich approval modal & resolution key routing | pending | task_05, task_06 |
| 08 | /trust list & revoke command | pending | task_04, task_06 |
| 09 | First-run onboarding & Approvals help | pending | task_07 |

---

## subtask-dag-execution — Sub-task DAG Execution

0 of 8 tasks completed. All tasks pending.

| # | Title | Status | Dependencies |
|---|-------|--------|--------------|
| 01 | Config execution_graph feature flag | pending | — |
| 02 | DAG decision schema, types, validation, and prompt guidance | pending | task_01 |
| 03 | DAG events, graph_id, and ExecutionGraphResult | pending | task_02 |
| 04 | Ready-set scheduler with fail-closed admission | pending | task_02, task_03 |
| 05 | Whole-plan approval gate (normal mode) | pending | task_03, task_04 |
| 06 | Single evolving Plan chat projection | pending | task_03 |
| 07 | Surface DAG state in /config | pending | task_01 |
| 08 | Fake-runtime DAG harness and integration suite | pending | task_04, task_05, task_06 |

---

## config-driven-keybindings — Config-Driven Keybindings

Not yet decomposed into tasks. Present: `_idea.md`, `_prd.md`, `adrs/`.
Needs `cy-create-techspec` then `cy-create-tasks`.

---

## mcp-integration — Harness-Owned MCP Tool Access

Not yet decomposed into tasks. Present: `_idea.md`, `_prd.md`, `adrs/`.
Needs `cy-create-techspec` then `cy-create-tasks`.
