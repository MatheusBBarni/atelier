# Live-Activity-First Agent Roster — Task List

## Tasks

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | View-model types: ActivityState + RosterRow + AppState field | completed | low | — |
| 02 | StepTiming map + lifecycle stamping | completed | medium | — |
| 03 | build_roster_rows builder (join, classify, elapsed, accent_index, NeedsInput pin) | completed | medium | task_01, task_02 |
| 04 | Wire rebuild into publish_state + 1Hz gated refresh tick | completed | medium | task_03 |
| 05 | activity_glyph / activity_label helpers (Set 1 glyphs + ASCII/NO_COLOR) | completed | low | task_01 |
| 06 | Roster render rewrite: weight, glyph+label, elapsed, current-step, animated indicator, summary header | completed | high | task_04, task_05 |
| 07 | Accent-by-identity consistency (roster/chat/dropdown) + strengthened contract tests | completed | medium | task_06 |
