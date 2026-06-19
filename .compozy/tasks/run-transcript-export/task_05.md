---
status: pending
title: "Egress-safety warning via git check-ignore"
type: backend
complexity: low
dependencies:
  - task_04
---

# Task 05: Egress-safety warning via git check-ignore

## Overview
Add a `git check-ignore`/`rev-parse` helper and wire it into the orchestrator's write step so an export warns — but never blocks — when the chosen target file is not gitignored or sits inside the repo working tree. This closes the `git add -A` sweep risk for non-default `--out` targets while leaving the already-ignored default path silent.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a helper that determines whether a target path is git-ignored by shelling out to `git check-ignore <path>`, using the `src/app/git.rs` subprocess pattern (500 ms timeout, `kill_on_drop`, `tokio::select!`).
- MUST treat command failure / non-repo as "not ignored" (fail-open to a warning), never an error.
- MUST emit a stderr warning from the export write step when the target is not ignored or is inside the repo working tree; MUST NOT block the export or change the exit code.
- The default `.atelier/exports/…` path (already under the ignored `.atelier/`) MUST NOT trigger the warning.
</requirements>

## Subtasks
- [ ] 5.1 Implement the `git check-ignore`/`rev-parse` helper with the timeout/`kill_on_drop` pattern.
- [ ] 5.2 Wire the warning into the orchestrator's write path for non-default `--out` targets.
- [ ] 5.3 Ensure fail-open behavior (no repo / git error → warn, never block).
- [ ] 5.4 Suppress the warning for the default ignored export directory.

## Implementation Details
Add the helper and its wiring inside `src/export.rs` (task_04's write step), copying the subprocess shape from `src/app/git.rs`. See TechSpec "Integration Points". No new module; no new dependency (reuse the `tokio` process primitives already used by `git.rs`).

### Relevant Files
- `src/app/git.rs` — `run_git`/`fetch_git_context` subprocess pattern (~134), 500 ms timeout, `kill_on_drop`.
- `src/export.rs` — the write step (task_04) where the warning attaches.

### Dependent Files
- `src/export.rs` — modified to call the helper before/after writing.

### Related ADRs
- [ADR-004: Egress warning via git check-ignore](../adrs/adr-004.md) — the fail-open warning approach.
- [ADR-001: Quarantine egress](../adrs/adr-001.md) — owner-only, outside the tracked tree.

## Deliverables
- The `git check-ignore`/`rev-parse` helper and its wiring in the export write step.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration coverage of the `--out` warning at binary level is provided in task_06 **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A path under a gitignored directory (temp repo fixture) → `ignored = true`, no warning.
  - [ ] A tracked / in-repo path → `ignored = false`, warning string produced.
  - [ ] No git repo at the target → `ignored = false` (fail-open), warning produced, no error returned.
  - [ ] The default `.atelier/exports/...` path produces no warning.
- Integration tests:
  - [ ] (covered in task_06) `--export-session … --out <non-ignored>` emits the stderr warning and still succeeds.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Warning fires for non-ignored / in-repo targets and never blocks or changes the exit code
- Default export path stays silent
