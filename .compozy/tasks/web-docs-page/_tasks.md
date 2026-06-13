# Atelier Documentation Site (/docs) — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Fix README runtime-requirements accuracy | completed | low | — |
| 02 | Extract a shared Base.astro layout | completed | medium | — |
| 03 | Scaffold the docs content collection, layout, nav, and styles | completed | medium | task_02 |
| 04 | Write the Quickstart page | completed | medium | task_03 |
| 05 | Write the Concepts page | pending | low | task_03 |
| 06 | Write the Governance and Safety page | pending | medium | task_03 |
| 07 | Emit machine-readable surfaces (llms.txt, twins, sitemap) | pending | medium | task_03, task_04, task_05, task_06 |
| 08 | Add the web-checks PR workflow with link checking | pending | medium | task_07 |
| 09 | Refactor the redacted-config builder for reuse | pending | medium | — |
| 10 | Build the emit-docs reference generator | pending | high | task_03, task_09 |
| 11 | Wire generated reference into the docs site | pending | medium | task_07, task_10 |
| 12 | Custom Pages deploy and generator step in CI | pending | high | task_08, task_10, task_11 |
