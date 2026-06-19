---
status: completed
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
- [x] 2.1 Add the `RawMcpServerConfig` / merged / effective `McpServerConfig` structs.
- [x] 2.2 Wire the section through `apply_raw` merge and `into_effective` validation.
- [x] 2.3 Add the `features.mcp_enabled` flag through `RawFeatures` → `Features`.
- [x] 2.4 Add redaction for MCP servers in `build_printable_config`.
- [x] 2.5 Add the commented example to the `--init-config` template.

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
- Unit tests (`src/config/mod.rs`):
  - [x] A `[mcp.servers.fs]` with `transport=stdio` and a `command` parses into one effective server. (`mcp_stdio_server_parses_into_one_effective_server`)
  - [x] A stdio server missing `command` fails `into_effective` with a message naming the server id. (`mcp_stdio_server_missing_command_fails_with_server_id`)
  - [x] `features.mcp_enabled` defaults to `false` and flips to `true` when set. (`mcp_enabled_defaults_false_and_flips_true`)
  - [x] `build_printable_config` shows `env` var names but omits any resolved secret value for an MCP server. (`printable_mcp_server_shows_env_names_not_values`)
  - [x] A home + local config both defining `mcp.servers.fs` merge per the ladder (local overrides home `args`). (`mcp_server_local_overrides_home_args`)
  - [x] _Extra:_ HTTP transport parses without a command (inert in V1). (`mcp_http_transport_parses_without_command`)
- Integration tests (`tests/cli.rs`):
  - [x] `--print-config` on a config with an MCP server renders the server without leaking a secret. (`print_config_redacts_mcp_server_env_value`)
  - [x] `--init-config` output parses back into a valid config including the MCP example. (`init_config_output_parses_back_including_mcp_example`)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **Config is confined to `src/config/mod.rs`.** Every `EffectiveConfig` construction outside `into_effective` (the doctor/app/test helpers) goes through `load_effective_config`/`into_effective`, so adding the `mcp_servers` field required no cross-file edits.
- **`env` redaction = names only.** `--print-config` renders each server's `env` as the sorted list of variable *names*; values (which may be literal secrets or `${VAR}` references) are never emitted, mirroring `api_key_env`. Verified by both a unit test and the CLI integration test (asserts the secret value is absent).
- **Symmetric in/out shape.** The printable nests servers under an `mcp.servers` table (`PrintableMcp`) so `--print-config` emits `[mcp.servers.<id>]` exactly as declared (like `[runtimes.<id>]`), rather than the raw Rust field name.
- **Layer merge mirrors runtimes.** `apply_mcp_server` replaces each present field per layer (a local config overrides home's `args`/`env` wholesale), consistent with `apply_runtime`.

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: all clean.
- 7 config unit tests + 2 CLI integration tests: pass.
- Full suite under a clean `HOME` (skipping the env-sensitive codex/cursor `availability` tests CLAUDE.md flags): **1318 passed, 4 ignored, 0 failed**. The only failures elsewhere are the documented environment-sensitive runtime-availability tests (pass under the real `HOME`: cursor 22/22) and the malformed-home-skill tests — neither touched by this task.

## Success Criteria
- All tests passing
- Test coverage >=80%
- A user can declare an MCP server in `atelier.toml` and see it in `--print-config` (redacted), gated behind `features.mcp_enabled`.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
