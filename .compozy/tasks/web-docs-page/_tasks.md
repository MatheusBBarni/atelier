# Atelier Documentation Site (/docs) — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Fix README runtime-requirements accuracy | completed | low | — |
| 02 | Extract a shared Base.astro layout | completed | medium | — |
| 03 | Scaffold the docs content collection, layout, nav, and styles | completed | medium | task_02 |
| 04 | Write the Quickstart page | completed | medium | task_03 |
| 05 | Write the Concepts page | completed | low | task_03 |
| 06 | Write the Governance and Safety page | pending | medium | task_03 |
| 07 | Emit machine-readable surfaces (llms.txt, twins, sitemap) | completed | medium | task_03, task_04, task_05, task_06 |
| 08 | Add the web-checks PR workflow with link checking | completed | medium | task_07 |
| 09 | Refactor the redacted-config builder for reuse | completed | medium | — |
| 10 | Build the emit-docs reference generator | completed | high | task_03, task_09 |
| 11 | Wire generated reference into the docs site | completed | medium | task_07, task_10 |
| 12 | Custom Pages deploy and generator step in CI | completed | high | task_08, task_10, task_11 |
