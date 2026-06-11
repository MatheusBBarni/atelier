---
status: pending
title: Git context module with change-gated polling
type: backend
complexity: medium
dependencies: []
---

# Task 5: Git context module with change-gated polling

## Overview

Create `src/app/git.rs` providing `fetch_git_context() -> Option<GitContext>` via a `git rev-parse` subprocess, and wire a 5-second change-gated poll plus immediate refreshes at startup and prompt submission (ADR-006). This feeds the footer (task_06) and welcome facts box (task_04) with the repo+branch that prevents wrong-branch agent runs. Hard kill-switch: if this exceeds half a day, cut it (ADR-001).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
1. `GitContext { repo_name, branch }` MUST derive `Clone + PartialEq` (the change-gate, TechSpec "Core Interfaces"); branch from `git rev-parse --abbrev-ref HEAD` (raw SHA acceptable when detached), repo name from the toplevel path's file name.
2. `fetch_git_context` MUST return `None` on: non-zero exit, 500ms timeout, missing `git` binary, not a repo — never an error (ADR-001 graceful omission). Raw `.git/HEAD` parsing is FORBIDDEN (worktree/submodule layouts, ADR-006).
3. The subprocess MUST follow the established pattern: `tokio::process::Command` + `kill_on_drop(true)` + `tokio::select!` timeout (`src/runtime/claude.rs:147-155, :419-438`).
4. A background poll task MUST run every 5 seconds, compare against the last value, and publish a state update ONLY on change (no idle redraws); it MUST be tied to the worker lifecycle and stop at shutdown (`shutdown_app_worker`, `src/tui/mod.rs:484-503`).
5. Immediate refresh MUST occur at startup and on `PromptSubmitted` handling (before/alongside `submit_prompt`, `src/app/mod.rs:771-774`) without blocking input handling.
6. `AppState` MUST gain `git_context: Option<GitContext>`; updates flow through the existing `publish_state` → `watch` channel → `sync_worker_state` path (verified: `src/app/mod.rs:3687-3691`, `src/tui/mod.rs:227, :444-450`).
7. After a `None` result the poller MUST keep running (a repo can appear via `git init` mid-session).
</requirements>

## Subtasks
- [ ] 5.1 Create `src/app/git.rs` with `GitContext` and `fetch_git_context` (subprocess + timeout + None-on-failure).
- [ ] 5.2 Declare `pub mod git;` in `src/app/mod.rs` (after `pub mod chat;`, :1).
- [ ] 5.3 Add `git_context` to `AppState` and a setter that publishes only on change.
- [ ] 5.4 Spawn the 5s poll task within the worker scope; ensure shutdown stops it.
- [ ] 5.5 Hook immediate refresh into startup and the `PromptSubmitted` path.
- [ ] 5.6 Tests: fetch parsing, None paths, change-gating, and the prompt-refresh hook.

## Implementation Details

Worker integration: `run_app_worker` (`src/tui/mod.rs:463-482`) processes commands sequentially; spawn the interval task inside the worker scope (or store an abortable handle) so `shutdown_app_worker`'s abort path covers it. State publishing uses the existing watch-channel mechanism — no new channels. See TechSpec "Core Interfaces" and "Integration Points"; do not duplicate the command pattern here.

### Relevant Files
- `src/app/git.rs` — new module.
- `src/app/mod.rs` — mod declaration (:1), `AppState` (:71-85), `handle_event` `PromptSubmitted` branch (:771-774), `publish_state` (:3687-3691), `#[tokio::test]` patterns (:5812+).
- `src/tui/mod.rs` — worker spawn (:238), `run_app_worker` (:463-482), `shutdown_app_worker` (:484-503), watch channel (:227), `sync_worker_state` (:444-450).
- `src/runtime/claude.rs` — subprocess + timeout pattern to mimic (:147-155, :419-438).
- `src/config/mod.rs` — `working_directory` (:361) as the subprocess cwd.

### Dependent Files
- `src/tui/mod.rs` — task_06 footer consumes `state.git_context`.
- `src/tui/welcome.rs` — task_04 facts box consumes the same field.

### Related ADRs
- [ADR-006: Polled Git Context Refresh](../adrs/adr-006.md) — cadence, change-gating, rejected alternatives.
- [ADR-001: V1 Scope and Sequencing](../adrs/adr-001.md) — subprocess-only mechanism, half-day kill-switch.

## Deliverables
- `src/app/git.rs` + `AppState.git_context` + poll/refresh wiring with clean shutdown.
- Unit tests with 80%+ coverage of fetch and gating logic **(REQUIRED)**
- Integration tests for the refresh hooks **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] In a temp git repo (init + commit + branch `feat/x`): `fetch_git_context` returns `Some` with `branch == "feat/x"` and repo_name == temp dir name.
  - [ ] In a plain temp dir (no repo): returns `None`.
  - [ ] With a PATH containing no git (or overridden command name): returns `None` without error.
  - [ ] Detached HEAD state: returns `Some` with a non-empty branch value (SHA).
  - [ ] Change-gate: feeding the same context twice produces exactly one state publish.
- Integration tests:
  - [ ] `#[tokio::test]`: after app startup in a temp repo, `state.git_context` becomes `Some` (startup refresh).
  - [ ] `#[tokio::test]`: switching branch in the temp repo then submitting a prompt updates `git_context` to the new branch (prompt-submission refresh).
  - [ ] Worker shutdown completes within the existing 500ms grace path with the poller active (no hang).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Non-git directories produce no errors anywhere in the UI (PRD phase-2 criterion).
- Implementation stayed within the half-day kill-switch budget.
