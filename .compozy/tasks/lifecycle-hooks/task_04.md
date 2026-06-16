---
status: pending
title: Off-thread hook dispatcher (channel + subprocess + hook events)
type: backend
complexity: high
dependencies:
  - task_01
  - task_03
---

# Task 4: Off-thread hook dispatcher (channel + subprocess + hook events)

## Overview
Build the long-lived dispatcher task that drains the bounded hook channel and executes each matched handler off the worker thread — running the user's shell command (payload on stdin) or invoking the notifier — then emits `hook_started`/`hook_completed` back to the worker. This keeps all subprocess work off the hot path (ADR-003) and is where best-effort delivery is enforced.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST define a bounded `mpsc` channel + a capacity constant for dispatch items (each item carries a `HookPayload` + the matched handler(s)).
- MUST run a `command` handler as a subprocess using the existing idiom (`Command` + `kill_on_drop(true)` + `select!` timeout), writing the normalized payload to **stdin only** (never argv), and MUST capture+redact stdout/stderr via `redact_sensitive_text`.
- MUST invoke the task_03 `Notifier` for a `notify` handler.
- MUST emit `hook_started` and `hook_completed` records via a back-channel to the worker — the dispatcher has no `&mut App`, so it MUST NOT call `record_event` directly (it sends the lifecycle record type from task_01 over a sender supplied at spawn; task_05 wires the worker to record it).
- MUST expose a shared dropped-event counter (incremented by the tap on a full channel) so task_08 can report it.
- MUST bound every hook with a configurable timeout and never block on a slow/hung hook.
</requirements>

## Subtasks
- [ ] 4.1 Define the dispatch item type, bounded channel, capacity constant, and the shared dropped-event counter.
- [ ] 4.2 Implement the dispatcher task loop draining the channel.
- [ ] 4.3 Execute `command` handlers as timed subprocesses with payload on stdin and redacted output.
- [ ] 4.4 Invoke the `Notifier` for `notify` handlers.
- [ ] 4.5 Emit `hook_started`/`hook_completed` lifecycle records over the worker back-channel.
- [ ] 4.6 Add unit/async tests for execution, timeout, drop counter, and notify dispatch.

## Implementation Details
Create `src/hooks/dispatch.rs`. Model the subprocess on `run_git` (`src/app/git.rs:72`) and the stdin write on `src/runtime/claude.rs:174`. Model the spawned task and back-channel on `spawn_parallel_runtime_task` (`src/app/mod.rs:5803`), which already routes off-thread results back to the worker via an `mpsc` sender. The dispatcher receives dispatch items (from the tap, task_05) and sends lifecycle records out; task_05 owns creating both channels and wiring the worker handler. See TechSpec "System Architecture → Dispatcher task" and "Known Risks → best-effort delivery".

### Relevant Files
- `src/hooks/dispatch.rs` — dispatcher task, channel, capacity, dropped counter (create).
- `src/app/git.rs:72` — `run_git` subprocess idiom (`kill_on_drop` + `select!` timeout) to mirror.
- `src/runtime/claude.rs:174` — stdin-write pattern for piping the payload.
- `src/app/mod.rs:5803` — `spawn_parallel_runtime_task`: the off-thread-result-to-worker pattern.
- `src/runtime/mod.rs:704` — `redact_sensitive_text` for hook output.

### Dependent Files
- `src/app/mod.rs` / `src/tui/mod.rs` — task_05 creates the channels, spawns this task, and records the lifecycle messages.
- `src/doctor/mod.rs` — task_08 reads the dropped-event counter.
- `src/app/chat/projection.rs` — task_06 projects the emitted `hook_started`/`hook_completed` events.

### Related ADRs
- [ADR-003: Off-funnel hook dispatch with enrich-at-tap actor resolution](../adrs/adr-003.md) — off-thread dispatch, back-channel recording, recursion guard.
- [ADR-005: Built-in notifier](../adrs/adr-005.md) — the notify action path.

## Deliverables
- A dispatcher task draining a bounded channel, executing command/notify handlers off-thread.
- Timed, redacted subprocess execution with payload on stdin.
- `hook_started`/`hook_completed` emission via the worker back-channel.
- A shared dropped-event counter.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration test: a command handler runs and the back-channel receives `hook_completed` **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] A `command` handler runs and receives the exact normalized JSON on stdin (asserted via a script that echoes stdin to a sentinel file).
  - [ ] A hook exceeding the timeout is killed and reported as `hook_completed` with a timeout status (no hang).
  - [ ] `try_send` on a full bounded channel increments the dropped-event counter.
  - [ ] A `notify` handler invokes the injected `Notifier` exactly once.
  - [ ] Hook stdout/stderr containing a `Bearer …` token is redacted before being recorded.
- Integration tests:
  - [ ] End-to-end: a dispatch item with a `command` handler produces `hook_started` then `hook_completed` lifecycle records on the back-channel with the exit code.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The dispatcher never blocks the worker thread and bounds every hook with a timeout
- Best-effort delivery is enforced (drop-on-full increments the counter)
- `hook_started`/`hook_completed` are emitted via the back-channel, not `record_event` directly
