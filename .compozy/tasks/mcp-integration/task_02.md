---
status: pending
title: Add mcp.servers config section, mcp_enabled flag, and redaction
type: backend
complexity: medium
dependencies: []
---

# Add mcp.servers config section, mcp_enabled flag, and redaction

## Overview
Let users declare MCP servers in `atelier.toml` exactly as they declare runtimes today. This adds a transport-agnostic `[mcp.servers.<name>]` section through the Raw→Merged→Effective config ladder, a `features.mcp_enabled` flag to opt in, and `--print-config` redaction so server credentials never print. Only stdio is wired in V1; `url`/HTTP fields parse but stay inert.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add a `[mcp.servers.<name>]` config section with `transport`, `command`, `args`, `env`, and an inert `url` field, mirroring the existing `RawRuntimeConfig` → `MergedRuntimeConfig` → effective ladder.
- MUST add `features.mcp_enabled: bool` (default `false`) alongside `parallel_step_groups`.
- MUST expose effective servers on `EffectiveConfig` (e.g., `mcp_servers: BTreeMap<String, McpServerConfig>`).
- MUST redact secrets in `--print-config`: show env-var names and `command`, never resolved secret values (mirror how `api_key_env` is handled).
- MUST validate that a server with `transport = stdio` has a `command`; surface a clear error otherwise.
- MUST ship a commented `[mcp.servers.*]` example in `--init-config` output.
</requirements>

## Subtasks
- [ ] 2.1 Add the `RawMcpServerConfig` / merged / effective `McpServerConfig` structs.
- [ ] 2.2 Wire the section through `apply_raw` merge and `into_effective` validation.
- [ ] 2.3 Add the `features.mcp_enabled` flag through `RawFeatures` → `Features`.
- [ ] 2.4 Add redaction for MCP servers in `build_printable_config`.
- [ ] 2.5 Add the commented example to the `--init-config` template.

## Implementation Details
Modify `src/config/mod.rs`: new server structs near the runtime structs; extend `Features`/`RawFeatures`; extend `EffectiveConfig`; extend `apply_raw`/`into_effective`; extend `build_printable_config`; extend the `--init-config` starter string. Reference TechSpec "Data Models" (`McpServerConfig`) and the existing runtime ladder as the precise template. See ADR-002 (transport-agnostic config) and ADR-003 (config-first).

### Relevant Files
- `src/config/mod.rs` — `RawRuntimeConfig` (~496), `MergedRuntimeConfig` (~542), `RuntimeConfig` (~286), `Features` (~197), `EffectiveConfig` (~402), `apply_raw` (~909), `into_effective` (~1302), `build_printable_config` (~2035), init-config template (~2195).

### Dependent Files
- `src/mcp/supervisor.rs` (task_03) — reads `EffectiveConfig.mcp_servers` to spawn connections.
- `src/doctor/mod.rs` (task_10) — iterates `mcp_servers` for health checks.

### Related ADRs
- [ADR-002: stdio-first V1; defer HTTP + OAuth as one bundle to V1.1](../adrs/adr-002.md) — config is transport-agnostic; only stdio wired.
- [ADR-003: Config-first MVP product surface for V1](../adrs/adr-003.md) — TOML is the V1 surface.

## Deliverables
- `[mcp.servers.*]` config through the full Raw→Merged→Effective ladder.
- `features.mcp_enabled` flag (default off).
- `--print-config` redaction for MCP servers.
- `--init-config` example block.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration tests loading a multi-file config **(REQUIRED)**.

## Tests
- Unit tests:
  - [ ] A `[mcp.servers.fs]` with `transport=stdio` and a `command` parses into one effective server.
  - [ ] A stdio server missing `command` fails `into_effective` with a message naming the server id.
  - [ ] `features.mcp_enabled` defaults to `false` and flips to `true` when set.
  - [ ] `build_printable_config` shows `env` var names but omits any resolved secret value for an MCP server.
  - [ ] A home + local config both defining `mcp.servers.fs` merge per the ladder (local overrides home `args`).
- Integration tests:
  - [ ] `--print-config` on a config with an MCP server renders the server without leaking a secret.
  - [ ] `--init-config` output parses back into a valid config including the MCP example.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can declare an MCP server in `atelier.toml` and see it in `--print-config` (redacted), gated behind `features.mcp_enabled`.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
