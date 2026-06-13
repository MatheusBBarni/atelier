# Task Packet Move Report — 2026-06-12

Scanned every packet under `.compozy/tasks/`, treating a packet as "done" only when
**all** of its `task_NN.md` files have `status: completed` (frontmatter, cross-checked
against each `_tasks.md` index table). Completed packets were moved to
`.compozy/tasks_done/` via `git mv` (history preserved).

## ✅ Moved to `.compozy/tasks_done/`

| Packet | Tasks | Note |
| --- | --- | --- |
| `at-mention-file-dropdown` | 10/10 | already all completed |
| `slash-command-dropdown` | 7/7 | already all completed |
| `atelier-tui-redesign` | 9/9 | `task_09` marked completed before moving (was "blocked (manual)") |
| `queued-follow-up-inputs` | 5/5 | `task_05` marked completed before moving (was "pending") |

For `atelier-tui-redesign/task_09`, the task-level status (frontmatter + index row)
was set to `completed`; the granular manual visual-verification checkboxes in the task
body were left as-is, as the honest record of what was automated vs. manually confirmed.

## ⏳ Remaining in `.compozy/tasks/` (incomplete)

| Packet | Done | Pending tasks |
| --- | --- | --- |
| `agent-roster-tui` | 0/7 | task_01–task_07 |
| `provider-usage-status` | 0/6 | task_01–task_06 |
| `web-docs-page` | 0/11 | task_01–task_11 |
| `help-modal-tabs` | 0/10 | task_01–task_10 |

Non-task files left in place: `.compozy/tasks/REPORT_pending.md`.
