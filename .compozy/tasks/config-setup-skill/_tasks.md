# Config-Setup Skill — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Author canonical `SKILL.md` wizard (scaffold + protocol) | completed | medium | — |
| 02 | `references/config-schema.md` — whole-config schema reference | completed | medium | — |
| 03 | `references/presets.md` — named starter presets | completed | medium | — |
| 04 | Add `all()` enum iteration for the drift test | completed | low | — |
| 05 | `sync-skills` harness + generated discovery mirrors | pending | medium | task_01, task_02, task_03 |
| 06 | Rust drift/correctness guard (`tests/atelier_config_skill.rs`) | pending | high | task_01, task_02, task_03, task_04, task_05 |
| 07 | First-run nudge when the config-setup skill is absent | pending | low | task_01 |
| 08 | README install + usage documentation | pending | low | task_01 |
| 09 | CI wiring — mirror-equality check + skill tests | pending | low | task_05, task_06 |

## Build Waves

- **Wave 0 (no deps):** task_01 (SKILL.md), task_02 (schema ref), task_03 (presets), task_04 (enum `all()`)
- **Wave 1:** task_05 (sync harness ← 01,02,03)
- **Wave 2:** task_06 (drift guard ← 01,02,03,04,05), task_07 (nudge ← 01), task_08 (README ← 01)
- **Wave 3:** task_09 (CI ← 05,06)

See `_techspec.md` "Development Sequencing" for the originating build order and `_prd.md` for requirements (F1–F7). Schema/enum lists are anchored to the **current** `src/config/mod.rs` (which now includes the MCP config surface from the `mcp-integration` packet), not the techspec's pre-MCP snapshot.
