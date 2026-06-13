---
status: completed
title: Implement provider status service and adapter boundary
type: backend
complexity: high
dependencies:
  - task_02
---

# Task 3: Implement provider status service and adapter boundary

## Overview
This task adds the runtime-layer service that discovers configured providers and asks provider adapters for lightweight runway status. It keeps provider-specific probing behind the runtime boundary so command routing and UI layers can consume typed provider status without knowing adapter details.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details — do not duplicate here
- FOCUS ON "WHAT" — describe what needs to be accomplished, not how
- MINIMIZE CODE — show code only to illustrate current structure or problem areas
- TESTS REQUIRED — every task MUST include tests in deliverables
</critical>

<requirements>
- The runtime status service MUST discover relevant providers from existing runtime or configuration state.
- The service MUST request provider status capabilities separately from provider status results.
- Provider-specific status checks MUST live near existing provider adapter modules, not in slash command metadata, TUI code, or app command routing.
- Provider checks MUST use bounded per-provider timeouts so one slow provider cannot block the full command result.
- Timeout, network, provider, authentication, and configuration failures MUST map to typed runway states without failing the whole command.
- Diagnostics returned to app or UI layers MUST be redacted and share-safe by default.
- The service MUST preserve provider-native meaning and MUST NOT infer exact quota from local activity, token counts, CLI history, recent errors, or failed command frequency.
</requirements>

## Subtasks
- [x] 3.1 Add a runtime status service entry point that returns one `ProviderRunwayStatus` per relevant provider. (`ProviderStatusService::collect_status`)
- [x] 3.2 Add provider adapter status hooks for the configured provider families covered by the existing runtime boundary. (`RuntimeAvailabilityProbe` reuses `check_runtime_availability` per-adapter dispatch)
- [x] 3.3 Add capability negotiation for auth, model availability, usage, rate limit, billing, incident, and local runtime health support. (`ProviderStatusCapabilities` + `provider_capabilities`, exposed via separate `capabilities()` hook)
- [x] 3.4 Add bounded timeout handling and partial-result behavior for slow or failing providers. (`with_timeout` + per-provider `tokio::time::timeout` over a `JoinSet`; `unresolved_status` row)
- [x] 3.5 Add redaction for provider diagnostics before status results leave the runtime layer. (`redact_secrets` applied to reason + diagnostics in `normalize_status`)
- [x] 3.6 Map adapter outcomes into the runway states and usage availability values defined by the runtime model. (`map_runtime_availability` + `classify_unavailable` + capability gate)

## Implementation Details
Create or update the runtime/provider status abstraction described in the TechSpec "Runtime Integration" and "Capability Negotiation" sections. Keep discovery, timeout handling, state normalization, and diagnostic redaction inside `src/runtime`, while provider-specific checks remain close to the existing Claude, Codex, Cursor, fake, and Z.ai adapter code.

### Relevant Files
- `src/runtime` — Runtime boundary that should own provider status types, service orchestration, and adapter integration.
- `src/runtime/status.rs` — Expected new module for provider status service behavior if it matches the surrounding module layout.
- `src/runtime/claude*` — Existing Claude adapter area that should expose Claude status capabilities and checks.
- `src/runtime/codex*` — Existing Codex adapter area that should expose Codex status capabilities and checks.
- `src/runtime/cursor*` — Existing Cursor adapter area that should expose Cursor status capabilities and checks.
- `src/runtime/fake*` — Deterministic fake adapter area for unit and routing tests.
- `src/runtime/zai*` — Existing Z.ai adapter area that should expose Z.ai status capabilities and checks.

### Dependent Files
- `src/app/mod.rs` — Submitted app command routing will call the status service after this task is complete.
- `src/slash_commands.rs` — Command metadata depends on the status service contract indirectly through the user-facing command.
- Runtime module declarations — New status modules must be exported consistently with existing runtime module structure.

### Related ADRs
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) — Requires truthful availability and usage semantics without unsupported quota inference.
- [ADR-002: Runway-First Provider Status](adrs/adr-002.md) — Requires runway-first states, provider-native reasons, and actionable next steps.

## Deliverables
- Runtime status service that returns typed provider runway status results.
- Provider adapter boundary for status capabilities and lightweight checks.
- Timeout and partial-result handling for provider status checks.
- Share-safe diagnostic redaction before results reach app/UI layers.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for provider status service behavior **(REQUIRED)**

## Tests
- Unit tests:
  - [x] Ready provider: valid auth/config/runtime signals produce `Ready` without exact usage unless supported usage data is returned. (`ready_provider_yields_ready_without_exact_usage`, `available_maps_to_ready_and_unknown_maps_to_unavailable_usage`)
  - [x] Unsupported usage: provider with no exact usage support produces `UnavailableUsage` or unsupported usage availability without invented remaining quota. (`exact_usage_without_capability_is_downgraded_to_unsupported`)
  - [x] Auth failure: invalid or missing credentials map to `Unauthenticated` with a user-actionable next action. (`auth_failure_maps_to_unauthenticated_with_next_action`)
  - [x] Missing provider/model config maps to `Misconfigured` without attempting unsupported provider calls. (`missing_config_maps_to_misconfigured`)
  - [x] Timeout maps only the affected provider to `ProviderError` or `UnavailableUsage` while other provider rows still return. (`slow_provider_times_out_without_blocking_others`)
  - [x] Diagnostics containing secrets, emails, account identifiers, or raw provider payloads are redacted before rendering boundaries. (`diagnostics_and_reasons_are_redacted_before_leaving_the_service`)
- Integration tests:
  - [x] Fake provider adapter returns deterministic status capabilities and status rows through the runtime service. (`tests/provider_status_service.rs::fake_adapter_returns_deterministic_rows_and_capabilities_through_service`)
  - [x] Multiple configured providers return independent rows when one provider succeeds and another fails. (`tests/provider_status_service.rs::multiple_providers_return_independent_rows_when_one_fails`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Runtime status service returns typed status results without formatted command strings as its core API.
- Provider-specific behavior is isolated behind runtime/provider adapter boundaries.
- A single slow or failing provider cannot block all provider status output.
- No default diagnostic output exposes secrets or account identifiers.
