---
status: pending
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
- [ ] 10.1 Add the per-server availability check function and register it in `run_doctor`.
- [ ] 10.2 Add the parity-matrix check populating `context`.
- [ ] 10.3 Compute the local trusted-completion metric from events and add it to the JSON report.
- [ ] 10.4 Ensure clean skip when MCP is disabled.

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
- Unit tests:
  - [ ] A reachable server yields an `Ok` `mcp_server.<id>` check.
  - [ ] A server that writes a startup line to stderr but is healthy is reported `Ok` (not a false failure), with remediation distinguishing the case.
  - [ ] An unreachable server yields an `Error`/`Warn` with remediation naming the likely cause.
  - [ ] The parity matrix marks a runtime×server pair that has no successful call as not-Ok.
  - [ ] With `mcp_enabled=false`, MCP checks are `Skipped` or absent.
- Integration tests:
  - [ ] `--doctor --json` with one fake server includes the server check, the parity matrix in `context`, and a numeric trusted-completion count.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can diagnose MCP setup and see cross-runtime parity from `--doctor`; metrics never leave the machine.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
