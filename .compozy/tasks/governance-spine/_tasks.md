# Governance Spine — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Governance module with shared decision and plan types | pending | medium | — |
| 02 | Add chat governance-decision variants and projection arm | pending | medium | task_01 |
| 03 | Pending governance decision state and resolver | pending | medium | task_01 |
| 04 | TUI governance decision card and key routing | pending | medium | task_02, task_03 |
| 05 | Early-abort gate in the drive loop with feature flag | pending | high | task_02, task_03 |
| 06 | Orchestrator prompt nudge for turn-one intent | pending | low | task_05 |
| 07 | Governance outcome proxy and calibration metrics in doctor | pending | medium | task_05 |
| 08 | Sibling conformance contract and test | pending | low | task_01 |

## Build Waves

- **Wave 0:** task_01
- **Wave 1:** task_02, task_03, task_08 (←01)
- **Wave 2:** task_04, task_05 (←02, 03)
- **Wave 3:** task_06, task_07 (←05)

See `_techspec.md` "Development Sequencing" for the originating build order and `_prd.md` for requirements (CF1–CF5).
