# Rich Approval Modal + Per-Session Trust List — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Risk assessment & command normalization | completed | medium | — |
| 02 | Approval floor configuration | completed | low | — |
| 03 | Floor + trust enforcement in the single decision point | completed | high | task_01, task_02 |
| 04 | Session TrustStore & per-action context wiring | completed | medium | task_02, task_03 |
| 05 | Approval resolution, trust grant & audit events | completed | high | task_03, task_04 |
| 06 | Chat projection for trust & floor events | completed | medium | task_05 |
| 07 | Rich approval modal & resolution key routing | completed | high | task_05, task_06 |
| 08 | /trust list & revoke command | completed | medium | task_04, task_06 |
| 09 | First-run onboarding & Approvals help | pending | medium | task_07 |
