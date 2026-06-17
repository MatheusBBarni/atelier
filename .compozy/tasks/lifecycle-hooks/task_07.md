---
status: completed
title: "`atelier --events follow` CLI"
type: backend
complexity: medium
dependencies:
  - task_01
---

# Task 7: `atelier --events follow` CLI

## Overview
Add a standalone `atelier --events follow` mode that tails the active session's on-disk event log and prints the normalized hook payload for each public event — reusing the same `normalize()` the live tap uses, so it is a faithful preview of exactly what a hook receives on stdin. This is the dry-run/test harness that drives hook adoption.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add an `--events <follow>` flag to the `Cli` struct using the existing clap `ValueEnum` pattern (as `--codemap` does) and dispatch it in `run_cli_with` as a standalone mode that does not start the TUI.
- MUST locate the latest/active session's `events.jsonl`, tail it, and parse events line-by-line via the existing history reader.
- MUST emit one normalized `HookPayload` JSON line per event by reusing `normalize()`, reconstructing `ActorCtx` by folding the session's own run/step events (the on-disk events do not carry agent/runtime directly).
- MUST skip events outside the public vocabulary (so the stream matches what hooks see).
- SHOULD exit cleanly on Ctrl-C.
</requirements>

## Subtasks
- [x] 7.1 Add the `--events` `ValueEnum` flag and `run_cli_with` dispatch branch.
- [x] 7.2 Locate the latest session log and tail it line-by-line.
- [x] 7.3 Fold run/step events into an `ActorCtx` map (step → agent → runtime).
- [x] 7.4 Emit normalized JSON per public event; skip non-public kinds.
- [x] 7.5 Add unit/integration tests over a fixture session log.

## Implementation Details
Edit `src/cli.rs`: add the flag to `Cli` (`:12-54`, mirror `CodemapCommand`) and a dispatch branch in `run_cli_with` (`:61-186`, early-return mode like `--doctor`). Reuse the history reader (the per-line parser around `src/history/mod.rs:272`, and the session enumeration used by `--clean-sessions`) to find and tail the latest session. The actor fold maps `step_id → agent` from `agent_step_started`/`run_started` payloads, then resolves `runtime` from config. Reuse `normalize()` (task_01). Put the reader logic in `src/hooks/` (e.g. `follow.rs`) to keep `cli.rs` thin. See TechSpec "API Endpoints" and "System Architecture → CLI".

### Relevant Files
- `src/cli.rs:12` — `Cli` struct; add `--events` `ValueEnum` flag.
- `src/cli.rs:61` — `run_cli_with`; add the standalone dispatch branch.
- `src/history/mod.rs:272` — per-line event parser + session enumeration to reuse.
- `src/hooks/mod.rs` — `normalize()` + a new `follow` reader (create).

### Dependent Files
- `README.md` — task_09 documents the `--events follow` surface.
- `src/hooks/mod.rs` — shares `normalize()` with the live tap (task_05).

### Related ADRs
- [ADR-004: Normalized payload contract](../adrs/adr-004.md) — one `normalize()` shared by tap and follow; actor reconstruction.

## Deliverables
- An `atelier --events follow` standalone CLI mode emitting normalized payloads from the latest session.
- Actor reconstruction by folding session events.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test: follow over a fixture session log emits the expected normalized JSON lines **(REQUIRED)**

## Tests
- Unit tests:
  - [x] The actor fold derives `runtime` for a `step_started` line from a prior `agent_step_started` event in the same session. — `actor_fold_resolves_runtime_for_a_later_event_at_the_same_step`
  - [x] A `runtime_stream_delta` line is skipped (non-public). — `runtime_stream_delta_line_is_skipped`
  - [x] A pre-agent `run_started` line emits a payload with `actor` null/orchestrator. — `pre_agent_run_started_has_null_actor`
  - [x] `--events follow` combined with an incompatible flag is rejected by the same validation style as existing modes. — `cli::tests::events_conflicts_with_doctor`
- Integration tests:
  - [x] Running follow over a fixture `events.jsonl` prints exactly the expected sequence of normalized JSON lines, matching what the live tap would produce for the same events. — `follow_over_fixture_log_emits_expected_normalized_lines`
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `atelier --events follow` streams normalized payloads identical to hook stdin
- Non-public events are excluded; actor is reconstructed from session events

## As-built notes
- **`src/hooks/follow.rs`** holds the reader; `cli.rs` stays thin. `EventsCommand` (clap `ValueEnum`, kebab-case → `follow`) is the `--events` value; the `run_cli_with` branch runs after config load (config supplies the agent→runtime map) and early-returns without starting the TUI. Incompatible-flag validation mirrors `--codemap`/`--emit-docs`.
- **`project_session_payloads(events, agent_runtimes)`** is the pure core (shared with tests): folds `step_id → agent` from any event carrying an `agent` field, resolves `actor { agent, runtime }` (runtime from config), and calls the shared `normalize(.., Metadata)` — so the stream is byte-for-byte what a metadata hook receives. Non-public kinds are skipped. Output is `redact_sensitive_text`-redacted for stdin parity.
- **Latest session** is the most-recently-*modified* `events.jsonl` (session ids are random, not time-ordered), reusing `list_session_event_paths`/`read_events_from_path`.
- **Ctrl-C:** the tail is a poll loop (250 ms) flushing stdout each pass; Ctrl-C terminates via default SIGINT, the `tail -f` convention. Deliberately avoids tokio's `signal` feature (and its transitive deps) for this read-only tool.
- **Test-flakiness note (not a regression):** under full-suite parallel load the env-sensitive CLI-shelling tests (`runtime::{codex,cursor,claude}` and the `doctor::` runtime-availability tests) can flake; they pass 119/119 and 21/21 respectively in isolation. Unrelated to this task's code.
