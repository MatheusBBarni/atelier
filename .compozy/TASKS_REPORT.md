# Compozy Tasks Report

Generated: 2026-06-19

Status of every PRD task packet under `.compozy/`. Completion is judged by the canonical
`status:` field on each `task_NN.md` (cross-checked against the `_tasks.md` index). A feature is
archived to `.compozy/tasks_done/` only when **all** of its tasks are `completed`.

> Note: un-ticked `- [ ]` lines inside archived task files are embedded test/verification
> checklists and `Deferred to task_NN` / `Not applicable` notes, not open work — the authoritative
> completion signal is the per-task `status:` field.

## Summary

| | Features | Task files |
|---|---|---|
| Remaining (`.compozy/tasks/`) | 0 | 0 pending + 0 not-yet-decomposed |
| Archived (`.compozy/tasks_done/`) | 24 | 194 completed |

- **Moved this run:** 11 features (95 tasks).

## Remaining — `.compozy/tasks/`

None — every PRD task packet is fully completed and archived. `.compozy/tasks/` holds no task
folders (only `REPORT_pending.md`).

## Moved to `.compozy/tasks_done/` this run

### approval-trust-list — Rich Approval Modal + Per-Session Trust List
All 9 tasks completed.

| # | Title |
|---|-------|
| 01 | Risk assessment & command normalization |
| 02 | Approval floor configuration |
| 03 | Floor + trust enforcement in the single decision point |
| 04 | Session TrustStore & per-action context wiring |
| 05 | Approval resolution, trust grant & audit events |
| 06 | Chat projection for trust & floor events |
| 07 | Rich approval modal & resolution key routing |
| 08 | /trust list & revoke command |
| 09 | First-run onboarding & Approvals help |

### config-driven-keybindings — Config-Driven Keybindings
All 9 tasks completed.

| # | Title |
|---|-------|
| 01 | Keybindings foundation module |
| 02 | Composer line-editing commands and handlers |
| 03 | Reserved-key single chokepoint |
| 04 | Default keymap wiring into key routing |
| 05 | Data-driven Keys help tab |
| 06 | Config keybindings section and ConfigLayer trust boundary |
| 07 | Keybinding validation and EffectiveConfig wiring |
| 08 | Resolve keybinding customizations end-to-end |
| 09 | Keybinding doctor check and config surfaces |

### config-setup-skill — Config-Setup Skill (npm-installable `atelier.toml` configurator)
All 9 tasks completed.

| # | Title |
|---|-------|
| 01 | Author canonical `SKILL.md` wizard (scaffold + protocol) |
| 02 | `references/config-schema.md` — whole-config schema reference |
| 03 | `references/presets.md` — named starter presets |
| 04 | Add `all()` enum iteration for the drift test |
| 05 | `sync-skills` harness + generated discovery mirrors |
| 06 | Rust drift/correctness guard (`tests/atelier_config_skill.rs`) |
| 07 | First-run nudge when the config-setup skill is absent |
| 08 | README install + usage documentation |
| 09 | CI wiring — mirror-equality check + skill tests |

### config-validation-ux — Config Validation UX with Scriptable Doctor Exit Codes
All 6 tasks completed.

| # | Title |
|---|-------|
| 01 | Relocate `edit_distance` to a shared `util` module and add `suggest_nearby_name` |
| 02 | Append near-miss "did you mean?" hints at config-load error sites |
| 03 | Add `EffectiveConfig::required_runtime_ids()` with guardrail test |
| 04 | Elevate unavailable orchestrator runtime to Error in `run_doctor` |
| 05 | Add `--doctor --strict` flag, exit gate, and discovery nudge |
| 06 | Document `--strict` and exit-code contract; dogfood in release CI |

### governance-spine — Governance Spine (Shared Decision Contract + Single-Agent Early-Abort)
All 8 tasks completed.

| # | Title |
|---|-------|
| 01 | Governance module with shared decision and plan types |
| 02 | Add chat governance-decision variants and projection arm |
| 03 | Pending governance decision state and resolver |
| 04 | TUI governance decision card and key routing |
| 05 | Early-abort gate in the drive loop with feature flag |
| 06 | Orchestrator prompt nudge for turn-one intent |
| 07 | Governance outcome proxy and calibration metrics in doctor |
| 08 | Sibling conformance contract and test |

### lifecycle-hooks — Lifecycle Hooks (V1: Observer Tier)
All 9 tasks completed.

| # | Title |
|---|-------|
| 01 | Hooks core types & normalize() + public-event vocabulary |
| 02 | Hooks config through the ladder + drop local-layer hooks |
| 03 | Notifier backends (OSC-native + fallback command) |
| 04 | Off-thread hook dispatcher (channel + subprocess + hook events) |
| 05 | Event tap + App wiring + dispatcher spawn |
| 06 | Hook transcript projection |
| 07 | `atelier --events follow` CLI |
| 08 | Doctor hooks check |
| 09 | Docs & recipes |

### mcp-integration — Harness-Owned MCP Tool Access
All 11 tasks completed.

| # | Title |
|---|-------|
| 01 | Add rmcp dependency, McpClient trait, and fake stdio server |
| 02 | Add mcp.servers config section, mcp_enabled flag, and redaction |
| 03 | Build the McpSupervisor actor and McpHandle |
| 04 | Add the McpTrustStore with durable trust tiers and pins |
| 05 | Add MCP action kinds, capability, validation, and execution |
| 06 | Apply record-time redaction at event write |
| 07 | Advertise an MCP tool-catalog snapshot to the orchestrator |
| 08 | Project MCP tool calls and events into the chat transcript |
| 09 | Extend the approval card with MCP description and trust controls |
| 10 | Add doctor MCP checks, parity matrix, and local metric |
| 11 | Add the emission repair loop, degrade flag, and spike harness |

### pluggable-api-provider — Pluggable API Provider Integration (BYOK)
All 6 tasks completed.

| # | Title |
|---|-------|
| 01 | Extract shared HTTP utilities into http_util.rs |
| 02 | Rename RuntimeKind::Zai to HttpApi and zai.rs to http_api.rs |
| 03 | Add auth header config fields to RuntimeConfig |
| 04 | Generalize HttpApiRuntime auth header construction |
| 05 | Config template, doctor migration hint, and docs |
| 06 | Update all test files and run full test suite |

### self-grading-retry-loop — Externally-Grounded Auto-Verification Loop
All 7 tasks completed.

| # | Title |
|---|-------|
| 01 | Add `[grading]` config section |
| 02 | Extract canonical-verification-command predicate |
| 03 | GraderVerdict type and exit-code-derived verdict deriver |
| 04 | Grade-round events and collapsing chat projection |
| 05 | Grading executor and FakeRuntime grading phrases |
| 06 | Grading trigger gate at run_agent_step |
| 07 | Cycle-exhaustion escalation (accept/retry/abort) |

### session-browser-resume — Session Browser & Transcript Resume
All 13 tasks completed.

| # | Title |
|---|-------|
| 01 | Add `RunState::is_terminal()` helper |
| 02 | `HistoryStore::open()` + self-healing metadata cache fields |
| 03 | Session summaries (`SessionSummary` + `list_session_summaries`) |
| 04 | New lifecycle event kinds + projection fold handlers |
| 05 | `GitContext` HEAD+dirty, HEAD baseline, `detect_drift` |
| 06 | Read-only preview fold + transcript sanitization |
| 07 | Session browser modal + off-thread session list |
| 08 | Transcript preview pane (off-thread fold load) |
| 09 | Discoverability: `/sessions` command + welcome cue |
| 10 | `LoadedSession` + `App::adopt_session()` + exhaustiveness test |
| 11 | Resume flow (re-adopt → write lifecycle events → re-render → Idle) |
| 12 | Resume safety: cautious-default approval + first-mutation drift interlock |
| 13 | Resume-rate instrumentation + dynamic post-crash hint |

### subtask-dag-execution — Sub-task DAG Execution
All 8 tasks completed.

| # | Title |
|---|-------|
| 01 | Config execution_graph feature flag |
| 02 | DAG decision schema, types, validation, and prompt guidance |
| 03 | DAG events, graph_id, and ExecutionGraphResult |
| 04 | Ready-set scheduler with fail-closed admission |
| 05 | Whole-plan approval gate (normal mode) |
| 06 | Single evolving Plan chat projection |
| 07 | Surface DAG state in /config |
| 08 | Fake-runtime DAG harness and integration suite |

## Previously archived (already in `.compozy/tasks_done/`)

| Feature | Title | Tasks |
|---|---|---|
| agent-question-tool | Clarification Select UI | 8/8 |
| agent-roster-tui | Live-Activity-First Agent Roster | 7/7 |
| at-mention-file-dropdown | @-Mention File Dropdown | 10/10 |
| atelier-tui-redesign | Atelier TUI Visual Identity | 9/9 |
| default-agent-prompts | Default Agent Prompts | 5/5 |
| help-modal-tabs | Tabbed Help Modal | 10/10 |
| prompt-history | Prompt History — Per-Project ↑/↓ Recall | 7/7 |
| provider-usage-status | Provider Usage Status | 6/6 |
| queued-follow-up-inputs | Queued Follow-Up Inputs | 5/5 |
| skill-prompt-loading | Skill Prompt Loading | 6/6 |
| slash-command-dropdown | Slash Command Dropdown | 7/7 |
| web-docs-page | Atelier Documentation Site (`/docs`) | 12/12 |
| workflow-command | Workflow Command | 7/7 |
