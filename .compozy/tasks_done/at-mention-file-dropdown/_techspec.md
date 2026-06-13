# TechSpec: @-Mention File Dropdown

## Executive Summary

This feature adds an `@`-triggered file/folder picker to the Atelier TUI composer, implementing the [PRD](_prd.md)'s "Complete V1" by extending the existing dropdown machinery rather than building a parallel system. The dropdown is **token-based** — it reuses `active_prompt_token(input, cursor, "@")` (already prefix-generic) so it activates mid-prompt exactly like `/agent:` and `/skill:`, and accepts via the established token-replacement path. The one genuinely new subsystem is a **`FileIndex`** (new `src/file_index.rs`): an `ignore`-crate walk of the working directory, filtered for secrets and noise, queried per-keystroke with `nucleo-matcher`.

The primary trade-off: the file walk runs **off the draw thread** in a background `spawn_blocking` task delivered over a channel (reusing the worker/git-poller pattern), trading a small amount of channel plumbing and a brief startup "index pending" window for guaranteed input responsiveness and mid-session freshness. Per-keystroke filtering is in-memory and synchronous. Net new dependencies: `ignore` and `nucleo-matcher`.

## System Architecture

### Component Overview

| Component | Responsibility | Boundary |
|---|---|---|
| **`FileIndex`** (`src/file_index.rs`, new) | Walk the working dir (`ignore` crate), apply secret/noise exclusions + working-dir pin + symlink rejection, hold `Vec<FileEntry>`, and answer `query()` with ranked, highlight-annotated suggestions via `nucleo-matcher`. | The only component that touches the filesystem. No TUI types. |
| **Index acquisition task** (worker, `src/tui/mod.rs`) | Run the walk in `spawn_blocking` at startup + on a coarse refresh tick; deliver `Vec<FileEntry>` to the TUI over a channel. | Mirrors the existing git-poller; never blocks the draw loop. |
| **`FileMentionDropdown`** (`src/tui/mod.rs`) | Activation, selection state, key handling, render, and insertion, mirroring `SkillDropdown` + the command dropdown's empty/dismiss behavior. | Pure UI/state; reads the cached entries, calls `FileIndex` query logic. |
| **Routing & render chains** (`src/tui/mod.rs`) | Slot the `@` dropdown into `key_event_to_tui_command_with_ui` and the render `if-let` chain, after the skill branch. | Must stay in sync (existing in-code invariant). |

**Data flow:** `config.working_directory` → acquisition task walks via `FileIndex::walk` (off-thread) → `Vec<FileEntry>` sent over a channel → cached on `TuiUiState.file_mention_entries` → on `@`, `file_mention_dropdown()` builds suggestions via `FileIndex::query(entries, q, 6)` → render draws the upward overlay with highlights → accept calls `apply_file_mention_suggestion()` which rewrites the `@token` to a bare path.

**PRD goal → component mapping:** "Eliminate broken references" → `FileMentionDropdown` insertion (only real paths) + `FileIndex` (existing paths only). "Faster than typing" → `FileIndex::query` fuzzy + ranking. "Stay safe/deterministic" → `FileIndex` exclusions + text-only insertion. "Meet the expectation" → render highlighting + empty/no-match states.

## Implementation Design

### Core Interfaces

The candidate model and index (new module). `FileEntry` is the structured "validated reference" ADR-001 called for:

```rust
// src/file_index.rs
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub rel_path: String,   // forward-slashed, relative to the working dir
    pub is_dir: bool,
    pub mtime: std::time::SystemTime,
    pub depth: usize,       // path component count, for shallow-first ranking
}

pub struct FileIndex {
    entries: Vec<FileEntry>,
    matcher: nucleo_matcher::Matcher,
}

impl FileIndex {
    /// Off-thread: gitignore-aware walk, secret/noise excluded, symlinks skipped.
    pub fn walk(root: &std::path::Path) -> Vec<FileEntry> { /* ignore::WalkBuilder */ }
    /// In-memory per keystroke: rank + cap, with matched-char offsets for highlight.
    pub fn query(&mut self, query: &str, limit: usize) -> Vec<FileSuggestion> { /* nucleo */ }
}
```

The suggestion carried to the UI (path + highlight offsets):

```rust
// src/file_index.rs
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSuggestion {
    pub rel_path: String,
    pub is_dir: bool,
    pub match_indices: Vec<u32>, // char offsets nucleo matched, for highlighting
}
```

The dropdown state, mirroring `SkillDropdown` (token-based) plus a command-style `empty` flag:

```rust
// src/tui/mod.rs
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileMentionDropdown {
    token: PromptToken,            // reused; query runs to next whitespace
    suggestions: Vec<crate::file_index::FileSuggestion>,
    selected: usize,
    empty: bool,                   // token active, non-empty query, zero matches
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileMentionDropdownCommand { Previous, Next, Accept, Dismiss }
```

Activation and insertion signatures (bodies mirror the skill dropdown):

```rust
// src/tui/mod.rs
fn file_mention_dropdown(input: &str, ui_state: &TuiUiState) -> Option<FileMentionDropdown>;
fn file_mention_dropdown_key_command(d: &FileMentionDropdown, key: KeyEvent) -> Option<TuiCommand>;
fn apply_file_mention_suggestion(            // replaces @token (incl. the `@`) with a bare path
    state: &mut AppState, ui_state: &mut TuiUiState,
    token: &PromptToken, suggestion: &FileSuggestion,
);
```

### Data Models

- **`FileEntry`** / **`FileSuggestion`** — above; owned by `FileIndex`.
- **`TuiUiState` additions:** `file_mention_entries: Vec<FileEntry>` (cached walk result), `file_mention_selection_index: usize`, `file_mention_dropdown_dismissed: Option<String>` (Esc-dismiss, mirrors `command_dropdown_dismissed`). All default to empty/`0`/`None`.
- **`TuiCommand` addition:** `FileMentionDropdown(FileMentionDropdownCommand)`.
- **Channel message:** `Vec<FileEntry>` (a `watch` or `mpsc` carrying the latest index snapshot), received in `run_loop` and stored on `TuiUiState`.

### API Endpoints

Not applicable — this is a local TUI feature with no network or HTTP surface.

## Integration Points

No external services. The one internal boundary is the **worker→TUI channel**: the acquisition task (off-thread) produces index snapshots; the synchronous render loop consumes the latest snapshot non-blockingly in `run_loop` (the same shape as `sync_worker_state` merging worker state today). Failure handling: if a walk errors (e.g. permission denied on a subtree), the `ignore` walker skips that subtree; a fully failed walk yields an empty index and the dropdown simply shows no candidates.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `Cargo.toml` | modified | Add `ignore`, `nucleo-matcher`. Low risk (both mature). | Add two `[dependencies]`. |
| `src/file_index.rs` | new | Walk + filter + query. Medium risk (perf on large repos, secret exclusion correctness). | Create module + unit tests. |
| `src/tui/mod.rs` — worker/`run_loop` | modified | Background walk task + channel receive. Medium risk (async wiring). | Add `spawn_blocking` + receiver. |
| `src/tui/mod.rs` — `TuiUiState` | modified | 3 fields + `Default` + `reset_dropdown_selections` + dismissal-clear. Low risk. | Extend struct/fns. |
| `src/tui/mod.rs` — dropdown | new code | `FileMentionDropdown` activation/keys/render/insert. Medium risk. | Add functions mirroring skill dropdown. |
| `src/tui/mod.rs` — routing + render chains | modified | Slot `@` after skill in **both** chains. Medium risk (desync). | Edit both; add a test asserting parity. |
| `TuiCommand` + executor | modified | New variant + execute arm. Low risk. | Add variant + handler. |
| `README.md` | modified | Document the `@` picker. Low risk. | Add to "TUI commands". |

## Testing Approach

### Unit Tests (in the `src/tui/mod.rs` test module + `src/file_index.rs`)
- **Activation:** `@token` at the cursor activates; cursor outside the token does not; suppressed during `pending_approval`, `pending_clarification`, and `WaitingForUser` (reuse the `command_state` / `state_with_input` helpers).
- **Empty query:** bare `@` lists top-N by mtime (recents), first row selected.
- **Filter + ranking:** fuzzy query orders by score then shallow-path then mtime; matched offsets are present for highlight.
- **Accept:** consumes the `@` → inserts a **bare path**, folder gets a trailing `/`, a trailing space is added, cursor lands after it; surrounding text preserved; no `AppEvent` dispatched. Tab and Enter both accept.
- **Dismiss / no-match:** Esc dismisses and survives cursor moves but clears on edit; non-empty zero-match renders the "No matching files" row and does not trap Enter.
- **Security exclusions:** gitignored files, secret-name files (`.env`, `*.pem`…), symlinks, and paths resolving outside the root never appear.

Mocks/boundaries: use `tempfile` (already a dev-dependency) to build a real directory tree with `.gitignore` + a `.env` + nested dirs + a symlink, and assert against the live `FileIndex::walk` output — no filesystem mocking.

### Integration Tests
- Build a tempfile repo, run the full path: `walk` → cache on `TuiUiState` → `render_to_text_with_ui` with input `"see @run"` → assert the rendered dropdown shows the ranked, highlighted candidates and that accept rewrites the buffer to the bare path. Drive the render+key cycle to confirm the routing/render chains agree.

## Development Sequencing

### Build Order
1. **Add dependencies** (`ignore`, `nucleo-matcher`) — no dependencies.
2. **`FileEntry` + `FileIndex::walk`** in `src/file_index.rs` (gitignore-aware walk, secret/force-exclude filter, working-dir pin, symlink rejection) — depends on step 1.
3. **`FileSuggestion` + `FileIndex::query`** (nucleo-matcher scoring, ranking blend, cap, highlight offsets) — depends on step 2.
4. **Background acquisition** (`spawn_blocking` walk in the worker + channel; initial + coarse periodic refresh) — depends on step 2 and the existing worker/channel infra.
5. **`TuiUiState` fields + wiring** (`file_mention_entries`, `file_mention_selection_index`, `file_mention_dropdown_dismissed`; receive the channel in `run_loop`; extend `reset_dropdown_selections` and the dismissal-clear path) — depends on step 4.
6. **`FileMentionDropdown` + `file_mention_dropdown()` activation** (empty/recents/no-match logic) — depends on steps 3 and 5.
7. **Key handling** (`FileMentionDropdownCommand`, `TuiCommand` variant, `file_mention_dropdown_key_command`, dispatch + Prev/Next wraparound) — depends on step 6.
8. **`apply_file_mention_suggestion`** (consume `@`, bare path, folder `/`, trailing space, cursor) — depends on step 6.
9. **`render_file_mention_dropdown`** (upward overlay, matched-char highlight, no-match row) — depends on step 6.
10. **Wire both chains** (activation into `key_event_to_tui_command_with_ui` after skill; render into the render `if-let` after skill) — depends on steps 7 and 9.
11. **Tests** (unit + integration per above) — depends on steps 2–10.
12. **README note** — depends on step 10.

### Technical Dependencies
- `ignore` crate (gitignore-aware walking) and `nucleo-matcher` (fuzzy scoring) — both net-new, no new tokio features required (`fs`, `rt-multi-thread`, `sync`, `time` already enabled).

## Monitoring and Observability

Minimal — a local interactive TUI with no telemetry surface. Under the existing `--debug` flag, optionally log the walk duration, candidate count, and refresh ticks to aid field diagnosis on large repos. No metrics or alerts.

## Technical Considerations

### Key Decisions
- **Background-worker index (ADR-003):** off-thread walk + channel; refines ADR-001's "lazy on first `@`". Trade-off: channel plumbing + a startup-pending window vs. guaranteed responsiveness and mid-session freshness.
- **`nucleo-matcher` (ADR-004):** matcher-only crate for synchronous per-keystroke scoring; full `nucleo` worker rejected as overkill, substring rejected by the product owner.
- **Placement & integration (ADR-005):** dropdown in `src/tui/mod.rs` (consistency), index in new `src/file_index.rs` (isolation); insertion consumes the `@` for a bare path.

### Known Risks
- **Secret/sensitive filename leakage** (the key safety risk): `.gitignore` excludes tracked-ignored files only, so a static secret-name denylist + working-dir pin + symlink/`..` rejection are required and tested. Residual: a sensitive file neither denylisted nor gitignored could still appear — denylist is best-effort; documented.
- **Large-repo walk cost** (medium on monorepos): coarse refresh interval + `spawn_blocking` + capped results; future event-driven refresh after write actions.
- **Routing/render chain desync** (medium): both chains must add `@` in the same position; mitigated by a parity test and the existing in-code comment.
- **Ranking-blend quality** (low–medium): score/depth/recency weights need tuning; mitigated by interaction tests and keeping weights adjustable.

## Architecture Decision Records

- [ADR-001: Scope @-Mention File Dropdown V1](adrs/adr-001.md) — Fuzzy `@` dropdown over a `.gitignore`-aware walk, bare-path insert through a structured reference seam, with security guardrails.
- [ADR-002: Package as a Complete Single-Release V1](adrs/adr-002.md) — Ship the full experience at once.
- [ADR-003: File-Index Acquisition via Background Worker Walk](adrs/adr-003.md) — Off-thread `spawn_blocking` walk + channel, refining ADR-001's lazy approach.
- [ADR-004: Fuzzy Matching via nucleo-matcher with a Ranking Blend](adrs/adr-004.md) — Matcher-only crate, synchronous per-keystroke scoring with a recency/depth ranking blend.
- [ADR-005: Component Placement and Dropdown Integration](adrs/adr-005.md) — Dropdown in `tui/mod.rs`, index in new `src/file_index.rs`, `@`-consuming insertion.
