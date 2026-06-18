---
status: completed
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
- [x] 7.1 Add an `mcp_catalog_snapshot` event recorded at run entry from the supervisor snapshot.
- [x] 7.2 Extend `build_orchestrator_prompt` (via `build_orchestrator_prompt_with_mcp`) to advertise tools from the snapshot.
- [x] 7.3 Add the replay-determinism guard test.
- [x] 7.4 Add a projection no-op arm for the snapshot event so it is preserved, not rendered.

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
- Unit tests (`src/orchestrator/mod.rs`):
  - [x] `build_orchestrator_prompt_with_mcp` with a snapshot containing two tools lists both with their server names. (`orchestrator_prompt_lists_snapshot_tools_with_servers`)
  - [x] An empty/absent snapshot omits the MCP-tools section entirely. (`orchestrator_prompt_omits_mcp_section_for_empty_snapshot`)
  - [x] Guard test: the orchestrator module never references the live supervisor handle type (source scan, mirroring `colors_live_only_in_theme_module`). (`orchestrator_prompt_never_reads_live_mcp_handle`)
  - [x] _Extra (`src/app/mod.rs`):_ run entry records `mcp_catalog_snapshot` when MCP is enabled, no-op when disabled. (`record_mcp_catalog_snapshot_emits_event_only_when_enabled`)
- Integration tests:
  - [x] Replaying the recorded snapshot (serialize → event payload → deserialize) produces a byte-identical orchestrator prompt. (`orchestrator_prompt_is_byte_identical_across_replay`)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **Signature kept backward-compatible.** `build_orchestrator_prompt(config)` now delegates to the new `build_orchestrator_prompt_with_mcp(config, &ToolCatalog)` with an empty default catalog, so the ~15 existing callers/tests are unchanged. The App run path calls the catalog-aware variant with the cached snapshot.
- **Snapshot recorded at top-level run entry.** `App::record_mcp_catalog_snapshot` (async) snapshots the supervisor, records the `mcp_catalog_snapshot` event, and refreshes the cached `App.mcp_catalog`; it is called from `submit_prompt_with_source` right after `run_started`. Follow-up/sub-task runs (sync construction) reuse the cached snapshot; the next top-level run re-snapshots (so a server dropped between runs shows at the next snapshot, not mid-run — ADR-001).
- **Closes the task_05 deferral.** `App::mcp_action_context()` now supplies the cached snapshot to the action context, so the validation pin-diff has real tool definitions to compare against.
- **Guard mechanism.** The guard test reads `src/orchestrator/mod.rs` via `include_str!` and asserts it never names the live handle type (needle built with `concat!` so the test's own source doesn't trip it). The module imports only `ToolCatalog` (a plain snapshot), never the handle.
- **Projection.** An explicit `"mcp_catalog_snapshot" => {}` arm preserves the audit event in the durable log without rendering it as chat (task_08 renders tool calls/results).

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- 4 orchestrator tests (advertise / omit / replay / guard) + 1 App snapshot test: pass.
- Full suite under a clean `HOME` (skipping env-sensitive cursor/codex subprocess tests): **1328 passed, 4 ignored, 0 failed**.

## Success Criteria
- All tests passing
- Test coverage >=80%
- The orchestrator advertises MCP tools deterministically from a recorded snapshot; replay is byte-stable.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
