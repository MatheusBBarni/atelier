---
status: pending
title: Add rmcp dependency, McpClient trait, and fake stdio server
type: backend
complexity: medium
dependencies: []
---

# Add rmcp dependency, McpClient trait, and fake stdio server

## Overview
Establish the foundation for harness-owned MCP: add the official `rmcp` SDK, wrap it behind an internal `McpClient` trait (the swap/mock seam), and ship an in-repo fake stdio MCP server for deterministic tests. Every downstream MCP task depends on this seam, so it must be stable and SDK-agnostic at the call sites.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a pinned `rmcp` dependency to `Cargo.toml` `[dependencies]`.
- MUST define an internal async `McpClient` trait (`list_tools`, `call_tool`, `read_resource`) so the supervisor and tests never bind to rmcp's public surface (see TechSpec "Core Interfaces").
- MUST provide a real `McpClient` implementation adapting rmcp's stdio client over a spawned child process (see TechSpec "Integration Points").
- MUST provide an in-repo fake stdio MCP server exposing fixtures: one read-only resource, one read-only-annotated tool, one effect tool, and one description-mutating tool (for later diff-on-change tests).
- MUST declare `pub mod mcp;` in `src/lib.rs` after `pub mod orchestrator;`.
- SHOULD keep `rmcp` types out of `McpClient` method signatures where practical so the SDK stays swappable behind the trait.
</requirements>

## Subtasks
- [ ] 1.1 Add the pinned `rmcp` dependency and confirm the crate builds.
- [ ] 1.2 Define the `McpClient` trait and its core result types (tool, tool result, resource).
- [ ] 1.3 Implement the real rmcp-backed stdio client behind the trait.
- [ ] 1.4 Build the in-repo fake stdio MCP server with the four fixtures.
- [ ] 1.5 Wire the new `mcp` module into the crate via `src/lib.rs`.

## Implementation Details
Create `src/mcp/mod.rs`, `src/mcp/client.rs` (trait + real impl), and `src/mcp/fake_server.rs`. Modify `Cargo.toml` `[dependencies]` and the module list in `src/lib.rs`. The real client adapts rmcp's `serve` + `TokioChildProcess` and `list_all_tools` / `call_tool`; follow the subprocess/stdio shape already used by the Cursor runtime. Model the fake server on the deterministic philosophy of `src/runtime/fake.rs`. See TechSpec "Core Interfaces" and ADR-004/ADR-008.

### Relevant Files
- `Cargo.toml` — add `rmcp` to `[dependencies]` (line ~12); single-crate project, no workspace.
- `src/lib.rs` — insert `pub mod mcp;` after `pub mod orchestrator;` (line ~12).
- `src/runtime/cursor.rs` — subprocess spawn + stdio read loop + `tokio::select!` cancel/timeout pattern to mirror for the real client.
- `src/runtime/fake.rs` — deterministic-server philosophy to mirror for the fake MCP server.

### Dependent Files
- `src/mcp/supervisor.rs` (task_03) — consumes the `McpClient` trait.
- `src/actions/mcp_handlers.rs` (task_05) — invokes via the supervisor over this trait.

### Related ADRs
- [ADR-004: Adopt the official rmcp Rust SDK](../adrs/adr-004.md) — wrap rmcp behind the `McpClient` trait.
- [ADR-008: Deterministic MCP testing via an in-repo fake stdio server](../adrs/adr-008.md) — the fixtures and tiers.

## Deliverables
- Pinned `rmcp` dependency in `Cargo.toml`.
- `McpClient` trait + real rmcp-backed implementation.
- In-repo fake stdio MCP server with the four fixtures.
- `mcp` module wired into `src/lib.rs`.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests spawning the fake server over real stdio **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A mock `McpClient` returns the four fixture tools from `list_tools`.
  - [ ] The real client returns a descriptive error (not a panic) when the spawn command path does not exist.
  - [ ] `read_resource` on the fake server's read-only resource URI returns its fixture content.
- Integration tests:
  - [ ] Spawning the fake server and calling `list_tools` returns exactly four tools with the expected names (resource-read, read-only tool, effect tool, mutating tool).
  - [ ] `call_tool` on the effect tool round-trips arguments and returns the expected content over real stdio.
  - [ ] The `initialize` handshake completes against the fake server (the connection reaches a ready state before the first call).
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `cargo build` succeeds with `rmcp` pinned; `cargo fmt --check` and `cargo clippy --all-targets` are clean.
- The fake server answers `list_tools` and `call_tool` over stdio through the `McpClient` trait.
