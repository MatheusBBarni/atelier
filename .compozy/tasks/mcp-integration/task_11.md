---
status: pending
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
- [ ] 11.1 Detect malformed MCP calls and build the structured repair re-prompt.
- [ ] 11.2 Cap repair attempts and route the final failure to the existing parse-error path.
- [ ] 11.3 Add the per-runtime degrade-not-abandon flag and honor it in the decision loop.
- [ ] 11.4 Build the emission-spike harness (high-tool-count fake server, smallest model, p95 with repair).

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
- Unit tests:
  - [ ] A `CallMcpTool` missing a required arg triggers exactly one repair re-prompt containing the tool schema and the validator diagnostic.
  - [ ] A still-malformed call after the capped repair routes to the existing parse-error path (no infinite loop).
  - [ ] With `degrade_not_abandon=true`, a runtime that cannot emit a valid call skips the tool and the run continues; with it false, the run surfaces the failure.
- Integration tests:
  - [ ] Using the fake runtime emitting a malformed-then-valid call, the repair loop recovers and the tool executes.
  - [ ] The emission-spike harness runs against the high-tool-count fake server and reports a p95 emission-with-repair figure.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Malformed tool calls are repaired or degraded gracefully; the spike harness produces a measurable p95 emission figure for the weakest model.
- `cargo fmt --check` and `cargo clippy --all-targets` are clean.
