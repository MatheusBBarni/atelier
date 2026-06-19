---
status: pending
title: Working-diff helpers
type: backend
complexity: low
dependencies: []
---

# Task 02: Working-diff helpers

## Overview
Give the app a deterministic way to read the current uncommitted working diff, which `/review` embeds in the reviewer prompt. No Rust-side diff API exists today — `src/app/git.rs` only reports a `dirty` boolean — so this adds read-only `git diff` helpers on the existing process primitive (ADR-004).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `working_diff(dir) -> Option<WorkingDiff>` helper (carrying the unified diff and the changed-file list) in `src/app/git.rs`, reusing the existing `run_git` process primitive.
- MUST invoke only read-only git (`git diff`, `git diff --name-only`); never mutating git.
- MUST return `None` when the directory is not a git repo or the working tree is clean (nothing to review).
- MUST cap an oversized diff and signal truncation so the reviewer prompt stays bounded.
- MUST apply a process timeout consistent with the existing `run_git` behavior.
</requirements>

## Subtasks
- [ ] 02.1 Add a `WorkingDiff { unified, files }` struct.
- [ ] 02.2 Implement `working_diff` / changed-file listing via `run_git`.
- [ ] 02.3 Return `None` on non-repo and clean-tree cases.
- [ ] 02.4 Truncate oversized diffs with an explicit marker.
- [ ] 02.5 Unit-test repo / non-repo / clean / oversized cases.

## Implementation Details
Modify only `src/app/git.rs`. Reuse the private `run_git(git_bin, dir, args)` primitive and follow the `fetch_git_context` pattern. See TechSpec "System Architecture → Diff acquisition" and "Known Risks → Oversized working diffs".

### Relevant Files
- `src/app/git.rs` — `run_git` (`:134`), `GitContext` (`:25`), `fetch_git_context` (`:42`); the new helpers live here.

### Dependent Files
- `src/app/mod.rs` (task_05/06) — the review engine calls `working_diff` and embeds the result in the reviewer prompt.

### Related ADRs
- [ADR-004: Opinion-only reviewer over an app-acquired git diff](../adrs/adr-004.md) — the app, not the reviewer, runs `git diff`.

## Deliverables
- `WorkingDiff` struct + `working_diff`/changed-file helpers in `src/app/git.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] In a temp git repo with an unstaged change, `working_diff` returns a non-empty unified diff and the changed-file list matches `git diff --name-only`.
  - [ ] A clean git repo returns `None`.
  - [ ] A non-repo directory returns `None`.
  - [ ] A diff exceeding the size cap is truncated and carries the truncation marker.
- Integration tests:
  - [ ] Exercised by task_05's end-to-end review test (the engine consumes `working_diff`).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `working_diff` returns the diff + file list for a dirty repo and `None` for clean/non-repo
- Oversized diffs are bounded with a visible truncation marker
