# Session Browser & Transcript Resume

## Overview

Every `atelier` launch starts a fresh in-memory session. Past sessions are durably persisted as event-sourced JSONL logs under `.atelier/sessions/<id>/`, but there is no in-TUI way to browse, reopen, or continue them — so a crash, a quit, or a context switch silently loses the working thread. The whole-log fold designed for exactly this (`ChatProjection::rebuild`) exists but is exercised only in tests; the event-sourcing spine the system is built on is **never replayed in production**.

This feature is a modal **session browser** (newest-first list with goal preview, timestamp, and run outcome) plus a **read-only transcript preview** and a **Resume** action that re-adopts a chosen session — continuing it with new prompts in the same durable log. It is for the `atelier` user who just lost a session and wants the thread back, and secondarily for auditing what an agent did across a long run. V1 is deliberately scoped to crash/quit recovery: it builds the irreducible foundation (session open, live rebind, persisted metadata) and the recovery UX, and **defers fuzzy/cross-session search** until resume-rate data earns it.

## Problem

When `atelier` dies mid-task — terminal closed, laptop slept, CLI killed, or a clean quit at the end of the day — the working thread is gone from the UI even though every event was durably written to disk. The user must restart cold: re-establish the goal, the roster, and the context the agent had accumulated. Research on interruptions puts the cost of fully regaining focus at roughly **23 minutes**; for a long agent run that had read dozens of files and made a sequence of decisions, restarting from zero is worse than an interruption — it discards accumulated context the system still has on disk.

The deeper problem is architectural: `atelier` is an event-sourced system whose chat is *derived* by projecting the event log, yet it only ever projects *forward*, incrementally, on a log it just created. It never folds a stored log back. So the durable history — the single most valuable asset the system produces — is write-only in practice. A crashed session's log frequently ends with a **dangling run** (a step in flight, or a pending clarification) that no live code path will ever read again.

Current workarounds are poor: the user re-types the goal and hopes the agent re-derives context, or manually `cat`s the JSONL to remember what happened. Neither continues the session; both lose the thread.

### Market Data

- **84%** of developers use or plan to use AI coding tools (Stack Overflow 2025, up from 76%), but trust *fell* to **29%** (−11 pts YoY) — reliability and recovery features directly address the trust gap.
- An in-TUI **session picker is now table stakes**: Claude Code (`--resume`/`/resume`), Gemini CLI ("Session Browser"), OpenAI Codex CLI (`codex resume`), Cursor, and Goose all ship session resume. Its absence is a competitive leak.
- **Native fuzzy/content search inside the picker is still rare** — only Gemini CLI clearly ships it; for Claude Code a whole third-party ecosystem (agent-sessions, agent-deck, CASS) sprang up to fill the gap.
- Regaining focus after an interruption costs **~23 min** (UC Irvine); Claude Code adoption reached **~18%** of developers by Jan 2026, a **6×** jump in 2025 — the category whose sessions atelier resumes is growing fast.

## Summary / Differentiator

The field has solved "resume the last conversation," but two weaknesses map directly to atelier's architecture. Competitors persist the *message transcript* while their live state decouples on crash (documented Cursor/Codex/Claude Code bugs). Because atelier records **every** lifecycle step as an event and derives chat by folding that log, resume is a deterministic replay of the same durable events — so it can faithfully restore a run that was killed mid-step, and the resumed view and the resumed agent state come from **one source**, eliminating the transcript↔UI desync that is the #1 resume bug elsewhere. Showing **run outcome** per session in the picker (others show message counts, not terminal state) is itself a small differentiator.

## Core Features

| #   | Feature | Priority | Description |
| --- | --- | --- | --- |
| F1 | Session picker modal | Critical | Newest-first list (goal preview, timestamp, run outcome). Loaded **off-thread** via the existing poller pattern; a case-insensitive substring narrow over the visible list. Slots into the top of the TUI key-routing precedence cascade. |
| F2 | Read-only transcript preview | Critical | Folds the selected session via `ChatProjection::rebuild` into a read-only chat view. **Sanitizes** control/ANSI sequences. Doubles as the confirmation gate before committing the mutating Resume. |
| F3 | Resume / session adoption | Critical | Adds `HistoryStore::open(root, session_id)`; builds a fully-replayed typed session-state value off-thread and **swaps it atomically** with one watch broadcast; re-seeds `session_goal`/`run_state`; appends in place; opens **Idle** awaiting a new prompt (no auto re-execution). |
| F4 | Lifecycle events on resume | High | Writes a terminal `run_interrupted` event for any dangling run and a `session_resumed` boundary event (timestamp, cwd, git HEAD+dirty, capability context, pre-resume tail hash). Keeps replay correct and forensic. |
| F5 | Resume safety model | High | Drift interlock at the **first mutating action** (on cwd/HEAD change, not dirtiness); resume defaults to restrictive `normal` approval mode; no replayed approvals; `0700/0600` file perms. |
| F6 | Persisted session metadata | High | Writes `goal` + `outcome` into `metadata.json` at the existing goal-set / run-terminal points; lazy fold-then-write-back for legacy sessions on first browse (self-healing, no migration). Avoids O(sessions×events) list rendering. |
| F7 | Resume-rate instrumentation | Medium | Records resume events + resumed-session completion, to prove the anchor and earn the deferred fuzzy/search scope with data. |

## KPIs

| KPI | Target | How to Measure |
| --- | --- | --- |
| Crash-recovery adoption — of sessions ending in a dangling (non-terminal) run, share reopened/resumed within 7 days | > 40% | Sessions whose last event is an in-flight run that later get a `session_resumed` event ÷ all such sessions |
| Time-to-continue — median from app launch to first new prompt into a resumed session | < 20 s | Timestamp delta `app_start` → first resumed `prompt_submitted` |
| Picker open latency — render the list for a 200-session history (off-thread) | < 200 ms p95 | Instrument modal-open → first paint; bench against synthetic history |
| Transcript fidelity — resumed/browsed transcripts that exactly match a full rebuild fold | 100% (zero desync) | Property test asserting projection equality across resume flows |
| Resumed-session completion (guard) — resumed sessions that reach a terminal `Completed` run | > 60% | Resumed sessions reaching `Completed` ÷ all resumed sessions (confirms append-in-place doesn't produce confused/abandoned threads) |

## Feature Assessment

| Criteria | Question | Score |
| --- | --- | --- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Strong |
| **Frequency** | How often would users encounter this value? | Strong |
| **Differentiation** | Does this set us apart or just match competitors? | Maybe |
| **Defensibility** | Is this easy to copy or does it compound over time? | Strong |
| **Feasibility** | Can we actually build this? | Strong |

Leverage type: **Compounding Feature** — it activates the event-sourcing spine the whole system is built on; value grows with every recorded session.

## Council Insights

- **Recommended approach:** Ship the de-risked core — newest-first list + read-only preview + Resume + the full safety model — and defer fuzzy/cross-session search until resume-rate data justifies it. Build the irreducible foundation (`open()`, atomic session rebind, persisted goal/outcome metadata) regardless of scope.
- **Key trade-offs:** Read-only preview stays in V1 (it's the confirmation gate for the irreversible Resume swap and is the same fold built anyway); fuzzy search leaves V1 (separate archival hypothesis + a searchable index over potentially-sensitive prompts). Drift safety is bought as a *conditional* interlock at the first mutating action, not friction on every resume.
- **Risks identified:**
  - Activating the production fold makes backward replay-compatibility a **maintained contract** — every event variant must stay foldable or be upcast (validate at `open()`, fail loud). *Mitigation:* ADR-003 + upcasting fixtures + production test coverage for `rebuild`.
  - Append-in-place can make replay lie unless lifecycle transitions are written as events. *Mitigation:* terminal `run_interrupted` + `session_resumed` events (ADR-002).
  - **Stale workspace state** on resume → silent repo corruption. *Mitigation:* drift interlock + default-`normal` caps (ADR-004).
  - Replay is O(total history) → log bloat on long sessions. *Mitigation:* snapshot/compaction named as a future ADR.
- **Stretch goal (V2+):** "Branch / fork from any step" — fork a new session from any prior point in a transcript and re-run forward with edits (the move only atelier's event-sourced architecture enables cheaply; also closes the "keep-going vs lands-at-Idle" gap).

## Integration with Existing Features

| Integration Point | How |
| --- | --- |
| `history::list_session_event_paths` / `read_events_from_path` | Enumerate + read session logs for the picker and the fold |
| `ChatProjection::rebuild` | Promoted from test-only to the production fold powering preview + resume |
| `HistoryStore` | Add `open(root, session_id)` (validates `schema_version`) alongside `create()` |
| `App` session lifecycle + one-active-run guard | Atomic typed session-state swap; resume gated by the existing guard |
| TUI key-routing precedence + `AppWorkerCommand` poller pattern | New modal at the top of the cascade; off-thread load via `tokio::select!` + watch channel |
| `SessionMetadata` (`metadata.json`) | Extend with `goal` + `outcome` as a derived, self-healing cache |
| Approval surface + `theme.rs` tokens | Drift acknowledgment folded into the approval prompt; modal uses semantic theme tokens |

## Out of Scope (V1)

- **Fuzzy / cross-session search** — deferred to V2 (the "cross-session search & audit" opportunity); serves a separate archival hypothesis and adds an index over sensitive prompts. Earned by resume-rate data.
- **Auto re-execution of the interrupted step** — V1 reconciles to Idle; auto-resuming a dangling run risks re-running side-effecting actions. Branch-from-step (V2) is the safe path to true continuation.
- **Branch / fork from an arbitrary step** — long-term stretch; requires fork semantics, not the append-in-place model V1 commits to.
- **Snapshot / compaction of long logs** — replay is O(history); deferred until open-latency is UX-visible.
- **Redaction-at-rest of secrets/PII** in stored transcripts — accepted V1 risk with `0700/0600` perms; revisit trigger is the first shared-host / non-solo deployment.
- **Cross-machine / remote session sync** — local-only.

## Architecture Decision Records

- [ADR-001: V1 Scope — read-only preview in, fuzzy search deferred](adrs/adr-001.md) — V1 = list + preview + Resume; fuzzy/cross-session search → data-earned V2.
- [ADR-002: Append-in-place resume with explicit lifecycle events](adrs/adr-002.md) — continue the same log; write terminal `run_interrupted` + `session_resumed`; metadata is a derived cache.
- [ADR-003: Production replay fold as a maintained schema-compatibility contract](adrs/adr-003.md) — promote `rebuild`, validate at `open()`, atomic off-thread session swap.
- [ADR-004: Resume safety model](adrs/adr-004.md) — drift interlock at first mutation, default-`normal` capability re-consent, untrusted-transcript rendering, file perms.

## Open Questions

- **Session-boundary enforcement (unresolved T3):** named `active_session` handle threaded through `App`, or a grouped session-state struct guarded by a compile/test-time exhaustiveness check (in the spirit of `colors_live_only_in_theme_module`)? — TechSpec decision.
- **"Keep going" expectation (Devil's Advocate):** is restoring *context* (transcript + goal + roster, landing at Idle) enough for the anchor, or does V1 need a one-line "here's where you left off" summary at the top of the resumed thread? Validate with resume-rate + completion data.
- **Resume-rate baseline is unknown** — needs crash/quit telemetry to confirm the anchor's frequency justifies the investment.
- **List ordering for moderate N** — is plain substring narrow sufficient, or is light fuzzy needed before the V2 search work? (Leaning substring.)
- **Upcasting fixtures** — which historical event variants already exist on disk and need replay/upcast test fixtures?
- **"Run outcome" display** — how to label sessions that ended mid-run (e.g. `Interrupted` vs `In progress (recovered)`).
