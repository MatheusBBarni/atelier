//! Runtime-owned provider runway status model.
//!
//! This module defines the typed contract that describes whether each
//! configured provider is usable right now and what the user should do next,
//! per the Provider Usage Status TechSpec and ADR-001 / ADR-002. It is
//! deliberately a *data* surface only: it holds no provider-probing logic, no
//! terminal/TUI formatting, and no slash-command metadata. Later tasks build
//! the provider status service (probing), the compact renderer, and the
//! `/provider:status` routing on top of these types.
//!
//! Truthfulness rules baked into the shape:
//!
//! - Exact remaining usage is representable *only* through
//!   [`UsageAvailability::Exact`], and the capability-gated
//!   [`UsageAvailability::exact_if_supported`] constructor refuses to produce
//!   it unless an adapter declares the supporting capability. Unsupported,
//!   unavailable, and not-applicable usage are first-class, distinct states —
//!   never errors and never inferred numbers (ADR-001).
//! - Capability support ([`ProviderStatusCapabilities`]) is modeled separately
//!   from a status result so unsupported exact usage is not mistaken for a
//!   failed provider check.
//! - Diagnostics carry only share-safe label/detail text; the type has no
//!   field for secrets, tokens, account identifiers, emails, organization IDs,
//!   or raw provider payloads (redaction itself lives in the service layer).

use crate::config::RuntimeKind;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stable identity for a provider whose runway status can be reported.
///
/// Mirrors the configured runtimes and adds [`ProviderId::Local`] for
/// local-only runtime health, which has no provider account behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Claude,
    Codex,
    Cursor,
    Zai,
    Fake,
    Local,
}

impl ProviderId {
    /// Default human-readable display name. Callers may override with a
    /// configured display name on the status result.
    pub fn default_display_name(&self) -> &'static str {
        match self {
            ProviderId::Claude => "Claude",
            ProviderId::Codex => "Codex",
            ProviderId::Cursor => "Cursor",
            ProviderId::Zai => "Z.ai",
            ProviderId::Fake => "Fake",
            ProviderId::Local => "Local",
        }
    }
}

impl From<RuntimeKind> for ProviderId {
    fn from(kind: RuntimeKind) -> Self {
        match kind {
            RuntimeKind::Claude => ProviderId::Claude,
            RuntimeKind::Codex => ProviderId::Codex,
            RuntimeKind::Cursor => ProviderId::Cursor,
            RuntimeKind::Zai => ProviderId::Zai,
            RuntimeKind::Fake => ProviderId::Fake,
        }
    }
}

/// The fixed runway state set from the TechSpec. Each maps to a top-level row
/// label in the compact `/provider:status` output and is matched directly —
/// never parsed from a formatted string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRunwayState {
    /// Required local/provider checks passed and no blocking signal is known.
    Ready,
    /// Provider-supplied data says usage is constrained, near limit,
    /// rate-limited, or reset-bound.
    LimitedRunway,
    /// Provider or runtime signal says the provider cannot be used for the
    /// intended session.
    Blocked,
    /// Provider may be usable, but exact remaining usage is unsupported,
    /// unavailable, or not exposed.
    UnavailableUsage,
    /// Credentials, account link, or login state is missing or invalid.
    Unauthenticated,
    /// Local provider configuration is missing, invalid, or points to an
    /// unusable model/provider.
    Misconfigured,
    /// A live check failed due to provider-side or network behavior that
    /// cannot be classified more specifically.
    ProviderError,
    /// Local runtime readiness can be described, but account usage does not
    /// apply.
    LocalOnlyStatus,
}

/// Provider-native usage metric. Kept provider-native rather than normalized
/// into a synthetic percentage (ADR-001). [`UsageMetric::Other`] preserves a
/// metric a provider exposes that does not map onto the common set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageMetric {
    Messages,
    Requests,
    Tokens,
    Credits,
    Other(String),
}

/// A provider-native usage amount with an optional unit label. Stored as a
/// floating amount so fractional credits are represented without loss; this is
/// only ever populated from documented provider data, never inferred.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageAmount {
    pub value: f64,
    pub unit: Option<String>,
}

impl UsageAmount {
    pub fn new(value: f64) -> Self {
        Self { value, unit: None }
    }

    pub fn with_unit(value: f64, unit: impl Into<String>) -> Self {
        Self {
            value,
            unit: Some(unit.into()),
        }
    }
}

/// The window a usage metric is measured over, when the provider documents it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindow {
    Daily,
    Weekly,
    Monthly,
    /// A rolling window of the given number of hours (e.g. a 5-hour window).
    Rolling {
        hours: u32,
    },
    Other(String),
}

/// Exact, documented remaining usage from a supported provider integration.
///
/// This is the *only* representation of exact usage. Constructing it requires
/// an `observed_at` timestamp so callers cannot present exact usage without
/// recording when it was observed. It must never be built from local CLI
/// state, token logs, request counts, or failed-command frequency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactUsage {
    pub metric: UsageMetric,
    pub remaining: UsageAmount,
    pub limit: Option<UsageAmount>,
    pub window: Option<UsageWindow>,
    pub reset_at: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,
}

/// Whether and how exact remaining usage is available for a provider.
///
/// `Unsupported`, `Unavailable`, and `NotApplicable` are deliberately distinct
/// truthful states — they are not errors and not estimates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageAvailability {
    /// Documented exact usage from a capability-supported provider integration.
    Exact(ExactUsage),
    /// The provider may be usable, but exact usage is not exposed by the
    /// integration at all.
    Unsupported { provider_url: Option<String> },
    /// Exact usage is supported in principle but could not be retrieved this
    /// invocation (timeout, transient failure, missing account link, etc.).
    Unavailable {
        reason: String,
        provider_url: Option<String>,
    },
    /// Account usage does not apply to this provider (e.g. a local runtime).
    NotApplicable,
}

impl UsageAvailability {
    /// Capability-gated constructor for exact usage.
    ///
    /// Returns [`UsageAvailability::Exact`] only when `capabilities` declares a
    /// supporting usage capability; otherwise it degrades truthfully to
    /// [`UsageAvailability::Unsupported`]. This is the guard that prevents
    /// exact usage from being represented without supporting provider
    /// capability context.
    pub fn exact_if_supported(
        capabilities: &ProviderStatusCapabilities,
        usage: ExactUsage,
        provider_url: Option<String>,
    ) -> Self {
        if capabilities.supports_exact_usage() {
            UsageAvailability::Exact(usage)
        } else {
            UsageAvailability::Unsupported { provider_url }
        }
    }

    /// True only for [`UsageAvailability::Exact`].
    pub fn is_exact(&self) -> bool {
        matches!(self, UsageAvailability::Exact(_))
    }
}

/// Reset timing for a usage window. `Unknown` is a first-class state: the
/// TechSpec requires saying `unknown` when a provider does not return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetAvailability {
    Known { reset_at: DateTime<Utc> },
    Unknown,
    NotApplicable,
}

/// How fresh the status data is. `observed_at` is recorded for live and cached
/// data so older results can be labeled in output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusFreshness {
    Live { observed_at: DateTime<Utc> },
    Cached { observed_at: DateTime<Utc> },
    Unknown,
}

/// Where a status result was derived from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    /// A documented provider API/account integration.
    ProviderApi,
    /// A local runtime/CLI readiness check.
    LocalRuntimeCheck,
    /// Local provider configuration only.
    LocalConfig,
    Unknown,
}

/// Typed reason code for a status, kept separate from the human-readable
/// summary so consumers can branch without parsing strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    Healthy,
    UsageConstrained,
    UsageBlocked,
    UsageUnsupported,
    UsageUnknown,
    AuthRequired,
    Misconfigured,
    ModelUnavailable,
    ProviderError,
    LocalOnly,
}

/// Short, share-safe reason for a status row. `summary` must not contain
/// secrets or account identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatusReason {
    pub code: ReasonCode,
    pub summary: String,
}

impl ProviderStatusReason {
    pub fn new(code: ReasonCode, summary: impl Into<String>) -> Self {
        Self {
            code,
            summary: summary.into(),
        }
    }
}

/// The single recommended next action for a provider row. `provider_url`
/// carries an official account/usage page when checking the provider is the
/// most actionable step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderNextAction {
    /// Nothing to do; safe to proceed.
    Proceed,
    /// Check the official provider account/usage page.
    CheckProviderUsage { provider_url: Option<String> },
    /// Authenticate or re-link the account.
    Authenticate,
    /// Fix local provider/model configuration.
    FixConfiguration,
    /// Switch to a different provider for this session.
    SwitchProvider,
    /// Reduce session scope to fit constrained runway.
    ReduceScope,
    /// Retry the status check or the run later.
    RetryLater,
    /// Verify local runtime/CLI availability.
    VerifyRuntime,
}

/// Whether a provider adapter supports a given class of status data. Modeled
/// separately from results so unsupported data classes are not treated as
/// failed checks (ADR-001 capability negotiation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    RequiresAccountLink,
    RequiresConfiguration,
}

/// The status data classes a provider adapter declares support for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatusCapabilities {
    pub auth_state: CapabilitySupport,
    pub model_availability: CapabilitySupport,
    pub subscription_usage: CapabilitySupport,
    pub api_billing_usage: CapabilitySupport,
    pub rate_limits: CapabilitySupport,
    pub local_runtime_health: CapabilitySupport,
    pub provider_incident_state: CapabilitySupport,
}

impl ProviderStatusCapabilities {
    /// Capabilities with every class `Unsupported` — a safe default for an
    /// adapter that only reports local readiness.
    pub fn none() -> Self {
        Self {
            auth_state: CapabilitySupport::Unsupported,
            model_availability: CapabilitySupport::Unsupported,
            subscription_usage: CapabilitySupport::Unsupported,
            api_billing_usage: CapabilitySupport::Unsupported,
            rate_limits: CapabilitySupport::Unsupported,
            local_runtime_health: CapabilitySupport::Unsupported,
            provider_incident_state: CapabilitySupport::Unsupported,
        }
    }

    /// True when either subscription or API billing usage is `Supported`, i.e.
    /// the adapter may return [`UsageAvailability::Exact`].
    pub fn supports_exact_usage(&self) -> bool {
        matches!(self.subscription_usage, CapabilitySupport::Supported)
            || matches!(self.api_billing_usage, CapabilitySupport::Supported)
    }
}

/// Severity of a share-safe provider diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// A single share-safe diagnostic line. Holds only a short `label` and
/// `detail`; there is intentionally no field for secrets, tokens, account
/// identifiers, emails, organization IDs, or raw provider payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDiagnostic {
    pub severity: DiagnosticSeverity,
    pub label: String,
    pub detail: String,
}

impl ProviderDiagnostic {
    pub fn new(
        severity: DiagnosticSeverity,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            label: label.into(),
            detail: detail.into(),
        }
    }

    pub fn info(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Info, label, detail)
    }

    pub fn warning(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Warning, label, detail)
    }

    pub fn error(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(DiagnosticSeverity::Error, label, detail)
    }
}

/// The full typed status for one provider. This is the core API later tasks
/// consume; renderers and routing read these fields rather than re-deriving
/// status from app state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_time() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid fixed timestamp")
    }

    fn sample_exact_usage() -> ExactUsage {
        ExactUsage {
            metric: UsageMetric::Messages,
            remaining: UsageAmount::new(42.0),
            limit: Some(UsageAmount::new(100.0)),
            window: Some(UsageWindow::Rolling { hours: 5 }),
            reset_at: Some(fixed_time()),
            observed_at: fixed_time(),
        }
    }

    #[test]
    fn exact_usage_is_only_reachable_through_the_exact_variant_with_a_timestamp() {
        // The only way to carry exact usage is the Exact variant, and building
        // ExactUsage requires an observed_at timestamp (no Default exists).
        let usage = UsageAvailability::Exact(sample_exact_usage());
        assert!(usage.is_exact());
        match usage {
            UsageAvailability::Exact(exact) => {
                assert_eq!(exact.observed_at, fixed_time());
                assert_eq!(exact.metric, UsageMetric::Messages);
            }
            other => panic!("expected Exact, got {other:?}"),
        }
    }

    #[test]
    fn exact_if_supported_requires_supporting_capability_context() {
        let mut caps = ProviderStatusCapabilities::none();
        // No supporting capability -> degrades to Unsupported, never Exact.
        let degraded = UsageAvailability::exact_if_supported(
            &caps,
            sample_exact_usage(),
            Some("https://example.test/usage".to_string()),
        );
        assert!(!degraded.is_exact());
        assert_eq!(
            degraded,
            UsageAvailability::Unsupported {
                provider_url: Some("https://example.test/usage".to_string()),
            }
        );

        // Declaring subscription usage support unlocks Exact.
        caps.subscription_usage = CapabilitySupport::Supported;
        let exact = UsageAvailability::exact_if_supported(&caps, sample_exact_usage(), None);
        assert!(exact.is_exact());

        // API billing support is the alternative gate.
        let mut billing = ProviderStatusCapabilities::none();
        billing.api_billing_usage = CapabilitySupport::Supported;
        assert!(
            UsageAvailability::exact_if_supported(&billing, sample_exact_usage(), None).is_exact()
        );
    }

    #[test]
    fn unsupported_unavailable_and_not_applicable_usage_are_distinct() {
        let unsupported = UsageAvailability::Unsupported { provider_url: None };
        let unavailable = UsageAvailability::Unavailable {
            reason: "timeout".to_string(),
            provider_url: None,
        };
        let not_applicable = UsageAvailability::NotApplicable;

        assert_ne!(unsupported, unavailable);
        assert_ne!(unsupported, not_applicable);
        assert_ne!(unavailable, not_applicable);
        // None of the non-exact states report as exact.
        for state in [&unsupported, &unavailable, &not_applicable] {
            assert!(!state.is_exact());
        }
    }

    #[test]
    fn every_required_runway_state_can_be_constructed_and_matched() {
        let states = [
            ProviderRunwayState::Ready,
            ProviderRunwayState::LimitedRunway,
            ProviderRunwayState::Blocked,
            ProviderRunwayState::UnavailableUsage,
            ProviderRunwayState::Unauthenticated,
            ProviderRunwayState::Misconfigured,
            ProviderRunwayState::ProviderError,
            ProviderRunwayState::LocalOnlyStatus,
        ];
        assert_eq!(states.len(), 8);
        // Match each variant directly (no string parsing) to prove exhaustive
        // coverage of the fixed set.
        for state in states {
            let matched = match state {
                ProviderRunwayState::Ready
                | ProviderRunwayState::LimitedRunway
                | ProviderRunwayState::Blocked
                | ProviderRunwayState::UnavailableUsage
                | ProviderRunwayState::Unauthenticated
                | ProviderRunwayState::Misconfigured
                | ProviderRunwayState::ProviderError
                | ProviderRunwayState::LocalOnlyStatus => true,
            };
            assert!(matched);
        }
    }

    #[test]
    fn capability_support_covers_all_four_cases() {
        let cases = [
            CapabilitySupport::Supported,
            CapabilitySupport::Unsupported,
            CapabilitySupport::RequiresAccountLink,
            CapabilitySupport::RequiresConfiguration,
        ];
        // Distinct from one another.
        for (i, a) in cases.iter().enumerate() {
            for (j, b) in cases.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
        // `none()` defaults every class to Unsupported and reports no exact
        // usage support.
        let none = ProviderStatusCapabilities::none();
        assert!(!none.supports_exact_usage());
        assert_eq!(none.subscription_usage, CapabilitySupport::Unsupported);
        assert_eq!(none.api_billing_usage, CapabilitySupport::Unsupported);
    }

    #[test]
    fn diagnostics_carry_only_share_safe_label_and_detail() {
        let diag = ProviderDiagnostic::warning(
            "auth",
            "credentials not found; configure the provider before use",
        );
        assert_eq!(diag.severity, DiagnosticSeverity::Warning);
        assert!(!diag.label.is_empty());
        assert!(!diag.detail.is_empty());
        // The type exposes only label/detail/severity; there is no field that
        // could carry a secret. A constructed diagnostic contains exactly the
        // share-safe text it was given.
        assert_eq!(diag.label, "auth");
        assert!(diag.detail.contains("configure the provider"));
    }

    #[test]
    fn provider_id_maps_from_runtime_kind_and_adds_local() {
        assert_eq!(ProviderId::from(RuntimeKind::Claude), ProviderId::Claude);
        assert_eq!(ProviderId::from(RuntimeKind::Codex), ProviderId::Codex);
        assert_eq!(ProviderId::from(RuntimeKind::Cursor), ProviderId::Cursor);
        assert_eq!(ProviderId::from(RuntimeKind::Zai), ProviderId::Zai);
        assert_eq!(ProviderId::from(RuntimeKind::Fake), ProviderId::Fake);
        assert_eq!(ProviderId::Zai.default_display_name(), "Z.ai");
        assert_eq!(ProviderId::Local.default_display_name(), "Local");
    }

    #[test]
    fn a_full_status_value_composes_the_typed_fields() {
        let status = ProviderRunwayStatus {
            provider_id: ProviderId::Local,
            display_name: ProviderId::Local.default_display_name().to_string(),
            selected_model: None,
            state: ProviderRunwayState::LocalOnlyStatus,
            reason: ProviderStatusReason::new(
                ReasonCode::LocalOnly,
                "runtime reachable; account usage does not apply",
            ),
            next_action: ProviderNextAction::Proceed,
            usage: UsageAvailability::NotApplicable,
            reset: ResetAvailability::NotApplicable,
            freshness: StatusFreshness::Live {
                observed_at: fixed_time(),
            },
            source: StatusSource::LocalRuntimeCheck,
            diagnostics: vec![ProviderDiagnostic::info("runtime", "binary on PATH")],
        };
        assert_eq!(status.state, ProviderRunwayState::LocalOnlyStatus);
        assert_eq!(status.usage, UsageAvailability::NotApplicable);
        assert_eq!(status.reason.code, ReasonCode::LocalOnly);
    }
}
