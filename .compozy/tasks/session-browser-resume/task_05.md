---
status: pending
title: "GitContext HEAD+dirty, HEAD baseline, detect_drift"
type: backend
complexity: medium
dependencies: []
---

# Task 05: GitContext HEAD+dirty, HEAD baseline, detect_drift

## Overview
Give resume the inputs it needs to detect a moved workspace: extend `GitContext` with a short HEAD SHA and a display-only `dirty` flag, record the HEAD into the session log at run boundaries (the comparison baseline), and add a pure `detect_drift` that reports cwd-moved and HEAD-changed. Consumed by the resume flow (task_11) and the interlock (task_12).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST extend `GitContext` with `head_sha: Option<String>` (short SHA) and `dirty: bool`, fetched within the existing git timeout; on timeout/non-git dir, leave them `None`/`false` and never block.
- MUST record the current short HEAD into the session log at run boundaries so the session's last-recorded HEAD is recoverable by folding.
- MUST provide `detect_drift` as a pure function over (stored cwd, stored head, live cwd, live head) returning `WorkspaceDrift { cwd_moved, head_changed }`.
- MUST treat `dirty` as display-only — it MUST NOT contribute to drift.
</requirements>

## Subtasks
- [ ] 5.1 Extend `GitContext` + `fetch_git_context` to capture short HEAD and dirty flag.
- [ ] 5.2 Record the short HEAD into the session log at run boundaries.
- [ ] 5.3 Implement `detect_drift` + `WorkspaceDrift`.
- [ ] 5.4 Add unit tests for the fetch (degrade-on-timeout) and the drift matrix.

## Implementation Details
Extend `GitContext` and `fetch_git_context` in `src/app/git.rs` (`:26`/`:34`) with `git rev-parse --short HEAD` and `git status --porcelain`, reusing the existing subprocess + 500ms timeout. Record HEAD at the run-boundary event(s) near `write_run_record` (`src/app/mod.rs:3439`) / run-end transitions. `detect_drift` is a small pure helper (in `git.rs` or alongside resume logic). See TechSpec "Core Interfaces" and ADR-007.

### Relevant Files
- `src/app/git.rs` — `GitContext` (`:26`), `fetch_git_context` (`:34`); add HEAD/dirty + `detect_drift`.
- `src/app/mod.rs` — `refresh_git_context` (`:4361`), run-boundary recording near `write_run_record` (`:3439`).

### Dependent Files
- `src/app/mod.rs` — task_11 puts HEAD/dirty into the `session_resumed` payload; task_12 gates the first mutation on `WorkspaceDrift`.
- `src/tui/welcome.rs` / footer — may display the dirty flag.

### Related ADRs
- [ADR-007: Drift detection model](adrs/adr-007.md) — extend GitContext, record HEAD baseline, drift = cwd-moved OR HEAD-changed, dirty display-only.
- [ADR-004: Resume safety model](adrs/adr-004.md) — drift drives the first-mutation interlock.

## Deliverables
- Extended `GitContext` (head_sha + dirty) and fetch.
- HEAD baseline recorded in the session log at run boundaries.
- `detect_drift` + `WorkspaceDrift`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test: HEAD recorded in a run then compared after an external HEAD change **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `fetch_git_context` in a git repo returns a non-empty short `head_sha`; in a non-git dir returns `head_sha = None` and does not block.
  - [ ] `dirty` is true after an uncommitted change and false on a clean tree.
  - [ ] `detect_drift`: same cwd + same head ⇒ no drift; different head ⇒ `head_changed`; missing/moved cwd ⇒ `cwd_moved`; `dirty` alone ⇒ no drift; missing head (non-git) ⇒ no `head_changed`.
- Integration tests:
  - [ ] Record a run in a git repo, externally commit (HEAD changes), then `detect_drift` against the live context reports `head_changed`.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Resume can determine whether the workspace moved (cwd or HEAD) since the session paused; dirty never triggers drift; non-git workspaces never block.
