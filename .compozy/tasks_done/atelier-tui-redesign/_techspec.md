# TechSpec: Atelier TUI Visual Identity

## Executive Summary

This spec implements the PRD's visual identity in four mechanically separable pieces: (1) a new `src/tui/theme.rs` module holding every color as a semantic token, resolved once at startup against detected terminal capabilities (`NO_COLOR`, `COLORTERM`) and threaded to all render functions as a field on the already-ubiquitous `TuiUiState`; (2) a welcome screen implemented as a synthetic `ChatItemKind::Welcome` chat item — it scrolls in scrollback for free and replaces the blocking "Loading skills..." interstitial; (3) a polled `GitContext` (background tokio interval + immediate refresh at startup and prompt submission, change-gated updates) feeding a persistent footer; (4) migration of all 88 inline `Color::` literals to tokens, enforced by a source-scanning test rather than CI configuration.

**Primary trade-off:** the full color migration lands in one window (concentrated diff risk in a 3,900-line module, mitigated by 66 existing `TestBackend` tests and a mechanical find-replace pattern) in exchange for a permanently enforceable single-source-of-color invariant and no period where two color systems coexist. Secondary trade-off: theme-on-`TuiUiState` puts read-only data on a `&mut` context struct — accepted for zero signature churn.

## System Architecture

### Component Overview

| Component | Location | Responsibility |
|---|---|---|
| **Theme** | `src/tui/theme.rs` (new) | Token definitions (from web palette), `TerminalCaps` detection, resolution (RGB → 256 → mono), `accent_for(index)` agent colors |
| **Welcome renderer** | `src/tui/welcome.rs` (new) | Renders `ChatItemKind::Welcome`: adaptive wordmark (isolates the `tui-big-text` dependency), facts box. Width-laddered at render time |
| **Git context** | `src/app/git.rs` (new) | `fetch_git_context(dir) -> Option<GitContext>` via `git rev-parse` subprocess with timeout; poll task wiring in the app worker |
| **UI config** | `src/config/mod.rs` (modified) | New optional `[ui]` TOML section → `UiConfig { hide_banner }` following the existing `RawConfig`/merge pattern |
| **Footer** | `src/tui/mod.rs` (modified) | Extends the existing status line (`:1883`): repo+branch · run state · agent count · `/help` hint |
| **Migration** | `src/tui/mod.rs` (modified) | All 88 literals → `ui_state.theme.*` tokens; existing helpers (`status_style`, `severity_badge_style`) repointed |

**Data flow:** startup → `TerminalCaps::detect()` → `Theme::resolve(caps)` → stored in `TuiUiState` → first frame renders welcome item (injected into `AppState.chat_items`) → skill loading proceeds → worker spawns git poll task → `Option<GitContext>` updates `AppState` only on value change → footer re-renders.

## Implementation Design

### Core Interfaces

```rust
// src/tui/theme.rs
pub struct Theme {
    pub text: Color,            // #f2ead8
    pub text_muted: Color,      // #b7ae9e
    pub text_dim: Color,        // #7e867a
    pub border: Color,          // line tones
    pub border_focused: Color,
    pub accent: Color,          // cyan #79e2e1 — brand accent
    pub status_ok: Color,       // green #8cffb0
    pub status_warn: Color,     // amber #ffb454
    pub status_error: Color,    // red #ff6f61
    pub user_prompt_bg: Color,  // replaces USER_EVENT_BG
    pub agent_accents: [Color; 5], // round-robin pool, red excluded
}

impl Theme {
    pub fn resolve(caps: TerminalCaps) -> Theme { /* RGB | Indexed | mono per token */ }
    pub fn accent_for(&self, agent_index: usize) -> Color { /* index % pool */ }
}
```

```rust
// src/tui/theme.rs
pub struct TerminalCaps {
    pub no_color: bool,   // NO_COLOR set non-empty
    pub truecolor: bool,  // COLORTERM in {truecolor, 24bit}
}
impl TerminalCaps {
    pub fn detect() -> Self { /* env reads, no I/O */ }
}
```

```rust
// src/app/git.rs
#[derive(Clone, PartialEq)]
pub struct GitContext {
    pub repo_name: String, // file_name of `git rev-parse --show-toplevel`
    pub branch: String,    // `git rev-parse --abbrev-ref HEAD` (raw SHA if detached)
}

/// None on: non-zero exit, timeout (500ms), git missing, not a repo.
pub async fn fetch_git_context(dir: &Path) -> Option<GitContext>;
```

`PartialEq` on `GitContext` is the change-gate: the poll task compares fresh vs. stored and emits a state update only on difference (ADR-006).

### Data Models

- `ChatItemKind` (in `src/app/chat/mod.rs`) gains a `Welcome` variant. The welcome `ChatItemView` carries empty `source`, no `lifecycle_key`, and is skipped by `ChatProjection` consumers (audit: add ignore arms where kinds are matched exhaustively).
- `AppState` gains `git_context: Option<GitContext>`.
- `TuiUiState` gains `theme: Theme`.
- `EffectiveConfig` gains `ui: UiConfig { hide_banner: bool }` (default `false`); `RawConfig` gains `ui: Option<RawUiConfig>` (respecting `deny_unknown_fields`).

### API Endpoints

Not applicable — no network or IPC surface. The only external interface is the `git` subprocess (see Integration Points).

## Integration Points

**git subprocess** — `tokio::process::Command::new("git")` with `current_dir(working_directory)`, `kill_on_drop(true)`, 500ms timeout via `tokio::select!` (pattern from `src/runtime/claude.rs:147-174`). No retry: a failed tick returns `None`; the next tick retries naturally. No authentication. Errors are silent by design (graceful omission, ADR-001); a debug-level diagnostic is recorded only on the first consecutive failure to aid support.

## Impact Analysis

| Component | Impact Type | Description and Risk | Required Action |
|---|---|---|---|
| `src/tui/mod.rs` | modified (heavy) | 88 literal replacements + footer + welcome dispatch; medium risk, mitigated by 66 tests | Mechanical migration; no structural refactor (ADR-001) |
| `src/tui/theme.rs` | new | Token + resolution module; low risk, pure functions | Unit tests for resolution and quantization |
| `src/tui/welcome.rs` | new | Wordmark + facts box; isolates `tui-big-text`; low risk | Width-ladder tests at 80/60/40 |
| `src/app/chat/mod.rs` | modified (small) | New `ChatItemKind::Welcome`; low risk | Audit exhaustive matches in projection |
| `src/app/mod.rs` | modified (small) | `git_context` field, welcome injection, poll wiring | Tie poll task to worker lifecycle |
| `src/app/git.rs` | new | Subprocess fetch; low risk, kill-switch ½ day | One test with fake-command unavailable path |
| `src/config/mod.rs` | modified (small) | `[ui]` section; low risk, existing pattern | Default + merge + one parse test |
| `Cargo.toml` | modified | `tui-big-text = "0.7"`; pins to ratatui 0.29 line | Confine usage to `welcome.rs` (ADR-004) |
| `README.md` + assets | modified | New screenshot + GIF | Produced last, gates announcement (ADR-002) |

## Testing Approach

### Unit Tests

- **Theme resolution**: truecolor caps → RGB values; non-truecolor → hand-picked `Indexed` values; `no_color` → mono styles. Pure-function tests in `theme.rs`.
- **`accent_for`**: cycles the pool, adjacent indices distinct.
- **Git context parsing**: branch/repo extraction from command output; `None` paths (the unavailable-git case via a nonexistent command name).
- **Source invariant** (replaces CI config): a test reads `src/tui/*.rs` and asserts `Color::` appears only in `theme.rs` — the PRD's "automated check in CI" with zero CI changes.

### Integration Tests

- **TestBackend renders** (existing convention, `render_to_text` helpers): welcome item at 80/60/40 columns (full wordmark / compact / plain text assertions); welcome absent when `hide_banner`; facts box content (version, agents count, branch line present/absent); footer with `Some`/`None` git context, run states, agent counts; NO_COLOR render contains all content strings.
- **Existing 66 tests** double as the migration regression net — they must pass unchanged except for deliberate color/title assertions.
- **Manual release checklist** (not automated): Terminal.app 256-color, iTerm2, Alacritty, `NO_COLOR=1` run, GIF/screenshot capture.

## Development Sequencing

### Build Order

1. **Config `[ui]` section** (`src/config/mod.rs`) — no dependencies.
2. **Theme module** (`src/tui/theme.rs`: caps, tokens, resolve, `accent_for`) — no dependencies.
3. **Thread theme + migrate all 88 literals + source-invariant test** — depends on 2. The repoint of `status_style`/`severity_badge_style` happens here.
4. **Welcome renderer + `ChatItemKind::Welcome` + startup injection + remove loading screen** — depends on 1 (hide_banner), 2, 3 (renders via tokens). Add `tui-big-text` here.
5. **Git context module + startup fetch + poll task + prompt-submission refresh** — depends on nothing above technically; scheduled after 4 to respect the kill-switch (cut without touching theme/welcome work).
6. **Footer** — depends on 3 (tokens) and 5 (git context).
7. **Per-agent accents in roster/output views + run-summary restyle** — depends on 3.
8. **Remaining surface polish (dropdowns, dialogs, help modal)** — depends on 3.
9. **Breakpoint/NO_COLOR test consolidation** — depends on 4, 6, 7, 8.
10. **README assets + 3-terminal checklist** — depends on all; gates announcement (ADR-002).

### Technical Dependencies

- `tui-big-text 0.7.x` availability on crates.io (verified — 0.7 line targets ratatui 0.29).
- `git` binary presence is **not** a dependency — absence degrades gracefully.
- Welcome wordmark visual round (gradient direction, `PixelSize` choice) needs a human eye during step 4 — prototype both Sextant and Quadrant, pick by appearance.

## Monitoring and Observability

Minimal by design (local TUI): first-consecutive-failure debug diagnostic for git fetch (existing `record_diagnostic` path); startup timing assertion stays a development-time measurement, not runtime telemetry. No metrics, no alerting.

## Technical Considerations

### Key Decisions

- **Resolve-at-startup theme on `TuiUiState`** — render code never branches on capabilities; tests construct theme variants directly. Gave up: reacting to mid-session capability changes (no terminal does this). (ADR-004)
- **Welcome as synthetic chat item** — scrollback persistence for free, zero layout changes. Gave up: chat model purity (`ChatItemKind` gains a non-event variant). (ADR-005)
- **Polled git context with change-gating** — footer tracks external branch switches within ~5s. Gave up: zero idle subprocess cost (sub-10ms `rev-parse` every 5s, negligible). Revised from event-driven-only by user decision. (ADR-006)
- **Round-robin agent accents** — deterministic distinctness, no config surface. Gave up: reorder-stability. (ADR-006)
- **Source-scanning invariant test over CI config** — the enforcement travels with `cargo test`, works in any CI, no workflow edits.

### Known Risks

- **256-color quantization quality** — algorithmic nearest-match can produce muddy tones; mitigation: hand-pick each token's `Indexed` fallback during step 2 (likelihood: medium, impact: cosmetic).
- **Exhaustive `ChatItemKind` matches** — new variant breaks compilation wherever matched exhaustively; that's the safety mechanism, not a risk per se; budget the audit in step 4.
- **Welcome item height vs. scroll accounting** — dynamic height on resize exercises the wrapped-line recomputation; covered by breakpoint tests (likelihood: low).
- **Migration assertion churn** — some of the 66 tests assert on colors/titles implicitly via strings; expect a handful of deliberate assertion updates in step 3 (likelihood: high, impact: trivial).
- **Poll task leakage** — interval task must abort on shutdown; tie to worker lifecycle as existing background tasks do (likelihood: low).

## Architecture Decision Records

- [ADR-001: V1 Scope and Sequencing](adrs/adr-001.md) — Theme seam + welcome together; full migration same window; subprocess git; no user theming in V1.
- [ADR-002: Unified Single-Release Rollout](adrs/adr-002.md) — Three internal phases; README refresh gates announcement.
- [ADR-003: Web Palette as Canonical Brand Source](adrs/adr-003.md) — TUI tokens derive from web CSS variables; spinner deferred.
- [ADR-004: Theme Module Architecture](adrs/adr-004.md) — Resolved token struct threaded via `TuiUiState`; `tui-big-text` isolated in welcome renderer.
- [ADR-005: Welcome Screen as Synthetic Chat Item](adrs/adr-005.md) — `ChatItemKind::Welcome` injected at startup; scrollback persistence via chat machinery.
- [ADR-006: Polled Git Context Refresh and Round-Robin Agent Accents](adrs/adr-006.md) — 5s change-gated poll + immediate refresh at decision moments; accents by roster order.
