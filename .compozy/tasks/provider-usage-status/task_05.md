---
status: pending
title: Route `/provider:status` Through Submitted App Commands
type: backend
complexity: medium
dependencies:
  - task_01
  - task_03
  - task_04
---

# Task 5: Route `/provider:status` Through Submitted App Commands

## Overview
This task makes the visible provider status command executable from Atelier's normal submitted command flow. It connects slash command metadata, the runtime provider status service, and compact status rendering without moving provider-specific probing into app or TUI command code.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- The submitted prompt flow MUST recognize `/provider:status` as a handled app command after command metadata, runtime status service, and formatter dependencies are in place.
- The command routing MUST call the provider status service through the runtime boundary described in the TechSpec rather than embedding provider-specific checks in `src/app/mod.rs` or TUI code.
- The command output MUST use the compact share-safe renderer from task_04 and preserve the same product meaning across terminal and UI surfaces.
- Unknown or unrelated slash commands MUST keep their existing behavior and must not be treated as provider status requests.
- Provider status command failures MUST render a useful provider status or command error without exposing secrets, account identifiers, raw provider payloads, or local credential values.
- The routed command SHOULD remain responsive by relying on bounded provider checks from the status service instead of adding unbounded work in the app command path.

## Subtasks
- [ ] 5.1 Add submitted command routing for the exact `/provider:status` command label.
- [ ] 5.2 Connect the route to the runtime provider status service from task_03.
- [ ] 5.3 Send the returned status results through the compact renderer from task_04.
- [ ] 5.4 Preserve current behavior for unrelated slash commands and ordinary prompts.
- [ ] 5.5 Ensure command errors and partial provider errors remain share-safe and actionable.
- [ ] 5.6 Add routing-level tests for handled and unhandled command paths.

## Implementation Details
Modify the existing submitted command flow in `src/app/mod.rs` and nearby app command tests. Reference the TechSpec sections "Command Routing", "Runtime Integration", and "Output Rules" for the intended boundary: app routing owns command recognition and response delivery, while provider discovery, probing, normalization, and redaction stay behind `src/runtime` status abstractions.

### Relevant Files
- `src/app/mod.rs` — Owns `App::submit_prompt` and the submitted app command routing surface called out by the TechSpec.
- `src/slash_commands.rs` — Provides command catalog metadata that should stay aligned with routed command labels.
- `src/runtime/status.rs` — Expected status model/service module created by prerequisite tasks.
- `src/runtime/mod.rs` — Expected runtime module export point for the provider status abstraction.
- App-level test files near `src/app/mod.rs` — Existing command routing tests should guide placement and naming for the new submitted command coverage.

### Dependent Files
- `src/slash_commands.rs` — Command metadata must remain synchronized with the routed command label.
- `src/runtime/status.rs` — Routing depends on the status service API and should not bypass it.
- Runtime adapter modules under `src/runtime` — Adapter status responses flow into this command through the status service.
- Existing TUI or prompt-prefix command surfaces — They should keep the same command meaning and not gain duplicate provider-specific status logic.

### Related ADRs
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) — Routing must preserve the no-inferred-quota constraint by using the runtime status service.
- [ADR-002: Runway-First Provider Usage Status](adrs/adr-002.md) — Routing must deliver the compact runway-first status experience instead of a diagnostics dump.

## Deliverables
- `/provider:status` is executable through `App::submit_prompt`.
- The command route delegates to the runtime status service and compact renderer.
- Existing unknown-command and ordinary prompt behavior remains intact.
- Safe error/partial-result handling for provider status command execution.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for submitted provider status command routing **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Exact command: submitting `/provider:status` invokes the provider status handler.
  - [ ] Trimmed command: submitting `/provider:status` with surrounding whitespace still reaches the intended route if existing command handling trims inputs.
  - [ ] Unknown command: submitting `/provider:unknown` keeps the existing unknown-command behavior.
  - [ ] Ordinary prompt: submitting a non-command prompt is not intercepted by provider status routing.
  - [ ] Service error: a provider status service error produces a share-safe command response.
- Integration tests:
  - [ ] Submitted command flow with deterministic fake provider statuses renders one compact row per configured fake provider.
  - [ ] Partial provider failure still returns available provider rows plus an actionable failed-provider row.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/provider:status` works from the normal submitted command flow.
- App routing contains no provider-specific quota, auth, model, billing, or runtime probing logic.
- Unknown slash commands and ordinary prompts behave as they did before this feature.
