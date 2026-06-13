# Idea: Prompt History — Per-Project ↑/↓ Recall

## Overview

`atelier` routes every prompt through the orchestrator from a single TUI input
box, yet the box has no memory. Developers re-run failed steps and tweak long,
multi-clause goals constantly — and today that means retyping. **Prompt History**
brings shell-style recall to the input box: press ↑/↓ to walk through prompts you
previously submitted *in this project*, persisted across restarts.

It serves the developer driving atelier daily (primary), the developer returning
to a repo after days, and anyone whose terminal muscle memory expects ↑ to "just
work." V1 is a deliberate **Quick Win**: because prompts are already persisted as
events, recall is a read-only projection over data the app already owns — roughly
two UI-state fields, a key-routing branch, and one background load. The ambition
is staged: ship the recall wedge now, then compound it into **outcome-aware
recall** (V2) — the differentiator only atelier's event sourcing can deliver.

## Summary / Differentiator

Persisted, per-working-directory ↑/↓ history is table stakes (Claude Code, Aider,
Codex). atelier's edge is two-fold: **per-project by default** (Codex is
global-only, with per-project an open, unshipped request), and a committed path
to **prompt→outcome metadata** — recall that shows "this prompt edited 3 files,
cost $0.04, was interrupted." No AI coding CLI ties a recalled prompt to its
consequence; atelier already records all of it via the originating `run_id`, so
the differentiator is a join away, not a new subsystem.

## Problem

The input box is the most-used surface in atelier, and it has no memory — every
prompt is typed from scratch. Two moments dominate the friction:

1. **Re-run after failure.** A step errors or routes slightly wrong; the user
   wants the *same* multi-clause goal back, verbatim, to resubmit or tweak one
   clause. Orchestrator prompts are long — re-keying 200 characters by hand is
   slow and error-prone.
2. **Returning to a project.** After days away, the user has lost the thread of
   what they were driving the harness to do. A quick ↑ through recent prompts is
   the cheapest memory jog.

The current workaround is scrolling terminal scrollback and retyping. ↑-recall is
a 40-year reflex every shell honors; its absence is a papercut taken many times
per session.

### Market Data

- **Table stakes:** persisted per-working-dir history with ↑/↓ **and** Ctrl-R
  ships in Claude Code, Aider, Codex CLI. Gemini CLI is in-memory only (lost on
  restart) — the laggard.
- **Per-project gap:** Codex CLI is global-only; per-project scoping is open and
  unshipped (#21202, #12627). Claude Code stores per-working-directory.
- **#1 shipped bug** across competitors: the multi-line ↑/↓ collision — ↑ jumps
  to history instead of moving the cursor through a wrapped prompt (Claude Code
  #20328/#63670, Codex #21833, Gemini #4220, Warp #11600).
- **Security:** secrets in history files is cataloged as MITRE ATT&CK T1552.003;
  AI prompts are a worse credential sink than shell commands.
- **Demand signal:** no rigorous prompt-reuse statistic exists; fzf / Atuin /
  McFly popularity is the strongest evidence the need is felt. Measure atelier's
  own telemetry rather than citing the uncited "80–90% repeats" anecdote.

## Core Features

| #  | Feature | Priority | Description |
| -- | ------- | -------- | ----------- |
| F1 | ↑/↓ recall | Critical | At the first/last visual row with no dropdown/queue focus, ↑ steps to the previous submitted prompt and ↓ to the next, replacing the input buffer with the recalled text. |
| F2 | Collision-safe gating | Critical | History fires only at the soft-wrap boundary from `move_input_cursor_vertically`; multi-line/wrapped editing keeps cursor nav. Directly avoids the #1 competitor bug. |
| F3 | Draft preservation | Critical | An in-progress draft is saved on first browse and restored when ↓ steps past the newest entry (readline "saved line"). No data loss. |
| F4 | Per-project projection | High | Recall is a read-only projection over `cwd/.multiagent/sessions/*` `prompt_submitted` events — newest-first, no new on-disk format. |
| F5 | Consecutive-dedup + size cap | High | Collapse identical consecutive entries; bound the in-memory ring via `UiConfig.prompt_history_max` (default 200). |
| F6 | Leading-space-skip | Medium | A prompt starting with a space is not surfaced in recall — the familiar shell escape hatch for secrets. |
| F7 | Recall instrumentation | Medium | Tag submissions `source: recalled\|fresh` and log recall depth, so the Ctrl-R widen-trigger is data-driven. |

## KPIs

| KPI | Target | How to Measure |
| --- | ------ | -------------- |
| Recall adoption | > 20% of submitted prompts originate from a recalled entry | Tag `prompt_submitted` with `source`; ratio over events |
| Repeat-prompt recall | > 30% of exact-repeat prompts submitted via recall vs retyped | Detect exact prior-match prompts in event stream, check `source` |
| History load latency | Non-blocking; available < 300ms for ≤ 500 entries | Instrument the background load |
| Recall interaction latency | < 16ms (one frame) per ↑/↓ step | Benchmark the cursor/history step |
| Multi-line collision defects | 0 | Test matrix + issue tracking |

## Feature Assessment

| Criteria | Question | Score |
| -------- | -------- | ----- |
| **Impact** | How much more valuable does this make the product? | Strong |
| **Reach** | What % of users would this affect? | Must do |
| **Frequency** | How often would users encounter this value? | Must do |
| **Differentiation** | Does this set us apart or just match competitors? | Maybe (V1) → Must do (with V2) |
| **Defensibility** | Is this easy to copy or does it compound over time? | Maybe (V1) → Strong (V2) |
| **Feasibility** | Can we actually build this? | Must do |

Leverage type: **Quick Win** (V1) compounding into a **Compounding Feature**
(V2 outcome-aware recall).

## Council Insights

- **Recommended approach:** ↑/↓ recall as a read-only projection over the event
  log (ADR-001, option A), per-project, newest-first, with draft-preservation and
  a soft-wrap collision gate. Frame to users as "recall recent prompts," not
  "searchable history."
- **Key trade-offs:** ↑/↓-only sits below the table-stakes bar (no search) until
  the fast-follow — accepted because ↑/↓ is the unavoidable substrate for search
  and owns the highest-frequency moment; Ctrl-R later is a filter over the same
  in-memory `Vec`, no rework. Per-project trades a fresh-repo "empty well" for
  relevance — neutralized by a scope-cycle fast-follow, not by defaulting global.
- **Risks identified:** soft-wrap collision (gate + test matrix); draft loss
  (saved line); secrets in recall (leading-space-skip; durable JSONL unchanged,
  already `0o600` + gitignored); wedge-becomes-floor (pre-committed Ctrl-R
  widen-trigger + instrumentation).
- **Stretch goal (V2+):** **Outcome-aware recall** — annotate each recalled
  prompt with its consequence (files changed, cost, interrupted, model) via
  `run_id`. The committed differentiator; V1's projection already enables it.
  Then Ctrl-R reverse-search and cross-project scope-cycling.

## Integration with Existing Features

| Integration Point | How |
| ----------------- | --- |
| Event log (`src/history`, `.multiagent/sessions/*/events.jsonl`) | New read-only primitive `HistoryStore::list_session_event_paths(root)`; fold `prompt_submitted` payloads into the recall ring. No write-path change. |
| TUI key routing (`src/tui/mod.rs`) | History branch in `move_input_cursor`, gated below dropdowns/queue and at the soft-wrap boundary from `move_input_cursor_vertically` (`:1259`). |
| Config (`UiConfig`, `src/config/mod.rs:201`) | Add `prompt_history_max` (+ `RawUiConfig` + merge default) next to `hide_banner`. |
| Event sourcing / chat projection | Recall is a projection; reuses "render derives from events." Enables the V2 `run_id`→outcome join. |

## Out of Scope (V1)

- **Ctrl-R reverse-search** — the "find last week's prompt" job; deferred behind a
  pre-committed widen-trigger. A filter over the same `Vec`, so deferring costs no
  rework.
- **Browsable history panel/archive** — a separate navigable modal with its own
  key-routing tier; multiplies the diff for long-tail value.
- **Outcome-aware recall (the V2 differentiator)** — committed but staged after
  the wedge ships and the substrate is proven.
- **Cross-project scope-cycling** — fast-follow to neutralize the per-project
  "empty well"; needs an empty-state affordance, not a V1 blocker.
- **Secret redaction-on-write** — lossy and a persistence-layer concern; the
  prompt is already in the event log unredacted today, so recall adds no new
  at-rest exposure. Leading-space-skip covers the cheap case.

## Architecture Decision Records

- [ADR-001: V1 Prompt History as Per-Project ↑/↓ Recall Projected from the Event Log](adrs/adr-001.md)
  — recall is a read-only projection over the existing append-only event log (not
  a new file); per-project default; newest-first; collision-gated; draft-preserving;
  leading-space-skip.

## Open Questions

- **Ctrl-R widen-trigger threshold** — the governance commitment from the council
  dissent: what measured value flips the decision to build search? (Candidate:
  reuse-rate > X% or median recall-depth > N.) Needs a number before the wedge ships.
- **Cross-session ordering** — newest-first is decided; strict timestamp
  interleave vs session-grouped? (Strict timestamp recommended for shell-fidelity.)
- **Slash commands in history** — recall `/goal`, `/config`, …, or exclude like
  Codex excludes accepted slash-commands? (Leaning exclude — normal prompts only.)
- **`/clear`-style boundary** — if a session-clear affordance is ever added, it
  must not wipe input recall (Gemini regressed here, #14171). Decouple by design.
- **Empty-state affordance copy** — exact hint on the first empty ↑ in a fresh
  project, teaching the future cross-scope path.
