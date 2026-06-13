---
status: pending
title: Add Provider Status Slash Command Metadata
type: backend
complexity: low
dependencies: []
---

# Task 1: Add Provider Status Slash Command Metadata

## Overview
This task adds `/provider:status` to the visible slash command catalog so users can discover the provider runway status command from the existing command surfaces. It deliberately keeps the catalog metadata-only and preserves existing unknown-command behavior.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add `/provider:status` to the existing slash command catalog with a compact description and usage string.
- MUST keep `src/slash_commands.rs` limited to command metadata and avoid adding dispatch or provider probing behavior there.
- MUST update fixed command label assertions and catalog ordering expectations deliberately.
- MUST preserve behavior for unrelated and unknown slash commands.
- SHOULD use the final command name from the TechSpec unless a project terminology decision changes it before implementation.
</requirements>

## Subtasks
- [ ] 1.1 Add the provider status command metadata to the slash command catalog.
- [ ] 1.2 Ensure the command description communicates compact provider runway/status intent.
- [ ] 1.3 Update command catalog tests that assert fixed labels, search results, or ordering.
- [ ] 1.4 Confirm no routing or provider-specific behavior is introduced in the metadata layer.
- [ ] 1.5 Verify unknown slash commands still follow the existing rejection path.

## Implementation Details
Modify the existing slash command catalog and tests only. Reference the TechSpec sections "Proposed User-Facing Command" and "Command Routing" for command name, scope, and the metadata-only boundary.

### Relevant Files
- `src/slash_commands.rs` - Existing visible slash command catalog and related command metadata tests.
- `.compozy/tasks/provider-usage-status/_prd.md` - Product requirements for the runway-first status command.
- `.compozy/tasks/provider-usage-status/_techspec.md` - Command metadata and routing guidance.

### Dependent Files
- `src/app/mod.rs` - Later routing work depends on the chosen visible command label.
- `src/runtime/status.rs` - Later runtime status work depends on the command label being separate from provider probing.

### Related ADRs
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) - Ensures command metadata does not imply exact quota support.
- [ADR-002: Runway-First Provider Usage Status](adrs/adr-002.md) - Establishes the user-facing runway/status framing.

## Deliverables
- `/provider:status` visible in the slash command catalog.
- Updated command catalog tests for command labels and metadata.
- Confirmation that slash command metadata remains dispatch-free.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for command discovery behavior **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Catalog includes exactly one `/provider:status` command entry.
  - [ ] `/provider:status` has a non-empty description and usage string.
  - [ ] Existing fixed-label command assertions are updated intentionally.
  - [ ] Unknown slash command metadata lookups still return the existing unknown result.
- Integration tests:
  - [ ] Command discovery or prompt-prefix dropdown includes `/provider:status` without altering unrelated command entries.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- `/provider:status` is discoverable through the existing slash command catalog.
- No provider probing, status formatting, or app routing is added to `src/slash_commands.rs`.
