---
status: pending
title: Add focused provider status verification coverage
type: test
complexity: high
dependencies:
  - task_01
  - task_02
  - task_03
  - task_04
  - task_05
---

# Task 6: Add focused provider status verification coverage

## Overview
This task verifies the complete `/provider:status` flow after the command metadata, runtime model, status service, formatter, and submitted-command routing are in place. It matters because the feature is explicitly trust-sensitive: unsupported exact usage, share-safe output, and partial provider failures must be tested so Atelier does not report false precision or leak sensitive provider details.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- The verification suite MUST prove that `/provider:status` appears in the visible slash command catalog and that unrelated unknown slash commands remain rejected.
- The verification suite MUST prove that `App::submit_prompt` routes `/provider:status` through the provider status handler without changing normal prompt submission behavior.
- The verification suite MUST cover every `ProviderRunwayState`: ready, limited runway, blocked, unavailable usage, unauthenticated, misconfigured, provider error, and local-only status.
- Exact usage rendering MUST be tested as allowed only when the runtime status model returns supported exact usage data.
- Unsupported or unavailable exact usage MUST render explicit truthful wording and a provider-native verification action, not an inferred quota.
- Default output MUST be tested for redaction of secrets, tokens, account identifiers, email addresses, organization IDs, and raw provider payload details.
- Timeout or provider-error scenarios MUST be tested to return partial provider status output instead of failing the whole command.
</requirements>

## Subtasks
- [ ] 6.1 Add catalog-level tests for `/provider:status` metadata and fixed command-label expectations.
- [ ] 6.2 Add submitted-command routing tests that exercise `/provider:status` from `App::submit_prompt`.
- [ ] 6.3 Add runtime status mapping tests using deterministic fake provider responses for all runway states.
- [ ] 6.4 Add formatter tests for exact usage gating, unsupported usage wording, freshness/reset display, and next-action output.
- [ ] 6.5 Add redaction tests proving sensitive diagnostics and account data are omitted from default output.
- [ ] 6.6 Add timeout and partial-result tests proving one slow or failed provider does not suppress other provider rows.

## Implementation Details
Add focused tests at the command, app-routing, runtime-status, and formatting boundaries created by the preceding tasks. Reference the TechSpec "Testing Plan", "Output Rules", "Errors and Diagnostics", and "Freshness and Timeouts" sections for the expected behavior; do not duplicate provider interface definitions from the TechSpec in test fixtures.

### Relevant Files
- `src/slash_commands.rs` - Command catalog tests should confirm `/provider:status` metadata and preserve existing command expectations.
- `src/app/mod.rs` - Submitted-command tests should verify `/provider:status` routing and unknown-command behavior.
- `src/runtime/status.rs` - Runtime status model and service tests should cover state mapping, exact usage gating, and partial failures.
- `src/runtime` - Provider adapter test doubles or fake status implementations should live near the runtime/provider boundary.
- `.compozy/tasks/provider-usage-status/_prd.md` - Source of product requirements for truthful runway status and share-safe output.
- `.compozy/tasks/provider-usage-status/_techspec.md` - Source of implementation and testing guidance for this feature.

### Dependent Files
- `src/slash_commands.rs` - Test expectations may need updates when the visible command list changes.
- `src/app/mod.rs` - Routing tests may require small test harness helpers around submitted app commands.
- `src/runtime/status.rs` - Test fixtures may require public or crate-visible constructors for deterministic provider status values.
- `Cargo.toml` - Only affected if existing test utilities require an additional dev dependency; avoid new dependencies unless already consistent with the project.

### Related ADRs
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) - Tests must enforce no inferred exact quota for unsupported providers.
- [ADR-002: Runway-First Provider Usage Status](adrs/adr-002.md) - Tests must enforce runway-first states and actionable next steps.

## Deliverables
- Command catalog tests covering `/provider:status` metadata and unchanged unknown-command behavior.
- App routing tests covering submitted `/provider:status` execution through the normal submitted-command flow.
- Runtime status tests covering every runway state, exact usage gating, unsupported usage, provider errors, and partial timeout output.
- Formatter/redaction tests covering share-safe default output and provider-native next-action wording.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for provider status submitted-command flow **(REQUIRED)**

## Tests
- Unit tests:
  - [ ] Slash command catalog includes exactly one `/provider:status` entry with the expected description and usage metadata.
  - [ ] Unknown slash command input such as `/provider:not-real` still follows the existing rejection path.
  - [ ] Runtime status mapping returns the expected `ProviderRunwayState` for fake ready, limited, blocked, unauthenticated, misconfigured, provider-error, unavailable-usage, and local-only provider responses.
  - [ ] Formatter renders exact remaining usage only for `UsageAvailability::Exact` returned through supported capability data.
  - [ ] Formatter renders explicit unsupported exact-usage wording and a provider-native verification action for unsupported usage.
  - [ ] Redaction removes fake tokens, account IDs, organization IDs, email addresses, and raw provider payload fields from default output.
  - [ ] Timeout handling marks the affected provider as provider error or unavailable usage while preserving other provider rows.
- Integration tests:
  - [ ] Submitted `/provider:status` produces one compact status row per configured fake provider through `App::submit_prompt`.
  - [ ] Submitted `/provider:status` with mixed fake provider outcomes returns partial output without failing the entire command.
  - [ ] Submitted non-status slash commands continue to use the existing routing behavior.
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Verification demonstrates that no unsupported provider can display inferred exact usage.
- Verification demonstrates that default provider status output is share-safe.
- Verification demonstrates that the submitted `/provider:status` command remains useful when one provider check fails or times out.
