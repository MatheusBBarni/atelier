# Pluggable API Provider Integration — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Extract shared HTTP utilities into http_util.rs | pending | high | — |
| 02 | Rename RuntimeKind::Zai to HttpApi and zai.rs to http_api.rs | pending | critical | task_01 |
| 03 | Add auth header config fields to RuntimeConfig | pending | medium | task_02 |
| 04 | Generalize HttpApiRuntime auth header construction | pending | medium | task_03 |
| 05 | Config template, doctor migration hint, and docs | pending | low | task_02 |
| 06 | Update all test files and run full test suite | pending | high | task_02, task_03, task_04 |
