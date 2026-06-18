//! Integration test for the MCP action vertical (task_05): a full
//! validate → execute round-trip of a `CallMcpTool` action against the in-repo
//! fake stdio MCP server, exercised through the real action contract.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use multiagent::actions::{
    execute_action_request, validate_action_request_with_scope, ActionDecision,
    ActionExecutionContext, ActionKind, ActionRequest, ActionStatus, McpActionContext,
};
use multiagent::config::{
    load_effective_config, ApprovalMode, ConfigLoadOptions, McpServerConfig, McpTransport,
    WorkspacePolicy,
};
use multiagent::mcp::{McpSupervisor, McpTrustStore};
use serde_json::json;
use tempfile::tempdir;

const FAKE_SERVER: &str = env!("CARGO_BIN_EXE_fake-mcp-server");

/// Load a config that defines one MCP-capable agent (`caller`).
fn mcp_caller_agent() -> multiagent::config::AgentProfile {
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("atelier.toml");
    std::fs::write(
        &config_path,
        r#"
[runtimes.fake]
type = "fake"

[agents.caller]
runtime = "fake"
capabilities = ["read", "mcp_tool"]
tools = ["call_mcp_tool", "read_mcp_resource", "list_mcp_resources"]
instructions = "MCP caller."
"#,
    )
    .unwrap();
    let config = load_effective_config(ConfigLoadOptions {
        working_directory: dir.path().to_path_buf(),
        config_path: Some(config_path),
    })
    .unwrap();
    config.agents.get("caller").unwrap().clone()
}

fn fake_server_config(id: &str) -> McpServerConfig {
    McpServerConfig {
        id: id.to_string(),
        transport: McpTransport::Stdio,
        command: Some(FAKE_SERVER.to_string()),
        args: Vec::new(),
        env: BTreeMap::new(),
        url: None,
    }
}

#[tokio::test]
async fn call_mcp_tool_validates_and_executes_round_trip() {
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog snapshot");

    // Trust the server so the call validates as Allowed (untrusted would prompt).
    let trust_dir = tempdir().unwrap();
    let mut trust = McpTrustStore::load(trust_dir.path());
    trust.promote("fake").unwrap();

    let mut context = ActionExecutionContext::new(
        PathBuf::from("."),
        WorkspacePolicy::default(),
        ApprovalMode::Yolo,
    );
    context.mcp = Some(McpActionContext {
        handle: handle.clone(),
        trust,
        catalog: Arc::new(catalog),
    });

    let request = ActionRequest {
        schema_version: 1,
        action_id: "a1".to_string(),
        step_id: "s1".to_string(),
        kind: ActionKind::CallMcpTool,
        params: json!({ "server": "fake", "tool": "effect_tool", "args": { "echo": "round-trip" } }),
    };

    // Validate: trusted + allowlisted + unpinned ⇒ Allowed.
    assert!(matches!(
        validate_action_request_with_scope(&agent, &context, &request),
        ActionDecision::Allowed
    ));

    // Execute: the tool's echoed result comes back as the ActionResult content.
    let result = execute_action_request(&agent, &context, &request).await;
    assert_eq!(result.status, ActionStatus::Completed);
    let content = result.content.expect("tool result content");
    assert!(
        content.to_string().contains("round-trip"),
        "expected the echoed args in the result, got: {content}"
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn call_mcp_tool_on_untrusted_server_requires_approval_before_execute() {
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog snapshot");

    // No promote: the server is untrusted.
    let trust_dir = tempdir().unwrap();
    let trust = McpTrustStore::load(trust_dir.path());

    let mut context = ActionExecutionContext::new(
        PathBuf::from("."),
        WorkspacePolicy::default(),
        ApprovalMode::Yolo,
    );
    context.mcp = Some(McpActionContext {
        handle: handle.clone(),
        trust,
        catalog: Arc::new(catalog),
    });

    let request = ActionRequest {
        schema_version: 1,
        action_id: "a2".to_string(),
        step_id: "s2".to_string(),
        kind: ActionKind::CallMcpTool,
        params: json!({ "server": "fake", "tool": "effect_tool", "args": {} }),
    };

    assert!(matches!(
        validate_action_request_with_scope(&agent, &context, &request),
        ActionDecision::RequiresApproval(_)
    ));
    // Execution short-circuits to ApprovalRequired without invoking the tool.
    let result = execute_action_request(&agent, &context, &request).await;
    assert_eq!(result.status, ActionStatus::ApprovalRequired);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn first_contact_prompts_then_promote_allows_then_revoke_prompts() {
    // The first-contact → approve+promote → revoke trust lifecycle (task_09),
    // observed through the validation gate. Promote/revoke are exactly what the
    // approval card's approve-and-trust and `/trust` revoke drive.
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog");
    let trust_dir = tempdir().unwrap();
    let mut trust = McpTrustStore::load(trust_dir.path());

    let request = ActionRequest {
        schema_version: 1,
        action_id: "a".to_string(),
        step_id: "s".to_string(),
        kind: ActionKind::CallMcpTool,
        params: json!({ "server": "fake", "tool": "effect_tool", "args": {} }),
    };

    let context = |trust: &McpTrustStore| {
        let mut context = ActionExecutionContext::new(
            PathBuf::from("."),
            WorkspacePolicy::default(),
            ApprovalMode::Yolo,
        );
        context.mcp = Some(McpActionContext {
            handle: handle.clone(),
            trust: trust.clone(),
            catalog: Arc::new(catalog.clone()),
        });
        context
    };

    // First contact: untrusted ⇒ prompt.
    assert!(matches!(
        validate_action_request_with_scope(&agent, &context(&trust), &request),
        ActionDecision::RequiresApproval(_)
    ));

    // Promote (approve-and-trust) ⇒ auto-allowed, no prompt.
    trust.promote("fake").unwrap();
    assert!(matches!(
        validate_action_request_with_scope(&agent, &context(&trust), &request),
        ActionDecision::Allowed
    ));

    // Revoke ⇒ prompts again.
    trust.revoke("fake").unwrap();
    assert!(matches!(
        validate_action_request_with_scope(&agent, &context(&trust), &request),
        ActionDecision::RequiresApproval(_)
    ));

    handle.shutdown().await.ok();
}

/// Build an MCP action context over `handle`/`catalog` with the given trust and
/// degrade-not-abandon flag.
fn mcp_context(
    handle: &multiagent::mcp::McpHandle,
    catalog: &multiagent::mcp::ToolCatalog,
    trust: McpTrustStore,
    degrade_not_abandon: bool,
) -> ActionExecutionContext {
    let mut context = ActionExecutionContext::new(
        PathBuf::from("."),
        WorkspacePolicy::default(),
        ApprovalMode::Yolo,
    );
    context.mcp = Some(McpActionContext {
        handle: handle.clone(),
        trust,
        catalog: Arc::new(catalog.clone()),
    });
    context.degrade_not_abandon = degrade_not_abandon;
    context
}

fn call_request(tool: &str, args: serde_json::Value) -> ActionRequest {
    ActionRequest {
        schema_version: 1,
        action_id: "a".to_string(),
        step_id: "s".to_string(),
        kind: ActionKind::CallMcpTool,
        params: json!({ "server": "fake", "tool": tool, "args": args }),
    }
}

#[tokio::test]
async fn malformed_mcp_call_carries_repair_hint_then_valid_call_recovers() {
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog");
    let trust_dir = tempdir().unwrap();
    let mut trust = McpTrustStore::load(trust_dir.path());
    trust.promote("fake").unwrap();
    let context = mcp_context(&handle, &catalog, trust, false);

    // Malformed: an unknown tool fails at the server; the result carries a
    // structured repair hint for the model's next emission.
    let bad = call_request("nonexistent_tool", json!({}));
    let failed = execute_action_request(&agent, &context, &bad).await;
    assert_eq!(failed.status, ActionStatus::Failed);
    assert!(
        failed
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("CallMcpTool"),
        "failure should carry the repair re-prompt"
    );

    // Repair: a corrected (valid) call executes successfully.
    let good = call_request("effect_tool", json!({ "echo": "ok" }));
    let recovered = execute_action_request(&agent, &context, &good).await;
    assert_eq!(recovered.status, ActionStatus::Completed);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn degrade_not_abandon_skips_failed_tool_instead_of_failing() {
    let agent = mcp_caller_agent();
    let handle = McpSupervisor::spawn(vec![fake_server_config("fake")], Duration::from_secs(5));
    let catalog = handle.snapshot_catalog().await.expect("catalog");
    let trust_dir = tempdir().unwrap();
    let mut trust = McpTrustStore::load(trust_dir.path());
    trust.promote("fake").unwrap();
    let bad = call_request("nonexistent_tool", json!({}));

    // degrade_not_abandon = true ⇒ a failed tool is skipped (run continues).
    let degrade = mcp_context(&handle, &catalog, trust.clone(), true);
    let result = execute_action_request(&agent, &degrade, &bad).await;
    assert_eq!(result.status, ActionStatus::Completed);

    // degrade_not_abandon = false ⇒ the failure is surfaced.
    let strict = mcp_context(&handle, &catalog, trust, false);
    let result = execute_action_request(&agent, &strict, &bad).await;
    assert_eq!(result.status, ActionStatus::Failed);

    handle.shutdown().await.ok();
}

/// Emission spike harness (task_11): measures p95 emission-with-repair latency
/// against a high-tool-count fake server. `#[ignore]`d behind an env var like the
/// repo's other live/heavy suites.
#[tokio::test]
#[ignore = "emission spike harness; run with MULTIAGENT_RUN_MCP_SPIKE=1"]
async fn emission_spike_reports_p95_with_repair() {
    if std::env::var("MULTIAGENT_RUN_MCP_SPIKE").is_err() {
        return;
    }
    let agent = mcp_caller_agent();
    let mut server = fake_server_config("fake");
    server
        .env
        .insert("FAKE_MCP_TOOL_COUNT".to_string(), "50".to_string());
    let handle = McpSupervisor::spawn(vec![server], Duration::from_secs(10));
    let catalog = handle.snapshot_catalog().await.expect("catalog");
    assert!(
        catalog.servers[0].tools.len() >= 50,
        "high-tool-count catalog expected"
    );
    let trust_dir = tempdir().unwrap();
    let mut trust = McpTrustStore::load(trust_dir.path());
    trust.promote("fake").unwrap();
    let context = mcp_context(&handle, &catalog, trust, false);

    // Each iteration: one malformed emission (fails, yields a repair hint) then
    // the repaired valid emission — the "emission-with-repair" round-trip.
    let mut latencies = Vec::new();
    for _ in 0..20u32 {
        let start = std::time::Instant::now();
        let _ = execute_action_request(
            &agent,
            &context,
            &call_request("nonexistent_tool", json!({})),
        )
        .await;
        let ok = execute_action_request(
            &agent,
            &context,
            &call_request("effect_tool", json!({ "echo": "x" })),
        )
        .await;
        assert_eq!(ok.status, ActionStatus::Completed);
        latencies.push(start.elapsed());
    }
    latencies.sort();
    let p95 = latencies[(latencies.len() as f64 * 0.95) as usize - 1];
    println!("emission-with-repair p95 (high-tool-count fake server): {p95:?}");

    handle.shutdown().await.ok();
}
