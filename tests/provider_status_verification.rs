//! Consolidated verification of the trust-sensitive `/provider:status`
//! contract, exercised end-to-end through the public runtime/status API:
//! every runway state renders, exact usage is capability-gated, unsupported
//! usage is truthful, default output is share-safe across every sensitive
//! category, and one failing/slow provider never suppresses the others.

use std::sync::Arc;
use std::time::Duration;

use chrono::DateTime;
use multiagent::runtime::status::{
    render_provider_status, state_label, CapabilitySupport, ExactUsage, ProviderDiagnostic,
    ProviderId, ProviderNextAction, ProviderProbeResult, ProviderRunwayState, ProviderRunwayStatus,
    ProviderStatusCapabilities, ProviderStatusReason, ProviderStatusService, ReasonCode,
    ResetAvailability, StaticStatusProbe, StatusFreshness, StatusSource, UsageAmount,
    UsageAvailability, UsageMetric,
};

const ALL_STATES: [ProviderRunwayState; 8] = [
    ProviderRunwayState::Ready,
    ProviderRunwayState::LimitedRunway,
    ProviderRunwayState::Blocked,
    ProviderRunwayState::UnavailableUsage,
    ProviderRunwayState::Unauthenticated,
    ProviderRunwayState::Misconfigured,
    ProviderRunwayState::ProviderError,
    ProviderRunwayState::LocalOnlyStatus,
];

fn probe_result(state: ProviderRunwayState, usage: UsageAvailability) -> ProviderProbeResult {
    ProviderProbeResult {
        state,
        reason: ProviderStatusReason::new(
            ReasonCode::UsageUnsupported,
            "exact remaining usage is unavailable from this provider integration",
        ),
        next_action: ProviderNextAction::CheckProviderUsage { provider_url: None },
        usage,
        reset: ResetAvailability::Unknown,
        selected_model: None,
        source: StatusSource::ProviderApi,
        diagnostics: Vec::new(),
    }
}

fn status_value(provider_id: ProviderId, state: ProviderRunwayState) -> ProviderRunwayStatus {
    ProviderRunwayStatus {
        provider_id,
        display_name: provider_id.default_display_name().to_string(),
        selected_model: None,
        state,
        reason: ProviderStatusReason::new(ReasonCode::UsageUnsupported, "reason"),
        next_action: ProviderNextAction::Proceed,
        usage: UsageAvailability::NotApplicable,
        reset: ResetAvailability::Unknown,
        freshness: StatusFreshness::Unknown,
        source: StatusSource::LocalRuntimeCheck,
        diagnostics: Vec::new(),
    }
}

#[test]
fn catalog_exposes_provider_status_exactly_once_and_rejects_unknown_siblings() {
    use multiagent::slash_commands::{available_commands_summary, catalog};
    let entries: Vec<_> = catalog()
        .iter()
        .filter(|spec| spec.label == "/provider:status")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].description.is_empty());
    assert!(!entries[0].usage.is_empty());
    assert!(available_commands_summary().contains("/provider:status"));
    // The catalog is a closed set: unknown sibling labels are absent, so they
    // keep flowing through the existing unknown-command rejection path.
    assert!(catalog()
        .iter()
        .all(|spec| spec.label != "/provider:not-real"));
}

#[test]
fn every_runway_state_renders_exactly_one_row_with_its_label() {
    let statuses: Vec<ProviderRunwayStatus> = ALL_STATES
        .iter()
        .map(|state| status_value(ProviderId::Fake, *state))
        .collect();
    let out = render_provider_status(&statuses);
    let data_rows = out.lines().skip(2).filter(|line| !line.is_empty()).count();
    assert_eq!(data_rows, ALL_STATES.len());
    for state in ALL_STATES {
        assert!(
            out.contains(state_label(state)),
            "missing label {} in {out}",
            state_label(state)
        );
    }
}

#[tokio::test]
async fn exact_usage_renders_only_with_supporting_capability() {
    let exact = ExactUsage {
        metric: UsageMetric::Messages,
        remaining: UsageAmount::new(7.0),
        limit: Some(UsageAmount::new(20.0)),
        window: None,
        reset_at: None,
        observed_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
    };

    // Supporting capability -> exact figures are rendered.
    let mut supported = ProviderStatusCapabilities::none();
    supported.subscription_usage = CapabilitySupport::Supported;
    let probe = StaticStatusProbe::new(
        ProviderId::HttpApi,
        "HTTP API",
        supported,
        probe_result(
            ProviderRunwayState::Ready,
            UsageAvailability::Exact(exact.clone()),
        ),
    );
    let rows = ProviderStatusService::new(vec![Arc::new(probe)])
        .collect_status()
        .await;
    let out = render_provider_status(&rows);
    assert!(out.contains("7 of 20 messages remaining"), "{out}");

    // No supporting capability -> downgraded, no figures, truthful wording.
    let probe = StaticStatusProbe::new(
        ProviderId::HttpApi,
        "HTTP API",
        ProviderStatusCapabilities::none(),
        probe_result(ProviderRunwayState::Ready, UsageAvailability::Exact(exact)),
    );
    let rows = ProviderStatusService::new(vec![Arc::new(probe)])
        .collect_status()
        .await;
    let out = render_provider_status(&rows);
    assert!(
        !out.contains("messages remaining"),
        "fabricated usage: {out}"
    );
    assert!(out.contains("unavailable"), "{out}");
}

#[tokio::test]
async fn unsupported_usage_renders_truthful_wording_and_verification_action() {
    let probe = StaticStatusProbe::new(
        ProviderId::Codex,
        "Codex",
        ProviderStatusCapabilities::none(),
        probe_result(
            ProviderRunwayState::UnavailableUsage,
            UsageAvailability::Unsupported {
                provider_url: Some("https://platform.openai.com/usage".to_string()),
            },
        ),
    );
    let rows = ProviderStatusService::new(vec![Arc::new(probe)])
        .collect_status()
        .await;
    let out = render_provider_status(&rows);
    assert!(out.contains("unavailable"), "{out}");
    assert!(out.contains("Check Codex account usage."), "{out}");
    assert!(!out.contains(" of "), "fabricated quota figure: {out}");
}

#[tokio::test]
async fn default_output_redacts_every_sensitive_category() {
    let mut result = probe_result(
        ProviderRunwayState::ProviderError,
        UsageAvailability::Unavailable {
            reason: "transient".to_string(),
            provider_url: None,
        },
    );
    result.reason = ProviderStatusReason::new(
        ReasonCode::ProviderError,
        "failed for dave@example.com key sk-AAAA1111BBBB2222CCCC org-ZZZZ9999 token \
         ghp_abcd1234EFGH5678ijkl payload 0011223344556677889900aabbccddee",
    );
    // Diagnostics carry secrets too — they must be omitted entirely from the
    // compact default output.
    result.diagnostics = vec![ProviderDiagnostic::error(
        "auth",
        "Authorization Bearer sk-SHOULD11NOT22LEAK33",
    )];
    let probe = StaticStatusProbe::new(
        ProviderId::Codex,
        "Codex",
        ProviderStatusCapabilities::none(),
        result,
    );
    let rows = ProviderStatusService::new(vec![Arc::new(probe)])
        .collect_status()
        .await;
    let out = render_provider_status(&rows);
    for secret in [
        "dave@example.com",
        "sk-AAAA1111BBBB2222CCCC",
        "org-ZZZZ9999",
        "ghp_abcd1234EFGH5678ijkl",
        "0011223344556677889900aabbccddee",
        "sk-SHOULD11NOT22LEAK33",
    ] {
        assert!(!out.contains(secret), "leaked {secret} in {out}");
    }
    assert!(
        out.contains("[redacted"),
        "expected a redaction marker: {out}"
    );
}

#[tokio::test]
async fn one_slow_provider_times_out_without_suppressing_others() {
    let healthy = StaticStatusProbe::new(
        ProviderId::Fake,
        "Fake",
        ProviderStatusCapabilities::none(),
        probe_result(ProviderRunwayState::Ready, UsageAvailability::NotApplicable),
    );
    let slow = StaticStatusProbe::new(
        ProviderId::Claude,
        "Claude",
        ProviderStatusCapabilities::none(),
        probe_result(ProviderRunwayState::Ready, UsageAvailability::NotApplicable),
    )
    .with_delay(Duration::from_secs(30));
    let service = ProviderStatusService::new(vec![Arc::new(healthy), Arc::new(slow)])
        .with_timeout(Duration::from_millis(50));
    let rows = service.collect_status().await;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].state, ProviderRunwayState::Ready);
    assert_eq!(rows[1].state, ProviderRunwayState::ProviderError);
    let out = render_provider_status(&rows);
    assert!(out.contains("Fake"));
    assert!(out.contains("Claude"));
    assert!(out.contains("provider error"));
}
