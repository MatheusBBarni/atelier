//! Integration coverage for the compact share-safe renderer: command routing
//! must be able to turn a list of typed provider statuses into default output
//! with one row per provider, in stable order, without provider-specific
//! branching.

use multiagent::runtime::status::{
    render_provider_status, state_label, ProviderDiagnostic, ProviderId, ProviderNextAction,
    ProviderRunwayState, ProviderRunwayStatus, ProviderStatusReason, ReasonCode, ResetAvailability,
    StatusFreshness, StatusSource, UsageAvailability,
};

fn status(
    provider_id: ProviderId,
    state: ProviderRunwayState,
    reason: &str,
    next_action: ProviderNextAction,
    usage: UsageAvailability,
    diagnostics: Vec<ProviderDiagnostic>,
) -> ProviderRunwayStatus {
    ProviderRunwayStatus {
        provider_id,
        display_name: provider_id.default_display_name().to_string(),
        selected_model: None,
        state,
        reason: ProviderStatusReason::new(ReasonCode::UsageUnsupported, reason),
        next_action,
        usage,
        reset: ResetAvailability::Unknown,
        freshness: StatusFreshness::Unknown,
        source: StatusSource::LocalRuntimeCheck,
        diagnostics,
    }
}

fn sample_statuses() -> Vec<ProviderRunwayStatus> {
    vec![
        status(
            ProviderId::Claude,
            ProviderRunwayState::Ready,
            "ready based on auth/config/runtime checks; exact remaining usage is unavailable",
            ProviderNextAction::CheckProviderUsage {
                provider_url: Some("https://claude.ai/settings/usage".to_string()),
            },
            UsageAvailability::Unsupported {
                provider_url: Some("https://claude.ai/settings/usage".to_string()),
            },
            vec![ProviderDiagnostic::info(
                "auth",
                "token sk-SHOULDNOTLEAK1234567890",
            )],
        ),
        status(
            ProviderId::Codex,
            ProviderRunwayState::UnavailableUsage,
            "auth/config look ok; exact remaining usage is unsupported",
            ProviderNextAction::CheckProviderUsage { provider_url: None },
            UsageAvailability::Unsupported { provider_url: None },
            Vec::new(),
        ),
        status(
            ProviderId::HttpApi,
            ProviderRunwayState::Misconfigured,
            "Z.ai api_key_env is not configured",
            ProviderNextAction::FixConfiguration,
            UsageAvailability::Unavailable {
                reason: "provider is unavailable for this session".to_string(),
                provider_url: Some("https://z.ai".to_string()),
            },
            Vec::new(),
        ),
        status(
            ProviderId::Local,
            ProviderRunwayState::LocalOnlyStatus,
            "runtime reachable; account usage does not apply",
            ProviderNextAction::Proceed,
            UsageAvailability::NotApplicable,
            Vec::new(),
        ),
    ]
}

#[test]
fn renders_one_row_per_provider_in_stable_order() {
    let statuses = sample_statuses();
    let out = render_provider_status(&statuses);

    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Provider status");
    assert_eq!(lines[1], "");
    // Exactly one data row per provider, in the order provided.
    let data_rows: Vec<&str> = lines[2..]
        .iter()
        .copied()
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(data_rows.len(), statuses.len());
    for (row, status) in data_rows.iter().zip(&statuses) {
        assert!(
            row.starts_with(&status.display_name),
            "row {row:?} should start with {}",
            status.display_name
        );
        assert!(
            row.contains(state_label(status.state)),
            "row {row:?} should contain label {}",
            state_label(status.state)
        );
    }
}

#[test]
fn default_output_is_command_surface_ready_and_share_safe() {
    let out = render_provider_status(&sample_statuses());
    // A plain string the submitted-command surface can print directly.
    assert!(out.starts_with("Provider status\n\n"));
    assert!(!out.ends_with('\n'));
    // The diagnostic secret is omitted from default output.
    assert!(
        !out.contains("sk-SHOULDNOTLEAK1234567890"),
        "secret leaked: {out}"
    );
    // Truthful runway wording, never an invented quota number.
    assert!(out.contains("exact remaining usage is unavailable"));
    assert!(!out.contains("remaining of"));
}

#[test]
fn empty_list_is_handled_without_panicking() {
    let out = render_provider_status(&[]);
    assert!(out.contains("Provider status"));
}
