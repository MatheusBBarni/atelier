# TechSpec — Live-Activity-First Agent Roster

## Executive Summary

The roster becomes a **progress-confident live board** by introducing one app-layer view-model — `RosterRow` — that joins `AgentView` (identity, canonical order) with `LiveStepView` (liveness) and is rebuilt centrally in `publish_state`. Time-dependent values (coarse elapsed, the `Stalled` flag) are computed in a **pure builder that takes `now: Instant` as a parameter** and are pre-formatted into the row, so the renderer stays a clock-free pure function of `AppState` and snapshot tests stay deterministic. A first-class `Stalled` state is detected from elapsed-since-last-stream-activity (the single chokepoint `push_live_stream_content`) against a fixed 30 s threshold, kept current by a bounded 1 Hz refresh added as a 4th arm to the existing app-worker `tokio::select!`, gated to active runs and change-gated before publish. Agent accent colors are decoupled from render-time position to a canonical-identity index so the one permitted reorder (the `NeedsInput` pin) cannot recolor an agent or break its link to the chat transcript.

**Primary trade-off:** we accept modest new app-layer state (a `RosterRow` vec on `AppState` and an internal `StepTiming` map) plus one new periodic timer, in exchange for deterministic tests, a pure renderer, and timely stall detection. The leading risk — a 1 Hz `select!` arm missing ticks under heavy streaming — is self-mitigated (stream deltas refresh the roster under load; the timer only needs to fire when an agent is quiet, i.e. the stall case) with a dedicated-task escalation path documented in ADR-004.

## System Architecture

### Component Overview

- **`RosterRow` view-model (`src/app/mod.rs`, new).** The single source of truth the renderer consumes. Carries identity, `accent_index`, `ActivityState`, pre-formatted `current_step` and `elapsed`, and the preserved terminal status label. Built by `build_roster_rows(...)`; stored as `AppState.roster_rows`.
- **`StepTiming` map (`App`, internal, not serialized).** `BTreeMap<step_id, StepTiming{ started_at, last_activity }>`. Stamped on step start, bumped on stream activity and status transitions, cleared on step end. Feeds elapsed + stall classification.
- **Roster refresh tick (`src/tui/mod.rs`, app worker).** A 4th `select!` arm at 1 Hz that drives `refresh_roster_tick()` so elapsed advances and stalls surface even when no events arrive.
- **Roster render block (`src/tui/mod.rs`, rewritten).** Iterates `state.roster_rows`, emitting glyph + label + elapsed + current-step with weight by activity, an animated active indicator, the `NeedsInput` top-pin, and a summary-header line.
- **Accent identity source (`src/app/mod.rs` + 3 render sites).** Canonical-order `accent_index` on `RosterRow`, read by roster, chat (`item_agent_accent`), and `/agent:` dropdown.
- **Glyph/label helpers (`src/tui/mod.rs`).** `activity_glyph`/`activity_label`, mirroring `status_style`/`agent_status_label`.

**Data flow.** Runtime stream events → `push_live_stream_content` (bumps `last_activity`, calls `set_agent_status`) → `publish_state` → `rebuild_roster_rows(now)` → `watch<AppState>` → `render()` reads `state.roster_rows`. In parallel, the 1 Hz tick → `refresh_roster_tick()` → (if run active) rebuild with fresh `now` → change-gated `publish_state`.

## Implementation Design

### Core Interfaces

```rust
// src/app/mod.rs — view-model (derives Clone/Debug/PartialEq/Eq/Serialize/Deserialize)
pub enum ActivityState { Active, NeedsInput, Stalled, Idle }

pub struct RosterRow {
    pub agent_id: String,             // stable identity key (AgentView.id)
    pub name: String,
    pub accent_index: usize,          // canonical-order index -> theme.accent_for()
    pub activity: ActivityState,
    pub runtime_model: String,        // "runtime/model"
    pub effort: String,
    pub thinking: bool,
    pub current_step: Option<String>, // step_label, active rows only
    pub elapsed: Option<String>,      // coarse "1m 20s", active rows only
    pub status: String,               // existing terminal labels preserved
}
```

```rust
// src/app/mod.rs — internal timing + pure builder with injected clock
struct StepTiming { started_at: Instant, last_activity: Instant } // not serialized
const STALL_THRESHOLD: Duration = Duration::from_secs(30);

fn build_roster_rows(
    agents: &[AgentView],
    live_steps: &[LiveStepView],
    timing: &BTreeMap<String, StepTiming>, // step_id -> timing
    now: Instant,                          // tests pass a fixed Instant
) -> Vec<RosterRow>;                        // join, classify, format, NeedsInput pin-sort
```

```rust
// src/tui/mod.rs — render helpers + worker hooks
fn activity_glyph(state: ActivityState, ascii: bool) -> &'static str; // ◐ ◔ ○ ·  / > ? ! .
fn activity_label(state: ActivityState) -> &'static str;              // working/waiting/stalled?/idle

impl App {
    fn rebuild_roster_rows(&mut self);   // called inside publish_state
    fn refresh_roster_tick(&mut self);   // 1 Hz arm; early-return unless work_indicator_active; change-gate
}
```

### Data Models

- **`AppState`** gains `pub roster_rows: Vec<RosterRow>` (`app/mod.rs:72-90`). `AgentView` and `LiveStepView` are unchanged on the wire; `LiveStepView` timing stays internal via the `StepTiming` map (no new serialized fields — keeps the durable history record clean, ADR-004).
- **`ActivityState` classification:** `WaitingForApproval`/`WaitingForAction` → `NeedsInput`; `Running`/`Streaming` with `now - last_activity >= 30s` → `Stalled`, else `Active`; no active step → `Idle`. Terminal statuses (`completed`/`failed`/`interrupted`/`disabled`) keep their existing labels.
- **Ordering:** stable sort by `pin_rank` (`NeedsInput` = 0, else 1) over the already-canonical agent order; `accent_index` is assigned from canonical order *before* the pin, so pinning never perturbs color.

### API Endpoints

Not applicable — this is a local TUI with no network API surface. The "interface" is the in-process `RosterRow` view-model and the helper functions above.

## Integration Points

Not applicable — no external services. Internal seams touched: the app-worker `select!` (`tui/mod.rs:731`), `publish_state` (`app/mod.rs:4050`), the stream chokepoint `push_live_stream_content` (`app/mod.rs:4320`), and the three accent surfaces (roster/chat/dropdown).

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `AppState` (`app/mod.rs:72`) | modified | New `roster_rows` field; cloned each publish. Low risk (small N). | Add field; populate in `publish_state`. |
| `build_roster_rows` / `ActivityState` / `RosterRow` | new | Core join + classification with injected clock. Medium (new logic). | Implement + unit tests. |
| `App` `StepTiming` map | new | Lifecycle-synced timing for elapsed/stall. Medium (sync correctness). | Stamp/bump/clear at 4 sites; tests. |
| App-worker `select!` (`tui/mod.rs:731`) | modified | New 1 Hz arm. Medium (missed-tick under load). | Add gated arm + change-gate; escalation noted. |
| `push_live_stream_content` (`app/mod.rs:4320`) | modified | Bump `last_activity`. Low. | One-line stamp. |
| Roster render block (`tui/mod.rs:2114-2177`) | modified | Rewritten to consume `roster_rows`. Medium (snapshot churn). | New render + snapshot tests. |
| Chat + dropdown accent (`tui/mod.rs:3115`, `2488`) | modified | Repoint to canonical `accent_index`. Medium (contract tests). | Repoint + strengthen 2 tests. |
| `activity_glyph`/`activity_label` | new | Glyph+label vocabulary, ASCII/NO_COLOR. Low. | Implement + NO_COLOR snapshot. |

## Testing Approach

### Unit Tests
- `build_roster_rows` with a fixed `now` + hand-built `StepTiming`: assert classification (within-threshold → `Active`; ≥30 s → `Stalled`; `WaitingForApproval` → `NeedsInput`; no step → `Idle`), using the existing `Instant::now() - Duration` offset pattern (`app/mod.rs:10416`).
- `NeedsInput` pin ordering: fixture `[fixer(NeedsInput), explorer(Active)]` from canonical `[explorer, fixer]`; assert order becomes `[fixer, explorer]` while `fixer.accent_index` stays its **canonical** index.
- Coarse elapsed formatter: whole seconds → `1m 20s` → minutes, correct pluralization, `None` on idle.

### Integration Tests (TUI snapshot, `TestBackend`)
- Render snapshots at 100×24 for: idle lineup, single-active (glyph+label+elapsed+current-step), needs-input pinned to top, stalled-in-place (frozen glyph + "stalled?"), and the summary-header counts line.
- Narrow-width snapshot (~30–40 cols) asserting graceful truncation (PRD CF8).
- `NO_COLOR` snapshot (`TerminalCaps{ no_color: true }`): every state disambiguated by glyph+label with colors collapsed to `Color::Reset`.
- Accent-identity: update `roster_names_carry_same_accents_as_chat` (`tui/mod.rs:8828`) and `agent_dropdown_ids_carry_same_accents_as_roster` (`8870`) to a pinned-reorder fixture; assert accent follows `agent.id`, using `title_cell_fg`.
- Determinism guard: `RunState::Idle` produces no tick-driven churn across repeated renders with no `now` advance.
- CI invariant: `colors_live_only_in_theme_module` (`tui/mod.rs:7686`) still passes (new glyph literals are not `Color::` literals).

*(Per the test-depth decision, V1 does not add `fake.rs` control phrases; stall/needs-input are covered via direct app-layer construction.)*

## Development Sequencing

### Build Order
1. Define `ActivityState` + `RosterRow` in `src/app/mod.rs`; add `roster_rows` to `AppState`. *(no deps)*
2. Add the internal `StepTiming` map to `App`; stamp `started_at`/`last_activity` in `set_active_step_with_metadata` (`3246`), bump in `push_live_stream_content` (`4320`) and `set_live_step_status` (`4362`), clear in `clear_active_step`. *(depends on 1)*
3. Implement `build_roster_rows(agents, live_steps, timing, now)`: join, classify (incl. `Stalled`), resolve `accent_index` from canonical order, format elapsed, `NeedsInput` pin-sort. *(depends on 1, 2)*
4. Add `rebuild_roster_rows(&mut self)`; call it inside `publish_state` (`4050`). *(depends on 3)*
5. Add the 1 Hz `roster_tick` arm to the app-worker `select!` (`tui/mod.rs:731`), gated on `work_indicator_active`, `Skip` missed-tick, change-gate before publish. *(depends on 4)*
6. Add `activity_glyph`/`activity_label` near `status_style` (`3366`) — Set 1 glyphs + ASCII fallback. *(depends on 1)*
7. Rewrite the roster render block (`2114-2177`) to consume `roster_rows`: glyph+label+elapsed+current-step, weight by activity, `accent_index`, animate Active via `work_spinner_frame`, render the summary header. *(depends on 1, 5, 6)*
8. Repoint chat (`3115`) and dropdown (`2488`) accents to the canonical `accent_index`. *(depends on 1)*
9. Update the two accent contract tests to the pinned-reorder fixture. *(depends on 7, 8)*
10. Add `build_roster_rows` unit tests (fixed `now`) + `TestBackend` snapshots (idle/active/needs-input/stalled/narrow/NO_COLOR). *(depends on 3, 7)*

### Technical Dependencies
- None external. **Sequencing vs. `atelier-tui-redesign` F5 is resolved (ADR-006):** the redesign is complete and merged to `main` (theme seam + per-agent accents landed), so this work branches from `main` and inherits F5 — no parallel-edit conflict. Land it before the redesign's still-pending manual asset capture (task 09). Apply ordinary rebase hygiene against other in-flight branches that touch `tui/mod.rs`.

## Monitoring and Observability

Local TUI; observability is the UI itself: the summary header (`▶ N working · ◔ M waiting · ○ K stalled`) and per-row state are the operational signal. No new logging is required; if desired, gate a `debug!` on stall transitions behind the existing tracing setup. Manual verification covers idle-vs-active tick behavior (no churn at rest) and the `NO_COLOR` checklist.

## Technical Considerations

### Key Decisions
- **App-layer view-model with injected clock** (ADR-003) — keeps the renderer pure and snapshots deterministic; rejected render-time computation (flaky) and threading `now` into `render()` (large blast radius).
- **1 Hz `select!` arm, gated + change-gated** (ADR-004) — minimal, matches the git/file-index pollers; rejected a dedicated task (more moving parts; retained as escalation) and `biased!` select (changes global fairness).
- **Stall from elapsed-since-last-activity** (ADR-004) — correct for hangs that stop emitting; rejected elapsed-since-start (false positives) and serializing the timestamp (pollutes history).
- **Accent-by-canonical-identity** (ADR-005) — pin-safe, preserves today's colors; rejected hashing (color churn/collisions) and per-site skip-pinned logic (re-creates coupling).

### Known Risks
- **Missed 1 Hz ticks under heavy streaming** (likelihood: low) — self-mitigated; escalate to a dedicated timer task if a stress test shows drift (ADR-004).
- **`StepTiming` desync/leak** (medium) — single chokepoint for activity (`push_live_stream_content`) + clear-on-end + unit tests bound this.
- **Snapshot churn from the row rewrite** (medium) — expected; the new snapshots are the acceptance criteria.
- **Fourth accent surface added later would silently break identity** (low) — documented single-source rule + contract tests as guard.
- ~~**Sequencing collision with redesign F5**~~ — *resolved* (ADR-006): redesign code is merged to `main`; no parallel edit. Residual: land before redesign task 09 asset capture; rebase against other `tui/mod.rs` WIP branches.

## Architecture Decision Records

- [ADR-001: V1 Mechanism and Scope](adrs/adr-001.md) — stable order + weight + summary header; `NeedsInput` pins; `ActivityState`; accent-by-identity; unified view-model *(idea phase; item 7 amended by ADR-002)*.
- [ADR-002: Progress-Confident Roster with a First-Class Stalled State](adrs/adr-002.md) — `Stalled`, coarse elapsed, bounded ~1 Hz refresh, animated indicator, glyph+label/NO_COLOR *(PRD phase)*.
- [ADR-003: Roster View-Model Architecture and Render-Time Determinism](adrs/adr-003.md) — app-layer `build_roster_rows` with injected clock; rebuild in `publish_state`; pure renderer.
- [ADR-004: Refresh Cadence and Stall-Detection Mechanism](adrs/adr-004.md) — 1 Hz `select!` arm gated + change-gated; stall from last-activity via `push_live_stream_content`; 30 s threshold.
- [ADR-005: Accent-by-Identity Decoupling](adrs/adr-005.md) — canonical-order `accent_index` on `RosterRow`; repoint roster/chat/dropdown; strengthen contract tests.
- [ADR-006: Sequencing Relative to the atelier-tui-redesign Effort](adrs/adr-006.md) — redesign is merged to `main`; branch from `main`, no parallel collision, land before redesign task 09 asset capture.
