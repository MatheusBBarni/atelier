---
status: pending
title: Background file-index acquisition
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 04: Background file-index acquisition

## Overview
Feed the TUI a live file index without ever blocking the draw loop: run `FileIndex::walk` off-thread and deliver snapshots over a channel from the app worker, refreshed on a coarse interval. This mirrors the existing 5-second git poller.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a worker→TUI channel carrying the latest index snapshot (`Vec<FileEntry>`).
- MUST run `FileIndex::walk` via `tokio::task::spawn_blocking` (the walk is synchronous) so the worker's async loop is never blocked.
- MUST perform an initial walk at startup and refresh on a coarse `tokio::time::interval` driven by a named constant (default ~15s).
- MUST root the walk at the working directory.
- MUST extract the walk-and-publish step into a unit-testable helper rather than burying it inside the `tokio::select!` arm.
- MUST NOT block the worker `select!` loop or the synchronous render loop.
</requirements>

## Subtasks
- [ ] 4.1 Add the index-snapshot channel between the worker and the TUI.
- [ ] 4.2 Add the refresh-interval constant.
- [ ] 4.3 Add a `spawn_blocking` walk-and-publish helper.
- [ ] 4.4 Invoke it at startup and on each refresh tick in the worker `select!`.
- [ ] 4.5 Thread the working directory into the worker.
- [ ] 4.6 Add unit/integration tests for the publish helper and refresh.

## Implementation Details
Modify `src/tui/mod.rs`: add the channel alongside the existing `watch`/`mpsc` setup in `run_tui`, and add a refresh arm to the `run_app_worker` `tokio::select!` loop, mirroring `GIT_POLL_INTERVAL`. See TechSpec "Component Overview" (index acquisition task) and ADR-003. The receiver may be parked until task_05 consumes it (stage with `#[allow(dead_code)]` if the compiler flags it).

### Relevant Files
- `src/tui/mod.rs` — `run_tui` channel setup and `run_app_worker` `select!` loop / `GIT_POLL_INTERVAL` pattern to mirror.
- `src/file_index.rs` — provides `FileIndex::walk`.
- `.compozy/tasks/at-mention-file-dropdown/_techspec.md` — "Component Overview" and "Integration Points".

### Dependent Files
- `src/tui/mod.rs` (`TuiUiState` / `run_loop`) — task_05 consumes the snapshot channel.

### Related ADRs
- [ADR-003: File-Index Acquisition via Background Worker Walk](../adrs/adr-003.md) — off-thread walk + channel + refresh.

## Deliverables
- Index-snapshot channel, refresh constant, and the `spawn_blocking` walk-and-publish helper wired into the worker.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests for refresh delivery **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] The refresh-interval constant equals the expected default.
  - [ ] The walk-and-publish helper sends a snapshot equal to `FileIndex::walk` for a `tempfile` root.
- Integration tests:
  - [ ] Running the publish helper against a `tempfile` root makes the receiver observe a non-empty snapshot containing a known file.
  - [ ] After creating a new file and triggering a refresh, the next snapshot includes that file.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The index is produced off-thread and refreshed without blocking the worker or draw loop
