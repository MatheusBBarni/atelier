# Config Validation UX with Scriptable Doctor Exit Codes — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Relocate `edit_distance` to a shared `util` module and add `suggest_nearby_name` | completed | medium | — |
| 02 | Append near-miss "did you mean?" hints at config-load error sites | completed | low | task_01 |
| 03 | Add `EffectiveConfig::required_runtime_ids()` with guardrail test | completed | low | — |
| 04 | Elevate unavailable orchestrator runtime to Error in `run_doctor` | completed | medium | task_03 |
| 05 | Add `--doctor --strict` flag, exit gate, and discovery nudge | completed | medium | task_04 |
| 06 | Document `--strict` and exit-code contract; dogfood in release CI | pending | low | task_05 |
