# TechSpec: Prompt History — Per-Project ↑/↓ Recall

## Executive Summary

Prompt History is a **read-only projection over the existing event log**, surfaced
as shell-style ↑/↓ recall in the TUI input box. No new persistence: a detached
background task reads `prompt_submitted` events across this project's
`.multiagent/sessions/*` at startup, builds a timestamp-sorted, deduped, capped
`Vec<String>`, and delivers it over a `watch` channel into UI-local `TuiUiState`.
Recall is decided by the same top/bottom visual-row boundary that
`move_input_cursor_vertically` already computes, so it only fires at the input
edges with no dropdown/queue focus — directly avoiding the multi-line collision
that is the #1 competitor bug. Submissions are tagged `Fresh|Recalled` by
extending `AppEvent::PromptSubmitted` so the `prompt_submitted` payload records
provenance for the recall-adoption KPI.

**Primary trade-off:** keeping recall state UI-local (instant, isolated,
consistent with cursor editing) requires extending the widely-referenced
`AppEvent::PromptSubmitted` variant — a mechanical, compile-time-checked change
across all match sites — rather than the smaller diff of skipping instrumentation.
We accept that churn to preserve the KPI and the Phase-3 widen-trigger data. The
design adds exactly one new history primitive (`list_session_event_paths`) and
reuses the proven file-index async pattern, so deferring Ctrl-R search, outcome
metadata, and cross-project scope costs no rework (each is a filter/parameter over
the same in-memory list).

## System Architecture

### Component Overview

Four layers, each a localized change to an existing module — no new packages or
directories.

1. **History reader (`src/history/mod.rs`)** — adds `list_session_event_paths(root)`
   (enumerate `sessions/*/events.jsonl`) and a projection helper that filters
   `prompt_submitted`, applies leading-space-skip, timestamp-sorts desc,
   consecutive-dedups, and caps. Pure, synchronous, tolerant of bad/legacy files.
   *(PRD: per-project scope, dedup+cap, leading-space-skip.)*
2. **Async loader + delivery (`src/tui/mod.rs` startup)** — a detached
   `tokio::spawn` + `spawn_blocking` task (mirroring `refresh_file_index`) runs the
   projection off-thread and sends the result over a new
   `watch::Sender<Vec<String>>`; the render loop syncs it into `TuiUiState`.
   *(PRD: non-blocking < 300ms.)*
3. **Recall interaction (`src/tui/mod.rs` input)** — three new `TuiUiState` fields
   (ring, cursor, saved draft); a recall branch in the `MoveInputCursor` handler of
   `execute_tui_command` (where `state.input` is mutable), keyed to the same
   top/bottom boundary `move_input_cursor_vertically` computes; draft save/restore;
   contextual hint in `render_input_status`. Active only when key routing falls
   through to normal input — the existing precedence chain guarantees no
   dropdown/queue/clarification owns ↑/↓.
   *(PRD: ↑/↓ recall, collision-safe gating, draft preservation, discoverability;
   all five user stories.)*
4. **Provenance + config (`src/app/mod.rs`, `src/config/mod.rs`)** —
   `enum PromptSource`, extended `AppEvent::PromptSubmitted(String, PromptSource)`,
   a `"source"` field on the `prompt_submitted` payload; `UiConfig`/`RawUiConfig`
   gain `prompt_history_enabled` (default true) + `prompt_history_max` (default 200)
   with merge. *(PRD: on-by-default + toggle, recall-adoption KPI.)*

**Data flow (load):** startup → `tokio::spawn` → `spawn_blocking`
(`list_session_event_paths` → `read_events_from_path` per file → project) →
`watch::Sender<Vec<String>>` → render-loop sync → `TuiUiState.prompt_history`.

**Data flow (recall):** ↑/↓ key → `key_event_to_tui_command_with_ui` (falls
through to normal input when empty + no queue/dropdown) → `MoveInputCursor(Up/Down)`
→ `execute_tui_command` handler → `try_recall_history` (boundary + gates) mutates
`state.input` + `prompt_history_cursor`, saving/restoring `prompt_history_draft`;
otherwise ordinary `move_input_cursor`.

**Data flow (submit + tag):** Enter → `PromptSubmitted(input, source)` where
`source = Recalled` iff `prompt_history_cursor != 0` → worker `submit_prompt` →
`record_event("prompt_submitted", {prompt, submitted_prompt, source})`; the
submitted text is also prepended to the in-memory ring.

## Implementation Design

### Core Interfaces

**Recall state (added to `TuiUiState`, `src/tui/mod.rs`):**

```rust
// inserted after `input_width: usize`
prompt_history: Vec<String>,    // timestamp-desc, deduped, capped; newest at front
prompt_history_cursor: usize,   // 0 = live draft; N = Nth-newest entry
prompt_history_draft: String,   // live draft saved while browsing history
```

**Submission provenance (`src/app` event types):**

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptSource { Fresh, Recalled }

// AppEvent::PromptSubmitted(String) becomes:
PromptSubmitted(String, PromptSource),
```

**New history primitives (`src/history/mod.rs`):**

```rust
/// Enumerate every session's event log under `<root>/sessions/*/events.jsonl`.
pub fn list_session_event_paths(root: &Path) -> Result<Vec<PathBuf>>;

/// Read all sessions, keep `prompt_submitted`, drop leading-space prompts,
/// sort by timestamp desc, consecutive-dedup, truncate to `max`.
/// Tolerant of unreadable/legacy files (skips them).
pub fn project_prompt_history(root: &Path, max: usize) -> Vec<String>;
```

**Recall handler + async delivery (`src/tui/mod.rs`):**

```rust
/// Returns true if recall consumed the key (mutating state.input + cursor/draft);
/// false → caller falls back to move_input_cursor.
fn try_recall_history(
    ui_state: &mut TuiUiState, state: &mut AppState, dir: InputCursorCommand,
) -> bool;

fn spawn_prompt_history_load(   // mirrors spawn_file_index_refresh
    working_directory: Option<PathBuf>, max: usize, sender: watch::Sender<Vec<String>>,
);
```

**Config (`src/config/mod.rs`):**

```rust
pub struct UiConfig {
    pub hide_banner: bool,
    pub prompt_history_enabled: bool, // default true
    pub prompt_history_max: usize,    // default 200
}
```

### Data Models

- **`prompt_submitted` payload (extended):**
  `{ "prompt": String, "submitted_prompt": String, "source": "fresh" | "recalled" }`.
  `source` is the only new field; older events without it default to `fresh`
  (serde ignores unknown keys; missing → `fresh`).
- **In-memory recall ring:** `Vec<String>`, newest-first; invariants: no
  consecutive duplicates, `len() <= prompt_history_max`. Seeded from disk at load;
  the current session's submissions are `insert(0, …)` on submit, then
  dedup-consecutive + truncate.
- **No schema/migration:** event-log `schema_version` stays 1; the added payload
  key is backward/forward compatible.

### API Endpoints

Not applicable — no external/HTTP API. The relevant internal surface is the TUI
command/event flow: the existing `TuiCommand::MoveInputCursor(InputCursorCommand)`
carries recall (no new variant), and `AppEvent::PromptSubmitted` gains the
`PromptSource` argument.

## Integration Points

Not applicable — no integration with systems outside the codebase. All interaction
is internal (`src/history`, `src/tui`, `src/app`, `src/config`); the only on-disk
boundary is the existing `.multiagent/sessions/*/events.jsonl`, read-only.

## Impact Analysis

| Component | Impact | Description and Risk | Required Action |
|---|---|---|---|
| `src/history/mod.rs` | new | `list_session_event_paths` + `project_prompt_history`. Low risk; pure read, tolerant of bad files. | Implement + unit tests (sort/dedup/cap/leading-space/tolerance). |
| `TuiUiState` | modified | 3 new fields; touches `Default`/constructors. Low risk. | Add fields; init empty; update ctors/test builders. |
| `MoveInputCursor` handler + `try_recall_history` (`execute_tui_command`) | modified/new | Recall mutation keyed to the boundary `move_input_cursor_vertically` computes. **Highest risk** — the #1 competitor bug (multi-line collision). | Implement helper; full key-handling matrix. |
| `render_input_status` | modified | Contextual "↑ recall" hint when empty + history present. Low risk; no new color literals. | Compute hint; respect `colors_live_only_in_theme_module`. |
| `run_tui` / `run_loop` | modified | New `watch` channel + spawn loader + per-tick sync. Low risk; mirrors file-index. | Wire channel; sync each loop tick. |
| `AppEvent::PromptSubmitted` | modified | Variant gains `PromptSource`. Compile-time churn across all match sites. | Update enum + every match/construction site + tests. |
| `submit_prompt` / `record_event` (`src/app/mod.rs`) | modified | Thread `source`; add `"source"` to payload. Low risk. | Pass source; extend `json!`. |
| `UiConfig` / `RawUiConfig` / merge (`src/config/mod.rs`) | modified | 2 fields + defaults + merge arms. Low risk. | Add fields; default true/200; merge; sample TOML. |

Every PRD goal/user story maps to a component above (load/projection → per-project +
perf; recall helper → ↑/↓ + collision-safe + draft; provenance/config → KPI +
toggle; hint → discoverability).

## Testing Approach

### Unit Tests

**History reader (`src/history`):** newest-first timestamp ordering across multiple
session files; consecutive-dedup; cap/truncate; leading-space-skip excludes;
tolerance — a file with `schema_version != 1` or a malformed line is skipped, not
fatal; empty/missing `sessions/` → empty `Vec`.

**TUI key-handling matrix (`src/tui` tests):**

- Recall fires: empty input + history + no queue/dropdown → ↑ loads newest; ↑ again
  → older; ↓ → newer; ↓ past newest → restores saved draft.
- **No collision:** wrapped/multi-line draft → ↑/↓ move the cursor; history only
  steps at the top/bottom boundary.
- Draft preservation: type → ↑ (saves draft) → ↓-past-newest restores exact draft +
  cursor.
- Yields: with a queued follow-up (empty input) ↑/↓ drive the queue; with a dropdown
  open, ↑/↓ drive the dropdown.
- Provenance: submit after recall (`cursor != 0`) → `Recalled`; fresh type → `Fresh`;
  recall→clear→type → `Fresh`.
- Toggle: `prompt_history_enabled = false` → ↑/↓ never recall; loader not spawned.
- In-session merge: submit prepends to the ring (dedup-consecutive, cap respected).

**Config:** merge applies both fields; defaults true/200 when `[ui]` omits them.
**Constraint:** the hint path adds no inline `Color::` literals.

### Integration Tests

Reuse the `fake` runtime: submit several prompts through a real run, then run
`project_prompt_history` against the produced `.multiagent/` and assert recall
surfaces them newest-first with `source` recorded on `prompt_submitted`. Extend
existing `app`/history tests; no new suite, no external environment dependencies.

## Development Sequencing

### Build Order

1. **History reader** (`list_session_event_paths` + `project_prompt_history`) —
   *no dependencies.* Pure functions + unit tests.
2. **Config fields** (`prompt_history_enabled`, `prompt_history_max` + merge) —
   *no dependencies.* Provides the gate/cap used downstream.
3. **`PromptSource` + `AppEvent::PromptSubmitted` extension + payload** —
   *no structural deps; do early* so downstream UI/worker compile against the final
   signature. Unblocks 5, 6.
4. **`TuiUiState` fields + async loader wiring** (`watch` channel,
   `spawn_prompt_history_load`, `run_loop` sync) — *depends on 1 (projection) and
   2 (cap/toggle).*
5. **Recall interaction** (`try_recall_history` in the `MoveInputCursor` handler,
   draft save/restore, gating) — *depends on 4 (populated ring) and 3 (so submit can
   tag).* The high-risk step; land with its full test matrix.
6. **Provenance wiring at submit** (`submit_prompt` passes `source`, payload
   `"source"`, in-session ring prepend) — *depends on 3 (enum) and 5 (cursor state
   that determines `source`).*
7. **Discoverability hint** (`render_input_status` contextual hint) — *depends on
   4 (history presence) and 5 (active recall).* Lowest risk; last.

### Technical Dependencies

None external. All work is within the existing crate; no new infrastructure,
services, or shared deliverables. The only cross-cutting change is the
`AppEvent::PromptSubmitted` signature (step 3), which gates compilation of steps
5–6 — sequence it first to avoid repeated match-site edits.

## Monitoring and Observability

- **Metric — recall adoption:** `prompt_submitted` events with `source == "recalled"`
  ÷ total (KPI > 20%).
- **Metric — repeat-prompt recall:** among prompts exactly matching a prior
  `prompt_submitted`, the fraction with `source == "recalled"` (KPI > 30%).
- **Log fields:** `prompt_submitted.source`; an optional `debug.log` line on load
  completion with `{sessions_scanned, prompts_loaded, elapsed_ms}` to validate the
  < 300ms target.
- **No alerting** (local single-user tool); the `debug.log` latency line is the
  diagnostic.

## Technical Considerations

### Key Decisions

- **UI-local recall state + extended `AppEvent` tag.** *Rationale:* instant,
  isolated, consistent with cursor editing; preserves the KPI. *Trade-off:* churn
  across `PromptSubmitted` match sites. *Rejected:* worker-owned state;
  no-instrumentation. → ADR-003.
- **Detached background projection (read-all + timestamp-sort + dedup + cap) via
  `watch`.** *Rationale:* never blocks paint; correct ordering; reuses the
  file-index pattern. *Trade-off:* reads all session files once per launch.
  *Rejected:* blocking load; lazy-on-first-↑; bounded early-exit read. → ADR-004.
- **Recall in the `MoveInputCursor` handler keyed to the existing boundary.**
  *Rationale:* the boundary math already distinguishes top/bottom visual rows —
  exactly the collision discriminator; minimal surface. *Trade-off:* recall
  semantics sit beside cursor math, demanding careful tests.

### Known Risks

- **Multi-line collision (high, industry-wide):** ↑ eating a wrapped draft.
  *Mitigation:* strict boundary gate (`current_line` / `input_len <= width`) + the
  full key-handling matrix; frame V1 as "recall recent prompts."
- **Provenance mis-tag (medium):** edited-after-recall ambiguity. *Mitigation:*
  `Recalled` iff `prompt_history_cursor != 0` at submit; clear-to-empty resets the
  cursor; tested.
- **Load latency on huge histories (low):** read-all scales with history size.
  *Mitigation:* off-thread + output cap; `debug.log` latency line; escalate to
  bounded read (ADR-004 Alt 3) only if measured.
- **`AppEvent` churn (low, compile-checked):** a missed match site is a build error,
  not a runtime bug. *Mitigation:* land step 3 first.

## Architecture Decision Records

- [ADR-001: V1 Prompt History as Per-Project ↑/↓ Recall Projected from the Event Log](adrs/adr-001.md)
  — read-only projection over the event log; per-project; newest-first;
  collision-gated; draft-preserving; leading-space-skip.
- [ADR-002: V1 Ships the Full Faithful-Parity Recall Set in One Release](adrs/adr-002.md)
  — deliver the complete shell-parity set at once rather than phasing conventions or
  pulling an outcome marker forward.
- [ADR-003: Recall State in TuiUiState; Tag Submissions via Extended AppEvent](adrs/adr-003.md)
  — UI-local recall state; extend `AppEvent::PromptSubmitted` with `PromptSource`
  for the recall-adoption KPI.
- [ADR-004: Asynchronous Background History Projection](adrs/adr-004.md)
  — detached `spawn_blocking` load via `watch`; read-all + timestamp-sort + dedup +
  cap; one new `list_session_event_paths` primitive.
