---
status: pending
title: Branded welcome screen as synthetic chat item
type: frontend
complexity: high
dependencies:
  - task_01
  - task_03
---

# Task 4: Branded welcome screen as synthetic chat item

## Overview

Build the branded welcome screen (PRD F1): a new `ChatItemKind::Welcome` injected as the first chat item at startup, rendered by a new `src/tui/welcome.rs` with an adaptive ASCII "Atelier" wordmark (`tui-big-text 0.7`) and a facts box (version, working directory, repo+branch when available, agents summary, preset, warnings count, `/help` hint). It replaces the "Loading skills..." interstitial and persists in scrollback (ADR-005).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. `ChatItemKind` MUST gain a `Welcome` variant; both exhaustive matches MUST be extended: `slug()` (`src/app/chat/mod.rs:224-237`) and `chat_kind_label()` (`src/tui/mod.rs:1801-1814`).
2. The welcome item MUST be injected once at app startup (after `ChatProjection::new()`, `src/app/mod.rs:~710`) so `sync_chat_items` (:3679-3685) carries it; it MUST have no `lifecycle_key` and never update after creation (ADR-005).
3. `chat_item_lines` (`src/tui/mod.rs:1662-1703`) MUST dispatch `Welcome` items to the new renderer in `src/tui/welcome.rs`; the `tui-big-text` dependency MUST be confined to that file (ADR-004).
4. The wordmark MUST width-ladder at render time: full lettering ≥80 cols, compact 60-79, plain styled text <60 (PRD F1); it MUST be skipped entirely when the theme is no-color or `config.ui.hide_banner` is true (facts render as plain text either way).
5. The facts box MUST show: version (`CARGO_PKG_VERSION`), working directory, agents summary (count + names/models from `AppState.agents`), active preset, config warnings count when >0, and the `/help` hint. Repo+branch line renders when `AppState.git_context` is `Some` and is omitted otherwise — the field lands in task_05; until integration, the line is driven by the same `Option` (compile-time integration point, not a runtime stub).
6. Both `render_skill_loading` call sites (`src/tui/mod.rs:240` startup, `:298` ReloadSkills) MUST be removed/replaced: startup renders the main UI immediately; skill reload signals via the existing `status_message` mechanism instead of a full-screen takeover.
7. Welcome MUST render with zero inline `Color::` literals (theme tokens only — the task_03 invariant test enforces this).
8. Startup overhead MUST stay under the PRD's 150ms budget (no blocking work added before first frame).
</requirements>

## Subtasks
- [ ] 4.1 Add the `Welcome` variant and extend both exhaustive matches.
- [ ] 4.2 Create `src/tui/welcome.rs`: wordmark (Sextant vs Quadrant `PixelSize` prototyped, pick by eye), facts box builder, width ladder.
- [ ] 4.3 Add `tui-big-text = "0.7"` to `Cargo.toml`.
- [ ] 4.4 Inject the welcome item at startup; honor `config.ui.hide_banner` and no-color caps.
- [ ] 4.5 Replace both skill-loading call sites; move reload feedback to `status_message`.
- [ ] 4.6 Remove/retire the `renders_skill_loading_state` test (:2335) and add welcome render tests at the three breakpoints.
- [ ] 4.7 Audit `ChatProjection` consumers for the new kind (verified safe — `items()`/`upsert()` don't filter by kind; confirm at integration).

## Implementation Details

The "No chat yet." empty state (`src/tui/mod.rs:1415`) becomes unreachable in practice once the welcome item exists; leave the branch as a safety net. Injection options analyzed in exploration — prefer injecting via the projection/`sync_chat_items` path so the item survives re-syncs (a prepend in `sync_chat_items` (:3684) or a startup `upsert` both work; pick the one that keeps the item stable across projection updates). See TechSpec "Implementation Design" and ADR-005 for behavior.

### Relevant Files
- `src/app/chat/mod.rs` — `ChatItemKind` (:26-39), `slug()` exhaustive match (:224-237).
- `src/tui/mod.rs` — `chat_kind_label()` (:1801-1814), `chat_item_lines` dispatch (:1662-1703), `render_skill_loading` (:1257-1283) + call sites (:240, :298), "No chat yet." (:1399-1415).
- `src/app/mod.rs` — `new_with_debug` init flow (:682-731), `chat_items` (:82, :701), `sync_chat_items` (:3679-3685).
- `src/app/chat/projection.rs` — `items()` (:218-220), `upsert()` (:1230-1265) — no kind filtering (verified).
- `src/tui/welcome.rs` — new renderer.
- `Cargo.toml` — dependency addition (:12-29).

### Dependent Files
- `src/tui/mod.rs` test module — `renders_skill_loading_state` (:2335) retires; new breakpoint tests added.
- `src/app/git.rs` (task_05) — supplies `AppState.git_context` the facts box reads.
- `src/config/mod.rs` — `ui.hide_banner` from task_01.

### Related ADRs
- [ADR-005: Welcome Screen as Synthetic Chat Item](../adrs/adr-005.md) — the mechanism this task implements.
- [ADR-004: Theme Module Architecture](../adrs/adr-004.md) — `tui-big-text` isolation.
- [ADR-002: Unified Single-Release Rollout](../adrs/adr-002.md) — phase 1 boundary.

## Deliverables
- `ChatItemKind::Welcome` + renderer + startup injection; loading screen removed.
- `tui-big-text 0.7` confined to `welcome.rs`.
- Unit tests with 80%+ coverage of width-ladder and facts-box logic **(REQUIRED)**
- Integration tests for startup render at three breakpoints **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Width ladder selection: 100 cols → full wordmark, 70 → compact, 50 → plain text line containing "Atelier".
  - [ ] Facts box includes version string equal to `CARGO_PKG_VERSION`, agent count matching a 3-agent state, and preset name when set.
  - [ ] Facts box omits the repo+branch line when `git_context` is `None`; includes "repo · branch" when `Some`.
  - [ ] `hide_banner = true` → no wordmark lines; facts box still renders.
  - [ ] No-color theme → no wordmark; facts content identical as plain text.
- Integration tests:
  - [ ] `render_to_text` at 80x24 on a fresh state shows wordmark + facts above the input composer; "No chat yet." absent.
  - [ ] After appending a user-prompt chat item, scrolling to top still shows the welcome content (scrollback persistence).
  - [ ] `/reload:skills` flow sets a `status_message` instead of drawing the loading screen.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Welcome renders correctly at 80/60/40 columns (PRD success metric).
- Source-invariant test still green (no new literals).
- Measured startup-to-first-frame delta < 150ms versus pre-change baseline.
