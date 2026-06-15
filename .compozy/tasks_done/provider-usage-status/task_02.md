---
status: completed
title: Define Runtime Provider Status Model
type: backend
complexity: medium
dependencies: []
---

# Task 2: Define Runtime Provider Status Model

## Overview
Define the typed provider runway status model under the runtime boundary so provider readiness, usage availability, freshness, source, and diagnostics have a stable internal contract. This task establishes the shared data shape required by later provider probing, command routing, and rendering work without embedding provider-specific behavior in UI or slash command metadata.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- MUST add runtime-owned provider status types that represent the TechSpec status model, including provider identity, display name, selected model, state, reason, next action, usage, reset, freshness, source, and diagnostics.
- MUST include the fixed runway states Ready, LimitedRunway, Blocked, UnavailableUsage, Unauthenticated, Misconfigured, ProviderError, and LocalOnlyStatus.
- MUST model exact usage as available only through an explicit UsageAvailability::Exact-style state and keep unsupported, unavailable, and not-applicable usage distinct.
- MUST model capability support separately from individual status results so unsupported usage is not treated as a failed provider check.
- MUST keep the model independent from terminal formatting, TUI widgets, slash command metadata, or provider-specific probing logic.
- MUST ensure diagnostics can be represented without requiring secrets, account identifiers, raw provider payloads, emails, or organization IDs.
</requirements>

## Subtasks
- [x] 2.1 Add a runtime status module for provider runway status types.
- [x] 2.2 Define fixed enums and structs for runway state, usage availability, reset availability, freshness, source, reason, next action, capabilities, and diagnostics.
- [x] 2.3 Connect the new module through the existing runtime module exports so later tasks can consume it.
- [x] 2.4 Add deterministic unit tests for state, capability, and usage modeling behavior.
- [x] 2.5 Confirm exact usage cannot be represented without the explicit exact usage variant and supporting provider capability context.

## Implementation Details
Create or update runtime-owned files only. Reference the TechSpec Status Model and Capability Negotiation sections for required fields and semantics instead of duplicating every interface detail here. The result should be a small, reusable model surface that later tasks can use for provider service implementation, command formatting, and app routing.

### Relevant Files
- `src/runtime/mod.rs` — Runtime module export boundary where a new status module should be exposed.
- `src/runtime` — Existing provider adapter boundary for Claude, Codex, Cursor, fake, and Z.ai integrations.
- `.compozy/tasks/provider-usage-status/_techspec.md` — Defines the target status model, capability negotiation shape, and exact-usage constraints.
- `.compozy/tasks/provider-usage-status/_prd.md` — Defines the product states and truthful runway requirements that the model must preserve.

### Dependent Files
- `src/runtime/status.rs` — New runtime status model module expected to be consumed by service and rendering tasks.
- `src/app/mod.rs` — Later routing work will call into runtime status behavior using these types.
- `src/slash_commands.rs` — Later command metadata work remains independent, but the command ultimately depends on this model through routing.

### Related ADRs
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) — Requires exact quota to be shown only when supported provider data exists.
- [ADR-002: Runway-First Provider Usage Status](adrs/adr-002.md) — Requires provider status to prioritize practical runway states and next actions.

## Deliverables
- Runtime provider status model module with typed runway status, usage, reset, freshness, source, capability, and diagnostic data.
- Runtime module exports that make the model available to later provider status service and command routing work.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for model consumption by later provider status service work **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Exact usage can only be represented through the explicit exact usage variant with observed timestamp data. (`exact_usage_is_only_reachable_through_the_exact_variant_with_a_timestamp`)
  - [x] Unsupported, unavailable, and not-applicable usage states remain distinct in equality or matching assertions. (`unsupported_unavailable_and_not_applicable_usage_are_distinct`)
  - [x] Every required runway state can be constructed and matched without string parsing. (`every_required_runway_state_can_be_constructed_and_matched`)
  - [x] Capability support variants cover supported, unsupported, requires-account-link, and requires-configuration cases. (`capability_support_covers_all_four_cases`)
  - [x] Provider diagnostics can be represented without storing raw secret-like values in the default data path. (`diagnostics_carry_only_share_safe_label_and_detail`)
- Integration tests:
  - [x] A fake runtime status result can be built from the exported runtime model and consumed outside the status module. (`tests/provider_status_model.rs::fake_runtime_status_can_be_built_from_the_exported_model`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Provider status data has a typed runtime-owned contract before any provider probing or rendering work begins.
- Unsupported exact usage is represented as a truthful status, not as an error or inferred quota.
