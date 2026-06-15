# Plan 010: Retire Legacy Chat Event Rendering

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in "STOP conditions" occurs, stop and report. When done, update this plan's status row in `plans/README.md` unless a reviewer told you they maintain the index.
>
> **Drift check (run first)**: `rtk git diff --stat cf40d98..HEAD -- src/app/mod.rs src/app/chat/projection.rs src/tui/mod.rs`
> If any in-scope file changed since this plan was written, compare "Current state" against live code before proceeding. On mismatch, stop and report.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `cf40d98`, 2026-06-14

## Why This Matters

The architecture says chat rendering should be derived from `ChatProjection`, not raw app internals. The app still carries `state.events`, pushes display strings for every event, and the TUI falls back to rendering those strings through `legacy_chat_line` when `chat_items` is empty. That leaves two visible surfaces for one lifecycle and lets tests pass against legacy strings while projection regressions break the real chat.

## Current State

Architecture guidance:

```text
CLAUDE.md:42 Rendering reads no app internals... chat transcript is derived by `src/app/chat/projection.rs`
```

State and recording:

```text
src/app/mod.rs:130 pub chat_items: Vec<ChatItemView>,
src/app/mod.rs:132 pub events: Vec<String>,
src/app/mod.rs:4236 self.chat_projection.apply_history_event(&event);
src/app/mod.rs:4237 self.sync_chat_items();
src/app/mod.rs:4238 self.state.events.push(display.into());
```

TUI fallback:

```text
src/tui/mod.rs:2756 let event_lines = if !state.chat_items.is_empty() {
src/tui/mod.rs:2787 } else if state.events.is_empty() {
src/tui/mod.rs:2790     state.events.iter().map(|event| legacy_chat_line(&theme, event))
src/tui/mod.rs:3965 fn legacy_chat_line<'a>(theme: &Theme, event: &'a str) -> Line<'a> {
```

Repo conventions:
- To make a feature appear in chat, emit events and extend `src/app/chat/projection.rs`.
- TUI render path should be a pure function of `(AppState, TuiUiState)`.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Projection tests | `rtk cargo test --lib chat` | chat/projection tests pass |
| TUI tests | `rtk cargo test --lib tui` | TUI tests pass |
| Full Rust gate | `rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked` | exit 0 |

## Scope

**In scope**:
- `src/app/mod.rs`
- `src/app/chat/projection.rs`
- `src/tui/mod.rs`

**Out of scope**:
- Do not redesign `ChatItemView`.
- Do not remove durable `HistoryEvent` persistence.
- Do not change run lifecycle semantics.

## Git Workflow

- Branch: `advisor/010-retire-legacy-chat-rendering`
- Commit message example: `refactor(tui): render chat only from projection`
- Do not push or open a PR unless instructed.

## Steps

### Step 1: Add Projection-First Characterization Tests

Before removing fallback rendering, add or strengthen tests proving `chat_items` covers:
- user prompt,
- action requested/completed,
- command result,
- approval requested/resolved,
- provider status,
- clarification answered,
- run completion.

Use existing tests that already inspect `app.state.chat_items` as patterns. Avoid adding assertions against `state.events`.

**Verify**: `rtk cargo test --lib chat` -> projection tests pass.

### Step 2: Stop Rendering `state.events` In TUI

In `src/tui/mod.rs`, remove the fallback branch that maps `state.events` through `legacy_chat_line`. If `chat_items` is empty and there is no pending approval, render `No chat yet.`.

Remove `legacy_chat_line` and tests that exist only for it.

**Verify**: `rtk cargo test --lib tui` -> TUI tests pass or show only legacy-string assertion failures to update.

### Step 3: Quarantine Or Remove `AppState.events`

Prefer removing `events` from `AppState` if tests and callers no longer need it. If removing it is too broad for this plan, keep it as a non-rendered debug/event-log compatibility field and add a comment that TUI must not render it.

Update tests that assert `app.state.events.join(...)` to assert equivalent `chat_items` content or durable history events.

**Verify**: `rtk rg -n "legacy_chat_line|state\\.events|events\\.join" src tests` -> no TUI rendering path remains; remaining `state.events` matches the chosen compatibility decision.

### Step 4: Run The Full Gate

Run:

```bash
rtk cargo fmt --check && rtk cargo clippy --all-targets && rtk cargo test --locked
```

**Verify**: exit 0; all tests pass.

## Test Plan

- Projection-first tests for action, approval, provider, clarification, and completion flows.
- TUI render tests with populated `chat_items`.
- Remove or rewrite legacy-string tests.

## Done Criteria

- [ ] `legacy_chat_line` is removed.
- [ ] TUI does not render `state.events` as chat.
- [ ] Tests assert projected chat items or durable history instead of legacy display strings.
- [ ] Full Rust gate exits 0.
- [ ] `plans/README.md` status row updated.

## STOP Conditions

Stop and report if:
- Removing `state.events` requires a broad public API migration.
- A flow has no `ChatProjection` representation and would disappear from chat.
- More than one lifecycle needs new projection design rather than test updates.

## Maintenance Notes

New user-visible chat behavior should be tested through `ChatProjection` and `ChatItemView`, not raw display strings.

