---
status: pending
title: Add MCP action kinds, capability, validation, and execution
type: backend
complexity: high
dependencies:
  - task_03
  - task_04
---

# Add MCP action kinds, capability, validation, and execution

## Overview
Make MCP a first-class action. This adds `CallMcpTool`, `ReadMcpResource`, and `ListMcpResources` to the action contract, a `Capability::McpTool`, the gating in validation (default-deny allowlist + trust tier + description-hash diff), and the executors that dispatch through `McpHandle`. Because adding enum variants forces every exhaustive match to compile, this is one vertical task that keeps the build green and ends with the model still unable to self-invoke (Claude-strip regression).

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `CallMcpTool`, `ReadMcpResource`, `ListMcpResources` to `ActionKind`, with params carried in the existing untyped `params: Value` (no type-system change).
- MUST add `Capability::McpTool` and corresponding `ToolName` variants, and update `ToolName::all()` + `required_capability()`.
- MUST update EVERY exhaustive `ActionKind` match so the crate compiles (`tool_name_for_action`, `execute_action_request`, `required_capability`, `action_target_display`).
- MUST map `ReadMcpResource`/`ListMcpResources` to the read capability (auto-allow) and `CallMcpTool` to `Capability::McpTool` (see ADR-007).
- MUST gate `CallMcpTool` in `validate_action_request_with_scope` by: per-`(agent, server, tool)` default-deny allowlist, trust tier (untrusted ⇒ `RequiresApproval`), and a changed description pin ⇒ `RequiresApproval`.
- MUST implement `execute_*` arms that dispatch via the `McpHandle` carried on `ActionExecutionContext`.
- MUST add a regression test proving the Claude runtime still strips MCP so the model cannot self-invoke a tool.
</requirements>

## Subtasks
- [ ] 5.1 Add the three `ActionKind` variants, `Capability::McpTool`, and the `ToolName` variants.
- [ ] 5.2 Update all exhaustive `ActionKind` matches to keep the build green.
- [ ] 5.3 Implement validation arms (allowlist + trust tier + pin diff) consulting the trust store.
- [ ] 5.4 Implement the `execute_*` arms dispatching through `McpHandle`.
- [ ] 5.5 Add the `mcp_handle` field to `ActionExecutionContext` and thread it from `App`.
- [ ] 5.6 Add the Claude-strip regression test.

## Implementation Details
Modify `src/actions/mod.rs` (enum + the four exhaustive matches + validation + new `execute_*`, optionally in `src/actions/mcp_handlers.rs`), `src/config/mod.rs` (`Capability`, `ToolName`), and `src/app/mod.rs` (the `record_action_result`/`action_target_display` arms + thread the handle). Consult the `McpTrustStore` (task_04) in validation and the `McpHandle` (task_03) in execution. Do not render chat here — that is task_08. See TechSpec "Core Interfaces" / "Impact Analysis"; ADR-001 and ADR-007.

### Relevant Files
- `src/actions/mod.rs` — `ActionKind` (~27), `validate_action_request_with_scope` (~176), `validate_action_scope` (~228), `tool_name_for_action` (~266), `execute_action_request` (~334), `required_capability` (~703), `ActionExecutionContext` (~97).
- `src/config/mod.rs` — `Capability` (~28), `ToolName` + `all()` + `required_capability()` (~39).
- `src/app/mod.rs` — `record_action_result_with_group` (~4742), `action_target_display` (~6354).
- `src/runtime/claude.rs` — MCP strip args (~40) for the regression test.

### Dependent Files
- `src/app/chat/projection.rs` (task_08) — adds string arms for the new action kinds.
- `src/app/mod.rs` (task_09) — approval card reads the surfaced description.

### Related ADRs
- [ADR-001: Broker MCP through the harness ActionRequest contract](../adrs/adr-001.md) — the action contract.
- [ADR-007: Read-only auto-allow via the protocol resource/tool split](../adrs/adr-007.md) — capability mapping + gating.

## Deliverables
- Three new `ActionKind`s + `Capability::McpTool` + `ToolName` variants, all matches updated.
- Validation gating (allowlist + trust tier + pin diff).
- `execute_*` arms dispatching via `McpHandle`.
- Claude-strip regression test.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test of a full validate→execute round-trip against the fake server **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] `ListMcpResources`/`ReadMcpResource` validate as read-capability and auto-allow.
  - [ ] `CallMcpTool` for a tool not in the agent's allowlist is `Denied`.
  - [ ] `CallMcpTool` for an allowlisted tool on an untrusted server returns `RequiresApproval`.
  - [ ] `CallMcpTool` whose pinned description changed returns `RequiresApproval` even when the server is trusted.
  - [ ] `CallMcpTool` on a trusted server with an allowlisted, unchanged tool is `Allowed`.
- Integration tests:
  - [ ] A full `CallMcpTool` round-trip (validate→execute) against the fake server returns the tool's result as an `ActionResult`.
  - [ ] With the Claude runtime configured, a model emitting a native tool-use cannot self-invoke (the strip args remain asserted).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- The crate compiles with all match arms handled; a brokered tool call works end-to-end and is correctly gated.
- The Claude runtime still cannot self-invoke MCP tools.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
