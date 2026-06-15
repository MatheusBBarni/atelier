---
status: pending
title: Advertise an MCP tool-catalog snapshot to the orchestrator
type: backend
complexity: medium
dependencies:
  - task_03
---

# Advertise an MCP tool-catalog snapshot to the orchestrator

## Overview
The orchestrator prompt must tell the model which MCP tools exist — but reading live connection state would make the prompt non-deterministic and break replay. This task records an immutable tool-catalog snapshot as an event at run entry and has `build_orchestrator_prompt` advertise tools from that snapshot only, enforced by a guard test in the spirit of `colors_live_only_in_theme_module`.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST capture an immutable `ToolCatalog` snapshot from the supervisor at run/planning entry and record it as an `mcp_catalog_snapshot` event.
- MUST have `build_orchestrator_prompt` advertise available MCP tools from the recorded snapshot, never from a live supervisor handle.
- MUST keep the prompt deterministic given the same recorded events (replay-safe).
- MUST add a guard test asserting the prompt builder depends on a snapshot, not a live connection.
- SHOULD reflect a server that dropped between runs at the next snapshot, not mid-run.
</requirements>

## Subtasks
- [ ] 7.1 Add an `mcp_catalog_snapshot` event recorded at run entry from the supervisor snapshot.
- [ ] 7.2 Extend `build_orchestrator_prompt` to advertise tools from the snapshot.
- [ ] 7.3 Add the replay-determinism guard test.
- [ ] 7.4 Add a projection no-op arm for the snapshot event so it is preserved, not rendered.

## Implementation Details
Modify `src/orchestrator/mod.rs` `build_orchestrator_prompt` to append an MCP-tools section sourced from the snapshot (mirror how enabled agents are listed). Add a snapshot-recording method in `src/app/mod.rs` that calls the supervisor's `SnapshotCatalog`. Add an `apply_history_event` arm in `src/app/chat/projection.rs` that preserves the snapshot event without rendering. See TechSpec "Implementation Design" and ADR-001 (snapshot invariant)/ADR-005.

### Relevant Files
- `src/orchestrator/mod.rs` — `build_orchestrator_prompt` (~672) and the agent-advertisement block.
- `src/app/mod.rs` — `record_event_with_group` (~4215) and run-entry path to emit the snapshot.
- `src/mcp/supervisor.rs` — `SnapshotCatalog` command (task_03).
- `src/app/chat/projection.rs` — `apply_history_event` dispatch (~58).
- `src/tui/mod.rs` — `colors_live_only_in_theme_module` as the guard-test pattern to mirror.

### Dependent Files
- None downstream; this task is read-only with respect to other MCP tasks.

### Related ADRs
- [ADR-001: Broker MCP through the harness ActionRequest contract](../adrs/adr-001.md) — the catalog-snapshot invariant.
- [ADR-005: McpSupervisor as a supervisor actor with a command channel](../adrs/adr-005.md) — the supervisor produces the snapshot.

## Deliverables
- `mcp_catalog_snapshot` event recorded at run entry.
- Orchestrator prompt advertising tools from the snapshot.
- Replay-determinism guard test.
- Projection arm preserving the snapshot event.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test of prompt determinism across replay **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `build_orchestrator_prompt` with a snapshot containing two tools lists both with their server names.
  - [ ] `build_orchestrator_prompt` with an empty/absent snapshot omits the MCP-tools section entirely.
  - [ ] The guard test fails if the prompt builder is given a live supervisor handle instead of a snapshot.
- Integration tests:
  - [ ] Replaying the same recorded events (including the snapshot) produces a byte-identical orchestrator prompt.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The orchestrator advertises MCP tools deterministically from a recorded snapshot; replay is byte-stable.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
