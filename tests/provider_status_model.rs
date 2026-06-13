//! Integration coverage proving the runtime provider status model is usable
//! from outside the `runtime::status` module — i.e. a later provider status
//! service / renderer can build and consume `ProviderRunwayStatus` through the
//! public runtime export boundary.

use chrono::DateTime;
use multiagent::config::RuntimeKind;
use multiagent::runtime::status::{
    CapabilitySupport, ExactUsage, ProviderDiagnostic, ProviderId, ProviderNextAction,
    ProviderRunwayState, ProviderRunwayStatus, ProviderStatusCapabilities, ProviderStatusReason,
    ReasonCode, ResetAvailability, StatusFreshness, StatusSource, UsageAmount, UsageAvailability,
    UsageMetric, UsageWindow,
};

#[test]
fn fake_runtime_status_can_be_built_from_the_exported_model() {
    // A provider id derived from a configured runtime kind, then a full
    // status value assembled entirely from exported types.
    let provider_id = ProviderId::from(RuntimeKind::Fake);
    assert_eq!(provider_id, ProviderId::Fake);

    let status = ProviderRunwayStatus {
        provider_id,
        display_name: provider_id.default_display_name().to_string(),
        selected_model: Some("fake-model".to_string()),
        state: ProviderRunwayState::UnavailableUsage,
        reason: ProviderStatusReason::new(
            ReasonCode::UsageUnsupported,
            "auth ok; exact remaining usage is unavailable from this provider integration",
        ),
        next_action: ProviderNextAction::CheckProviderUsage {
            provider_url: Some("https://example.test/usage".to_string()),
        },
        usage: UsageAvailability::Unsupported {
            provider_url: Some("https://example.test/usage".to_string()),
        },
        reset: ResetAvailability::Unknown,
        freshness: StatusFreshness::Unknown,
        source: StatusSource::LocalConfig,
        diagnostics: vec![ProviderDiagnostic::info(
            "runtime",
            "fake runtime is reachable",
        )],
    };

    assert_eq!(status.provider_id, ProviderId::Fake);
    assert_eq!(status.state, ProviderRunwayState::UnavailableUsage);
    assert!(!status.usage.is_exact());
    assert_eq!(status.diagnostics.len(), 1);
}

#[test]
fn capability_gated_exact_usage_is_consumable_externally() {
    let observed_at = DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp");
    let exact = ExactUsage {
        metric: UsageMetric::Credits,
        remaining: UsageAmount::with_unit(12.5, "credits"),
        limit: Some(UsageAmount::with_unit(100.0, "credits")),
        window: Some(UsageWindow::Monthly),
        reset_at: None,
        observed_at,
    };

    // Without supporting capability the external caller cannot obtain Exact.
    let caps = ProviderStatusCapabilities::none();
    let degraded = UsageAvailability::exact_if_supported(&caps, exact.clone(), None);
    assert!(!degraded.is_exact());

    // With a supported capability the same data becomes Exact.
    let mut supported = ProviderStatusCapabilities::none();
    supported.api_billing_usage = CapabilitySupport::Supported;
    let usage = UsageAvailability::exact_if_supported(&supported, exact, None);
    assert!(usage.is_exact());
}
