# TechSpec: Tabbed Help Modal

## Executive Summary

The redesign turns the stateless `render_help_modal(frame, &Theme)` (`src/tui/mod.rs:3257`)
into a **tabbed, snapshot-driven overlay**. The render function gains `&AppState` +
`&TuiUiState` (both already in scope at the call sites `:2194`/`:2236`), an `enum HelpTab`
selects one of six **pure per-tab builders**, and a theme-token tab strip replaces the single
scrolling `Vec<Line>`. The Ctrl-L roster's inline 3-line loop (`:2120`) is extracted into a
shared `agent_roster_items(theme, agents, style)` builder reused by the Getting Started tab in
`Compact` style. Tab navigation (Arrows + Tab) is handled entirely inside the existing
help-visible key branch (`:790`); ephemeral `help_active_tab` lives in `TuiUiState`, never in
the event-sourced snapshot. The empty-state hint is render-time text added to
`welcome.rs::facts_lines` beside the existing `/help` cue.

**Primary trade-off:** We accept updating three Commands-asserting tests and adding a net-new
tab-strip render path in the repo's most-contended file (`src/tui/mod.rs`), in exchange for
live, single-sourced, drift-proof help with minimal new state. We deliberately keep tabs as
pure builders (not a stateful widget) to stay aligned with the render-from-snapshot discipline
and minimize edits to the hot render body.

## System Architecture

### Component Overview

| Component | Responsibility | Boundary |
| --------- | -------------- | -------- |
| `HelpTab` enum | Identifies the six tabs; provides `ALL`, `title`, `next`, `prev` | New, in `src/tui/mod.rs` |
| `render_help_modal` (modified) | Draws the tab strip + delegates to the active tab's builder | Consumes `&AppState`, `&TuiUiState`, `&Theme`; renders only |
| Per-tab builders | Pure `… -> Vec<Line>` for Getting Started / Commands / Keys / Skills / Approvals / CLI | New free functions; no side effects |
| `agent_roster_items` (extracted) | Builds per-agent rows from `state.agents` in `Full`/`Compact` style | Shared by the Ctrl-L roster and Getting Started |
| Key routing (modified) | Maps Arrows/Tab → `HelpNextTab`/`HelpPrevTab` while help is open | Inside the existing help-visible branch only |
| `welcome::facts_lines` (modified) | Adds the one-line routing hint beside the `/help` cue | Render-time text; no events |

**Data flow:** `App` → `watch<AppState>` snapshot → `render` →
`render_help_modal(state, ui_state, theme)` → active `HelpTab` builder → `Vec<Line>`. Commands
derive from `slash_commands::catalog()`; Skills from `ui_state.skill_suggestions`; agents from
`state.agents`. No component reads `App` internals.

**External interactions:** none — this is an in-process TUI feature.

## Implementation Design

### Core Interfaces

```rust
// src/tui/mod.rs — tab identity (pure, no state beyond the active index in TuiUiState)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HelpTab { GettingStarted, Commands, Keys, Skills, Approvals, Cli }

impl HelpTab {
    const ALL: [HelpTab; 6] = [
        HelpTab::GettingStarted, HelpTab::Commands, HelpTab::Keys,
        HelpTab::Skills, HelpTab::Approvals, HelpTab::Cli,
    ];
    fn title(self) -> &'static str { /* "Getting Started", "Commands", … */ }
    fn next(self) -> HelpTab { /* wrap forward over ALL */ }
    fn prev(self) -> HelpTab { /* wrap backward over ALL */ }
}
```

```rust
// Modified render signature + per-tab builder shape (each builder is its own function)
fn render_help_modal(
    frame: &mut Frame,
    state: &AppState,
    ui_state: &TuiUiState,
    theme: &Theme,
);

fn getting_started_lines(state: &AppState, theme: &Theme) -> Vec<Line<'static>>;
fn commands_tab_lines(filter: &str, theme: &Theme) -> Vec<Line<'static>>; // filter == "" in MVP
fn skills_tab_lines(ui_state: &TuiUiState, theme: &Theme) -> Vec<Line<'static>>;
// keys_tab_lines / approvals_tab_lines / cli_tab_lines: fn(&Theme) -> Vec<Line<'static>>
```

```rust
// Shared roster builder — extracted from the inline loop at src/tui/mod.rs:2120
enum RosterRowStyle { Full, Compact }

fn agent_roster_items(
    agents: &[AgentView],
    style: RosterRowStyle,
    theme: &Theme,
) -> Vec<ListItem<'static>>; // Full = 3 lines/agent (Ctrl-L); Compact = 1 line (Getting Started)
```

```rust
// New TuiCommand variants (enum at src/tui/mod.rs:91)
enum TuiCommand {
    // … existing …
    HelpNextTab,
    HelpPrevTab,
    // Phase 2:
    HelpFilterCharacter(char),
    HelpFilterBackspace,
}
```

### Data Models

- **`TuiUiState` additions** (`src/tui/mod.rs:191`, init in `Default` `:233`):
  - `help_active_tab: HelpTab` — default `HelpTab::GettingStarted`; reset to default on
    `ToggleHelp` close.
  - *(Phase 2)* `help_filter: String` — default `""`; cleared on tab change and on close.
- **No persistent storage.** Both fields are ephemeral UI state, excluded from the serialized
  `AppState` snapshot. The empty-state hint requires no model.

### API Endpoints

N/A — in-process TUI feature with no network surface.

## Integration Points

No external services. Internal integration seams (all in-process): `slash_commands::catalog()`
(Commands tab source of truth), `state.agents` (Getting Started + roster),
`ui_state.skill_suggestions` (Skills tab), `welcome::facts_lines` (hint), `theme.rs` (all
color), and `key_event_to_tui_command_with_ui` (navigation).

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
| --------- | ----------- | -------------------- | --------------- |
| `render_help_modal` (`:3257`) | Modified | Signature + body change to tabbed render. Medium risk (central) | Rewrite to take `(state, ui_state, theme)`; update call sites `:2194`, `:2236` |
| `TuiUiState` + `Default` (`:191`/`:233`) | Modified | Add `help_active_tab` (+ Phase 2 `help_filter`). Low risk | Add fields + defaults + reset-on-close |
| `TuiCommand` enum + executor (`:91`/`:492`) | Modified | Add `HelpNextTab`/`HelpPrevTab` (+ Phase 2 filter cmds). Low risk | Add variants + handler arms mutating `help_active_tab` |
| Key routing help branch (`:790`) | Modified | Bind Arrows/Tab to tab nav. Low risk (branch returns `None` today) | Insert mappings; keep Esc/Ctrl-C |
| Ctrl-L roster loop (`:2120`) | Modified | Extract to `agent_roster_items`. Low risk (net cleanup) | Replace inline loop with `Full`-style call |
| `welcome::facts_lines` (`welcome.rs:312`) | Modified | Add one muted hint line. Low risk | Append routing line near `/help` cue |
| 3 help tests (`:4588`/`:4624`/`:4658`) | Modified | Default tab is now Getting Started, not Commands. Medium risk | Select Commands tab before asserting; preserve contracts |
| New per-tab builders + `HelpTab` | New | Pure functions. Low risk | Add functions + unit tests |

## Testing Approach

### Unit Tests

- **Per-tab builders:** each `*_tab_lines` returns its expected rows — `commands_tab_lines`
  contains every `catalog()` usage exactly once (preserves the catalog-derived contract);
  `keys_tab_lines`/`cli_tab_lines` contain the literal rows; `skills_tab_lines` reflects
  `ui_state.skill_suggestions`; `getting_started_lines` contains the mental-model line +
  example prompts + a compact agent row.
- **`agent_roster_items`:** `Full` yields 3 lines/agent, `Compact` yields 1; both cover
  availability styling. Add a test that the Ctrl-L roster output is unchanged after extraction
  (regression guard).
- **`HelpTab` navigation:** `next`/`prev` wrap correctly over `ALL`.
- **Key routing:** with `help_visible`, Right/Tab → `HelpNextTab`, Left/Shift-Tab →
  `HelpPrevTab`, Esc → `ToggleHelp`, other keys → `None`. Default
  `help_active_tab == GettingStarted`.
- **Updated breaking tests:** `renders_help_modal_commands`,
  `help_modal_command_rows_are_catalog_derived`, `readme_skill_command_wording_matches_help_language`
  set `help_active_tab = Commands` before asserting; README-wording and catalog-derived
  contracts retained.
- **Preserved contracts (unchanged):** Esc-closes-only-when-visible, dropdown suppression,
  mouse-wheel ignore, `colors_live_only_in_theme_module`.

### Integration Tests

- Render-chain test (existing `render_to_text_with_ui` harness): open help → default Getting
  Started body renders; cycle to each tab and assert the active body; Esc closes from a
  non-default tab. No new harness or fixtures required — reuse the in-memory
  `AppState`/`TuiUiState` builders already used by the help tests.

## Development Sequencing

### Build Order

1. **`HelpTab` enum + `RosterRowStyle`** — no dependencies. Add types + `next/prev/title/ALL`.
2. **Extract `agent_roster_items`** and refactor the Ctrl-L roster (`:2120`) to call it in
   `Full` style — depends on step 1 (`RosterRowStyle`). Verify roster output unchanged.
3. **`TuiUiState.help_active_tab`** field + `Default` + reset-on-close in the `ToggleHelp` arm
   (`:492`) — depends on step 1.
4. **Per-tab builders** (`getting_started_lines` uses `agent_roster_items` Compact;
   `commands/keys/skills/approvals/cli`) — depends on steps 1–2.
5. **Rewrite `render_help_modal`** to take `(state, ui_state, theme)`, draw the theme-token tab
   strip + active body; update call sites `:2194`/`:2236` — depends on steps 1, 3, 4.
6. **`TuiCommand::HelpNextTab`/`HelpPrevTab`** + executor arms + key bindings in the
   help-visible branch (`:790`) — depends on steps 3, 5.
7. **Empty-state hint** in `welcome::facts_lines` — independent; can land in parallel with
   steps 1–6.
8. **Tests:** update the 3 breaking tests + add per-tab, roster-extraction, nav, and
   default-tab tests — depends on steps 5, 6.

**Phase 2 (separate change):**

9. **Commands filter:** `help_filter` field + `HelpFilterCharacter/Backspace` + `.contains()`
   in `commands_tab_lines` — depends on steps 5, 6.
10. **First-approval explainer:** requires a show-once latch (new lightweight persisted flag) —
    depends on a small persistence mechanism to be designed in its own task.

### Technical Dependencies

- **Sequencing:** land after the in-flight TUI branches (`feat/at-mention-file-dropdown`,
  agent-roster, slash-command dropdown) to avoid conflict in `src/tui/mod.rs` (per
  ADR-001/002). No external/infra dependencies.

## Monitoring and Observability

This is a local TUI; "observability" means the event-sourced history can derive the PRD
metrics.

- **Time-to-first-successful-run / discovery:** derivable from existing run-lifecycle events
  (`Completed`) and session start; no new events strictly required.
- **Help-assisted success rate:** needs one minimal addition — record a `HelpOpened` history
  event when the modal opens — so `help-opened → task-completed` is computable (never count
  bare opens). This is local event sourcing consistent with the existing model; nothing leaves
  the machine.
- **Hint guardrail (Phase 2):** when the first-approval explainer lands, log a single "hint
  shown" marker to enforce ≤ 1 impression.
- No structured logging, alerting, or external telemetry.

## Technical Considerations

### Key Decisions

- **Decision:** Pure per-tab builders + `enum HelpTab`, not a stateful tab widget.
  **Rationale:** matches render-from-snapshot; independently testable; minimal churn.
  **Trade-off:** a small amount of dispatch boilerplate. **Rejected:** `ratatui::Tabs`
  (color-literal test risk), a stateful `HelpTabs` object (YAGNI).
- **Decision:** Signature change to pass `(&AppState, &TuiUiState)` rather than precomputing a
  view struct. **Rationale:** data is already in scope; no snapshot growth. **Trade-off:** the
  renderer reads two params. **Rejected:** new `AppState` fields (snapshot bloat).
- **Decision:** Shared `agent_roster_items` with `Full`/`Compact`. **Rationale:** one data
  path, two presentations, no drift. **Trade-off:** one style param. **Rejected:** a second
  parallel renderer.
- **Decision:** Phase-2 filter in a dedicated `help_filter` buffer. **Rationale:** isolates help
  from the live composer. **Rejected:** reusing `state.input` (could submit filter text as a
  prompt).

### Known Risks

- **Merge conflict in `src/tui/mod.rs`** (likely, given WIP) — mitigate by sequencing after
  adjacent branches and keeping new logic in new functions.
- **Tab-nav key leakage** to the base handler — mitigate by handling Arrows/Tab fully within
  the help-visible branch.
- **Test-contract erosion** when updating the 3 tests — mitigate by selecting the Commands tab
  and re-asserting the *same* catalog-derived/README-wording checks, never deleting them.

## Architecture Decision Records

- [ADR-001: V1 Scope for the Tabbed Help Modal](adrs/adr-001.md) — Tabbed overlay with live
  Agents/Skills via a render-signature change; Getting Started front door; static Approvals;
  deferred approval/roots plumbing; in-flow hint.
- [ADR-002: Phased Delivery Approach](adrs/adr-002.md) — Approach A (lean onboarding MVP; filter
  + first-approval explainer in Phase 2).
- [ADR-003: Tabbed Help Modal — Technical Architecture](adrs/adr-003.md) — `enum HelpTab` + pure
  per-tab builders driven by a render-signature change; shared `agent_roster_items`; ephemeral
  tab state; Arrows/Tab navigation; rejected `ratatui::Tabs`, a stateful widget, a catalog
  `category`, and reusing the composer input for the filter.
