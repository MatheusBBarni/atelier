# Session Transcript Export — Task List

Derived from `_techspec.md` (Build Order). All tasks are backend, in-crate, with no new third-party dependency. Tasks 01 and 02 have no dependencies and can start in parallel; 05 and 06 gate on the orchestrator (04).

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | `session_exported` event kind + `pub(crate)` private-file writer | pending | low | — |
| 02 | Single-source tiered secret scanner (`scan_secrets`) | pending | medium | — |
| 03 | Lean Markdown transcript serializer | pending | medium | task_02 |
| 04 | Export orchestrator and risk-adaptive review gate | pending | medium | task_01, task_02, task_03 |
| 05 | Egress-safety warning via `git check-ignore` | pending | low | task_04 |
| 06 | CLI `--export-session` wiring and end-to-end suite | pending | medium | task_04, task_05 |
