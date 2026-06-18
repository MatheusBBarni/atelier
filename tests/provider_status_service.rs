//! Integration coverage for the runtime provider status service: it must
//! produce one typed row per provider through the real adapter boundary, keep
//! providers independent when one fails, and discover providers from config —
//! all deterministically (no live CLI/network, no env dependence).

use std::sync::Arc;

use multiagent::config::{
    load_effective_config, ConfigLoadOptions, PromptMode, RuntimeConfig, RuntimeKind,
};
use multiagent::runtime::status::{
    ProviderId, ProviderRunwayState, ProviderStatusProbe, ProviderStatusService,
    RuntimeAvailabilityProbe,
};

fn runtime_config(id: &str, kind: RuntimeKind, api_key_env: Option<&str>) -> RuntimeConfig {
    RuntimeConfig {
        id: id.to_string(),
        kind,
        command: None,
        args: Vec::new(),
        prompt_mode: PromptMode::Stdin,
        base_url: None,
        api_key_env: api_key_env.map(|value| value.to_string()),
        degrade_not_abandon: false,
    }
}

#[tokio::test]
async fn fake_adapter_returns_deterministic_rows_and_capabilities_through_service() {
    let probe = RuntimeAvailabilityProbe::new(runtime_config("fake", RuntimeKind::Fake, None));

    // Capabilities are declared separately from the status result and are
    // deterministic across calls; the fake provider exposes no exact usage.
    let capabilities = probe.capabilities();
    assert_eq!(capabilities, probe.capabilities());
    assert!(!capabilities.supports_exact_usage());

    let service = ProviderStatusService::new(vec![Arc::new(probe)]);
    let rows = service.collect_status().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider_id, ProviderId::Fake);
    // The fake runtime is always Available -> Ready, never with exact usage.
    assert_eq!(rows[0].state, ProviderRunwayState::Ready);
    assert!(!rows[0].usage.is_exact());

    // Re-running yields the same provider and state (only freshness differs).
    let rows_again = service.collect_status().await;
    assert_eq!(rows_again.len(), 1);
    assert_eq!(rows_again[0].provider_id, ProviderId::Fake);
    assert_eq!(rows_again[0].state, ProviderRunwayState::Ready);
}

#[tokio::test]
async fn multiple_providers_return_independent_rows_when_one_fails() {
    let healthy = RuntimeAvailabilityProbe::new(runtime_config("fake", RuntimeKind::Fake, None));
    // Z.ai with no api_key_env configured resolves to Misconfigured without
    // reading any environment variable, so the failure is deterministic.
    let failing = RuntimeAvailabilityProbe::new(runtime_config("zai", RuntimeKind::Zai, None));

    let service = ProviderStatusService::new(vec![Arc::new(healthy), Arc::new(failing)]);
    let rows = service.collect_status().await;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].provider_id, ProviderId::Fake);
    assert_eq!(rows[0].state, ProviderRunwayState::Ready);
    assert_eq!(rows[1].provider_id, ProviderId::Zai);
    assert_eq!(rows[1].state, ProviderRunwayState::Misconfigured);
    // The failing provider did not suppress the healthy one.
    assert_ne!(rows[0].state, rows[1].state);
}

#[tokio::test]
async fn from_config_discovers_only_provider_families_used_by_enabled_agents() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("atelier.toml");
    std::fs::write(
        &config_path,
        r#"
[runtimes.fake]
type = "fake"

[runtimes.zai]
type = "zai"

[agents.orchestrator]
runtime = "fake"

[agents.explorer]
runtime = "fake"

[agents.fixer]
runtime = "fake"

[agents.reviewer]
runtime = "fake"

[agents.oracle]
runtime = "zai"

[agents.consul]
runtime = "fake"

[council.presets.default.architect]
runtime = "fake"

[council.presets.default.security]
runtime = "fake"

[council.presets.default.reviewer]
runtime = "fake"
"#,
    )
    .unwrap();

    let config = load_effective_config(ConfigLoadOptions {
        working_directory: dir.path().to_path_buf(),
        config_path: Some(config_path),
    })
    .unwrap();

    // Discovery is driven by the runtimes enabled agents actually reference
    // (fake + zai here), NOT by every runtime the merged built-in defaults
    // define. So codex/cursor/claude — defined by defaults but unused — are
    // not probed, which also keeps this test free of live CLI shell-outs.
    let service = ProviderStatusService::from_config(&config);
    let ids = service.provider_ids();
    assert!(ids.contains(&ProviderId::Fake), "missing fake: {ids:?}");
    assert!(ids.contains(&ProviderId::Zai), "missing zai: {ids:?}");
    assert!(
        !ids.contains(&ProviderId::Codex),
        "unused codex probed: {ids:?}"
    );
    assert!(
        !ids.contains(&ProviderId::Cursor),
        "unused cursor probed: {ids:?}"
    );
    assert!(
        !ids.contains(&ProviderId::Claude),
        "unused claude probed: {ids:?}"
    );
    assert_eq!(
        service.provider_count(),
        2,
        "expected exactly fake + zai: {ids:?}"
    );
}
