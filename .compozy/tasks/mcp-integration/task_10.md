---
status: completed
title: Add doctor MCP checks, parity matrix, and local metric
type: backend
complexity: medium
dependencies:
  - task_03
  - task_05
---

# Add doctor MCP checks, parity matrix, and local metric

## Overview
Give users a fast way to verify their MCP setup and prove the cross-runtime promise. `atelier --doctor` gains per-server health checks (reachable, handshake ok, and crucially distinguishing a real failure from harmless stderr noise) plus a runtimes×servers parity matrix that serves as the release gate, and `--doctor --json` surfaces the local trusted-completion metric — all derived from local state with no telemetry.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a per-server `mcp_server.<id>` doctor check (reachability + handshake), with remediation text that distinguishes a real failure from a server logging to stderr.
- MUST add an `mcp_parity` check whose `context` carries a runtimes×servers matrix; a missing pair is a non-Ok status (the release gate).
- MUST surface the local trusted-completion count in `--doctor --json`, computed from the local event log only.
- MUST NOT emit any network telemetry; all metrics are local and self-visible.
- SHOULD skip MCP checks cleanly when `features.mcp_enabled` is false.
</requirements>

## Subtasks
- [x] 10.1 Add the per-server availability check function and register it in `run_doctor`.
- [x] 10.2 Add the parity-matrix check populating `context`.
- [x] 10.3 Compute the local trusted-completion metric from events and add it to the JSON report.
- [x] 10.4 Ensure clean skip when MCP is disabled.

## Implementation Details
Modify `src/doctor/mod.rs`: register MCP checks in `run_doctor` after the runtime loop (mirror `check_runtime_availability`), probing servers via the supervisor (task_03). Build the parity matrix into a check's `context: Value`. Compute the metric by scanning local history events for trusted, completed MCP calls. See TechSpec "Monitoring and Observability" and ADR-002 (parity gate), PRD CF6/CF7.

### Relevant Files
- `src/doctor/mod.rs` — `run_doctor` (~55), `DoctorCheck`/`DoctorReport` (~11), `tool_access_check` (~199).
- `src/mcp/supervisor.rs` — used to probe server reachability (task_03).
- `src/config/mod.rs` — `mcp_servers` + `features.mcp_enabled` (task_02).
- `src/history/mod.rs` — event source for the local metric.

### Dependent Files
- None; doctor output only.

### Related ADRs
- [ADR-002: stdio-first V1; parity as the release gate](../adrs/adr-002.md) — parity matrix is the gate.

## Deliverables
- Per-server `mcp_server.<id>` doctor checks with actionable remediation.
- `mcp_parity` matrix check.
- Local trusted-completion metric in `--doctor --json`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test of doctor output with a configured fake server **(REQUIRED)**.

## Tests
- Unit tests (`src/doctor/mod.rs`):
  - [x] A reachable server yields an `Ok` `mcp_server.<id>` check. (integration `tests/mcp_doctor.rs::doctor_reports_reachable_fake_server_ok`)
  - [x] A server that logs to stderr but is healthy is reported `Ok` (not a false failure). (`tests/mcp_doctor.rs::doctor_treats_stderr_logging_server_as_ok`; remediation distinguishes the case — `mcp_server_check_reports_unreachable_command_as_error` asserts the stderr wording)
  - [x] An unreachable server yields an `Error` with remediation naming the cause. (`mcp_server_check_reports_unreachable_command_as_error`)
  - [x] The parity matrix marks a runtime×server pair with no successful call as not-Ok. (`mcp_parity_marks_uncompleted_pairs_not_ok`)
  - [x] With `mcp_enabled=false`, MCP checks are absent. (`mcp_checks_are_absent_when_disabled`)
  - [x] _Extra:_ trusted-completion metric counts only trusted completions; a completed pair shows in the matrix. (`mcp_parity_counts_trusted_completions_and_marks_pair_complete`)
- Integration tests (`tests/mcp_doctor.rs`):
  - [x] `run_doctor` (the source of `--doctor --json`) with one fake server includes the server check, the parity matrix in `context`, and a numeric trusted-completion count. (`doctor_json_has_server_check_parity_matrix_and_metric`)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **`mcp_tool_result` events are the metric/parity source.** task_05 executes MCP via the action pipeline; task_10 needs per-call `(runtime, server, status, trusted)`, so `record_action_completed[_with_group]` now also emits a local-only `mcp_tool_result` event for `CallMcpTool` (the techspec's observability event; task_08 already reserved its no-op projection arm). The `runtime` is threaded from the agent profile at the three completion call sites. The doctor scans these events from the local log only — **no network telemetry**.
- **Per-server check distinguishes stderr noise from failure.** `mcp_server_check` spawns the server and runs the `initialize` handshake via `RmcpClient::connect_stdio`. A server that logs to stderr but answers on stdout connects → `Ok`; the failure remediation explicitly states that stderr logging alone is not a failure. The fake server gained an opt-in `FAKE_MCP_STDERR_NOISE` env flag to prove this.
- **Parity matrix = enabled-agent runtimes × configured servers.** A cell is verified when a `completed` `mcp_tool_result` exists for that `(runtime, server)`; any missing pair makes the check `Warn` (the release gate). `Skipped` when there are no servers/agents; the whole MCP block is absent when `mcp_enabled = false`.

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- 4 doctor unit tests + 3 integration tests (reachable / stderr-noise / json shape): pass.
- Full suite under a clean `HOME` (skipping env-sensitive cursor/codex subprocess tests): **1344 passed, 4 ignored, 0 failed**.

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can diagnose MCP setup and see cross-runtime parity from `--doctor`; metrics never leave the machine.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
