# TechSpec — Session Browser & Transcript Resume

## Executive Summary

This feature promotes `atelier`'s event-sourced log from a write-only sink into a load-bearing, backward-compatible **production replay path**, and adds a TUI session browser on top of it. Phase 1 (MVP) ships a read-only browse + transcript preview that folds a chosen session's `events.jsonl` through the existing `ChatProjection::rebuild`; Phase 2 adds Resume, which adopts a session by building a fresh session-state value off-thread and applying it through a single audited `App::adopt_session` swap on the worker thread, then appends new prompts into the same log.

The implementation reuses three existing patterns and changes them minimally: the `spawn_file_index_refresh` watch-channel pattern (for off-thread session-list/preview loading), the `HistoryEvent { kind, payload }` string-kind model (so `run_interrupted`/`session_resumed` are purely additive — no schema bump), and the modal precedence cascade in `key_event_to_tui_command_with_ui` (for the picker). **Primary trade-off:** we accept a permanent replay-compatibility obligation and a *test-enforced* (not compile-enforced) session boundary, in exchange for minimal churn (no `ActiveSession` mega-refactor, no `HistoryEvent` enum conversion) and a faithful, no-desync recovery experience. The worker thread's sole ownership of `App` makes the swap atomic from the UI's perspective for free.

## System Architecture

### Component Overview

**History layer (`src/history/mod.rs`)** — owns persistence.
- `HistoryStore::open(root, session_id)` *(new)*: loads + schema-validates an existing session (sibling to `create`).
- `SessionMetadata` *(extended)* gains `goal`/`outcome` as a self-healing derived cache.
- `SessionSummary` + `list_session_summaries(root)` *(new)*: newest-first session rows for the picker (ULID reverse-sort; label/outcome derived or self-healed).
- New event kinds `run_interrupted`, `session_resumed`.

**Projection layer (`src/app/chat/projection.rs`)** — pure fold, App-independent.
- New `apply_history_event` arms for the two new kinds.
- Read-only preview = a throwaway `ChatProjection::rebuild(&events)` whose `items()` are rendered directly, **skipping** `apply_live_steps`/`apply_pending_approval`/welcome.

**App layer (`src/app/mod.rs`)** — session lifecycle + safety.
- `LoadedSession` + `App::adopt_session(loaded)` *(new)*: the single atomic swap point (ADR-006).
- Per-session approval override + drift gate fields (ADR-004/007).
- `AppEvent::ResumeSession(session_id)` *(new)*.

**Orchestrator (`src/orchestrator/mod.rs`)** — `RunState::is_terminal()` *(new)*.

**Git (`src/app/git.rs`)** — `GitContext` *(extended)* with `head_sha`/`dirty`; drift computation.

**TUI layer (`src/tui/mod.rs`, `welcome.rs`)** — presentation.
- `SessionBrowserState` in `TuiUiState`; new `TuiCommand`s; cascade slot; off-thread list/preview via watch side-channels; `Ctrl-R` + `/sessions`; transcript sanitization; welcome post-crash hint.

**Data flow:** open browser → worker spawns off-thread `list_session_summaries` → `watch<Vec<SessionSummary>>` → TuiUiState → render list. Select → off-thread read + `rebuild` → `watch<Option<SessionPreview>>` → render preview. Resume → `AppEvent::ResumeSession` → worker reads off-thread → `adopt_session` (writes `run_interrupted` + `session_resumed`, reconciles to Idle) → `watch<AppState>` re-renders full transcript.

## Implementation Design

### Core Interfaces

```rust
// src/history/mod.rs — open an existing session (sibling to create()).
impl HistoryStore {
    pub fn open(root: &Path, session_id: &str) -> Result<Self>; // validates SessionMetadata.schema_version == 1
    pub fn update_metadata_cache(&self, goal: Option<&str>, outcome: Option<&str>) -> Result<()>; // self-healing R-M-W
}

// SessionMetadata gains a derived cache (old files still read via serde default).
pub struct SessionMetadata {
    pub schema_version: u32,
    pub session_id: String,
    pub working_directory: PathBuf,
    pub started_at: String,
    #[serde(default)] pub goal: Option<String>,
    #[serde(default)] pub outcome: Option<String>,
    #[serde(default)] pub last_head_sha: Option<String>, // drift baseline (ADR-007)
}
```

```rust
// src/history/mod.rs — picker row, newest-first.
pub struct SessionSummary {
    pub session_id: String,
    pub label: String,        // goal, else first prompt, else timestamp+outcome
    pub started_at: String,
    pub outcome: RunState,     // last terminal/ended state, derived from the fold
    pub working_directory: PathBuf,
}
pub fn list_session_summaries(root: &Path) -> Vec<SessionSummary>; // tolerant; skips unreadable, self-heals stale cache
```

```rust
// src/app/mod.rs — the single atomic adoption point (ADR-006).
struct LoadedSession {
    history: HistoryStore,
    projection: ChatProjection,
    session_goal: Option<String>,
    drift: WorkspaceDrift,
    // ...every session-scoped field, reset to its adopted value
}
impl App {
    fn adopt_session(&mut self, loaded: LoadedSession); // reassign ALL session-scoped fields + one broadcast; reconciles dangling run to Idle
}
```

```rust
// src/app/git.rs — drift inputs + result (ADR-007). dirty is display-only, never a trigger.
pub struct GitContext { pub repo_name: String, pub branch: String,
    pub head_sha: Option<String>, pub dirty: bool }
pub struct WorkspaceDrift { pub cwd_moved: bool, pub head_changed: bool }
impl WorkspaceDrift { pub fn any(&self) -> bool { self.cwd_moved || self.head_changed } }
```

```rust
// src/orchestrator/mod.rs
impl RunState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, RunState::Completed | RunState::Failed | RunState::Interrupted | RunState::LimitReached)
    }
}
```

```rust
// src/tui/mod.rs — picker UI state (lives in TuiUiState, like help; data via watch side-channels).
struct SessionBrowserState {
    visible: bool,
    mode: BrowserMode,            // List | Preview
    summaries: Vec<SessionSummary>,
    selection_index: usize,
    filter: String,               // case-insensitive substring narrow
    preview_scroll: usize,
}
enum SessionBrowserCommand { Up, Down, FilterChar(char), FilterBackspace, OpenPreview, Back, Resume, Close }
```

### Data Models

- **`HistoryEvent`** unchanged structurally (`{ schema_version:1, kind:String, payload:Value, ... }`). New kinds:
  - `run_interrupted` — `{ run_id, prior_state }`.
  - `session_resumed` — `{ resumed_at, cwd, head_sha, dirty, prior_end_state, approval_mode, prior_tail_hash }`.
- **`SessionMetadata`** — extended as above; `goal`/`outcome`/`last_head_sha` are a derived, self-healing cache (log is authoritative; on disagreement the log wins and the cache is rewritten).
- **`SessionSummary` / `SessionPreview`** — view models for the picker and preview (preview = `Vec<ChatItemView>` from a throwaway projection, sanitized).
- **Per-session safety state on `App`** — `resume_approval_mode: Option<ApprovalMode>` (defaults to `Normal` on resume) and `pending_drift_ack: Option<WorkspaceDrift>` (cleared after the first acknowledged mutation).

### API Endpoints

Not applicable — `atelier` is a terminal app with no HTTP surface. The equivalent "public surface" is:
- **Keybinding:** `Ctrl-R` opens the browser (hardcoded in `key_event_to_tui_command`; `config-driven-keybindings` is a separate task that can later make it configurable).
- **Slash command:** `/sessions` (added to the `src/slash_commands.rs` catalog → dropdown + help overlay).
- **New `TuiCommand`s:** `ToggleSessionBrowser`, `SessionBrowser(SessionBrowserCommand)`.
- **New `AppEvent`:** `ResumeSession(String)`.
- **New `AppWorkerCommand` / watch channels:** a `watch::Sender<Vec<SessionSummary>>` (session list) and a `watch::Sender<Option<SessionPreview>>` (preview), populated by spawned off-thread loaders mirroring `spawn_file_index_refresh`.

## Integration Points

The only external interaction is the **git subprocess** (`src/app/git.rs`), extended with `git rev-parse --short HEAD` and `git status --porcelain` under the existing 500ms timeout. No auth; failures degrade gracefully (omit HEAD/dirty, never block resume). No network or other external systems.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|-----------|-------------|----------------------|-----------------|
| `src/history/mod.rs` | modified | Add `open()`, `SessionSummary`, `list_session_summaries`, metadata cache fields + self-heal write. Low risk (additive; `serde(default)`). | Implement + round-trip tests incl. legacy fixtures |
| `src/app/chat/projection.rs` | modified | Add fold arms for `run_interrupted`/`session_resumed`; promote `rebuild` to production. Medium risk: fold fidelity on real/legacy logs. | Add handlers + fold fixtures |
| `src/app/mod.rs` | modified | `LoadedSession`, `adopt_session`, `AppEvent::ResumeSession`, per-session approval override, drift gate. **Highest-risk area** (session swap, run-state reconcile). | Exhaustiveness test + E2E resume tests |
| `src/orchestrator/mod.rs` | modified | Add `RunState::is_terminal()`. Negligible risk. | Replace scattered `matches!` |
| `src/app/git.rs` | modified | `GitContext` gains `head_sha`/`dirty`; drift computation. Low risk; extra subprocess. | Implement + timeout-degrade test |
| `src/tui/mod.rs` | modified | `SessionBrowserState`, cascade slot, render list+preview, `Ctrl-R`, off-thread loaders, sanitization. Medium risk (precedence, latency). | Key-routing tests + latency bench |
| `src/tui/welcome.rs` | modified | Add post-crash hint to `WelcomeFacts`. Low risk. | Implement + render test |
| `src/slash_commands.rs` | modified | Register `/sessions`. Low risk; keep catalog/help/guidance aligned. | Add entry |
| `.atelier/sessions/*/metadata.json` | modified (data) | New optional fields; forward/backward compatible via `serde(default)`. | None (auto-heals) |

## Testing Approach

### Unit Tests
- **History:** `open()` round-trips a `create()`d session; rejects `schema_version != 1`; legacy metadata (missing `goal`/`outcome`/`last_head_sha`) reads via defaults; `list_session_summaries` orders newest-first (ULID), derives the correct label fallback (goal → first prompt → timestamp), and self-heals a stale/missing cache. Reuse the `write_session()` test helper for fixtures.
- **Projection:** fold fidelity — build multi-kind `HistoryEvent` vecs (incl. `run_interrupted`/`session_resumed`) via the `event()` helper, `rebuild`, assert `items()` (resume divider present; dangling run shown interrupted). Include an old-schema-shaped fixture to guard backward compatibility.
- **App:** `adopt_session` **exhaustiveness** test (mutate every session-scoped field to a sentinel, adopt, assert each replaced); dangling-run → `run_interrupted` + reconcile-to-Idle; `resume_approval_mode` defaults to `Normal`; first mutating action under drift requires ack and records it; `is_terminal()` matrix.
- **Git/drift:** `detect_drift` matrix (cwd same/moved × HEAD same/changed × dirty); dirty alone never triggers; missing-HEAD (non-git) never blocks; timeout degrades.
- **TUI:** `key_event_to_tui_command_with_ui` precedence for the new modal (where it sits vs help/clarification/approval; Esc closes without mutating app state; Enter selects); transcript sanitization strips control/ANSI.

### Integration Tests
- **FakeRuntime E2E** (`src/app/mod.rs` test patterns + `tempdir`): drive a run that ends mid-flight → simulate quit → `list_session_summaries` shows it → load preview (assert it matches the on-disk fold) → `ResumeSession` → assert `run_interrupted` + `session_resumed` appended to the **same** `events.jsonl`, transcript re-rendered, state Idle → submit a new prompt → assert appended to the same log. Add a fixture dir of pre-recorded multi-run logs (no equivalent exists today).
- **Latency bench:** `list_session_summaries` + preview fold for a synthetic 200-session / large-log history, asserting the off-thread load keeps the UI responsive (PRD target < 200ms p95 for the list).

## Development Sequencing

### Build Order
1. `RunState::is_terminal()` (`src/orchestrator/mod.rs`) — no dependencies.
2. `HistoryStore::open()` + `SessionMetadata` cache fields + self-healing write (`src/history/mod.rs`) — no dependencies.
3. `SessionSummary` + `list_session_summaries` (newest-first, label fallback, self-heal) — **depends on step 2**.
4. New event kinds + projection fold handlers + fold-fidelity fixtures (`src/app/chat/projection.rs`) — depends on step 1 (`is_terminal` for outcome); otherwise additive.
5. `GitContext` HEAD/dirty + record HEAD baseline at run boundaries + `detect_drift` (`src/app/git.rs`, `src/app/mod.rs`) — no new dependencies.
6. Read-only preview fold (throwaway `rebuild` + transcript sanitization) — **depends on steps 2, 3, 4**.
7. **Phase 1 MVP:** TUI session-browser modal — `SessionBrowserState`, cascade slot, list+preview render, `Ctrl-R` + `/sessions`, off-thread loaders via watch side-channels, welcome post-crash hint — **depends on steps 3, 6**.
8. `LoadedSession` + `App::adopt_session()` + exhaustiveness test — **depends on steps 2, 4**.
9. Resume flow: `AppEvent::ResumeSession` → off-thread read → `adopt_session` → write `run_interrupted` + `session_resumed` → re-render full transcript → land Idle — **depends on steps 4, 5, 8**.
10. Per-session cautious approval default + first-mutation drift interlock folded into the approval prompt — **depends on steps 5, 9**.
11. **Phase 2 complete:** resume-rate instrumentation (derive from `session_resumed` + run outcomes) + wire the post-crash hint to the newest session's outcome — **depends on steps 3, 9**.

### Technical Dependencies
No infrastructure or external-service prerequisites. All work is in-repo. The `config-driven-keybindings` task is **not** a blocker (the trigger key is hardcoded for now).

## Monitoring and Observability

`atelier` is a local CLI; observability is the durable event log plus lightweight timing.
- **Log events (structured):** `session_resumed` (carries `resumed_at`, `cwd`, `head_sha`, `dirty`, `prior_end_state`, `approval_mode`) and `run_interrupted` make resume auditable in the log itself.
- **Derived metrics (PRD Success Metrics):** crash-recovery adoption (sessions ending non-terminal that later get a `session_resumed`); time-to-continue (`app_start` → first resumed `prompt_submitted`); resumed-session completion (resumed sessions reaching a terminal `Completed`). All computable by folding the log — no external telemetry.
- **Latency:** instrument browser-open → first list paint and preview-fold duration (debug log / test bench) against the < 200ms p95 target.
- **Fidelity invariant:** a property assertion that a resumed/previewed transcript equals a full `rebuild` of the on-disk log (zero desync).

## Technical Considerations

### Key Decisions
- **Single `adopt_session()` + exhaustiveness test** (ADR-006): captures the no-partial-reset property with minimal churn. *Trade-off:* test-time, not compile-time, enforcement. *Rejected:* a named `ActiveSession` refactor (too much churn), inline reset (partial-reset risk).
- **Off-thread read, worker-thread fold+swap:** the disk read of `events.jsonl` runs off-thread (file-index pattern, `Vec<HistoryEvent>` is `Send`); the cheap in-memory `rebuild` + swap run on the worker, which solely owns `App` → atomic to the UI. *Rejected:* moving `App` across threads (App isn't `Send`-friendly for that).
- **Additive string-kind events + self-healing metadata** (ADR-008): new events need no enum/schema change; the metadata cache can't permanently lie. *Rejected:* `HistoryEvent` enum conversion, authoritative metadata.
- **cwd + HEAD drift, gated at first mutation** (ADR-007): full ADR-004 fidelity; dirty is display-only. *Rejected:* cwd-only / HEAD-only.
- **`Ctrl-R` trigger** (hardcoded): fzf "recall" precedent, no XOFF collision, free today.

### Known Risks
- **Fold fidelity on legacy logs** (medium): `rebuild` has never run on logs it didn't just write. *Mitigation:* fold-fixture tests over old-shaped logs; the `schema_version == 1` gate fails loud on anything incompatible. No upcasting is needed while the version stays `1`, but *semantic* drift (a reused `kind`/payload meaning) remains a latent risk — covered by fixtures.
- **Preview/resume latency on very long logs** (low–medium): replay is O(history). *Mitigation:* off-thread load now; snapshot/compaction named as a future ADR.
- **Git subprocess cost** (low): two extra calls. *Mitigation:* existing 500ms timeout; degrade (omit) on timeout, never block.
- **Trigger-key churn** (low): `config-driven-keybindings` may later relocate `Ctrl-R`. *Mitigation:* the `/sessions` command + welcome hint guarantee discoverability regardless.
- **Modal precedence regressions** (low): the new modal joins a 9-branch cascade. *Mitigation:* key-routing precedence tests.

## Architecture Decision Records

- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — V1 = list + preview + Resume; search → later phase.
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — same log; `run_interrupted` + `session_resumed`; metadata is a derived cache.
- [ADR-003: Production replay fold as a maintained schema-compatibility contract](adrs/adr-003.md) — promote `rebuild`, validate at `open()`, atomic swap.
- [ADR-004: Resume safety model](adrs/adr-004.md) — drift interlock at first mutation, cautious-default re-consent, untrusted-transcript rendering, file perms.
- [ADR-005: Product approach — recovery-first, phased delivery](adrs/adr-005.md) — browse+preview MVP → resume → search.
- [ADR-006: Session adoption via a single `adopt_session()` swap + exhaustiveness test](adrs/adr-006.md) — one audited swap point; off-thread read, worker-thread fold/swap.
- [ADR-007: Drift detection model](adrs/adr-007.md) — extend GitContext (HEAD+dirty), record a HEAD baseline, gate first mutation on cwd-or-HEAD change.
- [ADR-008: Lifecycle events as additive string-kinds + self-healing metadata cache](adrs/adr-008.md) — no schema bump; metadata self-heals from the log; `is_terminal()`.
