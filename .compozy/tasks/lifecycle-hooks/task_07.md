---
status: pending
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
- [ ] 7.1 Add the `--events` `ValueEnum` flag and `run_cli_with` dispatch branch.
- [ ] 7.2 Locate the latest session log and tail it line-by-line.
- [ ] 7.3 Fold run/step events into an `ActorCtx` map (step → agent → runtime).
- [ ] 7.4 Emit normalized JSON per public event; skip non-public kinds.
- [ ] 7.5 Add unit/integration tests over a fixture session log.

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
  - [ ] The actor fold derives `runtime` for a `step_started` line from a prior `agent_step_started` event in the same session.
  - [ ] A `runtime_stream_delta` line is skipped (non-public).
  - [ ] A pre-agent `run_started` line emits a payload with `actor` null/orchestrator.
  - [ ] `--events follow` combined with an incompatible flag is rejected by the same validation style as existing modes.
- Integration tests:
  - [ ] Running follow over a fixture `events.jsonl` prints exactly the expected sequence of normalized JSON lines, matching what the live tap would produce for the same events.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `atelier --events follow` streams normalized payloads identical to hook stdin
- Non-public events are excluded; actor is reconstructed from session events
