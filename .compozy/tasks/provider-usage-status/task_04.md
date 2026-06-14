---
status: completed
title: Render Compact Share-Safe Provider Status Output
type: backend
complexity: medium
dependencies:
  - task_02
---

# Task 4: Render Compact Share-Safe Provider Status Output

## Overview
This task creates the formatter for provider runway status results so terminal and app command surfaces can show the same compact product meaning. It turns typed status data into a scan-friendly, share-safe summary without leaking secrets or implying exact usage where the provider integration does not support it.

<critical>
- ALWAYS READ the PRD and TechSpec before starting
- REFERENCE TECHSPEC for implementation details - do not duplicate here
- FOCUS ON "WHAT" - describe what needs to be accomplished, not how
- MINIMIZE CODE - show code only to illustrate current structure or problem areas
- TESTS REQUIRED - every task MUST include tests in deliverables
</critical>

<requirements>
- MUST render each `ProviderRunwayStatus` as exactly one compact default row containing provider display name, top-level status label, short reason, and one next action.
- MUST render unsupported or unavailable exact usage with explicit truthful language instead of estimating remaining quota.
- MUST render exact usage only when the typed result contains capability-gated `UsageAvailability::Exact` data.
- MUST keep default output share-safe by excluding secrets, tokens, account identifiers, email addresses, organization IDs, and raw provider payloads.
- MUST preserve local runtime health as local-only status and never present it as account usage.
- SHOULD include freshness or source confidence when the typed status result exposes that information in a compact form.
</requirements>

## Subtasks
- [x] 4.1 Add a compact formatter for typed provider runway status results. (`render_provider_status`)
- [x] 4.2 Map every provider runway state to stable user-facing status labels and concise wording. (`state_label`)
- [x] 4.3 Add explicit output for unsupported usage, unavailable usage, exact usage, unknown reset timing, and local-only status. (`render_detail`, `render_exact_usage`, `render_reset`)
- [x] 4.4 Add redaction or omission behavior for diagnostics and sensitive values in default output. (diagnostics omitted; name/reason via `redact_secrets`)
- [x] 4.5 Expose the formatter through a small API that command routing can call without knowing provider-specific details. (`render_provider_status(&[ProviderRunwayStatus]) -> String`)
- [x] 4.6 Add focused formatter tests for state labels, exact-usage gating, unsupported wording, and redaction.

## Implementation Details
Create or update the provider status rendering layer near the runtime status model, or in the existing app output formatting location if the repository has a clearer established pattern. Reference the TechSpec "Output Rules" and "Status Model" sections for the exact product constraints and avoid copying interface definitions into the task implementation.

### Relevant Files
- `src/runtime/status.rs` - Expected home for provider status types and a natural location for share-safe status rendering helpers if task 02 creates it.
- `src/runtime/mod.rs` - May need module exports so app command routing can use the formatter.
- `src/app/mod.rs` - Consumes formatted output from the submitted app command route in a later task.
- `src/slash_commands.rs` - Provides user-facing command metadata that should align with the rendered command behavior.

### Dependent Files
- `src/app/mod.rs` - Will depend on the formatter API when `/provider:status` is routed.
- `src/runtime/status.rs` - The renderer depends on the typed model and must stay aligned with status enum variants.
- Runtime or app test modules - Need deterministic expectations for compact output and redaction behavior.

### Related ADRs
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md) - Requires exact usage to appear only when supported by a documented provider surface.
- [ADR-002: Runway-First Provider Status](adrs/adr-002.md) - Requires the output to prioritize practical runway and next action over raw accounting.

## Deliverables
- Compact provider status rendering API for default command output.
- Stable status labels and wording for every provider runway state.
- Explicit unsupported exact usage and provider-native verification wording.
- Share-safe handling for diagnostics and sensitive values in default output.
- Unit tests with 80%+ coverage **(REQUIRED)**
- Integration tests for formatter consumption by command routing **(REQUIRED)**

## Tests
- Unit tests:
  - [x] `Ready` with unsupported usage renders ready wording and says exact remaining usage is unavailable. (`ready_with_unsupported_usage_renders_ready_wording`)
  - [x] `UnavailableUsage` with unsupported usage renders the explicit unsupported message and a provider-native verification action. (`unavailable_usage_renders_unsupported_message_and_verification_action`)
  - [x] `UsageAvailability::Exact` renders exact usage only when the typed usage value is exact. (`exact_usage_renders_only_when_usage_is_exact`)
  - [x] `LocalOnlyStatus` renders local runtime readiness without account usage wording. (`local_only_renders_without_account_usage_wording`)
  - [x] Diagnostics containing token-like, email-like, or account-like strings are omitted or redacted from default output. (`diagnostics_with_sensitive_strings_are_omitted_from_default_output`)
  - [x] Unknown reset timing renders as unknown rather than inferred text. (`unknown_reset_renders_as_unknown_not_inferred_text`)
- Integration tests:
  - [x] A deterministic list of provider statuses renders one compact row per provider in stable order when the service returns multiple providers. (`tests/provider_status_render.rs::renders_one_row_per_provider_in_stable_order`)
  - [x] Formatted output is suitable for the submitted app command surface without provider-specific branching in command metadata. (`tests/provider_status_render.rs::default_output_is_command_surface_ready_and_share_safe`)
- Test coverage target: >=80%
- All tests must pass

## Success Criteria
- All tests passing
- Test coverage >=80%
- Default output is compact, readable, and includes one row per provider.
- Unsupported exact usage is clearly described without false precision.
- Sensitive account or credential values cannot appear in the default rendered output.
