# Externally-Grounded Auto-Verification Loop — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Add `[grading]` config section | completed | low | — |
| 02 | Extract canonical-verification-command predicate | completed | low | — |
| 03 | GraderVerdict type and exit-code-derived verdict deriver | pending | low | task_02 |
| 04 | Grade-round events and collapsing chat projection | pending | medium | task_03 |
| 05 | Grading executor and FakeRuntime grading phrases | pending | high | task_01, task_03, task_04 |
| 06 | Grading trigger gate at run_agent_step | pending | medium | task_05 |
| 07 | Cycle-exhaustion escalation (accept/retry/abort) | pending | medium | task_05, task_06 |
