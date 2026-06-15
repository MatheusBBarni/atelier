# Compozy Tasks Report

Generated: 2026-06-15

Status of every PRD task packet under `.compozy/`, split into **remaining** (still
in `.compozy/tasks/`) and **archived** (moved to `.compozy/tasks_done/` once every
task reached `completed`).

Completion is judged by the canonical `status:` field on each `task_NN.md` (cross-checked
against the `_tasks.md` index). A feature is archived only when **all** of its tasks are
`completed`.

> Note on checklists: several archived task files still contain un-ticked `- [ ]` lines.
> These are embedded **test/verification checklists** and `Deferred to task_NN` /
> `Not applicable` notes, not open work — the referenced follow-up tasks are themselves
> `completed`. The authoritative signal is the per-task `status:` field, which is
> `completed` across every archived feature.

---

## Summary

| | Features | Task files |
|---|---|---|
| Remaining (`.compozy/tasks/`) | 4 | 17 pending + 2 not-yet-decomposed |
| Archived (`.compozy/tasks_done/`) | 13 | 99 completed |

- **Moved this session:** 6 features (49 completed tasks) — see [Moved this session](#moved-to-compozytasks_done-this-session).
- **Previously archived:** 7 features (50 completed tasks).

---

## Remaining — `.compozy/tasks/`

Four features are still open. Two have a full task breakdown (all `pending`); two are at the
idea/PRD stage with no `task_NN.md` files generated yet.

### approval-trust-list — Rich Approval Modal + Per-Session Trust List
**0 of 9 tasks completed — all pending.**

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Risk assessment & command normalization | pending | medium | — |
| 02 | Approval floor configuration | pending | low | — |
| 03 | Floor + trust enforcement in the single decision point | pending | high | task_01, task_02 |
| 04 | Session TrustStore & per-action context wiring | pending | medium | task_02, task_03 |
| 05 | Approval resolution, trust grant & audit events | pending | high | task_03, task_04 |
| 06 | Chat projection for trust & floor events | pending | medium | task_05 |
| 07 | Rich approval modal & resolution key routing | pending | high | task_05, task_06 |
| 08 | /trust list & revoke command | pending | medium | task_04, task_06 |
| 09 | First-run onboarding & Approvals help | pending | medium | task_07 |

### subtask-dag-execution — Sub-task DAG Execution
**0 of 8 tasks completed — all pending.**

| # | Title | Status | Complexity | Dependencies |
|---|-------|--------|------------|--------------|
| 01 | Config execution_graph feature flag | pending | low | — |
| 02 | DAG decision schema, types, validation, and prompt guidance | pending | high | task_01 |
| 03 | DAG events, graph_id, and ExecutionGraphResult | pending | medium | task_02 |
| 04 | Ready-set scheduler with fail-closed admission | pending | critical | task_02, task_03 |
| 05 | Whole-plan approval gate (normal mode) | pending | high | task_03, task_04 |
| 06 | Single evolving Plan chat projection | pending | high | task_03 |
| 07 | Surface DAG state in /config | pending | low | task_01 |
| 08 | Fake-runtime DAG harness and integration suite | pending | high | task_04, task_05, task_06 |

### config-driven-keybindings — Config-Driven Keybindings
**Not yet decomposed.** Artifacts present: `_idea.md`, `_prd.md`, `adrs/`. No `_techspec.md`
or `task_NN.md` files yet — needs `cy-create-techspec` / `cy-create-tasks` before execution.

### mcp-integration — Harness-Owned MCP Tool Access
**Not yet decomposed.** Artifacts present: `_idea.md`, `_prd.md`, `adrs/`. No `_techspec.md`
or `task_NN.md` files yet — needs `cy-create-techspec` / `cy-create-tasks` before execution.

---

## Moved to `.compozy/tasks_done/` this session

Six features were verified all-`completed` and moved (via `git mv`) from `.compozy/tasks/`
to `.compozy/tasks_done/`.

### agent-roster-tui — Live-Activity-First Agent Roster
**All 7 tasks completed.**

| # | Title |
|---|-------|
| 01 | View-model types: ActivityState + RosterRow + AppState field |
| 02 | StepTiming map + lifecycle stamping |
| 03 | build_roster_rows builder (join, classify, elapsed, accent_index, NeedsInput pin) |
| 04 | Wire rebuild into publish_state + 1Hz gated refresh tick |
| 05 | activity_glyph / activity_label helpers (Set 1 glyphs + ASCII/NO_COLOR) |
| 06 | Roster render rewrite: weight, glyph+label, elapsed, current-step, animated indicator, summary header |
| 07 | Accent-by-identity consistency (roster/chat/dropdown) + strengthened contract tests |

### default-agent-prompts — Default Agent Prompts
**All 5 tasks completed.**

| # | Title |
|---|-------|
| 01 | Model prompt source and generation surfaces |
| 02 | Implement role-contract default prompts |
| 03 | Align generated starter instruction files |
| 04 | Add prompt drift and role-boundary tests |
| 05 | Generate and validate Compozy task artifacts |

### help-modal-tabs — Tabbed Help Modal
**All 10 tasks completed.**

| # | Title |
|---|-------|
| 01 | HelpTab enum and roster row style types |
| 02 | Extract shared agent_roster_items builder |
| 03 | Add help_active_tab state to TuiUiState |
| 04 | Static reference tab builders (Keys, CLI, Approvals) |
| 05 | Live and Getting Started tab builders |
| 06 | Tabbed render_help_modal with active-tab dispatch |
| 07 | Help tab navigation keys and commands |
| 08 | Empty-state onboarding hint in welcome facts |
| 09 | Commands tab substring filter (Phase 2) |
| 10 | First-approval explainer with show-once latch (Phase 2) |

### prompt-history — Prompt History (Per-Project ↑/↓ Recall)
**All 7 tasks completed.**

| # | Title |
|---|-------|
| 01 | History reader and prompt projection |
| 02 | UI config: prompt-history enable flag and size cap |
| 03 | PromptSource enum and AppEvent::PromptSubmitted extension |
| 04 | TuiUiState recall fields and async history loader |
| 05 | Up/Down recall interaction with collision gate and draft preservation |
| 06 | Submission provenance tagging at submit |
| 07 | Recall discoverability hint line and help entry |

### provider-usage-status — Provider Usage Status
**All 6 tasks completed.**

| # | Title |
|---|-------|
| 01 | Add provider status command metadata |
| 02 | Define runtime provider status model |
| 03 | Implement provider status service and adapter boundary |
| 04 | Render compact share-safe provider status output |
| 05 | Route `/provider:status` through submitted app commands |
| 06 | Add focused provider status verification coverage |

### web-docs-page — Atelier Documentation Site (`/docs`)
**All 12 tasks completed.**

| # | Title |
|---|-------|
| 01 | Fix README runtime-requirements accuracy |
| 02 | Extract a shared Base.astro layout |
| 03 | Scaffold the docs content collection, layout, nav, and styles |
| 04 | Write the Quickstart page |
| 05 | Write the Concepts page |
| 06 | Write the Governance and Safety page |
| 07 | Emit machine-readable surfaces (llms.txt, twins, sitemap) |
| 08 | Add the web-checks PR workflow with link checking |
| 09 | Refactor the redacted-config builder for reuse |
| 10 | Build the emit-docs reference generator |
| 11 | Wire generated reference into the docs site |
| 12 | Custom Pages deploy and generator step in CI |

---

## Previously archived (already in `.compozy/tasks_done/`)

Seven features were archived before this session. All tasks `completed`.

| Feature | Title | Tasks |
|---------|-------|-------|
| agent-question-tool | Clarification Select UI | 8/8 |
| at-mention-file-dropdown | @-Mention File Dropdown | 10/10 |
| atelier-tui-redesign | Atelier TUI Visual Identity | 9/9 |
| queued-follow-up-inputs | Queued Follow-Up Inputs | 5/5 |
| skill-prompt-loading | Skill Prompt Loading | 6/6 |
| slash-command-dropdown | Slash Command Dropdown | 7/7 |
| workflow-command | Workflow Command | 7/7 |
