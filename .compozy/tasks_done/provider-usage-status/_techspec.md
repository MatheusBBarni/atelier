# Provider Usage Status TechSpec

Status: Draft
Date: 2026-06-12
PRD: [_prd.md](_prd.md)
ADRs:
- [ADR-001: Truthful Provider Usage Status Scope](adrs/adr-001.md)
- [ADR-002: Runway-First Provider Usage Status](adrs/adr-002.md)

## Summary

Implement a V1 provider usage status surface that answers whether each configured provider is usable right now and what the user should do next before starting or continuing a long Atelier session.

The V1 contract is availability and runway. Atelier reports provider availability, detected account/configuration state, recent relevant failures, and qualitative runway. Exact remaining quota, credits, messages, requests, or reset timing are shown only when a supported provider integration returns documented exact data with clear semantics. When exact usage is unsupported or unavailable, Atelier says that directly and provides a provider-native verification path instead of estimating.

The first implementation should fit the existing command architecture:

- Add visible command metadata in `src/slash_commands.rs`.
- Route submitted app commands through `App::submit_prompt` in `src/app/mod.rs`.
- Keep provider probing behind a runtime/provider status abstraction under `src/runtime` rather than embedding provider-specific behavior in slash command metadata or TUI code.
- Keep default output compact and share-safe.

## Goals

- Add a user-invoked provider runway status command or equivalent submitted app command.
- Report a practical status per relevant provider in under 15 seconds for median invocation.
- Preserve provider-native semantics for usage, billing, rate limits, auth, model availability, and local runtime health.
- Represent unsupported exact usage as a first-class truthful state.
- Avoid local-state quota inference from CLI history, recent failures, token counts, or activity patterns.
- Provide enough typed data for terminal and UI surfaces to render the same product meaning.

## Non-Goals

- Implementing a broad account center or background usage dashboard.
- Scraping dashboards or calling undocumented provider endpoints.
- Estimating Claude, Codex, or other subscription quota from local activity.
- Combining subscription quota, API billing usage, rate limits, provider incidents, auth state, and local runtime health into one global number.
- Long-term analytics, notifications, purchase flows, or provider plan management.

## Proposed User-Facing Command

Use `/provider:status` as the visible slash command label unless project terminology review selects a shorter command before implementation.

Default output should be compact:

```text
Provider status

Claude  ready              Exact usage unavailable from provider. Check Claude account usage before a long run.
Codex   unavailable usage  Auth ok; exact remaining quota unsupported. Check Codex account usage.
Z.ai    misconfigured      Missing provider credentials. Update provider config.
Local   local-only status  Runtime reachable; account usage does not apply.
```

Each row must include:

- Provider display name.
- Top-level status label.
- Source/freshness confidence in compact form when known.
- Short reason.
- One next action.

Detailed output can be added later, but V1 should not require users to read a diagnostics dump to decide whether to proceed, wait, reduce scope, or switch providers.

## Status Model

### ProviderRunwayStatus

Provider status results should use a typed model rather than formatted strings as the core API.

```rust
pub struct ProviderRunwayStatus {
    pub provider_id: ProviderId,
    pub display_name: String,
    pub selected_model: Option<String>,
    pub state: ProviderRunwayState,
    pub reason: ProviderStatusReason,
    pub next_action: ProviderNextAction,
    pub usage: UsageAvailability,
    pub reset: ResetAvailability,
    pub freshness: StatusFreshness,
    pub source: StatusSource,
    pub diagnostics: Vec<ProviderDiagnostic>,
}
```

### ProviderRunwayState

Use a small fixed state set aligned to the PRD:

```rust
pub enum ProviderRunwayState {
    Ready,
    LimitedRunway,
    Blocked,
    UnavailableUsage,
    Unauthenticated,
    Misconfigured,
    ProviderError,
    LocalOnlyStatus,
}
```

State semantics:

- `Ready`: Required local/provider checks passed and no blocking signal is known.
- `LimitedRunway`: Provider-supplied data or documented provider signal says usage is constrained, near limit, rate-limited, or reset-bound.
- `Blocked`: Provider-supplied or runtime signal says the provider cannot be used for the intended session.
- `UnavailableUsage`: Provider may be usable, but exact remaining usage is unsupported, unavailable, or not exposed.
- `Unauthenticated`: Credentials, account link, or login state is missing or invalid.
- `Misconfigured`: Local provider configuration is missing, invalid, or points to an unusable model/provider.
- `ProviderError`: A live check failed due to provider-side or network/provider API behavior that cannot be classified more specifically.
- `LocalOnlyStatus`: Local runtime readiness can be described, but account usage does not apply.

### UsageAvailability

```rust
pub enum UsageAvailability {
    Exact(ExactUsage),
    Unsupported { provider_url: Option<String> },
    Unavailable { reason: String, provider_url: Option<String> },
    NotApplicable,
}

pub struct ExactUsage {
    pub metric: UsageMetric,
    pub remaining: UsageAmount,
    pub limit: Option<UsageAmount>,
    pub window: Option<UsageWindow>,
    pub reset_at: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,
}
```

`Exact` is allowed only when a provider adapter marks the data source as supported and documented for the returned metric. Do not create `Exact` from local CLI state, token logs, request counts, errors, or failed command frequency.

### Capability Negotiation

Each runtime/provider adapter should declare status capabilities separately from the status result.

```rust
pub struct ProviderStatusCapabilities {
    pub auth_state: CapabilitySupport,
    pub model_availability: CapabilitySupport,
    pub subscription_usage: CapabilitySupport,
    pub api_billing_usage: CapabilitySupport,
    pub rate_limits: CapabilitySupport,
    pub local_runtime_health: CapabilitySupport,
    pub provider_incident_state: CapabilitySupport,
}

pub enum CapabilitySupport {
    Supported,
    Unsupported,
    RequiresAccountLink,
    RequiresConfiguration,
}
```

This prevents UI code from treating unsupported exact quota as a failed check and prevents adapters from normalizing incompatible provider concepts into a synthetic percentage.

## Runtime Integration

Add a provider status module under `src/runtime`, for example `src/runtime/status.rs`, and expose it through the runtime boundary used by existing adapters.

Responsibilities:

- Discover relevant configured providers from existing config/runtime state.
- Ask each provider adapter for capabilities.
- Run lightweight checks with bounded timeouts.
- Normalize adapter results into `ProviderRunwayStatus` without losing provider-native meaning.
- Redact secrets and account identifiers before returning diagnostics to app/UI layers.

Provider-specific implementation should live near the existing adapter modules for `claude`, `codex`, `cursor`, `fake`, and `zai`. The command layer should call the status service and format results; it should not know provider-specific probing details.

## Command Routing

Repository context shows `src/slash_commands.rs` is a metadata-only catalog and does not dispatch commands. Execution is split between TUI-local handling, `App::submit_prompt`, and prompt-prefix dropdowns.

Implementation steps:

1. Add `/provider:status` metadata to `src/slash_commands.rs` with a compact description and usage string.
2. Update slash command tests that assert the fixed command labels.
3. Add `App::submit_prompt` routing for `/provider:status`.
4. Route execution to a status service behind the runtime boundary.
5. Render a compact summary into the existing app output surface used by submitted app commands.
6. Preserve unknown-command behavior for unrelated slash commands.

Aliases can be considered after V1. If an alias is added, tests should cover both command labels and routing behavior.

## Output Rules

Default output must be share-safe:

- Do not include secrets, tokens, account identifiers, email addresses, organization IDs, or raw provider payloads.
- Prefer provider display names and model names already visible in local configuration.
- Redact diagnostic values by default.
- Show exact usage only when capability-gated and documented.
- Show freshness when exact usage or live checks are older than the current invocation.
- Say `unknown` for reset timing if the provider does not return it.

Recommended wording for unsupported exact usage:

```text
Exact remaining usage is unavailable from this provider integration. Verify usage in the provider account page before a long run.
```

Recommended wording for ready without exact quota:

```text
Ready based on auth/config/runtime checks. Exact remaining usage is unavailable from this provider integration.
```

## Freshness and Timeouts

The command should target the PRD metric of median result under 15 seconds.

- Use per-provider bounded checks so one slow provider does not block all output.
- Return partial results with `ProviderError` or `UnavailableUsage` when a provider check times out.
- Include `observed_at` for live data and `cached_at` if a future cache is introduced.
- Do not introduce a background monitor in V1.

If caching is not implemented in V1, omit cached status entirely rather than showing stale local state as current provider usage.

## Configuration

Initial implementation should use existing provider configuration as the source of relevant providers. Add new config only if necessary to support:

- Enabling/disabling live provider checks.
- Provider-native usage/account URLs.
- Per-provider timeout bounds.
- Account-linking metadata for providers that support exact usage.

Any added config must avoid storing secrets in plain output and must not be required for providers that can only return local readiness.

## Errors and Diagnostics

Classify errors before rendering:

- Authentication failure -> `Unauthenticated`.
- Missing provider/model config -> `Misconfigured`.
- Provider says quota/rate/billing exhausted -> `LimitedRunway` or `Blocked` based on provider semantics.
- Provider usage endpoint unsupported -> `UnavailableUsage` with `Unsupported` usage.
- Network/provider outage/ambiguous provider failure -> `ProviderError`.
- Local command/runtime unavailable -> `Misconfigured` or `LocalOnlyStatus`, depending on provider type.

Recent provider errors can inform the reason field, but must not be converted into exact quota numbers.

## Testing Plan

Add focused tests at the existing command and runtime boundaries:

- `src/slash_commands.rs`: command catalog includes `/provider:status`; fixed-label assertions are updated deliberately.
- `src/app/mod.rs`: `App::submit_prompt` routes `/provider:status` to the status handler.
- Unknown command tests continue to reject unrelated commands.
- Runtime status service maps provider adapter responses into each `ProviderRunwayState`.
- Exact usage is rendered only when `UsageAvailability::Exact` is returned by a supported capability.
- Unsupported exact usage renders the explicit unsupported message and provider-native verification action.
- Redaction tests confirm secrets/account identifiers do not appear in default output.
- Timeout or provider error tests return partial output instead of failing the entire command.

Use fake provider adapters for deterministic command and rendering tests. Provider-specific live integration tests should be opt-in and should not gate normal CI unless stable account surfaces exist.

## Acceptance Criteria

- A user can invoke the provider runway/status command from the normal submitted command flow.
- Each configured relevant provider returns exactly one compact status row in default output.
- The command distinguishes ready, limited runway, blocked, unavailable usage, unauthenticated, misconfigured, provider error, and local-only status.
- Exact usage appears only for capability-gated provider-supported data.
- Providers without exact usage support clearly say exact remaining usage is unavailable or unsupported.
- Every non-ready state includes a reason and next action.
- Local runtime health is never presented as provider account usage.
- Default output is safe to paste into logs or support requests.
- Median expected invocation remains under 15 seconds with provider timeouts configured.

## Open Implementation Questions

- Confirm final user-facing command name against project terminology guidance before coding.
- Decide whether V1 needs a detailed flag/view or only the compact default output.
- Decide exact provider-native verification URLs for Claude, Codex, Z.ai, Cursor, and any local providers.
- Decide whether provider status results should be exposed to UI state immediately or remain command-output only for MVP.
- Decide whether account linking exists in V1 or is deferred until a provider with documented exact usage is selected.

## Verification Notes

This TechSpec is based on the approved PRD, ADR-001, ADR-002, and repository findings from the planning pass:

- `src/slash_commands.rs` is metadata-only and needs a deliberate catalog/test update for new visible commands.
- `src/app/mod.rs` owns submitted app command routing and is the closest routing/test surface.
- `src/runtime` already owns provider adapter boundaries for Claude, Codex, Cursor, fake, and Z.ai.
- No existing quota, subscription, remaining-credit, or provider usage telemetry surface was found in the planning pass.
- The selected V1 scope is availability and runway, not exact quota as a universal product promise.
