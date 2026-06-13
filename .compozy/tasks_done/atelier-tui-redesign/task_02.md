---
status: completed
title: "Create theme module: caps detection, tokens, resolution, agent accents"
type: backend
complexity: medium
dependencies: []
---

# Task 2: Create theme module: caps detection, tokens, resolution, agent accents

## Overview

Create `src/tui/theme.rs`: the single source of color truth. It defines semantic tokens derived from the web palette (`web/src/styles/global.css`), detects terminal capabilities (`NO_COLOR`, `COLORTERM`), and resolves each token to RGB, a hand-picked ANSI-256 index, or monochrome at startup — so render code never branches on capabilities (ADR-004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. The `Theme` struct MUST expose the semantic tokens defined in TechSpec "Core Interfaces" (text, text_muted, text_dim, border, border_focused, accent, status_ok, status_warn, status_error, user_prompt_bg, agent_accents).
2. Token values MUST derive from the web palette hex values (ADR-003): text #f2ead8, muted #b7ae9e, dim #7e867a, green #8cffb0, amber #ffb454, cyan #79e2e1, red #ff6f61.
3. `TerminalCaps::detect()` MUST treat any non-empty `NO_COLOR` as no-color mode and `COLORTERM in {truecolor, 24bit}` as truecolor; detection MUST be injectable (pure function over env values) for testing.
4. `Theme::resolve(caps)` MUST produce: RGB values under truecolor; hand-picked `Color::Indexed` fallbacks under 256-color; terminal-default/monochrome styling under no-color.
5. `accent_for(index)` MUST cycle the `agent_accents` pool deterministically (index % pool length) and MUST NOT include `status_error` red in the pool (ADR-006).
6. The existing `USER_EVENT_BG` const value (`Color::Rgb(18, 52, 71)`, `src/tui/mod.rs:39`) MUST move into the theme as `user_prompt_bg` (replacement value may differ per brand palette).
7. No NO_COLOR/COLORTERM handling exists anywhere today (verified) — this module introduces it; nothing else may read those vars.
</requirements>

## Subtasks
- [x] 2.1 Create `src/tui/theme.rs` with the `Theme` struct, serde-ready, and the web-palette token table (RGB + chosen 256-index + mono treatment per token).
- [x] 2.2 Implement `TerminalCaps` with a pure resolver over provided env values plus a `detect()` wrapper reading the process env.
- [x] 2.3 Implement `Theme::resolve(caps)` covering the three capability tiers.
- [x] 2.4 Implement `accent_for(index)` round-robin over the agent accent pool.
- [x] 2.5 Declare `pub mod theme;` in `src/tui/mod.rs` (after the use block, ~line 38).
- [x] 2.6 Unit-test resolution per tier and accent cycling.

## Implementation Details

New file plus a one-line module declaration. Follow the module-declaration pattern of `src/app/chat/mod.rs:1-3`. Env reading mimics `env::var_os` patterns (`src/cli.rs:96`). Pure-function test style mimics `src/app/chat/diff_preview.rs:107-132`. See TechSpec "Core Interfaces" for the exact struct shape — do not duplicate it here.

### Relevant Files
- `src/tui/theme.rs` — new module (all logic lives here).
- `src/tui/mod.rs` — `mod theme;` declaration (~:38); `USER_EVENT_BG` const at :39 is the value being absorbed (actual call-site migration happens in task_03).
- `web/src/styles/global.css` — canonical palette hex values (read-only reference, ADR-003).
- `src/app/chat/diff_preview.rs` — pure-function unit test pattern to mimic (:107-132).

### Dependent Files
- `src/tui/mod.rs` — task_03 threads the resolved `Theme` through `TuiUiState`; this task only adds the module.
- `src/lib.rs` — no re-export needed (`tui` consumers are internal; verified zero external imports).

### Related ADRs
- [ADR-003: Web Palette as Canonical Brand Source](../adrs/adr-003.md) — token values.
- [ADR-004: Theme Module Architecture](../adrs/adr-004.md) — resolve-at-startup, definition/resolution separation.
- [ADR-006: Polled Git Context and Round-Robin Agent Accents](../adrs/adr-006.md) — accent pool semantics.

## Deliverables
- `src/tui/theme.rs` with `Theme`, `TerminalCaps`, `resolve`, `accent_for`.
- Module declaration in `src/tui/mod.rs`.
- Unit tests with 80%+ coverage of resolution and cycling logic **(REQUIRED)**
- Integration smoke: theme constructible in a TestBackend test context **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Caps from `NO_COLOR=""`/unset + `COLORTERM=truecolor` → truecolor=true, no_color=false.
  - [x] Caps from `NO_COLOR=1` → no_color=true regardless of COLORTERM.
  - [x] `resolve` under truecolor returns `Color::Rgb` for `text`, `accent`, `status_warn` with the web hex values.
  - [x] `resolve` under 256-color returns `Color::Indexed` for every token (no `Rgb` leaks).
  - [x] `resolve` under no_color returns no RGB/Indexed brand colors (terminal-default/mono styles only).
  - [x] `accent_for(0..2*pool)` cycles: adjacent indices distinct, index `i` == index `i + pool_len`.
  - [x] Red (`status_error` value) does not appear in `agent_accents`.
- Integration tests:
  - [x] A `Theme::resolve(TerminalCaps{..})` value can style a minimal TestBackend render without panicking.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `cargo clippy` clean on the new module.
- No other module reads `NO_COLOR`/`COLORTERM` (grep-verifiable).
