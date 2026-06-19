---
status: completed
title: references/config-schema.md — whole-config schema reference
type: docs
complexity: medium
dependencies: []
---

# references/config-schema.md — whole-config schema reference

## Overview
Author the accuracy-critical `references/config-schema.md`: a complete, annotated reference for every `atelier.toml` section, every enum (by serde name), the merge order, the file locations, and the `schema_version`. Because the loader rejects unknown fields, this doc is the single biggest lever on the PRD's "0 hallucinated keys / ≥90% first-attempt validity" metrics, so it must mirror `src/config/mod.rs` exactly.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST document every config section: `schema_version`, `approval_mode`, `[approval]`, `[features]`, `[ui]`, `[workspace]`, `[runtimes.*]`, `[mcp.servers.*]`, `[limits]`, `[council]` (+ `council.presets.*`), `[agents.*]`, `[presets.*]` — each field with its type and default.
- MUST document all five enums **by serde name, sourced from the CURRENT `src/config/mod.rs` (not the techspec snapshot)**: `RuntimeKind` (codex/claude/cursor/zai/fake), `ApprovalMode` (yolo/normal), `AgentEffort` (minimal/low/medium/high/xhigh), `Capability` (plan/read/answer/challenge/edit/command/verify/review/**mcp_tool**), `ToolName` (read_file/list_files/search_text/run_command/apply_patch/write_file/record_note/**call_mcp_tool**/**read_mcp_resource**/**list_mcp_resources**).
- MUST document the MCP additions that landed in the `mcp-integration` packet: `features.mcp_enabled`, the `[mcp.servers.<id>]` section (`transport` = stdio|http, `command`, `args`, `env` table of name→value, inert `url`), and the `mcp_tool` capability — so the drift test (task_06) passes and generated configs covering MCP load.
- MUST document the merge order (built-in defaults → home `~/.config/.atelier/atelier.toml` → local `./atelier.toml` → CLI flags) and the config file locations.
- MUST state `schema_version = 1` exactly once in a form the drift test can assert.
- MUST contain only valid, loadable ` ```toml ` example blocks (no stray/typo keys, no invalid enum values).
- MUST NOT reproduce Rust type definitions verbatim — describe fields/types in prose + annotated TOML.
</requirements>

## Subtasks
- [x] 2.1 Document the scalar/top-level sections (`schema_version`, `approval_mode`, `[approval]`, `[features]`, `[ui]`, `[workspace]`, `[limits]`).
- [x] 2.2 Document `[runtimes.*]` (per-kind fields) and the `[mcp.servers.*]` section + `features.mcp_enabled`.
- [x] 2.3 Document `[agents.*]`, `[presets.*]`, and `[council]` (+ council presets/members).
- [x] 2.4 Document all five enums by serde name (anchored to current `src/config/mod.rs`), the merge order, and file locations.
- [x] 2.5 Provide annotated, individually-loadable ` ```toml ` examples; confirm each parses mentally against the loader.

## Implementation Details
Create `skills/atelier-config-setup/references/config-schema.md`. Treat `src/config/mod.rs` as ground truth — read the `Raw*`/effective structs and the five enums, and the `[mcp.servers.*]` ladder + `features.mcp_enabled` that the `mcp-integration` packet added. The techspec's "Core Interfaces" enum snippet predates MCP and is **stale** (it lists 8 `Capability`/7 `ToolName` variants); use the current source. See TechSpec "Data Models (references/config-schema.md)" and ADR-005 (enum/TOML drift guard).

### Relevant Files
- `src/config/mod.rs` — the five enums (`RuntimeKind` ~307, `ApprovalMode` ~27, `AgentEffort` ~324, `Capability` ~54 incl. `McpTool`, `ToolName` ~71 incl. the 3 MCP tools); the section structs (`Features`, `UiConfig`, `WorkspacePolicy`, `Limits`, `RuntimeConfig`, `McpServerConfig`, `AgentProfile`, `CouncilConfig`); `schema_version` (=1 at merge ~1956); merge order in `load_effective_config`.
- `src/config/mod.rs` `starter_config_text()` — the `--init-config` template, a good annotated example baseline.

### Dependent Files
- `tests/atelier_config_skill.rs` (task_06) — enum-coverage + TOML-load tests read this file.
- `skills/atelier-config-setup/SKILL.md` (task_01) — points here.

### Related ADRs
- [ADR-005: Skill correctness via a lightweight enum/TOML drift guard](../adrs/adr-005.md)
- [ADR-001: Whole-config schema reference, anti-drift posture](../adrs/adr-001.md)

## Deliverables
- `skills/atelier-config-setup/references/config-schema.md` covering 100% of sections + all 5 enums (incl. the MCP surface), merge order, file locations, and `schema_version = 1`.
- Every embedded ` ```toml ` block is a valid, loadable config.
- Tests asserting enum coverage + TOML-load **(REQUIRED)** — implemented in task_06.

## Tests
(Asserted by the task_06 module.)
- Unit tests:
  - [ ] Every serde variant of all 5 enums appears in this doc, with no stray/unknown variant strings. (task_06 enum-coverage; will fail if `mcp_tool` / the 3 MCP tools are omitted)
  - [ ] The documented `schema_version` equals `1`. (task_06)
- Integration tests:
  - [ ] Every ` ```toml ` block in this doc loads via `load_effective_config`/`RawConfig` without error. (task_06 TOML-load)
- Test coverage target: >=80% (of the documented config surface)
- All tests must pass

## Success Criteria
- All tests passing (via task_06)
- 100% of the 6+ sections and all 5 enums (current variants, incl. MCP) are documented accurately.
- A user/agent can author any documented section without hitting an unknown-field/enum error.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
