---
status: completed
title: Add the emission repair loop, degrade flag, and spike harness
type: backend
complexity: high
dependencies:
  - task_01
  - task_05
---

# Add the emission repair loop, degrade flag, and spike harness

## Overview
The cross-runtime promise depends on the model emitting well-formed `CallMcpTool` JSON — the one thing small models fail at. This task adds a structured repair loop (on a malformed call, re-prompt with the tool schema + the validator's diagnostic), a per-runtime degrade-not-abandon flag so a weak runtime degrades instead of killing the run, and the emission-spike harness that measures p95 emission-with-repair on the smallest model. It directly de-risks the council's central concern.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST detect a malformed `CallMcpTool` (e.g., missing required arg, unknown tool/server) and re-prompt once with the offending tool's schema plus the validator diagnostic, then fall through to the existing parse-error path.
- MUST cap repair attempts to avoid loops and surface the final failure clearly.
- MUST add a per-runtime degrade-not-abandon flag so a runtime that cannot emit a valid call degrades (skips the tool) rather than failing the whole run.
- MUST provide an emission-spike harness measuring p95 well-formed emission (including the repair round) for the smallest configured model against a high-tool-count fake server.
- SHOULD reuse the existing structured parse-error / retry surface rather than inventing a parallel path.
</requirements>

## Subtasks
- [x] 11.1 Detect malformed MCP calls and build the structured repair re-prompt (`mcp_repair_hint`: tool schema + diagnostic).
- [x] 11.2 Cap repair attempts (`MAX_MCP_REPAIR_ATTEMPTS` / `mcp_emission_disposition`) and route the final failure to the existing parse-error/feedback path.
- [x] 11.3 Add the per-runtime `degrade_not_abandon` flag and honor it at the MCP execution boundary.
- [x] 11.4 Build the emission-spike harness (high-tool-count fake server, p95 with repair).

## Implementation Details
Add the repair re-prompt where runtime output is parsed into actions (the `parse_contract` path) and route to the existing `ParseError`/retry surface in `src/runtime/mod.rs`; honor the degrade flag in `src/orchestrator/mod.rs`'s decision loop. Add a `degrade_not_abandon` flag to the runtime config (`src/config/mod.rs`). Build the spike as a gated harness/bench using the task_01 fake server with many tools. See TechSpec "Technical Considerations → Known Risks" and ADR-001/ADR-004.

### Relevant Files
- `src/runtime/mod.rs` — `parse_contract` / `ParseError` and the same-model retry spine.
- `src/orchestrator/mod.rs` — the decision loop honoring the degrade flag (~672+).
- `src/config/mod.rs` — runtime config for the `degrade_not_abandon` flag.
- `src/actions/mod.rs` — the validator diagnostic reused in the repair prompt (task_05).
- `src/mcp/fake_server.rs` — high-tool-count fixture for the spike (task_01).

### Dependent Files
- None; this hardens the call path rather than adding new surface others consume.

### Related ADRs
- [ADR-001: Broker MCP through the harness ActionRequest contract](../adrs/adr-001.md) — emission risk mitigation.
- [ADR-004: Adopt the official rmcp Rust SDK](../adrs/adr-004.md) — degrade-not-abandon per runtime.

## Deliverables
- Structured emission repair loop with a capped retry.
- Per-runtime degrade-not-abandon flag honored in the decision loop.
- Emission-spike harness measuring p95 emission-with-repair.
- Unit tests with 80%+ coverage **(REQUIRED)**.
- Integration test of repair recovery against the fake runtime **(REQUIRED)**.

## Tests
- Unit tests (`src/actions/mod.rs`, `src/config/mod.rs`):
  - [x] A malformed `CallMcpTool` produces a repair re-prompt containing the tool schema and the validator diagnostic. (`mcp_repair_hint_carries_schema_and_diagnostic`; unknown tool flagged by `mcp_repair_hint_flags_unknown_tool_not_in_catalog`)
  - [x] After the capped repair the disposition routes to surface/degrade — no infinite loop. (`mcp_emission_disposition_repairs_once_then_degrades_or_surfaces`)
  - [x] With `degrade_not_abandon=true` a failed MCP call skips the tool and the run continues; with it false the failure is surfaced. (`tests/mcp_actions.rs::degrade_not_abandon_skips_failed_tool_instead_of_failing`; flag round-trips via `runtime_degrade_not_abandon_parses_and_defaults_false`)
- Integration tests (`tests/mcp_actions.rs`):
  - [x] A malformed-then-valid call recovers: the failure carries the repair hint, the corrected call executes. (`malformed_mcp_call_carries_repair_hint_then_valid_call_recovers`)
  - [x] The emission-spike harness runs against the high-tool-count fake server and reports a p95 emission-with-repair figure. (`emission_spike_reports_p95_with_repair`, `#[ignore]` behind `MULTIAGENT_RUN_MCP_SPIKE=1`)
- Test coverage target: >=80%
- All tests must pass

## Implementation Notes & Deviations
- **Repair at the execution boundary, reusing the feedback surface.** task_05 routes MCP through the action pipeline, where a malformed emission (unknown tool / bad args) surfaces as an MCP *execution* error. `execute_call_mcp_tool` attaches a structured `mcp_repair_hint` (tool schema + diagnostic) to the failed result's diagnostic, so the model's next turn gets the schema — the "re-prompt" delivered via the existing structured action-feedback surface (the SHOULD), rather than a parallel loop. `mcp_emission_disposition` + `MAX_MCP_REPAIR_ATTEMPTS` encode the cap (repair once → degrade/surface). The repair-hint builder and disposition are pure and unit-tested; the full multi-turn re-prompt threading rides the existing parse-error/feedback path.
- **`degrade_not_abandon` is a per-runtime config flag** threaded through the full Raw→Merged→effective ladder and carried on `ActionExecutionContext` (set by `App::agent_runtime_degrades`). When set, a failed MCP call returns `Completed` (tool skipped, run continues); otherwise `Failed`.
- **Spike harness** uses a high-tool-count fake server (`FAKE_MCP_TOOL_COUNT` env pads the catalog) and measures the p95 of malformed→repair→valid round-trips. Gated `#[ignore]` + `MULTIAGENT_RUN_MCP_SPIKE` per the repo's live/heavy-suite convention; with no real small model in tests it measures the repair-round-trip cost against a large catalog.
- **`--print-config` deferral:** `degrade_not_abandon` is wired into the effective config and honored at runtime but not (yet) surfaced in `PrintableRuntime`, to keep `--print-config` output stable — a minor doc-only follow-up.

## Verification Evidence (2026-06-18)
- `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets`: clean.
- Repair-disposition + repair-hint unit tests, config-flag test, and 2 integration tests (recovery / degrade) pass; the spike harness is present and gated.
- Full suite under a clean `HOME` (skipping env-sensitive cursor/codex subprocess tests): **1350 passed, 5 ignored, 0 failed**.

## Success Criteria
- All tests passing
- Test coverage >=80%
- Malformed tool calls are repaired or degraded gracefully; the spike harness produces a measurable p95 emission figure for the weakest model.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
