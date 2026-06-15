---
status: pending
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
- [ ] 3.1 Define `McpHandle`, the `McpCommand` enum, and the actor loop.
- [ ] 3.2 Implement connection spawn + `initialize` + health/timeout/kill lifecycle.
- [ ] 3.3 Implement concurrent per-connection dispatch for `CallTool`/`ReadResource`.
- [ ] 3.4 Implement `SnapshotCatalog` returning an immutable `ToolCatalog`.
- [ ] 3.5 Own the supervisor on `App` and shut it down cleanly.

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
- Unit tests:
  - [ ] `SnapshotCatalog` against two mocked connections returns a catalog containing both servers' tools.
  - [ ] A `CallTool` for an unknown server id replies with a descriptive error, not a hang.
  - [ ] `Shutdown` drains the channel and reports completion.
- Integration tests:
  - [ ] The supervisor spawns the fake stdio server, and a `CallTool` round-trips through `McpHandle`.
  - [ ] A server whose process exits mid-session causes its next `CallTool` to fail without hanging the supervisor or other servers.
  - [ ] A `CallTool` exceeding the per-call timeout returns a timeout error and the child is killed.
  - [ ] Two concurrent `CallTool`s to the same server both complete (no head-of-line blocking).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The supervisor owns all connections; a hung or dead server never hangs the run or other servers.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
