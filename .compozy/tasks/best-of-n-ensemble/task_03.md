---
status: pending
title: "Attempt isolation overlay and scratch lifecycle"
type: backend
complexity: high
dependencies:
  - task_02
---

# Task 03: Attempt isolation overlay and scratch lifecycle

## Overview
Make N attempts edit the same files safely: redirect each attempt's writes into its own scratch directory while reads fall through to the real tree, and guarantee scratch cleanup on every exit path. This is the medium-risk core of the isolation model and the seam the runner and promotion build on.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- For an `AttemptScope` context, a WRITE MUST resolve into `scratch_dir/<relpath>`; a READ MUST resolve to the scratch copy if present, else the real working tree (copy-on-write overlay).
- `validate_action_scope` MUST allow writes confined to the attempt's `scratch_dir` and MUST deny escapes, fail-closed.
- Scratch directories MUST be created per attempt under `.atelier/race/<run_id>/<attempt_id>/` and removed on win, loss, all-fail, cancel, and panic (RAII-style guard) plus a startup sweep of stale dirs.
- Deletions and renames inside an attempt MUST be represented as overlay tombstones so the later promotion diff (Task 09) is correct.
- MUST NOT change behavior for `Unrestricted` or `ParallelFileScope` scopes.
</requirements>

## Subtasks
- [ ] 3.1 Add the overlay branch to `resolve_action_path` (write→scratch, read→scratch-first then real).
- [ ] 3.2 Add the `AttemptScope` arm to `validate_action_scope` (confine writes to scratch, fail-closed).
- [ ] 3.3 Add a scratch-lifecycle module: create, RAII cleanup, startup sweep.
- [ ] 3.4 Represent deletions/renames as tombstones in the overlay.
- [ ] 3.5 Add tests for read-after-write, fall-through reads, escape denial, and cleanup.

## Implementation Details
The single choke point is `resolve_action_path` — both `execute_write_file` and `execute_apply_patch` route through it (see TechSpec "System Architecture" and ADR-006). Reuse the workspace-root convention `working_directory.join(".atelier")` from the history module for scratch placement. Keep the hot-path branch minimal and correct for reads-after-writes within an attempt.

### Relevant Files
- `src/actions/mod.rs:2000` — `resolve_action_path`; add the overlay branch.
- `src/actions/mod.rs:1745` — `execute_write_file`; confirm it flows through the choke point.
- `src/actions/mod.rs:1775`,`1950` — `execute_apply_patch` / `apply_unified_diff`; writes land in scratch.
- `src/actions/mod.rs:455`,`1501` — `validate_action_scope` / parallel write-path check; add the `AttemptScope` arm.
- `src/history/mod.rs:262` — `.atelier` root resolution to reuse for scratch paths.

### Dependent Files
- `src/app/mod.rs` (Task 07) — sets the `AttemptScope` per attempt.
- Task 09 — reads the scratch write-set to build the promotion diff.

### Related ADRs
- [ADR-006: Writes-Redirect + Diff-Replay Isolation and Promotion](../adrs/adr-006.md) — the isolation contract.

## Deliverables
- Overlay-aware `resolve_action_path` + `validate_action_scope` arm.
- A scratch-lifecycle module with guaranteed cleanup + startup sweep.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for isolation under concurrent attempts **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Write to `src/lib.rs` under `AttemptScope` lands at `scratch_dir/src/lib.rs`, not the real path.
  - [ ] Read of a file the attempt just wrote returns the scratch contents (read-after-write).
  - [ ] Read of an untouched file returns the real-tree contents (fall-through).
  - [ ] A write resolving outside `scratch_dir` is `Denied` (escape, fail-closed).
  - [ ] A deletion is represented as a tombstone and reflected on subsequent reads.
- Integration tests:
  - [ ] Two concurrent `AttemptScope`s writing the same relpath do not see each other's contents.
  - [ ] Scratch dir is removed after the owning guard drops; a stale dir from a prior run is swept at startup.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Concurrent attempts are isolated; reads fall through; escapes are denied fail-closed.
- No scratch directory leaks across the tested exit paths.
