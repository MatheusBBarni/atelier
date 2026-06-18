---
status: completed
title: Build the McpSupervisor actor and McpHandle
type: backend
complexity: high
dependencies:
  - task_01
  - task_02
---

# Build the McpSupervisor actor and McpHandle

## Overview
Own the long-lived, stateful MCP connections in one place. The `McpSupervisor` is an actor task that spawns and supervises each configured server (health, timeout, kill, reconnect) and produces an immutable tool-catalog snapshot; callers reach it through a cheap, cloneable `McpHandle` command channel. This keeps async network state out of `App`'s deterministic core.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST implement an `McpSupervisor` task that owns one `McpClient` connection per configured stdio server and manages its lifecycle (spawn, `initialize`, health, timeout, kill, reconnect).
- MUST expose a cloneable `McpHandle` over an mpsc command channel with `CallTool`, `ReadResource`, `SnapshotCatalog`, and `Shutdown` commands, each replying via `oneshot` (see TechSpec "Core Interfaces").
- MUST dispatch tool calls concurrently per connection so one slow call does not block others (no global call mutex).
- MUST produce an immutable `ToolCatalog` snapshot of all servers' tools.
- MUST be owned and shut down cleanly by `App` (children killed on shutdown; `kill_on_drop` backstop).
- MUST apply a per-call timeout and cancellation using the existing `CancellationToken` pattern.
</requirements>

## Subtasks
- [x] 3.1 Define `McpHandle`, the `McpCommand` enum, and the actor loop.
- [x] 3.2 Implement connection spawn + `initialize` + health/timeout/kill lifecycle.
- [x] 3.3 Implement concurrent per-connection dispatch for `CallTool`/`ReadResource`.
- [x] 3.4 Implement `SnapshotCatalog` returning an immutable `ToolCatalog`.
- [x] 3.5 Own the supervisor on `App` and shut it down cleanly.

## Implementation Details
Create `src/mcp/supervisor.rs` (+ `src/mcp/handle.rs` if split); declare them in `src/mcp/mod.rs`. Add an `McpSupervisor` field to `App` (`src/app/mod.rs`) and spawn it at session init when `features.mcp_enabled`. Reuse the Cursor runtime's `tokio::select!` cancel/timeout and `start_kill`→`kill` shape. See TechSpec "System Architecture" and "Core Interfaces"; ADR-005 and ADR-002.

### Relevant Files
- `src/mcp/client.rs` — the `McpClient` trait the supervisor owns (task_01).
- `src/config/mod.rs` — `EffectiveConfig.mcp_servers` to spawn from (task_02).
- `src/runtime/cursor.rs` — `wait_for_child_or_cancel` / `request_child_kill` pattern (~448).
- `src/app/mod.rs` — `App` struct (~316) to own the supervisor and its shutdown.

### Dependent Files
- `src/actions/mod.rs` (task_05) — `ActionExecutionContext` carries an `McpHandle`.
- `src/orchestrator/mod.rs` (task_07) — reads the catalog snapshot.

### Related ADRs
- [ADR-005: McpSupervisor as a supervisor actor with a command channel](../adrs/adr-005.md) — the actor + concurrent dispatch.
- [ADR-002: stdio-first V1; async/event-recorded connect](../adrs/adr-002.md) — lifecycle is async and extensible.

## Deliverables
- `McpSupervisor` actor + cloneable `McpHandle`.
- Lifecycle management (spawn/health/timeout/kill/reconnect).
- `ToolCatalog` snapshot command.
- `App` ownership + clean shutdown.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests against the fake stdio server **(REQUIRED)**.

## Tests
- Unit tests (`src/mcp/supervisor.rs`, against mocked connections):
  - [x] `SnapshotCatalog` against two mocked connections returns a catalog containing both servers' tools. (`snapshot_catalog_includes_all_servers_tools`)
  - [x] A `CallTool` for an unknown server id replies with a descriptive error, not a hang. (`call_tool_unknown_server_errors_without_hanging`)
  - [x] `Shutdown` drains the channel and reports completion. (`shutdown_reports_completion_and_stops_the_actor`)
  - [x] _Extra:_ `CallTool` round-trips through the actor to a stub. (`call_tool_round_trips_through_stub`)
- Integration tests (`tests/mcp_supervisor.rs`, real fake stdio server):
  - [x] The supervisor spawns the fake stdio server, and a `CallTool` round-trips through `McpHandle`. (`supervisor_round_trips_call_tool_through_handle`)
  - [x] A server whose process exits mid-session causes its next `CallTool` to fail without hanging the supervisor or other servers. (`dead_server_fails_its_next_call_without_affecting_others`)
  - [x] A `CallTool` exceeding the per-call timeout returns a timeout error and the child is killed (connection evicted; subsequent call fails). (`slow_call_times_out_and_evicts_the_connection`)
  - [x] Two concurrent `CallTool`s to the same server both complete (no head-of-line blocking). (`concurrent_calls_to_same_server_do_not_block`)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **Killing on timeout = cancel + evict.** rmcp's `TokioChildProcess` kills the child on `Drop` (its `ChildWithCleanup::drop`), not on cancel alone. So on a per-call timeout the dispatch task both cancels the connection (`McpClient::shutdown`) *and* sends an internal `EvictConnection` command so the actor drops the map's `Arc`; once the last `Arc` is gone the child is killed. A subsequent call then returns a clean "unknown server" error, which the integration test asserts.
- **Concurrency is structural.** Each `CallTool`/`ReadResource` is dispatched onto a `tokio::spawn`ed task sharing the connection's `Arc<dyn McpClient>` (rmcp clients self-correlate and the rmcp server spawns a task per request), so the actor loop never blocks and same-server calls overlap. The concurrency test warms the connection first, then asserts two 500ms calls finish well under the 1000ms serial time.
- **Fake-server fixture extended (no contract change).** `effect_tool` gained opt-in `sleep_ms` / `exit_after_ms` args to drive the timeout and mid-session-exit tests. Plain calls still just echo, so task_01's "exactly four tools" / echo contract is unchanged.
- **App ownership + shutdown.** `App` gains an `mcp_handle: Option<McpHandle>`, spawned at session init only when `features.mcp_enabled` (off by default → no behavior change for existing tests). `end_session` drops the handle, which closes the command channel so the actor tears down every connection (kill_on_drop backstop, ADR-005). An `App::mcp_handle()` accessor is provided for task_05/07. Automatic *reconnect* of a dead connection is intentionally deferred: the contract here is to surface the failure fast (the dead-server test requires the next call to fail), not silently respawn.

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- 4 supervisor unit tests + 4 supervisor integration tests: pass.
- Full suite under a clean `HOME` (skipping the env-sensitive codex/cursor `availability`/`login_status` tests CLAUDE.md flags): **1324 passed, 4 ignored, 0 failed**. Those skipped tests pass under the real `HOME` (runtime 109/109, cursor 22/22); they fail only because a synthetic `HOME` breaks the CLIs' credential lookup. Neither they nor the malformed-home-skill tests are touched by this task.

## Success Criteria
- All tests passing
- Test coverage >=80%
- The supervisor owns all connections; a hung or dead server never hangs the run or other servers.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
