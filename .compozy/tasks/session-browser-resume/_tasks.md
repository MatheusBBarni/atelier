# Session Browser & Transcript Resume — Task List

Derived from `_techspec.md` (Build Order). Phase 1 (read-only browse + preview MVP) = tasks 01–04, 06–09. Phase 2 (resume) = tasks 05, 10–13. Numbering follows dependency order, not phase.

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Add `RunState::is_terminal()` helper | completed | low | — |
| 02 | `HistoryStore::open()` + self-healing metadata cache fields | completed | medium | — |
| 03 | Session summaries (`SessionSummary` + `list_session_summaries`) | completed | medium | task_01, task_02 |
| 04 | New lifecycle event kinds + projection fold handlers | completed | medium | — |
| 05 | `GitContext` HEAD+dirty, HEAD baseline, `detect_drift` | completed | medium | — |
| 06 | Read-only preview fold + transcript sanitization | completed | medium | task_02, task_04 |
| 07 | Session browser modal + off-thread session list | completed | high | task_03 |
| 08 | Transcript preview pane (off-thread fold load) | completed | high | task_06, task_07 |
| 09 | Discoverability: `/sessions` command + welcome cue | completed | low | task_07 |
| 10 | `LoadedSession` + `App::adopt_session()` + exhaustiveness test | pending | high | task_01, task_02, task_04 |
| 11 | Resume flow (re-adopt → write lifecycle events → re-render → Idle) | pending | high | task_04, task_05, task_08, task_10 |
| 12 | Resume safety: cautious-default approval + first-mutation drift interlock | pending | high | task_05, task_11 |
| 13 | Resume-rate instrumentation + dynamic post-crash hint | pending | medium | task_03, task_09, task_11 |
