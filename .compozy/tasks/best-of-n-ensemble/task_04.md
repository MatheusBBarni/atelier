---
status: pending
title: "Verdict ledger and task-type signature"
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 04: Verdict ledger and task-type signature

## Overview
Persist every attempt's oracle outcome to a durable, cross-session ledger and derive the coarse task-type signature that buckets win-rates. The ledger is the data engine for routing (Task 05) and the read-back (Task 12).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST append one record per attempt to `.atelier/race/verdicts.jsonl` with the fields in TechSpec "Data Models" (`schema_version`, `timestamp`, `run_id`, `task_signature`, `runtime`, `model`, `oracle_outcome`, `exit_code`, `won`, `cost_tokens`).
- The ledger MUST be append-only and cross-session (independent of per-session history compaction).
- MUST derive `TaskSignature` deterministically from the instruction + target files, producing a coarse, versioned `{ sig_version, primary_language, change_kind }`.
- MUST expose a read/aggregate API returning per-`(task_signature, runtime)` win-rate counts for consumers.
- The ledger writer MUST tolerate a missing file (first write creates it) and corrupt trailing lines (skip, don't crash).
</requirements>

## Subtasks
- [ ] 4.1 Create the ledger module: record type + append writer at the workspace `.atelier/race/` root.
- [ ] 4.2 Implement deterministic `TaskSignature` derivation (language + change-kind classification).
- [ ] 4.3 Implement the read/aggregate API (win-rate by signature+runtime).
- [ ] 4.4 Handle missing-file and malformed-line cases gracefully.
- [ ] 4.5 Add tests for write/read round-trip, signature determinism, and aggregation.

## Implementation Details
Place the ledger in a dedicated module (see TechSpec "System Architecture"; module convention is `working_directory.join(".atelier")` per the history module). Do not reuse per-session `read_events` for aggregation — the ledger is the cross-session source of truth (ADR-007). Keep `change_kind` a small closed set so buckets accumulate samples.

### Relevant Files
- `src/history/mod.rs:262`,`362`,`417` — `.atelier` root + `append_event`/`read_events` as the JSONL persistence pattern to mirror (not reuse for aggregation).
- `src/orchestrator/mod.rs:369` — `GraderVerdict` (source of `oracle_outcome`/`exit_code`).
- `src/orchestrator/mod.rs` (Task 02 types) — `TaskSignature`/`ChangeKind`.

### Dependent Files
- Task 05 — consumes the aggregate API for routing.
- Task 07 — writes a ledger record per graded attempt.
- Task 12 — renders the aggregate in `/provider:status`.

### Related ADRs
- [ADR-007: Dedicated Verdict Ledger + Coarse Task-Type Signature](../adrs/adr-007.md) — storage + signature.
- [ADR-002: Race Is the Data Engine](../adrs/adr-002.md) — record verdicts from day one.

## Deliverables
- A verdict-ledger module (record, append writer, aggregate reader).
- Deterministic, versioned `TaskSignature` derivation.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for cross-session ledger persistence **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Appending two records then reading returns both in order.
  - [ ] The same instruction + files yields an identical `TaskSignature` across calls (determinism).
  - [ ] A Rust async-fn change classifies as `{primary_language: rust, change_kind: feature}` (or the defined bucket) and a `*_test.rs` change as `change_kind: test`.
  - [ ] Aggregation over mixed records returns correct per-`(signature, runtime)` win-rate.
  - [ ] A malformed trailing line is skipped without error.
- Integration tests:
  - [ ] Records written in one session are readable by an aggregate call in a later session (separate `App` instance, same workspace).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The ledger persists across sessions and aggregates to correct win-rates.
- Signature derivation is deterministic and versioned.
